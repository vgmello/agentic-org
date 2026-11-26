//! Relayed wake summary checks.

use super::inbound_message::{relayed_wake_summary, MAX_RELAYED_WAKE_SUMMARY_CHARS};

#[test]
fn relayed_wake_summary_preserves_multibyte_utf8_boundaries() {
    let short = "olá 🚀 mundo";
    assert_eq!(relayed_wake_summary(short, "msg-1"), short);

    let exact = "é".repeat(MAX_RELAYED_WAKE_SUMMARY_CHARS);
    assert_eq!(relayed_wake_summary(&exact, "msg-2"), exact);

    let over = "🚀".repeat(MAX_RELAYED_WAKE_SUMMARY_CHARS + 5);
    let truncated = relayed_wake_summary(&over, "msg-3");
    assert!(truncated.starts_with(&"🚀".repeat(MAX_RELAYED_WAKE_SUMMARY_CHARS)));
    assert!(truncated.contains("agent-os msg read --id msg-3"));
}
