//! Relayed wake summary and wake-kind checks.

use org_proto::chat::{ConversationKind, DeliveredBecause};

use super::inbound_message::{
    relayed_wake_kind_for_message, relayed_wake_summary, MAX_RELAYED_WAKE_SUMMARY_CHARS,
};

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

/// An `@mention` inside a DM or group DM relays as a direct message, never a mention, so the
/// Chief of Staff never runs a private DM in her shared wake partition (`TASK-80.1`); in a
/// channel a mention stays a mention.
#[test]
fn a_mention_inside_a_dm_relays_as_a_direct_message() {
    for kind in [ConversationKind::Dm, ConversationKind::GroupDm] {
        for (because, mentioned) in [
            (DeliveredBecause::Mention, true),
            (DeliveredBecause::Mention, false),
            (DeliveredBecause::Addressed, true),
        ] {
            assert_eq!(
                relayed_wake_kind_for_message(kind, "msg", "tess", because, mentioned, None),
                Some("direct_message"),
                "{kind:?} {because:?} {mentioned}"
            );
        }
    }
    assert_eq!(
        relayed_wake_kind_for_message(
            ConversationKind::Channel,
            "msg",
            "tess",
            DeliveredBecause::Mention,
            true,
            None
        ),
        Some("mention")
    );
}
