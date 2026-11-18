//! The host's own log, to stderr (launchd keeps it).

/// The longest text from a server that goes into one log line.
const MAX_UNTRUSTED_BYTES: usize = 2048;

/// Logs one line. Never pass it secrets; pass text a server sent through [`untrusted`].
pub(crate) fn log(message: &str) {
    #[cfg(test)]
    CAPTURED.with(|captured| {
        if let Some(lines) = captured.borrow_mut().as_mut() {
            lines.push(message.to_owned());
        }
    });
    eprintln!("agentic-org: {message}");
}

#[cfg(test)]
thread_local! {
    static CAPTURED: std::cell::RefCell<Option<Vec<String>>> = const { std::cell::RefCell::new(None) };
}

/// Runs `f` and returns what it logged on this thread, so a test can assert
/// that no secret reaches the host log.
#[cfg(test)]
pub(crate) fn capture<R>(f: impl FnOnce() -> R) -> (R, Vec<String>) {
    CAPTURED.with(|captured| *captured.borrow_mut() = Some(Vec::new()));
    let result = f();
    let lines = CAPTURED.with(|captured| captured.borrow_mut().take().unwrap_or_default());
    (result, lines)
}

/// Text a server sent (e.g. a `failed` message), made safe for one log line: control characters,
/// line and paragraph separators and direction controls are escaped, so it can't forge lines or
/// reorder what is shown, and it is cut to about 2 KiB.
pub(crate) fn untrusted(text: &str) -> String {
    let mut out = String::new();
    for c in text.chars() {
        if out.len() >= MAX_UNTRUSTED_BYTES {
            out.push('…');
            break;
        }
        if c.is_control() || breaks_or_reorders(c) {
            out.extend(c.escape_default());
        } else {
            out.push(c);
        }
    }
    out
}

/// Characters that some viewers show as a line break, or that reorder the text around them.
fn breaks_or_reorders(c: char) -> bool {
    matches!(
        c,
        '\u{2028}' | '\u{2029}' // line and paragraph separators
            | '\u{200E}' | '\u{200F}' | '\u{061C}' // direction marks
            | '\u{202A}'..='\u{202E}' // embeddings and overrides
            | '\u{2066}'..='\u{2069}' // isolates
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capture_collects_this_threads_lines_only_while_active() {
        log("before");
        let ((), lines) = capture(|| {
            log("one");
            std::thread::spawn(|| log("other thread")).join().unwrap();
        });
        assert_eq!(lines, ["one"]);
        log("after");
        assert_eq!(capture(|| ()).1, Vec::<String>::new());
    }

    #[test]
    fn untrusted_text_is_escaped_and_cut() {
        assert_eq!(untrusted("a\nb\u{1b}[2Jc"), "a\\nb\\u{1b}[2Jc");
        assert_eq!(untrusted("plain é"), "plain é");
        assert_eq!(untrusted("a\u{2028}b\u{202E}c"), "a\\u{2028}b\\u{202e}c");
        let long = untrusted(&"x".repeat(10_000));
        assert!(long.len() <= MAX_UNTRUSTED_BYTES + '…'.len_utf8());
        assert!(long.ends_with('…'));
    }
}
