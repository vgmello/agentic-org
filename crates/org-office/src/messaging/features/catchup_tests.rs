//! Unit tests for the catch-up ladder's pure helpers: snippets, watermarks and thread stats.

use org_proto::chat::SNIPPET_MAX_CHARS;
use org_proto::msg::{MessagePriority, Meta};
use org_store::testkit::id;

use super::*;
use crate::messaging::records::{MessageAttachmentRecord, SenderKind, SCHEMA_VERSION};

fn msg(n: u64, sender: u64, created_at: &str, root: Option<u64>) -> MessageRecord {
    let mid = id(n).as_str().to_owned();
    MessageRecord {
        schema_version: SCHEMA_VERSION,
        organization_id: id(1),
        id: mid.clone(),
        conversation_id: id(2),
        sender_kind: SenderKind::Employee,
        sender_employee_id: Some(id(sender)),
        sender_office_id: None,
        sender_display_name: format!("E{sender}"),
        kind: "msg".into(),
        priority: MessagePriority::Normal,
        subject: None,
        body_markdown: format!("body {n}"),
        metadata: Meta::new(),
        thread_root_id: root.map(|r| id(r).as_str().to_owned()),
        reply_to_message_id: root.map(|r| id(r).as_str().to_owned()),
        also_in_channel: false,
        created_at: created_at.into(),
        expires_at: None,
        ttl_ms: None,
        origin_office_id: id(3),
        origin_incarnation: "inc".into(),
        origin_sequence: n,
        org_epoch: 1,
        org_sequence: 1,
        content_sha256: String::new(),
        attachments: Vec::new(),
        mailbox_content_hash: None,
    }
}

fn marker(last: Option<u64>, read_at: Option<&str>) -> ConversationReadMarkerRecord {
    ConversationReadMarkerRecord {
        organization_id: id(1),
        conversation_id: id(2),
        viewer_employee_id: id(50),
        read_at: read_at.map(str::to_owned),
        last_read_message_id: last.map(|n| id(n).as_str().to_owned()),
    }
}

#[test]
fn snippet_is_one_line_and_never_longer_than_the_cap() {
    assert_eq!(
        snippet("  Hello\n\tworld \r\n again  "),
        "Hello world again"
    );
    assert_eq!(snippet(""), "");
    assert_eq!(snippet("\n\t "), "");
    let exact = "a".repeat(SNIPPET_MAX_CHARS);
    assert_eq!(
        snippet(&exact),
        exact,
        "a body of exactly the cap is not cut"
    );
    let long = "word ".repeat(100);
    let cut = snippet(&long);
    assert_eq!(cut.chars().count(), SNIPPET_MAX_CHARS);
    assert!(cut.ends_with('…'), "{cut}");
    assert!(!cut.contains("  ") && !cut.contains('\n'));
}

#[test]
fn snippet_cuts_multibyte_text_on_character_boundaries() {
    // 4-byte emoji, 3-byte CJK, 2-byte accented and combining text across the cut.
    for unit in ["🦀", "漢", "é", "e\u{301}", "👩‍💻"] {
        let body = format!("{}\n{}", unit.repeat(200), unit.repeat(5));
        let cut = snippet(&body);
        assert!(cut.chars().count() <= SNIPPET_MAX_CHARS, "{unit}: {cut}");
        assert!(cut.ends_with('…'), "{unit}");
        assert!(
            cut.len() > SNIPPET_MAX_CHARS,
            "{unit}: kept multibyte bytes"
        );
        assert!(std::str::from_utf8(cut.as_bytes()).is_ok());
    }
    // 158 characters, a space, then more: the cut lands after the space, which is trimmed.
    let mixed = format!("{} tail", "ß🦀".repeat(79));
    let cut = snippet(&mixed);
    assert_eq!(cut.chars().count(), SNIPPET_MAX_CHARS - 1);
    assert!(cut.starts_with("ß🦀ß🦀") && cut.ends_with("🦀…"), "{cut}");
}

#[test]
fn watermark_boundary_uses_the_marker_message_then_its_time() {
    let msgs = vec![
        msg(10, 60, "2026-11-21T12:00:00.000Z", None),
        msg(11, 61, "2026-11-21T12:01:00.000Z", None),
        msg(12, 60, "2026-11-21T12:02:00.000Z", None),
    ];
    assert_eq!(watermark_boundary(&msgs, None), 0);
    assert_eq!(watermark_boundary(&msgs, Some(&marker(Some(10), None))), 1);
    assert_eq!(watermark_boundary(&msgs, Some(&marker(Some(12), None))), 3);
    // A vanished anchor falls back to the marker's time; a time-only marker covers up to it.
    let gone = marker(Some(99), Some("2026-11-21T12:01:00.000Z"));
    assert_eq!(watermark_boundary(&msgs, Some(&gone)), 2);
    assert_eq!(watermark_boundary(&msgs, Some(&marker(None, None))), 0);
    let viewer = id(60);
    let new: Vec<&str> = new_messages(&msgs, 1, &viewer)
        .map(|m| m.id.as_str())
        .collect();
    assert_eq!(
        new,
        vec![id(11).as_str()],
        "the viewer's own posts are not new"
    );
    assert_eq!(new_messages(&msgs, 3, &viewer).count(), 0);
}

#[test]
fn thread_stats_count_replies_and_latest_reply_in_one_pass() {
    let mut also = msg(14, 61, "2026-11-21T12:04:00.000Z", Some(10));
    also.also_in_channel = true;
    let mut with_files = msg(15, 61, "2026-11-21T12:05:00.000Z", None);
    with_files.attachments.push(MessageAttachmentRecord {
        id: "a1".into(),
        content_ref: "blob".into(),
        media_type: "text/plain".into(),
        byte_count: 3,
        content_sha256: String::new(),
    });
    let msgs = vec![
        msg(10, 60, "2026-11-21T12:00:00.000Z", None),
        msg(11, 61, "2026-11-21T12:03:00.000Z", Some(10)),
        msg(12, 62, "2026-11-21T12:01:00.000Z", Some(10)),
        msg(13, 60, "2026-11-21T12:02:00.000Z", Some(13)),
        also,
        with_files,
    ];
    let stats = ThreadStats::of(&msgs);
    assert_eq!(
        stats.of_root(id(10).as_str()),
        (3, Some("2026-11-21T12:04:00.000Z".to_owned()))
    );
    assert_eq!(
        stats.of_root(id(13).as_str()),
        (0, None),
        "a self-rooted message"
    );
    assert_eq!(stats.of_root(id(11).as_str()), (0, None));
    let top: Vec<bool> = msgs.iter().map(is_top_level).collect();
    assert_eq!(top, vec![true, false, false, true, true, true]);
    assert_eq!(msgs[5].attachments.len(), 1);
}
