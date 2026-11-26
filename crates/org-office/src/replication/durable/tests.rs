use std::io::Write;
use std::path::Path;
use std::sync::{Arc, Mutex};

use org_sync::organization::{append_hq, OrgStore};
use org_sync_proto::{Codec, Digest, Epoch, OfficeId, OrgId, OriginIncarnation};

use super::{DurableReplicationParams, DurableReplicationStore, ReplicationProjection};
use crate::error::OfficeError;
use crate::replication::{memory::ReplicationCoreData, ReplicationStore};

const HQ: &str = "0195a000-0000-7000-8000-000000000002";

fn params(durable: &Path) -> DurableReplicationParams<'_> {
    DurableReplicationParams {
        durable_root: durable,
        org_id: OrgId::new("0195a000-0000-7000-8000-000000000001").unwrap(),
        local_office: OfficeId::new(HQ).unwrap(),
        hq_office: OfficeId::new(HQ).unwrap(),
        ca: Digest::from_bytes([0x42; 32]),
        epoch: Epoch::new(1).unwrap(),
        incarnation: OriginIncarnation::new("0195a000-0000-7000-8000-000000000004").unwrap(),
        initial_snapshot_required: false,
    }
}

fn append(store: &DurableReplicationStore, payload: &'static [u8]) -> Result<(), String> {
    let codec = Codec::new("org.native.changes", 1).unwrap();
    OrgStore::transact(
        store,
        Arc::new(move |tx| append_hq(tx, codec.clone(), Arc::from(payload))),
    )
    .map(|_| ())
    .map_err(|e| e.to_string())
}

#[test]
fn unprojected_durable_store_passes_eng3_conformance() {
    let mut holders = Vec::new();
    crate::replication::conformance::run_eng3_replication_suite(|spec| {
        let dir = tempfile::tempdir().unwrap();
        let store = DurableReplicationStore::open_unprojected(DurableReplicationParams {
            durable_root: dir.path(),
            org_id: spec.org_id,
            local_office: spec.local_office,
            hq_office: spec.hq_office,
            ca: spec.ca,
            epoch: spec.epoch,
            incarnation: spec.incarnation,
            initial_snapshot_required: spec.initial_snapshot_required,
        })
        .unwrap();
        holders.push(dir);
        Box::new(store)
    });
}

/// TASK-172: an engine without a projection keeps the same durable state file, written at
/// open and on every commit, and reopens from it.
#[test]
fn unprojected_durable_store_writes_the_state_file_and_reopens_from_it() {
    let dir = tempfile::tempdir().unwrap();
    let state = dir
        .path()
        .join(format!("local/offices/{HQ}/replication/state.v1.json"));
    {
        let store = DurableReplicationStore::open_unprojected(params(dir.path())).unwrap();
        let written: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&state).unwrap()).unwrap();
        assert_eq!(written["seq"], 0);
        append(&store, b"persisted").unwrap();
    }
    let written: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&state).unwrap()).unwrap();
    assert_eq!(written["seq"], 1);
    let reopened = DurableReplicationStore::open_unprojected(params(dir.path())).unwrap();
    assert_eq!(reopened.sync_status().unwrap().seq, 1);
    assert_eq!(reopened.org_events_after(1, 0).unwrap().len(), 1);
}

/// Records the sequence of every projection and fails the ones it is told to.
struct Recording {
    projected: Arc<Mutex<Vec<u64>>>,
    fail_next: Arc<Mutex<bool>>,
}

impl ReplicationProjection for Recording {
    fn project(&mut self, data: &ReplicationCoreData) -> Result<(), OfficeError> {
        if std::mem::take(&mut *self.fail_next.lock().unwrap()) {
            return Err(OfficeError::Invalid("projection failed"));
        }
        self.projected
            .lock()
            .unwrap()
            .push(data.checkpoint.position.seq);
        Ok(())
    }
}

/// `TASK-52` review F19, now for every engine's projection: a failed projection leaves the
/// committed state in place and is reconciled before the next mutation.
#[test]
fn a_failed_projection_is_reconciled_from_the_committed_state_before_the_next_commit() {
    let dir = tempfile::tempdir().unwrap();
    let projected = Arc::new(Mutex::new(Vec::new()));
    let fail_next = Arc::new(Mutex::new(false));
    let store = DurableReplicationStore::open(params(dir.path()), |data| {
        projected.lock().unwrap().push(data.checkpoint.position.seq);
        Ok(Recording {
            projected: projected.clone(),
            fail_next: fail_next.clone(),
        })
    })
    .unwrap();

    *fail_next.lock().unwrap() = true;
    assert!(append(&store, b"first").is_err());
    assert_eq!(store.sync_status().unwrap().seq, 1, "the commit stands");
    assert_eq!(*projected.lock().unwrap(), [0]);

    append(&store, b"second").unwrap();
    assert_eq!(*projected.lock().unwrap(), [0, 1, 2]);
}

fn replication_dir(durable: &Path) -> std::path::PathBuf {
    durable.join(format!("local/offices/{HQ}/replication"))
}

fn read_json(path: &Path) -> serde_json::Value {
    serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap()
}

fn log_lines(dir: &Path) -> Vec<serde_json::Value> {
    let bytes = std::fs::read(dir.join("events.v1.ndjson")).unwrap();
    assert!(bytes.ends_with(b"\n"), "whole lines only");
    bytes
        .split(|b| *b == b'\n')
        .filter(|l| !l.is_empty())
        .map(|l| serde_json::from_slice(l).unwrap())
        .collect()
}

fn payloads(store: &DurableReplicationStore) -> Vec<Vec<u8>> {
    store
        .org_events_after(1, 0)
        .unwrap()
        .into_iter()
        .map(|e| e.payload.to_vec())
        .collect()
}

const PAYLOADS: [&[u8]; 4] = [b"one", b"two", b"three", b"four"];

/// TASK-140: a commit appends its new events to `events.v1.ndjson` and rewrites only the small
/// state file, which names the log's committed extent and holds no events.
#[test]
fn events_are_appended_to_the_log_and_the_state_file_stays_small() {
    let dir = tempfile::tempdir().unwrap();
    let repl = replication_dir(dir.path());
    let store = DurableReplicationStore::open_unprojected(params(dir.path())).unwrap();
    assert!(
        !repl.join("events.v1.ndjson").exists(),
        "no log before the first event"
    );
    append(&store, b"first").unwrap();
    let small = std::fs::metadata(repl.join("state.v1.json")).unwrap().len();
    for _ in 0..40 {
        append(&store, b"a payload that would grow an inline history").unwrap();
    }
    let state = read_json(&repl.join("state.v1.json"));
    assert_eq!(state["seq"], 41);
    assert_eq!(state["events"], serde_json::json!([]));
    assert_eq!(state["events_log"]["count"], 41);
    let log_len = std::fs::metadata(repl.join("events.v1.ndjson"))
        .unwrap()
        .len();
    assert_eq!(state["events_log"]["bytes"], log_len);
    let lines = log_lines(&repl);
    assert_eq!(lines.len(), 41);
    assert_eq!(
        state["events_log"]["last_digest_hex"],
        lines[40]["digest_hex"]
    );
    assert_eq!(lines[0]["seq"], 1);
    assert_eq!(lines[40]["seq"], 41);
    // The state file does not grow with the history.
    let now = std::fs::metadata(repl.join("state.v1.json")).unwrap().len();
    assert!(now < small + 64, "{small} -> {now}");

    drop(store);
    let reopened = DurableReplicationStore::open_unprojected(params(dir.path())).unwrap();
    assert_eq!(reopened.sync_status().unwrap().seq, 41);
    assert_eq!(payloads(&reopened).len(), 41);
    assert_eq!(payloads(&reopened)[0], b"first");
}

/// Rewrites the committed state as a pre-`TASK-140` state file: every event inline, no log
/// extent; and leaves `stale` where the log was.
fn make_legacy(dir: &Path, stale: &[u8]) {
    let repl = replication_dir(dir);
    let mut state = read_json(&repl.join("state.v1.json"));
    let lines = log_lines(&repl);
    let object = state.as_object_mut().unwrap();
    object.remove("events_log");
    object.insert("events".into(), serde_json::Value::Array(lines));
    std::fs::write(
        repl.join("state.v1.json"),
        serde_json::to_vec_pretty(&state).unwrap(),
    )
    .unwrap();
    std::fs::write(repl.join("events.v1.ndjson"), stale).unwrap();
}

/// A state file with its events inline (every state from before `TASK-140`, and the commit
/// point of a non-append rewrite) is the authority over any log beside it, and its next commit
/// moves the events to a fresh log.
#[test]
fn an_inline_state_file_is_read_whatever_log_lies_beside_it_and_migrated_by_the_next_commit() {
    let dir = tempfile::tempdir().unwrap();
    let repl = replication_dir(dir.path());
    {
        let store = DurableReplicationStore::open_unprojected(params(dir.path())).unwrap();
        for payload in &PAYLOADS[..3] {
            append(&store, payload).unwrap();
        }
    }
    make_legacy(
        dir.path(),
        b"{\"not\": \"an event\"}\ngarbage with no newline",
    );

    let store = DurableReplicationStore::open_unprojected(params(dir.path())).unwrap();
    assert_eq!(payloads(&store), PAYLOADS[..3]);
    // Nothing migrates on a read.
    assert!(read_json(&repl.join("state.v1.json"))
        .get("events_log")
        .is_none());

    append(&store, PAYLOADS[3]).unwrap();
    let state = read_json(&repl.join("state.v1.json"));
    assert_eq!(state["events"], serde_json::json!([]));
    assert_eq!(state["events_log"]["count"], 4);
    assert_eq!(log_lines(&repl).len(), 4);
    drop(store);
    let reopened = DurableReplicationStore::open_unprojected(params(dir.path())).unwrap();
    assert_eq!(payloads(&reopened), PAYLOADS);

    // A commit that does not touch the events also migrates a legacy state.
    drop(reopened);
    make_legacy(dir.path(), b"");
    let store = DurableReplicationStore::open_unprojected(params(dir.path())).unwrap();
    store.record_replicated_op_id("op-1").unwrap();
    assert_eq!(
        read_json(&repl.join("state.v1.json"))["events_log"]["count"],
        4
    );
    drop(store);
    let reopened = DurableReplicationStore::open_unprojected(params(dir.path())).unwrap();
    assert_eq!(payloads(&reopened), PAYLOADS);
    assert!(reopened.is_replicated_op_id("op-1").unwrap());
}

/// A crash between appending to the log and committing the state file leaves bytes past the
/// committed extent, possibly a torn line: they are never read, the log is cut back at open,
/// and the next append starts at the committed end.
#[test]
fn an_uncommitted_or_torn_log_tail_is_cut_and_never_read() {
    let dir = tempfile::tempdir().unwrap();
    let repl = replication_dir(dir.path());
    let log = repl.join("events.v1.ndjson");
    {
        let store = DurableReplicationStore::open_unprojected(params(dir.path())).unwrap();
        append(&store, PAYLOADS[0]).unwrap();
        append(&store, PAYLOADS[1]).unwrap();
    }
    let committed = std::fs::metadata(&log).unwrap().len();
    let mut uncommitted = std::fs::read(&log).unwrap();
    let whole_line = log_lines(&repl)[1].to_string();
    uncommitted.extend_from_slice(whole_line.as_bytes());
    uncommitted.extend_from_slice(b"\n{\"epoch\":1,\"seq\":4,\"prev");
    std::fs::write(&log, &uncommitted).unwrap();

    let store = DurableReplicationStore::open_unprojected(params(dir.path())).unwrap();
    assert_eq!(payloads(&store), PAYLOADS[..2]);
    assert_eq!(std::fs::metadata(&log).unwrap().len(), committed);

    // An append that failed after writing (its state never committed) is cut the same way
    // by the next append.
    std::fs::OpenOptions::new()
        .append(true)
        .open(&log)
        .unwrap()
        .write_all(b"{\"torn\":")
        .unwrap();
    append(&store, PAYLOADS[2]).unwrap();
    assert_eq!(log_lines(&repl).len(), 3);
    drop(store);
    let reopened = DurableReplicationStore::open_unprojected(params(dir.path())).unwrap();
    assert_eq!(payloads(&reopened), PAYLOADS[..3]);
}

/// A log missing, shorter than, or different from the extent its state file committed is
/// corruption: opening fails closed instead of serving a partial history.
#[test]
fn a_log_that_does_not_hold_its_committed_extent_fails_closed() {
    let dir = tempfile::tempdir().unwrap();
    let repl = replication_dir(dir.path());
    let log = repl.join("events.v1.ndjson");
    {
        let store = DurableReplicationStore::open_unprojected(params(dir.path())).unwrap();
        append(&store, PAYLOADS[0]).unwrap();
        append(&store, PAYLOADS[1]).unwrap();
    }
    let good = std::fs::read(&log).unwrap();

    std::fs::write(&log, &good[..good.len() - 5]).unwrap();
    assert!(DurableReplicationStore::open_unprojected(params(dir.path())).is_err());

    let mut swapped = good.clone();
    let at = swapped.iter().position(|b| *b == b'1').unwrap();
    swapped[at] = b'2';
    std::fs::write(&log, &swapped).unwrap();
    assert!(DurableReplicationStore::open_unprojected(params(dir.path())).is_err());

    std::fs::remove_file(&log).unwrap();
    assert!(DurableReplicationStore::open_unprojected(params(dir.path())).is_err());

    std::fs::write(&log, &good).unwrap();
    let store = DurableReplicationStore::open_unprojected(params(dir.path())).unwrap();
    assert_eq!(payloads(&store), PAYLOADS[..2]);
    // An append never papers over a short log either.
    std::fs::write(&log, &good[..10]).unwrap();
    assert!(append(&store, PAYLOADS[2]).is_err());
    assert_eq!(store.sync_status().unwrap().seq, 2, "nothing committed");
}

/// Clearing the events (a re-enrollment, a snapshot install) commits an empty extent and
/// empties the log; a reopen serves no events.
#[test]
fn clearing_the_events_commits_an_empty_log() {
    let dir = tempfile::tempdir().unwrap();
    let repl = replication_dir(dir.path());
    let store = DurableReplicationStore::open_unprojected(params(dir.path())).unwrap();
    for payload in &PAYLOADS[..3] {
        append(&store, payload).unwrap();
    }
    store
        .reset_for_re_enrollment(
            OfficeId::new("0195a000-0000-7000-8000-000000000009").unwrap(),
            Digest::from_bytes([0x43; 32]),
            Epoch::new(2).unwrap(),
        )
        .unwrap();
    let state = read_json(&repl.join("state.v1.json"));
    assert_eq!(state["events_log"]["bytes"], 0);
    assert_eq!(state["events_log"]["count"], 0);
    assert_eq!(
        std::fs::metadata(repl.join("events.v1.ndjson"))
            .unwrap()
            .len(),
        0
    );
    drop(store);
    let reopened = DurableReplicationStore::open_unprojected(params(dir.path())).unwrap();
    assert_eq!(reopened.sync_status().unwrap().seq, 0);
    assert!(reopened.org_events_after(2, 0).unwrap().is_empty());
    assert!(reopened.org_events_after(1, 0).unwrap().is_empty());
}

/// Review N1a: a state file that does not decode (here an unknown version) is refused before
/// the log is touched, so an uncommitted tail past its extent stays exactly as it was.
#[test]
fn a_refused_state_file_never_cuts_the_log() {
    let dir = tempfile::tempdir().unwrap();
    let repl = replication_dir(dir.path());
    let log = repl.join("events.v1.ndjson");
    {
        let store = DurableReplicationStore::open_unprojected(params(dir.path())).unwrap();
        append(&store, PAYLOADS[0]).unwrap();
        append(&store, PAYLOADS[1]).unwrap();
    }
    let committed = std::fs::read(&log).unwrap();
    let first_payload = log_lines(&repl)[0]["payload_b64"]
        .as_str()
        .unwrap()
        .to_owned();
    let mut with_tail = committed.clone();
    with_tail.extend_from_slice(b"{\"epoch\":1,\"seq\":3,\"torn");
    std::fs::write(&log, &with_tail).unwrap();

    let state_path = repl.join("state.v1.json");
    let good_state = std::fs::read(&state_path).unwrap();
    let mut state = read_json(&state_path);
    state["version"] = serde_json::json!(2);
    std::fs::write(&state_path, serde_json::to_vec_pretty(&state).unwrap()).unwrap();
    assert!(DurableReplicationStore::open_unprojected(params(dir.path())).is_err());
    assert_eq!(
        std::fs::read(&log).unwrap(),
        with_tail,
        "the log is untouched"
    );

    // A committed event whose own digest no longer checks out is refused the same way.
    std::fs::write(&state_path, &good_state).unwrap();
    let mut altered = with_tail.clone();
    let at = altered
        .windows(first_payload.len())
        .position(|w| w == first_payload.as_bytes())
        .unwrap();
    altered[at] = if altered[at] == b'A' { b'B' } else { b'A' };
    std::fs::write(&log, &altered).unwrap();
    assert!(DurableReplicationStore::open_unprojected(params(dir.path())).is_err());
    assert_eq!(
        std::fs::read(&log).unwrap(),
        altered,
        "the log is untouched"
    );

    // The intact state file opens and only then cuts the uncommitted tail.
    std::fs::write(&log, &with_tail).unwrap();
    let store = DurableReplicationStore::open_unprojected(params(dir.path())).unwrap();
    assert_eq!(payloads(&store), PAYLOADS[..2]);
    assert_eq!(std::fs::read(&log).unwrap(), committed);
}

/// Review N1b: the committed log names its extent consistently (bytes, count, last digest),
/// yet repeats a position: opening fails closed rather than keeping either copy.
#[test]
fn a_log_that_repeats_an_event_position_fails_closed() {
    let dir = tempfile::tempdir().unwrap();
    let repl = replication_dir(dir.path());
    let log = repl.join("events.v1.ndjson");
    {
        let store = DurableReplicationStore::open_unprojected(params(dir.path())).unwrap();
        append(&store, PAYLOADS[0]).unwrap();
        append(&store, PAYLOADS[1]).unwrap();
    }
    let lines = log_lines(&repl);
    let mut repeated = Vec::new();
    for line in [&lines[0], &lines[1], &lines[1]] {
        repeated.extend_from_slice(line.to_string().as_bytes());
        repeated.push(b'\n');
    }
    std::fs::write(&log, &repeated).unwrap();
    let state_path = repl.join("state.v1.json");
    let mut state = read_json(&state_path);
    state["events_log"]["bytes"] = serde_json::json!(repeated.len());
    state["events_log"]["count"] = serde_json::json!(3);
    state["events_log"]["last_digest_hex"] = lines[1]["digest_hex"].clone();
    std::fs::write(&state_path, serde_json::to_vec_pretty(&state).unwrap()).unwrap();

    let refused = DurableReplicationStore::open_unprojected(params(dir.path()));
    assert!(refused.is_err());
    assert_eq!(
        std::fs::read(&log).unwrap(),
        repeated,
        "the log is untouched"
    );
}

/// Review N1b: stored events whose digests each check out, but which do not chain within
/// their epoch (one spliced from another history, or one missing between two), fail closed.
#[test]
fn stored_events_that_do_not_chain_within_an_epoch_fail_closed() {
    let ours = tempfile::tempdir().unwrap();
    let theirs = tempfile::tempdir().unwrap();
    for (dir, payloads) in [
        (ours.path(), [PAYLOADS[0], PAYLOADS[1], PAYLOADS[2]]),
        (theirs.path(), [&b"uno"[..], b"dos", b"tres"]),
    ] {
        let store = DurableReplicationStore::open_unprojected(params(dir)).unwrap();
        for payload in payloads {
            append(&store, payload).unwrap();
        }
    }
    let our_lines = log_lines(&replication_dir(ours.path()));
    let their_lines = log_lines(&replication_dir(theirs.path()));
    let state_path = replication_dir(ours.path()).join("state.v1.json");
    let committed = read_json(&state_path);
    let with_inline = |events: Vec<serde_json::Value>| {
        let mut state = committed.clone();
        let object = state.as_object_mut().unwrap();
        object.remove("events_log");
        object.insert("events".into(), serde_json::Value::Array(events));
        std::fs::write(&state_path, serde_json::to_vec_pretty(&state).unwrap()).unwrap();
    };

    // Seq 2 from another history: its own digest is right, its predecessor is not ours.
    with_inline(vec![
        our_lines[0].clone(),
        their_lines[1].clone(),
        our_lines[2].clone(),
    ]);
    assert!(DurableReplicationStore::open_unprojected(params(ours.path())).is_err());

    // Seq 2 missing.
    with_inline(vec![our_lines[0].clone(), our_lines[2].clone()]);
    assert!(DurableReplicationStore::open_unprojected(params(ours.path())).is_err());

    // The unbroken chain opens.
    with_inline(our_lines.clone());
    let store = DurableReplicationStore::open_unprojected(params(ours.path())).unwrap();
    assert_eq!(payloads(&store), PAYLOADS[..3]);
}
