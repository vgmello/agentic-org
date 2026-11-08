//! `org-msg`: Versioned envelope contract and Maildir-style spool operations for `org-agent`
//! and `org-relay`.
//!
//! Coupling metrics (per `msg/architecture-rules.md`):
//! - Efferent coupling (`Ce`): 2 (`serde`, `serde_json`)
//! - Afferent coupling (`Ca`): 2 (`org-agent`, `org-relay`)
//! - Instability (`I`): 0.10–0.20 (Stable foundational contract)

pub mod envelope;
pub mod spool;
pub mod time;
pub mod ulid;

pub use envelope::{
    json_quote, validate_agent_id, Envelope, MessageKind, MetaValue, ENVELOPE_VERSION,
    MAX_BODY_BYTES, MAX_ENVELOPE_BYTES,
};
pub use spool::{
    is_interrupted, poll_unix_listener, request_interrupt, DeliverOutcome, DeliverSummary, Spool,
    SpoolError, SpoolStatus, WakeListener,
};
pub use time::{
    format_epoch_ms_iso8601, format_iso8601_utc, is_valid_iso8601_utc, parse_duration_arg,
};
pub use ulid::{encode_ulid, generate_ulid, is_valid_ulid};

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    use std::fs;
    use std::os::unix::net::UnixListener;
    use std::path::PathBuf;
    use std::process;
    use std::time::Duration;

    fn temp_spool(name: &str) -> (PathBuf, Spool) {
        let dir = PathBuf::from("/tmp").join(format!(
            "om_{}_{}_{}",
            name,
            process::id(),
            &generate_ulid()[20..]
        ));
        let _ = fs::remove_dir_all(&dir);
        let spool = Spool::open(&dir).expect("open spool");
        (dir, spool)
    }

    #[test]
    fn ulids_are_monotonic_and_valid() {
        let mut prev = generate_ulid();
        assert!(is_valid_ulid(&prev));
        for _ in 0..500 {
            let next = generate_ulid();
            assert!(is_valid_ulid(&next));
            assert!(
                next > prev,
                "expected monotonic ULID: {next} should be > {prev}"
            );
            prev = next;
        }
    }

    #[test]
    fn malformed_input_corpus_is_rejected_and_dead_lettered_without_panic() {
        let (dir, spool) = temp_spool("malformed");
        spool.register("agy-senior-dev").unwrap();

        // 1. Truncated input
        assert!(Envelope::from_json_bytes(br#"{"v":1,"id":"01J"#).is_err());

        // 2. Invalid UTF-8
        assert!(Envelope::from_json_bytes(&[0xff, 0xfe, 0xfd, 0x00]).is_err());

        // 3. Lone surrogate escape
        let lone_surrogate = br#"{"v":1,"id":"01JC0000000000000000000000","from":"ada","to":"agy-senior-dev","kind":"msg","reply_to":null,"created_at":"2026-11-08T00:00:00.000Z","expires_at":null,"subject":null,"body":"\uD800","meta":{}}"#;
        assert!(Envelope::from_json_bytes(lone_surrogate).is_err());

        // 4. Nesting depth > 128 in meta
        let mut deep_json = String::from(
            r#"{"v":1,"id":"01JC0000000000000000000000","from":"ada","to":"agy-senior-dev","kind":"msg","reply_to":null,"created_at":"2026-11-08T00:00:00.000Z","expires_at":null,"subject":null,"body":"hi","meta":{"nested":"#,
        );
        for _ in 0..150 {
            deep_json.push('[');
        }
        for _ in 0..150 {
            deep_json.push(']');
        }
        deep_json.push_str("}}");
        assert!(Envelope::from_json_bytes(deep_json.as_bytes()).is_err());

        // 5. Body > 256 KB
        let huge_body = "x".repeat(MAX_BODY_BYTES + 1);
        assert!(Envelope::new_msg("ada", "agy-senior-dev", None, huge_body, BTreeMap::new()).is_err());

        // 6. Unknown fields are ignored
        let with_unknown = br#"{"v":1,"id":"01JC0000000000000000000000","from":"ada","to":"agy-senior-dev","kind":"msg","reply_to":null,"created_at":"2026-11-08T00:00:00.000Z","expires_at":null,"subject":"hi","body":"ok","meta":{},"future_field":{"a":1},"extra_flag":true}"#;
        let parsed = Envelope::from_json_bytes(with_unknown).expect("unknown fields ignored");
        assert_eq!(parsed.body, "ok");

        // 7. Wrong version `v` rejected with clear error
        let wrong_v = br#"{"v":2,"id":"01JC0000000000000000000000","from":"ada","to":"agy-senior-dev","kind":"msg","reply_to":null,"created_at":"2026-11-08T00:00:00.000Z","expires_at":null,"subject":null,"body":"hi","meta":{}}"#;
        let err = Envelope::from_json_bytes(wrong_v).unwrap_err();
        assert!(err.contains("unsupported envelope version v=2"), "err={err}");

        // 8. Bad file in outbox/new goes to dead/ with a reason file, never panics
        let bad_outbox = dir.join("spool/outbox/new/BADFILE01.json");
        fs::write(&bad_outbox, b"{not valid json").unwrap();
        let outcome = spool.deliver_one(&bad_outbox).unwrap();
        assert!(matches!(outcome, DeliverOutcome::Dead { .. }));
        assert!(dir.join("spool/dead/BADFILE01.json").is_file());
        let reason = fs::read_to_string(dir.join("spool/dead/BADFILE01.reason")).unwrap();
        assert!(reason.contains("invalid_envelope"), "reason={reason}");

        // 9. Non-ISO-8601 created_at or expires_at rejected
        let bad_ts = br#"{"v":1,"id":"01JC0000000000000000000000","from":"ada","to":"agy-senior-dev","kind":"msg","reply_to":null,"created_at":"not-a-timestamp","expires_at":null,"subject":null,"body":"hi","meta":{}}"#;
        assert!(Envelope::from_json_bytes(bad_ts).is_err());
        let bad_exp = br#"{"v":1,"id":"01JC0000000000000000000000","from":"ada","to":"agy-senior-dev","kind":"request","reply_to":null,"created_at":"2026-11-08T00:00:00.000Z","expires_at":"9999-99-99","subject":null,"body":"hi","meta":{}}"#;
        assert!(Envelope::from_json_bytes(bad_exp).is_err());

        // 10. Filename stem mismatch in outbox/new is dead-lettered
        let env_mismatch =
            Envelope::new_msg("ada", "agy-senior-dev", None, "mismatch", BTreeMap::new()).unwrap();
        let mismatch_outbox = dir.join("spool/outbox/new/01JC0000000000000000000000.json");
        fs::write(&mismatch_outbox, env_mismatch.to_json_bytes().unwrap()).unwrap();
        let outcome_mismatch = spool.deliver_one(&mismatch_outbox).unwrap();
        assert!(matches!(outcome_mismatch, DeliverOutcome::Dead { .. }));

        // 11. Corrupt file in inbox/<agent>/new is quarantined by claim_next so valid message is claimed
        let corrupt_inbox = dir.join("spool/inbox/agy-senior-dev/new/00000000000000000000000000.json");
        fs::write(&corrupt_inbox, b"{corrupt inbox json").unwrap();
        let valid_msg =
            Envelope::new_msg("ada", "agy-senior-dev", None, "valid after corrupt", BTreeMap::new())
                .unwrap();
        spool.enqueue(&valid_msg).unwrap();
        spool.deliver_all().unwrap();
        let next = spool.claim_next("agy-senior-dev").unwrap().expect("should claim valid msg");
        assert_eq!(next.id, valid_msg.id);
        assert!(!corrupt_inbox.exists(), "corrupt file should be quarantined out of inbox/new");

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn expired_and_deregistered_messages_go_to_dead() {
        let (dir, spool) = temp_spool("deadletter");
        spool.register("temp-agent").unwrap();

        // Message 1: recipient deregistered after enqueue
        let m1 = Envelope::new_msg("ada", "temp-agent", None, "hello", BTreeMap::new()).unwrap();
        spool.enqueue(&m1).unwrap();
        spool.deregister("temp-agent").unwrap();

        let summary = spool.deliver_all().unwrap();
        assert_eq!(summary.dead, 1);
        let reason1 = fs::read_to_string(dir.join(format!("spool/dead/{}.reason", m1.id))).unwrap();
        assert!(reason1.contains("unknown_recipient: temp-agent"));

        // Message 2: expires_at in the past
        spool.register("agy-senior-dev").unwrap();
        let mut m2 =
            Envelope::new_msg("ada", "agy-senior-dev", None, "expired req", BTreeMap::new())
                .unwrap();
        m2.kind = MessageKind::Request;
        m2.expires_at = Some("2000-01-01T00:00:00.000Z".into());
        spool.enqueue(&m2).unwrap();

        let summary2 = spool.deliver_all().unwrap();
        assert_eq!(summary2.dead, 1);
        let reason2 = fs::read_to_string(dir.join(format!("spool/dead/{}.reason", m2.id))).unwrap();
        assert!(reason2.contains("expired"));

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn stale_wake_socket_is_unlinked_and_wait_for_reply_leaves_others_unclaimed() {
        let (dir, spool) = temp_spool("wakesock");
        spool.register("ada").unwrap();
        spool.register("agy").unwrap();

        // Create a stale socket (bound, then closed without unlinking file)
        let stale_path = dir.join("spool/inbox/ada/wake/999999-STALE.sock");
        {
            let _l = UnixListener::bind(&stale_path).unwrap();
        }
        assert!(stale_path.exists());

        // Also send an unrelated message to ada + the matching reply
        let req = Envelope::new_request(
            "ada",
            "agy",
            Some("ping".into()),
            "question?",
            BTreeMap::new(),
            Duration::from_secs(30),
        )
        .unwrap();
        let unrelated =
            Envelope::new_msg("agy", "ada", Some("other".into()), "keep me unread", BTreeMap::new())
                .unwrap();
        let reply = req
            .new_reply("agy", None, "here is the answer", BTreeMap::new())
            .unwrap();

        let listener = spool.bind_wake_listener("ada").unwrap();
        spool.enqueue(&unrelated).unwrap();
        spool.enqueue(&reply).unwrap();
        let summary = spool.deliver_all().unwrap();
        assert_eq!(summary.delivered, 2);

        // Stale socket should have been unlinked by deliver_all -> notify_agent_wake_sockets
        assert!(!stale_path.exists(), "stale wake socket should be unlinked");

        // wait_for_reply claims ONLY `reply`, leaving `unrelated` in `new/`
        let claimed_reply = spool
            .wait_for_reply_with_listener("ada", &req.id, &listener, Duration::from_secs(1))
            .unwrap();
        assert_eq!(claimed_reply.id, reply.id);
        assert_eq!(claimed_reply.body, "here is the answer");

        let still_unread = spool.list_inbox("ada", false).unwrap();
        assert_eq!(still_unread.len(), 1);
        assert_eq!(still_unread[0].id, unrelated.id);

        let _ = fs::remove_dir_all(&dir);
    }
}
