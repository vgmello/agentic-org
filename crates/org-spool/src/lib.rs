//! `org-spool`: Versioned envelope contract, per-agent `flock` metadata (`profile.json`,
//! `status.json`, `triage.json`), and Maildir-style spool operations for `org-agent`.
//!
//! Coupling metrics (per `docs/architecture-rules.md`):
//! - Efferent coupling (`Ce`): 3 (`serde`, `serde_json`, `libc`)
//! - Instability (`I`): ~0.20 (Stable platform primitive)

pub mod envelope;
pub mod member;
pub mod messaging;
pub mod meta;
pub mod office_ingest;
pub mod profile_edit;
pub mod relay;
mod safe_read;
pub mod spool;
pub mod time;
pub mod ulid;

pub use envelope::{
    json_quote, validate_agent_id, Envelope, MessageKind, MessagePriority, MetaValue,
    ENVELOPE_VERSION, MAX_BODY_BYTES, MAX_ENVELOPE_BYTES, MAX_SUBJECT_BYTES,
};
pub use member::{
    canonical_member_id, check_not_reserved, is_owner, is_reserved_name, member_id_arg,
    resolve_caller, resolve_member,
};
pub use messaging::{InboxQuery, Outgoing, INBOX_MAX_LIMIT, INBOX_SUMMARY_DEFAULT_LIMIT};
pub use meta::{
    parse_snooze_until, sanitize_inline_text, DeferOutcome, DirectoryCard, DoneOutcome, InboxItem,
    MemberProfile, MemberStatus, MessageTriageEntry, NotifyItem, NotifyOptions, NotifyReport,
    RefEntry, TeamEntry, TriageState, NOTIFY_MAX_ITEMS, OWNER_ID,
};
pub use profile_edit::{authorize_profile_edit, ProfileUpdate};
pub use relay::RelayOutcome;
pub use spool::{
    is_interrupted, poll_unix_listener, poll_unix_listener_events, request_interrupt,
    DeliverOutcome, DeliverSummary, Spool, SpoolError, SpoolStatus, WakeEvents, WakeListener,
};
pub use time::{
    check_wait_timeout, format_epoch_ms_iso8601, format_iso8601_utc, is_valid_iso8601_utc,
    parse_duration_arg, MAX_WAIT_TIMEOUT,
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
    use std::sync::Arc;
    use std::thread;
    use std::time::{Duration, SystemTime};

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
    fn ulids_stay_monotonic_while_other_threads_generate() {
        // Regression: the clock used to be read before the lock, so a thread holding an older
        // timestamp could reset the state and the next id in the same ms could sort lower.
        let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let noise: Vec<_> = (0..4)
            .map(|_| {
                let stop = Arc::clone(&stop);
                thread::spawn(move || {
                    while !stop.load(std::sync::atomic::Ordering::Relaxed) {
                        let _ = generate_ulid();
                    }
                })
            })
            .collect();
        let mut prev = generate_ulid();
        for _ in 0..20_000 {
            let next = generate_ulid();
            assert!(next > prev, "{next} should be > {prev}");
            prev = next;
        }
        stop.store(true, std::sync::atomic::Ordering::Relaxed);
        noise.into_iter().for_each(|h| h.join().unwrap());
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
        assert!(
            Envelope::new_msg("ada", "agy-senior-dev", None, huge_body, BTreeMap::new()).is_err()
        );

        // 6. Unknown fields are ignored
        let with_unknown = br#"{"v":1,"id":"01JC0000000000000000000000","from":"ada","to":"agy-senior-dev","kind":"msg","reply_to":null,"created_at":"2026-11-08T00:00:00.000Z","expires_at":null,"subject":"hi","body":"ok","meta":{},"future_field":{"a":1},"extra_flag":true}"#;
        let parsed = Envelope::from_json_bytes(with_unknown).expect("unknown fields ignored");
        assert_eq!(parsed.body, "ok");

        // 7. Wrong version `v` rejected with clear error
        let wrong_v = br#"{"v":2,"id":"01JC0000000000000000000000","from":"ada","to":"agy-senior-dev","kind":"msg","reply_to":null,"created_at":"2026-11-08T00:00:00.000Z","expires_at":null,"subject":null,"body":"hi","meta":{}}"#;
        let err = Envelope::from_json_bytes(wrong_v).unwrap_err();
        assert!(
            err.contains("unsupported envelope version v=2"),
            "err={err}"
        );

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
        let corrupt_inbox =
            dir.join("spool/inbox/agy-senior-dev/new/00000000000000000000000000.json");
        fs::write(&corrupt_inbox, b"{corrupt inbox json").unwrap();
        let valid_msg = Envelope::new_msg(
            "ada",
            "agy-senior-dev",
            None,
            "valid after corrupt",
            BTreeMap::new(),
        )
        .unwrap();
        spool.enqueue(&valid_msg).unwrap();
        spool.deliver_all().unwrap();
        let next = spool
            .claim_next("agy-senior-dev")
            .unwrap()
            .expect("should claim valid msg");
        assert_eq!(next.id, valid_msg.id);
        assert!(
            !corrupt_inbox.exists(),
            "corrupt file should be quarantined out of inbox/new"
        );

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn expired_and_deregistered_messages_go_to_dead() {
        let (dir, spool) = temp_spool("deadletter");
        spool.register("temp-agent").unwrap();

        let m1 = Envelope::new_msg("ada", "temp-agent", None, "hello", BTreeMap::new()).unwrap();
        spool.enqueue(&m1).unwrap();
        spool.deregister("temp-agent").unwrap();

        let summary = spool.deliver_all().unwrap();
        assert_eq!(summary.dead, 1);
        let reason1 = fs::read_to_string(dir.join(format!("spool/dead/{}.reason", m1.id))).unwrap();
        assert!(reason1.contains("unknown_recipient: temp-agent"));

        spool.register("agy-senior-dev").unwrap();
        let mut m2 = Envelope::new_msg(
            "ada",
            "agy-senior-dev",
            None,
            "expired req",
            BTreeMap::new(),
        )
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

        let stale_path = dir.join("spool/inbox/ada/wake/999999-STALE.sock");
        {
            let _l = UnixListener::bind(&stale_path).unwrap();
        }
        assert!(stale_path.exists());

        let req = Envelope::new_request(
            "ada",
            "agy",
            Some("ping".into()),
            "question?",
            BTreeMap::new(),
            Duration::from_secs(30),
        )
        .unwrap();
        let unrelated = Envelope::new_msg(
            "agy",
            "ada",
            Some("other".into()),
            "keep me unread",
            BTreeMap::new(),
        )
        .unwrap();
        let reply = req
            .new_reply("agy", None, "here is the answer", BTreeMap::new())
            .unwrap();

        let listener = spool.bind_wake_listener("ada").unwrap();
        spool.enqueue(&unrelated).unwrap();
        spool.enqueue(&reply).unwrap();
        let summary = spool.deliver_all().unwrap();
        assert_eq!(summary.delivered, 2);

        assert!(!stale_path.exists(), "stale wake socket should be unlinked");

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

    #[test]
    fn priority_u_n_wake_tags_and_wait_urgent_only() {
        let (dir, spool) = temp_spool("urgent_wake");
        spool.register("ada").unwrap();
        spool.register("agy").unwrap();

        let listener = spool.bind_wake_listener("agy").unwrap();

        // 1. Normal message writes 'N' tag and does NOT satisfy wait_for_inbox_filtered(..., urgent_only=true)
        let normal_msg = Envelope::new_msg(
            "ada",
            "agy",
            Some("fyi".into()),
            "normal body",
            BTreeMap::new(),
        )
        .unwrap();
        assert!(!normal_msg.is_urgent());
        spool.enqueue(&normal_msg).unwrap();
        let out1 = spool
            .deliver_one(&dir.join(format!("spool/outbox/new/{}.json", normal_msg.id)))
            .unwrap();
        assert_eq!(
            out1,
            DeliverOutcome::Delivered {
                id: normal_msg.id.clone(),
                to: "agy".into(),
                urgent: false
            }
        );
        let events1 =
            listener.wait_slice_with_cap(Duration::from_millis(50), Duration::from_millis(50));
        assert!(events1.any);
        assert!(
            !events1.urgent,
            "normal message must send 'N' wake tag, not 'U'"
        );

        let err = spool
            .wait_for_inbox_filtered("agy", None, true, Duration::from_millis(80))
            .unwrap_err();
        assert!(matches!(err, SpoolError::Timeout(_)));

        // 2. High-priority message writes 'U' tag and wakes wait_for_inbox_filtered(..., urgent_only=true)
        let mut high_msg = Envelope::new_msg(
            "ada",
            "agy",
            Some("urgent review".into()),
            "please look now",
            BTreeMap::new(),
        )
        .unwrap();
        high_msg.set_priority(MessagePriority::High);
        assert!(high_msg.is_urgent());
        spool.enqueue(&high_msg).unwrap();
        let out2 = spool
            .deliver_one(&dir.join(format!("spool/outbox/new/{}.json", high_msg.id)))
            .unwrap();
        assert_eq!(
            out2,
            DeliverOutcome::Delivered {
                id: high_msg.id.clone(),
                to: "agy".into(),
                urgent: true
            }
        );
        let events2 =
            listener.wait_slice_with_cap(Duration::from_millis(50), Duration::from_millis(50));
        assert!(events2.any);
        assert!(
            events2.urgent,
            "high-priority message must send 'U' wake tag"
        );

        let woken = spool
            .wait_for_inbox_filtered("agy", None, true, Duration::from_millis(100))
            .unwrap();
        assert_eq!(woken.id, high_msg.id);

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn read_then_defer_restores_unread_and_resurfaces_with_incremented_triage_gen() {
        let (dir, spool) = temp_spool("read_defer");
        spool.register("ada").unwrap();
        spool.register("agy").unwrap();

        let msg = Envelope::new_msg(
            "ada",
            "agy",
            Some("Design review".into()),
            "Check section 3",
            BTreeMap::new(),
        )
        .unwrap();
        spool.enqueue(&msg).unwrap();
        spool.deliver_all().unwrap();

        // Agent reads the message during triage (`new/` -> `cur/`)
        let claimed = spool.claim("agy", &msg.id).unwrap();
        assert_eq!(claimed.id, msg.id);
        assert!(
            spool
                .list_inbox_items("agy", false, true, None)
                .unwrap()
                .is_empty(),
            "claimed message moved to cur/"
        );

        // Agent decides to snooze it until after-task (`cur/` -> `new/` and triage_gen = 1)
        let defer1 = spool
            .defer_message(
                "agy",
                &msg.id,
                "after-task",
                Some("finish current PR".into()),
            )
            .unwrap();
        assert_eq!(defer1.status, "snoozed");
        assert_eq!(defer1.triage_gen, 1);
        assert_eq!(defer1.snoozed_until, "after-task");

        // Hidden from default inbox and `read --next`, visible with `include_all = true`
        assert!(spool
            .list_inbox_items("agy", false, true, None)
            .unwrap()
            .is_empty());
        assert!(spool.claim_next("agy").unwrap().is_none());
        let all_items = spool.list_inbox_items("agy", true, true, None).unwrap();
        assert_eq!(all_items.len(), 1);
        assert_eq!(all_items[0].status, "snoozed");
        assert_eq!(all_items[0].triage_gen, 1);

        // Mid-task notify (after_task = false) does NOT release after-task snooze
        let mid_notify = spool.notify("agy", &NotifyOptions::default()).unwrap();
        assert_eq!(mid_notify.count, 0);

        // Natural break notify (after_task = true) releases it once, bumps triage_gen to 2, and reports reason="after_task"
        let break_opts = NotifyOptions {
            session: Some("agy:sess-1".into()),
            after_task: true,
            urgent_only: false,
            sweep_after: Duration::from_secs(300),
        };
        let break_notify = spool.notify("agy", &break_opts).unwrap();
        assert_eq!(break_notify.count, 1);
        assert_eq!(break_notify.items[0].id, msg.id);
        assert_eq!(break_notify.items[0].reason, "after_task");
        assert_eq!(break_notify.items[0].triage_gen, 2);

        // Calling notify again with the same session does NOT re-notify the same (id, triage_gen)
        let repeat_notify = spool.notify("agy", &break_opts).unwrap();
        assert_eq!(repeat_notify.count, 0);

        // Snoozing again for 10ms bumps triage_gen and allows it to resurface in the same session when due!
        let defer2 = spool.defer_message("agy", &msg.id, "10ms", None).unwrap();
        assert_eq!(defer2.triage_gen, 3);
        thread::sleep(Duration::from_millis(20));

        let due_notify = spool
            .notify(
                "agy",
                &NotifyOptions {
                    session: Some("agy:sess-1".into()),
                    after_task: false,
                    urgent_only: false,
                    sweep_after: Duration::from_secs(300),
                },
            )
            .unwrap();
        assert_eq!(due_notify.count, 1);
        assert_eq!(due_notify.items[0].reason, "due");
        assert_eq!(due_notify.items[0].triage_gen, 4);

        // Marking `done` moves it to `cur/` and it never resurfaces
        let done = spool
            .done_message("agy", &msg.id, Some("handled".into()))
            .unwrap();
        assert_eq!(done.status, "done");
        assert!(spool
            .list_inbox_items("agy", false, true, None)
            .unwrap()
            .is_empty());
        let after_done = spool.notify("agy", &break_opts).unwrap();
        assert_eq!(after_done.count, 0);

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn notify_sanitizes_subjects_omits_bodies_and_truncates_to_80_chars() {
        let (dir, spool) = temp_spool("notify_sanitize");
        spool.register("ada").unwrap();
        spool.register("agy").unwrap();
        // `set_profile` rejects control characters, so write the file as a hand edit (or an
        // older build) could have: notify must still sanitize what it reads.
        let profile = MemberProfile {
            id: "ada".into(),
            name: "Ada\nStaff\tLead".into(),
            kind: "agent".into(),
            owner: false,
            role_title: Some("Staff Engineer".into()),
            teams: vec![],
            reports_to: None,
            office: None,
        };
        assert!(spool.set_profile(&profile).is_err());
        fs::write(
            dir.join("spool/inbox/ada/profile.json"),
            serde_json::to_vec(&profile).unwrap(),
        )
        .unwrap();

        let long_subj = format!("Line1\nLine2\r\x07{}", "A".repeat(100));
        let secret_body = "TOP_SECRET_BODY_NEVER_IN_NOTIFY_OUTPUT";
        let req = Envelope::new_request(
            "ada",
            "agy",
            Some(long_subj),
            secret_body,
            BTreeMap::new(),
            Duration::from_secs(60),
        )
        .unwrap();
        spool.enqueue(&req).unwrap();
        spool.deliver_all().unwrap();

        let report = spool.notify("agy", &NotifyOptions::default()).unwrap();
        assert_eq!(report.count, 1);
        assert_eq!(report.urgent_count, 1);
        assert!(report.urgent);
        assert!(!report.text.contains(secret_body));
        assert!(!report.items[0].subject.contains('\n'));
        assert!(!report.items[0].subject.contains('\x07'));
        assert!(report.items[0].subject.chars().count() <= 80);
        assert_eq!(report.items[0].from_name, "Ada Staff Lead");
        assert!(report.text.contains("- [URGENT] Ada Staff Lead (ada):"));

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn directory_who_and_agent_status_with_ttl() {
        let (dir, spool) = temp_spool("who_status");
        spool
            .set_profile(&MemberProfile {
                id: "agy-senior-dev".into(),
                name: "Agy Senior Dev".into(),
                kind: "agent".into(),
                owner: false,
                role_title: Some("Senior Engineer".into()),
                teams: vec![TeamEntry {
                    id: "platform".into(),
                    name: "Platform".into(),
                    title_in_team: Some("Messaging".into()),
                }],
                reports_to: Some(RefEntry {
                    id: "ac-staff-engineer-ada".into(),
                    name: "Ada".into(),
                }),
                office: Some(RefEntry {
                    id: "hq".into(),
                    name: "HQ".into(),
                }),
            })
            .unwrap();

        spool
            .set_status(
                "agy-senior-dev",
                "Implementing Phase A CLI",
                Some(Duration::from_secs(3600)),
            )
            .unwrap();

        let card = spool.resolve_directory_member("agy-senior-dev").unwrap();
        assert_eq!(card.name, "Agy Senior Dev");
        assert_eq!(card.status.as_deref(), Some("Implementing Phase A CLI"));
        assert_eq!(card.teams.len(), 1);

        // Owner ("owner" or "vgmello") resolves out-of-the-box
        let owner = spool.resolve_directory_member("owner").unwrap();
        assert_eq!(owner.id, "vgmello");
        assert!(owner.owner);

        // Team filter works
        let platform = spool.list_directory(Some("Platform"), None).unwrap();
        assert_eq!(platform.len(), 1);
        assert_eq!(platform[0].id, "agy-senior-dev");

        // Expired status is treated as None
        let future_iso = format_iso8601_utc(SystemTime::now() + Duration::from_secs(7200));
        assert!(spool
            .get_active_status("agy-senior-dev", &future_iso)
            .is_none());

        spool.clear_status("agy-senior-dev").unwrap();
        let cleared = spool.resolve_directory_member("agy-senior-dev").unwrap();
        assert!(cleared.status.is_none());

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn concurrent_defer_done_and_notify_under_flock_lose_zero_updates() {
        let (dir, spool) = temp_spool("flock_concurrency");
        spool.register("ada").unwrap();
        spool.register("agy").unwrap();

        let mut ids = Vec::new();
        for i in 0..24 {
            let env = Envelope::new_msg(
                "ada",
                "agy",
                Some(format!("msg-{i}")),
                format!("body-{i}"),
                BTreeMap::new(),
            )
            .unwrap();
            ids.push(env.id.clone());
            spool.enqueue(&env).unwrap();
        }
        spool.deliver_all().unwrap();

        let spool = Arc::new(spool);
        let mut handles = Vec::new();

        // First 12 threads defer their message, next 12 threads mark their message done,
        // while 4 reader threads concurrently call `notify --after-task`.
        for (idx, id) in ids.iter().cloned().enumerate() {
            let s = Arc::clone(&spool);
            handles.push(thread::spawn(move || {
                if idx < 12 {
                    s.defer_message("agy", &id, "1h", Some(format!("defer-{idx}")))
                        .unwrap();
                } else {
                    s.done_message("agy", &id, Some(format!("done-{idx}")))
                        .unwrap();
                }
            }));
        }
        for r in 0..4 {
            let s = Arc::clone(&spool);
            handles.push(thread::spawn(move || {
                for _ in 0..5 {
                    let _ = s
                        .notify(
                            "agy",
                            &NotifyOptions {
                                session: Some(format!("sess-{r}")),
                                after_task: true,
                                urgent_only: false,
                                sweep_after: Duration::from_secs(300),
                            },
                        )
                        .unwrap();
                }
            }));
        }

        for h in handles {
            h.join().unwrap();
        }

        let all_items = spool.list_inbox_items("agy", true, true, None).unwrap();
        assert_eq!(all_items.len(), 24);
        let snoozed_count = all_items.iter().filter(|i| i.status == "snoozed").count();
        let done_count = all_items.iter().filter(|i| i.status == "done").count();
        assert_eq!(
            snoozed_count, 12,
            "all 12 concurrent defers must be preserved in triage.json"
        );
        assert_eq!(
            done_count, 12,
            "all 12 concurrent dones must be preserved in triage.json"
        );

        let _ = fs::remove_dir_all(&dir);
    }
}
