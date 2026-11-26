use super::*;

#[test]
fn plain_text_is_borrowed() {
    assert!(matches!(
        sanitize("plain é ✓ 名前"),
        Cow::Borrowed("plain é ✓ 名前")
    ));
    assert!(matches!(sanitize(""), Cow::Borrowed("")));
}

#[test]
fn escape_sequences_and_controls_become_visible() {
    assert_eq!(sanitize("\x1b[31mred\x1b[0m"), "\\u{1b}[31mred\\u{1b}[0m");
    assert_eq!(sanitize("a\u{9b}31mb"), "a\\u{9b}31mb", "8-bit CSI");
    assert_eq!(sanitize("del\u{7f}"), "del\\u{7f}");
    assert_eq!(sanitize("a\tb\nc\rd"), "a\\u{9}b\\u{a}c\\u{d}d");
    assert_eq!(sanitize("\0"), "\\u{0}");
    assert_eq!(sanitize("\u{85}"), "\\u{85}", "C1 next line");
}

#[test]
fn bidi_and_separators_become_visible() {
    assert_eq!(sanitize("abc\u{202e}fed"), "abc\\u{202e}fed");
    assert_eq!(sanitize("a\u{2028}b\u{2029}c"), "a\\u{2028}b\\u{2029}c");
    for c in [
        '\u{061C}', '\u{200E}', '\u{200F}', '\u{202A}', '\u{202B}', '\u{202C}', '\u{202D}',
        '\u{2066}', '\u{2067}', '\u{2068}', '\u{2069}',
    ] {
        let escaped = sanitize(&c.to_string()).into_owned();
        assert!(!escaped.contains(c), "{escaped}");
        assert!(escaped.starts_with("\\u{"), "{escaped}");
    }
    // Neighbours of the escaped ranges are ordinary text.
    assert_eq!(
        sanitize("\u{2027}\u{202F}\u{2065}"),
        "\u{2027}\u{202F}\u{2065}"
    );
}

#[test]
fn json_controls_are_escaped_and_round_trip() {
    let value = serde_json::json!({
        "name": "a\u{7f}b\u{9b}31m\u{85}c\u{2028}d\u{2029}e",
        "plain": "é ✓",
        "ctrl": "\x1b[2J\n",
    });
    let raw = serde_json::to_string(&value).unwrap();
    assert!(raw.contains('\u{9b}'), "serde_json leaves C1 raw: {raw}");
    let escaped = escape_json_controls(raw);
    assert!(!escaped.contains(needs_json_escape), "{escaped}");
    assert!(escaped.contains("\\u007f") && escaped.contains("\\u009b"));
    assert!(escaped.contains("\\u2028") && escaped.contains("\\u2029"));
    let back: serde_json::Value = serde_json::from_str(&escaped).unwrap();
    assert_eq!(back, value);
}

#[test]
fn json_without_controls_is_unchanged() {
    let raw = String::from("{\"a\":\"é\"}");
    let pointer = raw.as_ptr();
    let same = escape_json_controls(raw);
    assert_eq!(same, "{\"a\":\"é\"}");
    assert_eq!(same.as_ptr(), pointer, "no copy");
}
