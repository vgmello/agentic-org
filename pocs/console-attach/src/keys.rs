use std::time::Duration;

/// Delay after ESC so TUI programs like vi/vim do not confuse `ESC + char` with an Alt/Meta key.
pub const ESC_SETTLE_DELAY: Duration = Duration::from_millis(50);

/// Small delay between sequential named keys.
pub const KEY_STEP_DELAY: Duration = Duration::from_millis(20);

/// An atomic action to apply to the PTY input stream.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InputAction {
    SendBytes(Vec<u8>),
    Pause(Duration),
}

/// Translates a named key (case-insensitive) into its terminal byte/escape sequence.
pub fn parse_key(name: &str) -> Result<Vec<u8>, String> {
    let lower = name.trim().to_ascii_lowercase();
    match lower.as_str() {
        "esc" | "escape" => Ok(vec![0x1b]),
        "enter" | "return" | "cr" => Ok(vec![b'\r']),
        "lf" | "newline" => Ok(vec![b'\n']),
        "tab" => Ok(vec![b'\t']),
        "backspace" | "bs" => Ok(vec![0x7f]),
        "space" => Ok(vec![b' ']),
        "up" => Ok(b"\x1b[A".to_vec()),
        "down" => Ok(b"\x1b[B".to_vec()),
        "right" => Ok(b"\x1b[C".to_vec()),
        "left" => Ok(b"\x1b[D".to_vec()),
        "home" => Ok(b"\x1b[H".to_vec()),
        "end" => Ok(b"\x1b[F".to_vec()),
        "delete" | "del" => Ok(b"\x1b[3~".to_vec()),
        "pgup" | "pageup" => Ok(b"\x1b[5~".to_vec()),
        "pgdn" | "pagedown" => Ok(b"\x1b[6~".to_vec()),
        _ => {
            if let Some(rest) = lower
                .strip_prefix("ctrl-")
                .or_else(|| lower.strip_prefix("c-"))
            {
                let bytes = rest.as_bytes();
                if bytes.len() == 1 && bytes[0].is_ascii_lowercase() {
                    return Ok(vec![bytes[0] - b'a' + 1]);
                }
            }
            Err(format!("unknown key name: '{name}'"))
        }
    }
}

/// Translates a high-level instruction string into a sequence of PTY input actions.
///
/// Supported commands:
/// - `type <text>`           : send literal text bytes
/// - `line <text>`           : send literal text followed by Enter (`\r`)
/// - `key <name>`            : send a named key (e.g. `esc`, `enter`, `ctrl-c`, `up`)
/// - `keys <k1> <k2> ...`    : send multiple named keys with short pacing delays
/// - `raw <hex>`             : send raw hex-encoded bytes (spaces ignored)
/// - `wait <ms>`             : sleep for `<ms>` milliseconds
/// - `vi:insert <text>`      : ESC -> i -> `<text>` -> ESC
/// - `vi:append-line <text>` : ESC -> o -> `<text>` -> ESC
/// - `vi:cmd <ex-cmd>`       : ESC -> `:<ex-cmd>\r`
/// - `vi:save`               : ESC -> `:w\r`
/// - `vi:force-save`         : ESC -> `:w!\r`
/// - `vi:save-quit`          : ESC -> `:wq\r`
/// - `vi:force-save-quit`    : ESC -> `:wq!\r`
/// - `vi:quit`               : ESC -> `:q!\r`
pub fn translate_command(input: &str) -> Result<Vec<InputAction>, String> {
    let trimmed = input.trim_start();
    if trimmed.is_empty() {
        return Err("empty command".to_string());
    }

    let (verb, rest) = match trimmed.find(char::is_whitespace) {
        Some(idx) => {
            let v = &trimmed[..idx];
            // Strip exactly one separating space so `type  leading-space` can preserve extra spaces.
            let remainder = &trimmed[idx..];
            let r = remainder
                .strip_prefix(' ')
                .unwrap_or(remainder.trim_start());
            (v, r)
        }
        None => (trimmed, ""),
    };

    match verb {
        "type" => {
            if rest.is_empty() {
                return Err("usage: type <text>".to_string());
            }
            Ok(vec![InputAction::SendBytes(rest.as_bytes().to_vec())])
        }
        "line" => {
            let mut bytes = rest.as_bytes().to_vec();
            bytes.push(b'\r');
            Ok(vec![InputAction::SendBytes(bytes)])
        }
        "key" => {
            if rest.trim().is_empty() {
                return Err("usage: key <name>".to_string());
            }
            Ok(vec![InputAction::SendBytes(parse_key(rest.trim())?)])
        }
        "keys" => {
            let names: Vec<&str> = rest.split_whitespace().collect();
            if names.is_empty() {
                return Err("usage: keys <k1> <k2> ...".to_string());
            }
            let mut actions = Vec::with_capacity(names.len() * 2);
            for (idx, name) in names.iter().enumerate() {
                if idx > 0 {
                    actions.push(InputAction::Pause(KEY_STEP_DELAY));
                }
                actions.push(InputAction::SendBytes(parse_key(name)?));
            }
            Ok(actions)
        }
        "raw" => {
            let bytes = decode_hex(rest)?;
            Ok(vec![InputAction::SendBytes(bytes)])
        }
        "wait" => {
            let ms: u64 = rest
                .trim()
                .parse()
                .map_err(|_| format!("invalid wait duration in ms: '{}'", rest.trim()))?;
            Ok(vec![InputAction::Pause(Duration::from_millis(ms))])
        }
        "vi:insert" => Ok(vi_insert_actions(rest)),
        "vi:append-line" => Ok(vi_append_line_actions(rest)),
        "vi:cmd" => {
            let cmd = rest.trim();
            if cmd.is_empty() {
                return Err("usage: vi:cmd <ex-command>".to_string());
            }
            Ok(vi_ex_command(cmd))
        }
        "vi:save" => Ok(vi_ex_command("w")),
        "vi:force-save" => Ok(vi_ex_command("w!")),
        "vi:save-quit" => Ok(vi_ex_command("wq")),
        "vi:force-save-quit" => Ok(vi_ex_command("wq!")),
        "vi:quit" => Ok(vi_ex_command("q!")),
        other => Err(format!(
            "unknown command '{other}'. Try: type, line, key, keys, raw, wait, vi:insert, vi:append-line, vi:cmd, vi:save, vi:force-save, vi:save-quit, vi:quit"
        )),
    }
}

fn vi_ex_command(ex_cmd: &str) -> Vec<InputAction> {
    let mut seq = Vec::with_capacity(ex_cmd.len() + 2);
    seq.push(b':');
    seq.extend_from_slice(ex_cmd.as_bytes());
    seq.push(b'\r');
    vec![
        InputAction::SendBytes(vec![0x1b]),
        InputAction::Pause(ESC_SETTLE_DELAY),
        InputAction::SendBytes(seq),
    ]
}

fn vi_insert_actions(text: &str) -> Vec<InputAction> {
    vec![
        InputAction::SendBytes(vec![0x1b]),
        InputAction::Pause(ESC_SETTLE_DELAY),
        InputAction::SendBytes(b"i".to_vec()),
        InputAction::Pause(KEY_STEP_DELAY),
        InputAction::SendBytes(text.as_bytes().to_vec()),
        InputAction::Pause(KEY_STEP_DELAY),
        InputAction::SendBytes(vec![0x1b]),
        InputAction::Pause(ESC_SETTLE_DELAY),
    ]
}

fn vi_append_line_actions(text: &str) -> Vec<InputAction> {
    vec![
        InputAction::SendBytes(vec![0x1b]),
        InputAction::Pause(ESC_SETTLE_DELAY),
        InputAction::SendBytes(b"o".to_vec()),
        InputAction::Pause(KEY_STEP_DELAY),
        InputAction::SendBytes(text.as_bytes().to_vec()),
        InputAction::Pause(KEY_STEP_DELAY),
        InputAction::SendBytes(vec![0x1b]),
        InputAction::Pause(ESC_SETTLE_DELAY),
    ]
}

fn decode_hex(input: &str) -> Result<Vec<u8>, String> {
    let clean: String = input.chars().filter(|c| !c.is_whitespace()).collect();
    if clean.is_empty() || !clean.len().is_multiple_of(2) {
        return Err("raw hex must contain an even number of hex digits".to_string());
    }
    let mut out = Vec::with_capacity(clean.len() / 2);
    for pair in clean.as_bytes().as_chunks::<2>().0 {
        let s = std::str::from_utf8(pair).map_err(|_| "invalid UTF-8 in hex".to_string())?;
        let byte = u8::from_str_radix(s, 16).map_err(|_| format!("invalid hex byte: '{s}'"))?;
        out.push(byte);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn translates_named_keys() {
        assert_eq!(parse_key("esc").unwrap(), vec![0x1b]);
        assert_eq!(parse_key("ENTER").unwrap(), vec![b'\r']);
        assert_eq!(parse_key("ctrl-c").unwrap(), vec![0x03]);
        assert_eq!(parse_key("ctrl-z").unwrap(), vec![0x1a]);
        assert_eq!(parse_key("up").unwrap(), b"\x1b[A");
    }

    #[test]
    fn translates_vi_save_to_escape_colon_w_enter() {
        let actions = translate_command("vi:save").unwrap();
        assert_eq!(
            actions,
            vec![
                InputAction::SendBytes(vec![0x1b]),
                InputAction::Pause(ESC_SETTLE_DELAY),
                InputAction::SendBytes(b":w\r".to_vec()),
            ]
        );
    }

    #[test]
    fn translates_vi_force_save() {
        let actions = translate_command("vi:force-save").unwrap();
        assert_eq!(actions[0], InputAction::SendBytes(vec![0x1b]));
        assert_eq!(actions[2], InputAction::SendBytes(b":w!\r".to_vec()));
    }

    #[test]
    fn translates_raw_hex() {
        let actions = translate_command("raw 1b 3a 77 0d").unwrap();
        assert_eq!(actions, vec![InputAction::SendBytes(b"\x1b:w\r".to_vec())]);
    }
}
