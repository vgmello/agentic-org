use org_proto::msg::MessageHeader;

use crate::parse_utc;

/// ADR-0003 §3.3.2: whether a message with a deadline (an ask) is past it at `now_ms`. It is
/// live while `now < expires_at + skew_bound`, where `skew_bound = min(ttl_ms / 4, 2000 ms)`
/// tolerates a little clock drift between offices. A message with no `expires_at` never
/// passes its deadline; one whose `expires_at` can't be read has (never act on a request
/// whose deadline is unknown). It reads only the deadline: who the recipient is, the state
/// and `expired_waiting_person` are the caller's business.
pub fn past_deadline(header: &MessageHeader, now_ms: u64) -> bool {
    let Some(expires_at) = header.expires_at.as_deref() else {
        return false;
    };
    let Some(deadline) = parse_utc(expires_at) else {
        return true;
    };
    let skew_bound = (header.ttl_ms.unwrap_or(0) / 4).min(2000);
    now_ms >= deadline.saturating_add(skew_bound)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::format_utc;
    use org_proto::msg::{MessageLifecycleState, MessagePriority};

    fn header(expires_at: Option<String>, ttl_ms: Option<u64>) -> MessageHeader {
        MessageHeader {
            id: "01JCMSG0000000000000000001".into(),
            thread_id: "01JCMSG0000000000000000001".into(),
            reply_to: None,
            from_member_id: "a".into(),
            from_display_name: "A".into(),
            to_member_id: "b".into(),
            to_display_name: "B".into(),
            kind: "request".into(),
            priority: MessagePriority::Urgent,
            urgent: true,
            subject: None,
            created_at: "2026-11-08T12:00:00.000Z".into(),
            expires_at,
            ttl_ms,
            body_bytes: 2,
            meta_bytes: 0,
            local_seq: 1,
            state: MessageLifecycleState::Unread,
            triage_gen: 0,
            snoozed_until: None,
            snooze_after_task: false,
            snooze_note: None,
            read_at: None,
            expired_waiting_person: false,
        }
    }

    #[test]
    fn deadline_plus_bounded_skew() {
        let deadline = 1_794_139_200_000;
        let at = |ttl| header(format_utc(deadline), ttl);
        // A 120 s ask: the skew bound caps at 2 s.
        assert!(!past_deadline(&at(Some(120_000)), deadline + 1_999));
        assert!(past_deadline(&at(Some(120_000)), deadline + 2_000));
        // A 4 s ask: a quarter of its ttl, 1 s.
        assert!(!past_deadline(&at(Some(4_000)), deadline + 999));
        assert!(past_deadline(&at(Some(4_000)), deadline + 1_000));
        // No ttl: no grace.
        assert!(past_deadline(&at(None), deadline));
        assert!(!past_deadline(&at(None), deadline - 1));
        // No deadline never passes; an unreadable one has.
        assert!(!past_deadline(&header(None, None), u64::MAX));
        assert!(past_deadline(&header(Some("soon".into()), Some(1_000)), 0));
    }
}
