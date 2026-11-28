//! Standalone unit tests for `org-channel-watcher` (`WatcherStore`, `ChannelWatermark`,
//! `ChannelWatcher` eligibility/catch-up, and `CliWatcherRuntimeProvider` fail-closed guards).

use std::sync::Arc;
use std::time::Duration;

use org_copilot::watcher::DERIVED_SUMMARY_NOTICE;
use org_office::organization_family::records::{
    By, Meta, ModelProfile, ProviderAuthMode, ProviderConnection,
};
use org_office::{Office, TestOffice};
use org_office_sessions::RuntimeRegistry;
use org_proto::chat::{ChatChannelSummarizeParams, ConversationKind};
use org_store::{Id, Timestamp};

use crate::catchup::CATCHUP_QUESTION;
use crate::runtime::CliWatcherRuntimeProvider;
use crate::store::{
    ChannelWatermark, UsageLedgerEntry, WatchEndNotice, WatchEntry, WatcherStore, WatchesFile,
    WatermarksFile,
};
use crate::{ChannelWatcher, WatcherConfig, WatcherRuntimeProvider};

#[test]
fn watermark_advances_monotonically_per_origin_and_incarnation() {
    let mut wm = ChannelWatermark::default();
    assert_eq!(wm.last_seq("off-a", "inc-1"), 0);

    wm.advance("off-a", "inc-1", 5);
    assert_eq!(wm.last_seq("off-a", "inc-1"), 5);
    // Out-of-order lower sequences never regress the watermark.
    wm.advance("off-a", "inc-1", 3);
    assert_eq!(wm.last_seq("off-a", "inc-1"), 5);

    // Distinct incarnations and offices track independently.
    wm.advance("off-a", "inc-2", 2);
    wm.advance("off-b", "inc-1", 9);
    assert_eq!(wm.last_seq("off-a", "inc-1"), 5);
    assert_eq!(wm.last_seq("off-a", "inc-2"), 2);
    assert_eq!(wm.last_seq("off-b", "inc-1"), 9);
}

#[test]
fn watcher_store_persists_watches_watermarks_bounded_notices_and_usage() {
    let dir = tempfile::tempdir().unwrap();
    let store = WatcherStore::open(dir.path(), "office-1");

    let mut watches = WatchesFile {
        version: 1,
        watches: vec![WatchEntry {
            watch_id: "w-1".into(),
            conversation_id: "conv-1".into(),
            question_id: "q-1".into(),
            asker_id: "emp-1".into(),
            question_text: "What is the deploy target?".into(),
            additions: vec!["Staging only".into()],
            opened_at_ms: 1_000,
            judged_count: 2,
            ended_at_ms: None,
            ended_reason: None,
        }],
    };
    store.write_watches(&watches).unwrap();
    assert_eq!(store.read_watches(), watches);
    assert!(watches.watches[0].is_open());
    watches.watches[0].ended_at_ms = Some(2_000);
    assert!(!watches.watches[0].is_open());

    let mut wm_file = WatermarksFile {
        version: 1,
        channels: Default::default(),
    };
    wm_file
        .channels
        .entry("conv-1".into())
        .or_default()
        .advance("office-1", "inc-1", 42);
    store.write_watermarks(&wm_file).unwrap();
    assert_eq!(store.read_watermarks(), wm_file);

    // Notice queue caps at 200 entries and partitions cleanly per employee.
    let notices: Vec<WatchEndNotice> = (0..210)
        .map(|i| WatchEndNotice {
            employee_id: if i % 2 == 0 {
                "emp-a".into()
            } else {
                "emp-b".into()
            },
            conversation_id: "conv-1".into(),
            question_id: format!("q-{i}"),
            reason: "time_limit".into(),
            recorded_at_ms: i as u64,
        })
        .collect();
    store.append_notices(&notices).unwrap();
    let drained_a = store.drain_notices_for("emp-a");
    assert_eq!(drained_a.len(), 100);
    assert_eq!(drained_a[0].question_id, "q-10");
    assert!(store.drain_notices_for("emp-a").is_empty());
    assert_eq!(store.drain_notices_for("emp-b").len(), 100);

    let usage = UsageLedgerEntry {
        id: "u-1".into(),
        consumer_kind: "one_off".into(),
        employee_id: None,
        organization_id: "org-1".into(),
        project_id: None,
        conversation_id: "conv-1".into(),
        feature: "channel.watch".into(),
        model_profile_id: Some("prof-1".into()),
        question_ids: vec!["q-1".into()],
        outcome: "ok".into(),
        input_tokens: Some(12),
        output_tokens: Some(5),
        duration_ms: 40,
        recorded_at: "2026-11-27T00:00:00.000Z".into(),
    };
    store.append_usage(&usage).unwrap();
    assert_eq!(store.read_usage_records(), vec![usage]);
}

#[test]
fn watcher_store_enforces_owner_only_permissions_and_rejects_symlink_traps() {
    use std::os::unix::fs::PermissionsExt as _;

    let dir = tempfile::tempdir().unwrap();
    let store = WatcherStore::open(dir.path(), "office-perm");
    let watcher_dir = dir
        .path()
        .join("local")
        .join("offices")
        .join("office-perm")
        .join("watcher");
    for ancestor in [
        dir.path().join("local"),
        dir.path().join("local").join("offices"),
        dir.path().join("local").join("offices").join("office-perm"),
        watcher_dir.clone(),
    ] {
        let mode = std::fs::symlink_metadata(&ancestor)
            .unwrap()
            .permissions()
            .mode()
            & 0o7777;
        assert_eq!(mode, 0o700, "expected 0700 on {}", ancestor.display());
    }

    // Plant a predictable `.tmp` symlink before writing watches; atomic writes must ignore it.
    let outside = dir.path().join("outside-target.txt");
    std::fs::write(&outside, b"untouched").unwrap();
    org_platform::fs::create_symlink(&outside, &watcher_dir.join("watches.v1.tmp")).unwrap();

    store
        .write_watches(&WatchesFile {
            version: 1,
            watches: Vec::new(),
        })
        .unwrap();
    store
        .write_watermarks(&WatermarksFile {
            version: 1,
            channels: Default::default(),
        })
        .unwrap();
    store
        .append_notices(&[WatchEndNotice {
            employee_id: "emp-1".into(),
            conversation_id: "conv-1".into(),
            question_id: "q-1".into(),
            reason: "done".into(),
            recorded_at_ms: 1,
        }])
        .unwrap();
    let usage = UsageLedgerEntry {
        id: "u-perm".into(),
        consumer_kind: "one_off".into(),
        employee_id: None,
        organization_id: "org-1".into(),
        project_id: None,
        conversation_id: "conv-1".into(),
        feature: "channel.watch".into(),
        model_profile_id: None,
        question_ids: vec![],
        outcome: "ok".into(),
        input_tokens: None,
        output_tokens: None,
        duration_ms: 5,
        recorded_at: "2026-11-27T00:00:00.000Z".into(),
    };
    store.append_usage(&usage).unwrap();

    assert_eq!(std::fs::read(&outside).unwrap(), b"untouched");
    for file in [
        "watches.v1.json",
        "watermarks.v1.json",
        "watch-notices.v1.json",
        "usage-one-off.v1.jsonl",
    ] {
        let path = watcher_dir.join(file);
        let mode = std::fs::symlink_metadata(&path)
            .unwrap()
            .permissions()
            .mode()
            & 0o7777;
        assert_eq!(mode, 0o600, "expected 0600 on {}", path.display());
    }

    // Loose pre-existing usage ledger is tightened to 0600 on append.
    let usage_path = watcher_dir.join("usage-one-off.v1.jsonl");
    std::fs::set_permissions(&usage_path, std::fs::Permissions::from_mode(0o644)).unwrap();
    store.append_usage(&usage).unwrap();
    assert_eq!(
        std::fs::symlink_metadata(&usage_path)
            .unwrap()
            .permissions()
            .mode()
            & 0o7777,
        0o600
    );

    // Symlink at usage ledger path fails closed (O_NOFOLLOW) without touching target.
    std::fs::remove_file(&usage_path).unwrap();
    org_platform::fs::create_symlink(&outside, &usage_path).unwrap();
    assert!(store.append_usage(&usage).is_err());
    assert_eq!(std::fs::read(&outside).unwrap(), b"untouched");
}

#[test]
fn cli_watcher_runtime_provider_fails_closed_when_unconfigured_or_disabled() {
    let provider = CliWatcherRuntimeProvider::new(RuntimeRegistry::empty());
    let meta = Meta::created(&By::System, Timestamp(1_700_000_000_000));
    let conn_id = Id::parse("0195a000-0000-7000-8000-000000000011").unwrap();
    let profile = ModelProfile {
        id: Id::parse("0195a000-0000-7000-8000-000000000010").unwrap(),
        key: "model-x".into(),
        name: "Model X".into(),
        description: "Test model".into(),
        is_auto_generated: false,
        generated_for_connection_id: None,
        archived: false,
        connection_ids: vec![conn_id.clone()],
        meta: meta.clone(),
    };
    assert!(provider.runtime_for_profile(&profile, None).is_none());

    let make_conn = |enabled: bool, archived: bool| ProviderConnection {
        id: conn_id.clone(),
        key: "conn-x".into(),
        name: "Conn".into(),
        provider_type: "test".into(),
        auth_mode: ProviderAuthMode::SubscriptionCli,
        subscription_identity: None,
        runtime_driver: "unregistered".into(),
        endpoint_url: None,
        enabled,
        archived,
        max_concurrent_turns: 4,
        meta: meta.clone(),
    };
    assert!(provider
        .runtime_for_profile(&profile, Some(&make_conn(false, false)))
        .is_none());
    assert!(provider
        .runtime_for_profile(&profile, Some(&make_conn(true, true)))
        .is_none());
    assert!(provider
        .runtime_for_profile(&profile, Some(&make_conn(true, false)))
        .is_none());
}

#[test]
fn watcher_rejects_non_channel_or_person_asks_and_answers_empty_catchup_without_model() {
    assert!(CATCHUP_QUESTION.len() <= 2000);
    let office = Arc::new(Office::for_test(TestOffice::default()));
    let owner_id = office.owner_member_id().to_owned();
    let watcher = ChannelWatcher::new(
        Arc::clone(&office),
        None,
        Arc::new(CliWatcherRuntimeProvider::new(RuntimeRegistry::empty())),
        WatcherConfig {
            cadence: Duration::ZERO,
            ..WatcherConfig::default()
        },
    );

    assert_eq!(
        watcher.on_channel_ask_opened(&owner_id, "q-dm", "dm-1", ConversationKind::Dm, "hello"),
        (false, Some("not_a_channel".into()))
    );
    assert_eq!(
        watcher.on_channel_ask_opened(
            &owner_id,
            "q-person",
            "ch-1",
            ConversationKind::Channel,
            "hello"
        ),
        (false, Some("person_asker".into()))
    );

    let digest = watcher
        .summarize_channel(
            &owner_id,
            &ChatChannelSummarizeParams {
                channel: "#general".into(),
                since_read: true,
                ..Default::default()
            },
        )
        .unwrap();
    assert_eq!(digest.new_message_count, Some(0));
    assert_eq!(digest.derived_notice, DERIVED_SUMMARY_NOTICE);
    assert!(watcher.store().read_usage_records().is_empty());
}

#[test]
fn availability_probe_bounds_hanging_cli_and_reaps_child() {
    use std::os::unix::fs::PermissionsExt as _;

    let dir = tempfile::tempdir().unwrap();
    let write_script = |name: &str, body: &str| {
        let p = dir.path().join(name);
        std::fs::write(&p, body).unwrap();
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o700)).unwrap();
        p
    };

    let ok_bin = write_script("ok.sh", "#!/bin/sh\nexit 0\n");
    let fail_bin = write_script("fail.sh", "#!/bin/sh\nexit 1\n");
    let hang_bin = write_script("hang.sh", "#!/bin/sh\nexec sleep 30\n");

    assert!(crate::runtime::probe_binary_available(
        &ok_bin,
        &["--version"],
        Duration::from_secs(1),
    ));
    assert!(!crate::runtime::probe_binary_available(
        &fail_bin,
        &["--version"],
        Duration::from_secs(1),
    ));

    let started = std::time::Instant::now();
    assert!(!crate::runtime::probe_binary_available(
        &hang_bin,
        &["--version"],
        Duration::from_millis(120),
    ));
    assert!(
        started.elapsed() < Duration::from_secs(2),
        "hanging probe must return promptly, took {:?}",
        started.elapsed()
    );
}
