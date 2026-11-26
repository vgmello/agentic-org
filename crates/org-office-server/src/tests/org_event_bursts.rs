//! A satellite applying bursts of HQ `OrgEvent` frames in verified batches (`TASK-140`
//! review B1, B2), against a real HQ and an enrolled satellite: a frame the batch cannot
//! verify itself applies alone, after the events before it, so a failure never lets a later
//! frame's files reach `org-store`; and the replicated-files operation is named by exactly the
//! files it applies, so a retry the self-echo filter trims never reuses an operation ID.

use std::collections::BTreeMap;
use std::sync::Arc;

use base64::engine::general_purpose::STANDARD;
use base64::Engine as _;
use org_office::{EngineChoice, Office, OrgDeltaPayload, TestOffice};
use org_proto::tasks::{TasksCreateParams, TasksUpdateParams};
use org_store::{DurablePath, Files, StoreError};
use org_sync::organization::{apply, OrgEvent, PeerIdentity, PeerRole, MAX_PAYLOAD_BYTES};
use org_sync_proto::{Codec, Digest, Position, SyncFrame};

use super::a2a_messaging::enroll_satellite;
use crate::federation::{HqFederationServer, SatelliteSyncSession};
use crate::federation_link::org_events::{
    apply_satellite_org_event_frames, replicated_files_op_id,
};
use crate::federation_link::StaticPeer;

struct Fixture {
    hq: Arc<Office>,
    owner: String,
    hq_team: String,
    server: HqFederationServer,
    _sat_dir: tempfile::TempDir,
    sat: Arc<Office>,
    hq_peer: PeerIdentity,
}

/// A real HQ serving federation, and a satellite enrolled with it and holding HQ's snapshot.
fn fixture(name: &str, suffix: u8) -> Fixture {
    let hq = Arc::new(Office::for_test_with_real_ca(TestOffice {
        engine: EngineChoice::Sqlite,
    }));
    let owner = hq.owner_member_id().to_owned();
    let hq_team = hq.org_snapshot().unwrap().teams[0].id.clone();
    let server = HqFederationServer::start(
        Arc::clone(&hq),
        "127.0.0.1:0".parse().unwrap(),
        &["127.0.0.1".into()],
        None,
    )
    .unwrap();
    let hq_addr = server.local_addr().to_string();
    let (sat_dir, sat) = enroll_satellite(&hq, &owner, &hq_addr, name, suffix);
    let cp = sat.replication().org_checkpoint().unwrap();
    let hq_peer = PeerIdentity {
        org_id: cp.org_id.clone(),
        office_id: cp.hq_office.clone(),
        role: PeerRole::Hq,
        authority: cp.highest_epoch,
    };
    Fixture {
        hq,
        owner,
        hq_team,
        server,
        _sat_dir: sat_dir,
        sat,
        hq_peer,
    }
}

fn create_task(office: &Office, actor: &str, team_id: &str, title: &str) -> String {
    let task_id = office.next_uuid_v7();
    office
        .task_create(
            actor,
            TasksCreateParams {
                command_id: office.next_uuid_v7(),
                task_id: task_id.clone(),
                team_id: Some(team_id.to_owned()),
                project_id: None,
                assigned_team_id: None,
                assignee_id: None,
                task_type: None,
                work_type: None,
                title: title.into(),
                body_markdown: "burst test".into(),
                priority: None,
                effort: None,
                severity: None,
                rank_order: None,
                labels: Vec::new(),
                reference_files: Vec::new(),
                modified_files: Vec::new(),
                depends_on: Vec::new(),
                details: None,
            },
        )
        .unwrap()
        .unwrap();
    task_id
}

/// HQ publishes its committed journal and returns its events after `after`. A task create
/// inside the office's reserved number block is answered `committed` (`ADR-0019`): HQ
/// publishes it once it is flushed and confirmed, which the sync waits for.
fn hq_events_after(hq: &Office, after: Position) -> Vec<OrgEvent> {
    hq.sync_store().unwrap();
    hq.sync_org_journal_to_replication().unwrap();
    hq.replication()
        .org_events_after(after.epoch.get(), after.seq)
        .unwrap()
}

fn sat_position(sat: &Office) -> Position {
    sat.replication().org_checkpoint().unwrap().position
}

fn frame(ev: &OrgEvent) -> SyncFrame {
    SyncFrame::OrgEvent {
        org_id: ev.org_id.as_str().to_owned(),
        epoch: ev.position.epoch.get(),
        seq: ev.position.seq,
        previous_hex: ev.previous.to_hex(),
        digest_hex: ev.digest.to_hex(),
        codec_name: ev.codec.id().to_owned(),
        codec_version: ev.codec.version(),
        payload_b64: STANDARD.encode(&ev.payload),
    }
}

/// A well-formed event at `position` after `previous`: its digest is its own.
fn event_at(
    like: &OrgEvent,
    position: Position,
    previous: Digest,
    codec: Codec,
    payload: Arc<[u8]>,
) -> OrgEvent {
    let digest =
        org_sync_proto::event_digest_v1(&like.org_id, position, previous, &codec, &payload)
            .unwrap();
    OrgEvent {
        org_id: like.org_id.clone(),
        position,
        previous,
        digest,
        codec,
        payload,
    }
}

/// `events` moved to follow `after` in its chain, payloads unchanged.
fn rechained(after: &OrgEvent, events: &[OrgEvent]) -> Vec<OrgEvent> {
    let mut prev = after.clone();
    let mut out = Vec::new();
    for ev in events {
        let position = Position {
            epoch: prev.position.epoch,
            seq: prev.position.seq + 1,
        };
        let next = event_at(
            &prev,
            position,
            prev.digest,
            ev.codec.clone(),
            Arc::clone(&ev.payload),
        );
        out.push(next.clone());
        prev = next;
    }
    out
}

fn files_of(ev: &OrgEvent) -> Vec<(String, Option<Vec<u8>>)> {
    serde_json::from_slice::<OrgDeltaPayload>(&ev.payload)
        .unwrap()
        .decode_files()
        .unwrap()
}

/// What the satellite's `org-store` holds at every path `events` write or remove.
fn stored_at(sat: &Office, events: &[OrgEvent]) -> BTreeMap<String, Option<Vec<u8>>> {
    let files = Files::open(sat.durable_root()).unwrap();
    let mut out = BTreeMap::new();
    for ev in events {
        for (path, _) in files_of(ev) {
            let held = files
                .read(&DurablePath::parse(&path).unwrap())
                .unwrap()
                .map(|bytes| bytes.to_vec());
            out.insert(path, held);
        }
    }
    out
}

/// Applies `frames`, which fail at the frame after `prefix`, and checks that exactly `prefix`
/// applied: the checkpoint is its last event, and every path a frame of `later` writes holds
/// what it held before, or what `prefix` wrote there; nothing of `later` reached `org-store`.
fn assert_only_prefix_applies(
    sat: &Office,
    hq_peer: &PeerIdentity,
    frames: Vec<SyncFrame>,
    prefix: &[OrgEvent],
    later: &[OrgEvent],
) -> String {
    let mut expected = stored_at(sat, later);
    let error = apply_satellite_org_event_frames(sat, hq_peer, frames)
        .expect_err("the burst fails at its bad frame");
    assert_eq!(
        sat_position(sat),
        prefix.last().unwrap().position,
        "the verified prefix applied, nothing after it"
    );
    for ev in prefix {
        for (path, content) in files_of(ev) {
            if let Some(slot) = expected.get_mut(&path) {
                *slot = content;
            }
        }
    }
    assert_eq!(
        stored_at(sat, later),
        expected,
        "no file of a frame after the failing one reached org-store"
    );
    error
}

/// Review B1: `[valid, divergent duplicate, valid]`. The duplicate is checked only by
/// `org-sync` against the retained history, so it applies alone after the valid prefix and
/// its refusal is reported before the next frame's files are ever buffered.
#[test]
fn a_divergent_duplicate_in_a_burst_applies_only_the_prefix_before_it() {
    let f = fixture("Burst Dup", 41);
    let start = sat_position(&f.sat);
    let task_a = create_task(&f.hq, &f.owner, &f.hq_team, "Task A");
    let a = hq_events_after(&f.hq, start);
    assert!(!a.is_empty());
    let task_b = create_task(&f.hq, &f.owner, &f.hq_team, "Task B");
    let b = hq_events_after(&f.hq, a.last().unwrap().position);
    assert!(!b.is_empty());

    // Same position as A's last event, a different (well-formed) event: divergent history.
    let last_a = a.last().unwrap();
    let divergent = event_at(
        last_a,
        last_a.position,
        last_a.previous,
        last_a.codec.clone(),
        Arc::from(&br#"{"v":1,"files":[]}"#[..]),
    );
    assert_ne!(divergent.digest, last_a.digest);
    let mut frames: Vec<SyncFrame> = a.iter().map(frame).collect();
    frames.push(frame(&divergent));
    frames.extend(b.iter().map(frame));

    let error = assert_only_prefix_applies(&f.sat, &f.hq_peer, frames, &a, &b);
    assert!(error.contains("divergence"), "{error}");
    assert!(f.sat.task_inspect(&task_a).unwrap().is_ok());
    assert!(
        f.sat.task_inspect(&task_b).unwrap().is_err(),
        "task B never reached the satellite"
    );

    // The real history still applies afterwards.
    let frames = b.iter().map(frame).collect();
    assert_eq!(
        apply_satellite_org_event_frames(&f.sat, &f.hq_peer, frames).unwrap(),
        b.len()
    );
    assert!(f.sat.task_inspect(&task_b).unwrap().is_ok());
    f.server.stop();
}

/// Review B1: `[valid, unsupported codec, valid]`. An event of any codec but
/// `org.native.changes` v1 has no files to verify here and applies alone: `org-sync` refuses
/// the unsupported one after the prefix applied, and the valid frame chained after it (its
/// own chain checks pass) never merges its files into `org-store`.
#[test]
fn an_unsupported_codec_in_a_burst_applies_only_the_prefix_before_it() {
    let f = fixture("Burst Codec", 42);
    let start = sat_position(&f.sat);
    create_task(&f.hq, &f.owner, &f.hq_team, "Task A");
    let a = hq_events_after(&f.hq, start);
    let task_c = create_task(&f.hq, &f.owner, &f.hq_team, "Task C");
    let c = hq_events_after(&f.hq, a.last().unwrap().position);

    let last_a = a.last().unwrap();
    let unsupported = event_at(
        last_a,
        Position {
            epoch: last_a.position.epoch,
            seq: last_a.position.seq + 1,
        },
        last_a.digest,
        Codec::new("org.unknown", 1).unwrap(),
        Arc::from(&b"opaque"[..]),
    );
    let after_unsupported = rechained(&unsupported, &c);
    let mut frames: Vec<SyncFrame> = a.iter().map(frame).collect();
    frames.push(frame(&unsupported));
    frames.extend(after_unsupported.iter().map(frame));

    let error = assert_only_prefix_applies(&f.sat, &f.hq_peer, frames, &a, &after_unsupported);
    assert!(error.contains("unsupported_version"), "{error}");
    assert!(
        f.sat.task_inspect(&task_c).unwrap().is_err(),
        "task C never reached the satellite"
    );
    f.server.stop();
}

/// Review B-P2-1: `[valid, oversized org.native.changes, valid]`. The oversized event is a real
/// task's changes (files `org-store` accepts) padded with JSON whitespace past `org-sync`'s
/// `MAX_PAYLOAD_BYTES`, with its own digest. The satellite's pre-apply check runs `org-sync`'s
/// own gates, so it is refused with the error a lone apply reports before its files, or any
/// later frame's, are buffered: the prefix commits, and nothing of the oversized event or of
/// the valid frame after it reaches `org-store`.
#[test]
fn an_oversized_event_in_a_burst_applies_only_the_prefix_before_it() {
    let f = fixture("Burst Oversized", 44);
    let start = sat_position(&f.sat);
    let task_a = create_task(&f.hq, &f.owner, &f.hq_team, "Task A");
    let a = hq_events_after(&f.hq, start);
    let task_c = create_task(&f.hq, &f.owner, &f.hq_team, "Task C");
    let c = hq_events_after(&f.hq, a.last().unwrap().position);
    let task_d = create_task(&f.hq, &f.owner, &f.hq_team, "Task D");
    let d = hq_events_after(&f.hq, c.last().unwrap().position);

    let mut padded = c[0].payload.to_vec();
    padded.resize(MAX_PAYLOAD_BYTES + 1, b' ');
    let last_a = a.last().unwrap();
    let oversized = event_at(
        last_a,
        Position {
            epoch: last_a.position.epoch,
            seq: last_a.position.seq + 1,
        },
        last_a.digest,
        c[0].codec.clone(),
        Arc::from(padded),
    );
    assert_eq!(oversized.codec.id(), "org.native.changes");
    assert_eq!(
        files_of(&oversized),
        files_of(&c[0]),
        "well-formed task files"
    );
    let after_oversized = rechained(&oversized, &d);
    let mut later = vec![oversized.clone()];
    later.extend(after_oversized.iter().cloned());
    let mut frames: Vec<SyncFrame> = a.iter().map(frame).collect();
    frames.push(frame(&oversized));
    frames.extend(after_oversized.iter().map(frame));

    let error = assert_only_prefix_applies(&f.sat, &f.hq_peer, frames, &a, &later);
    assert!(f.sat.task_inspect(&task_a).unwrap().is_ok());
    for task in [&task_c, &task_d] {
        assert!(
            f.sat.task_inspect(task).unwrap().is_err(),
            "no task after the prefix reached the satellite"
        );
    }
    // The very error a lone apply of the oversized event reports, and it commits nothing.
    let peer = StaticPeer(f.hq_peer.clone());
    let lone = apply(f.sat.replication().as_ref(), &peer, oversized).unwrap_err();
    assert_eq!(error, format!("apply_org_event failed: {lone}"));
    assert!(error.contains("invalid_request"), "{error}");
    assert_eq!(sat_position(&f.sat), last_a.position);
    f.server.stop();
}

/// Review B2: a batch's files committed (`[HQ task T2, satellite-owned task T1 rev 2]`) but
/// its events not, as a crash between the two leaves it. The retry's self-echo filter now
/// drops T1 (the satellite holds rev 2), so the retry applies other files: it must be a new
/// operation, not the old ID with other files (`StoreError::Conflict`), and it converges.
#[test]
fn a_retry_the_self_echo_filter_trims_is_a_new_operation_and_converges() {
    let f = fixture("Burst Retry", 43);
    let sat_team = f.sat.org_snapshot().unwrap().teams[0].id.clone();

    // T1: the satellite's own task, at HQ and echoed back.
    let t1 = create_task(&f.sat, &f.owner, &sat_team, "T1");
    let mut session = SatelliteSyncSession::connect(Arc::clone(&f.sat)).unwrap();
    assert_eq!(session.flush_work_outbox().unwrap(), 1);
    session.pull_org_events().unwrap();
    let start = sat_position(&f.sat);

    // E10: HQ creates T2. E11: the satellite's update of T1 to rev 2, echoed by HQ.
    let t2 = create_task(&f.hq, &f.owner, &f.hq_team, "T2");
    let update: TasksUpdateParams = serde_json::from_value(serde_json::json!({
        "command_id": f.sat.next_uuid_v7(),
        "task": t1,
        "expected_revision": 1,
        "title": "T1 rev 2",
    }))
    .unwrap();
    f.sat.task_update(&f.owner, update).unwrap().unwrap();
    assert_eq!(session.flush_work_outbox().unwrap(), 1);
    drop(session);
    let events = hq_events_after(&f.hq, start);
    assert_eq!(events.len(), 2, "E10 (T2) and E11 (T1 rev 2)");
    let (e10, e11) = (&events[0], &events[1]);
    let mut merged = files_of(e10);
    merged.extend(files_of(e11));
    assert!(merged.iter().any(|(path, _)| path.contains(&t2)));
    assert!(merged.iter().any(|(path, _)| path.contains(&t1)));

    // The first attempt: both events' files committed, the checkpoint not advanced.
    let hq_office = f.hq_peer.office_id.as_str();
    let first_attempt = replicated_files_op_id(hq_office, e10, e11, &merged);
    f.sat
        .apply_replicated_state_files(&first_attempt, merged.clone())
        .unwrap();
    assert_eq!(sat_position(&f.sat), start);

    // The retry's filter drops T1 (and its sidecars) the satellite already holds.
    let trimmed = f
        .sat
        .filter_self_echoed_org_event_files(merged.clone())
        .unwrap();
    assert!(
        trimmed.len() < merged.len(),
        "the retry applies other files"
    );
    assert!(trimmed.iter().all(|(path, _)| !path.contains(&t1)));
    let retry = replicated_files_op_id(hq_office, e10, e11, &trimmed);
    // Reusing the first ID with other files is what conflicted.
    let reused = f
        .sat
        .apply_replicated_state_files(&first_attempt, trimmed)
        .expect_err("one operation ID, other files");
    let cause = std::error::Error::source(&reused).and_then(|e| e.downcast_ref::<StoreError>());
    assert!(matches!(cause, Some(StoreError::Conflict)), "{reused:?}");

    let frames = events.iter().map(frame).collect();
    let applied = apply_satellite_org_event_frames(&f.sat, &f.hq_peer, frames)
        .expect("the retry converges without an operation ID conflict");
    assert_eq!(applied, 2);
    assert_ne!(retry, first_attempt, "other files, another operation");
    assert_eq!(sat_position(&f.sat), e11.position);
    assert!(f.sat.task_inspect(&t2).unwrap().is_ok());
    assert_eq!(
        f.sat.task_inspect(&t1).unwrap().unwrap().task.revision,
        2,
        "the satellite keeps its own T1 rev 2"
    );
    // An exact retry of the first attempt still meets its idempotency receipt.
    f.sat
        .apply_replicated_state_files(&first_attempt, merged)
        .unwrap();
    f.server.stop();
}
