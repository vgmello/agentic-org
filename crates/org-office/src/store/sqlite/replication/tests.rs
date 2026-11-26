use std::fs;
use std::path::Path;
use std::sync::Arc;

use org_sync::organization::{append_hq, OrgStore};
use org_sync_proto::{Codec, Digest, Epoch, OfficeId, OrgId, OriginIncarnation};

use super::SqliteReplicationProjection;
use crate::replication::durable::{DurableReplicationParams, DurableReplicationStore};
use crate::replication::ReplicationStore;

/// The SQLite engine's replication store: the durable state with the SQLite projection.
fn open_sqlite(runtime: &Path, params: DurableReplicationParams<'_>) -> DurableReplicationStore {
    let office = params.local_office.clone();
    DurableReplicationStore::open(params, |data| {
        SqliteReplicationProjection::open(runtime, &office, data)
    })
    .unwrap()
}

#[test]
fn sqlite_replication_store_passes_eng3_conformance_and_rebuilds_after_runtime_delete() {
    let mut holders = Vec::new();
    crate::replication::conformance::run_eng3_replication_suite(|spec| {
        let dir = tempfile::tempdir().unwrap();
        let durable = dir.path().join("durable");
        let runtime = dir.path().join("runtime");
        fs::create_dir_all(&durable).unwrap();
        fs::create_dir_all(&runtime).unwrap();
        let store = open_sqlite(
            &runtime,
            DurableReplicationParams {
                durable_root: &durable,
                org_id: spec.org_id,
                local_office: spec.local_office,
                hq_office: spec.hq_office,
                ca: spec.ca,
                epoch: spec.epoch,
                incarnation: spec.incarnation,
                initial_snapshot_required: spec.initial_snapshot_required,
            },
        );
        holders.push(dir);
        Box::new(store)
    });

    // Verify durable survival + runtime/ delete-and-rebuild on SQLite.
    let dir = tempfile::tempdir().unwrap();
    let durable = dir.path().join("durable");
    let runtime = dir.path().join("runtime");
    fs::create_dir_all(&durable).unwrap();
    fs::create_dir_all(&runtime).unwrap();
    let org_id = OrgId::new("0195a000-0000-7000-8000-000000000001").unwrap();
    let hq_id = OfficeId::new("0195a000-0000-7000-8000-000000000002").unwrap();
    let inc = OriginIncarnation::new("0195a000-0000-7000-8000-000000000004").unwrap();
    let ca = Digest::from_bytes([0x42; 32]);
    let params = || DurableReplicationParams {
        durable_root: &durable,
        org_id: org_id.clone(),
        local_office: hq_id.clone(),
        hq_office: hq_id.clone(),
        ca,
        epoch: Epoch::new(1).unwrap(),
        incarnation: inc.clone(),
        initial_snapshot_required: false,
    };
    {
        let store = open_sqlite(&runtime, params());
        let codec = Codec::new("org.native.changes", 1).unwrap();
        OrgStore::transact(
            &store,
            Arc::new(move |tx| append_hq(tx, codec.clone(), Arc::from(b"persisted".as_slice()))),
        )
        .unwrap();
    }
    // Delete rebuildable runtime/ completely and reopen.
    fs::remove_dir_all(&runtime).unwrap();
    fs::create_dir_all(&runtime).unwrap();
    let reopened = open_sqlite(&runtime, params());
    assert_eq!(reopened.sync_status().unwrap().seq, 1);
    assert_eq!(reopened.org_events_after(1, 0).unwrap().len(), 1);
    let db = runtime
        .join(hq_id.as_str())
        .join("protected")
        .join("replication.db");
    assert!(db.is_file(), "the projection is rebuilt under runtime/");
}

const ORG: &str = "0195a000-0000-7000-8000-000000000001";
const HQ: &str = "0195a000-0000-7000-8000-000000000002";
const SAT: &str = "0195a000-0000-7000-8000-000000000003";
const INC: &str = "0195a000-0000-7000-8000-000000000004";

fn hq_params(durable: &Path) -> DurableReplicationParams<'_> {
    DurableReplicationParams {
        durable_root: durable,
        org_id: OrgId::new(ORG).unwrap(),
        local_office: OfficeId::new(HQ).unwrap(),
        hq_office: OfficeId::new(HQ).unwrap(),
        ca: Digest::from_bytes([0x42; 32]),
        epoch: Epoch::new(1).unwrap(),
        incarnation: OriginIncarnation::new(INC).unwrap(),
        initial_snapshot_required: false,
    }
}

/// Every row of the four projected tables, in key order.
fn dump(runtime: &Path) -> Vec<String> {
    let db = runtime.join(HQ).join("protected").join("replication.db");
    let conn = rusqlite::Connection::open(db).unwrap();
    let mut rows = Vec::new();
    for query in [
        "SELECT org_id, local_office, hq_office, epoch, seq, highest_epoch, protocol_level, \
         restore_pending, initial_snapshot_required FROM replication_meta",
        "SELECT epoch, seq, digest_hex, codec_name, codec_version FROM replication_org_events \
         ORDER BY epoch, seq",
        "SELECT seq, record_id, digest_hex FROM replication_work_outbox ORDER BY seq",
        "SELECT message_id, destination_office, deadline_ms FROM replication_message_outbox \
         ORDER BY message_id",
    ] {
        let mut stmt = conn.prepare(query).unwrap();
        let columns = stmt.column_count();
        let found = stmt
            .query_map([], |row| {
                let cells: Vec<String> = (0..columns)
                    .map(|i| format!("{:?}", row.get::<_, rusqlite::types::Value>(i).unwrap()))
                    .collect();
                Ok(format!("{query}: {}", cells.join("|")))
            })
            .unwrap();
        rows.extend(found.map(Result::unwrap));
    }
    rows
}

fn append_event(store: &DurableReplicationStore, payload: &'static [u8]) {
    let codec = Codec::new("org.native.changes", 1).unwrap();
    OrgStore::transact(
        store,
        Arc::new(move |tx| append_hq(tx, codec.clone(), Arc::from(payload))),
    )
    .unwrap();
}

fn message(id: &str) -> org_sync::messages::Message {
    org_sync::messages::Message {
        org_id: OrgId::new(ORG).unwrap(),
        origin: OfficeId::new(HQ).unwrap(),
        destination: OfficeId::new(SAT).unwrap(),
        id: id.into(),
        fingerprint: Digest::from_bytes([0x11; 32]),
        payload: Arc::from(b"hello".as_slice()),
        deadline_ms: 1_700_000_060_000,
    }
}

/// TASK-140: the SQLite projection is brought up to each commit by touching only what changed;
/// after every kind of change it holds exactly the rows a full rebuild from the durable state
/// would.
#[test]
fn the_incremental_projection_always_equals_a_full_rebuild() {
    let dir = tempfile::tempdir().unwrap();
    let durable = dir.path().join("durable");
    let runtime = dir.path().join("runtime");
    fs::create_dir_all(&durable).unwrap();
    fs::create_dir_all(&runtime).unwrap();
    let store = open_sqlite(&runtime, hq_params(&durable));
    let mut rebuilds = 0;
    let mut check = |what: &str| {
        rebuilds += 1;
        let rebuilt = dir.path().join(format!("rebuilt-{rebuilds}"));
        fs::create_dir_all(&rebuilt).unwrap();
        drop(open_sqlite(&rebuilt, hq_params(&durable)));
        assert_eq!(dump(&runtime), dump(&rebuilt), "{what}");
    };
    check("opened");

    append_event(&store, b"one");
    append_event(&store, b"two");
    check("events appended");

    let codec = Codec::new("work-delta", 1).unwrap();
    let work = |n: u64| vec![Arc::from(format!("work-{n}").as_bytes())];
    store
        .publish_satellite_work_entries(codec.clone(), "task-1", work(1), INC, 1, 1)
        .unwrap();
    store
        .publish_satellite_work_entries(codec.clone(), "task-2", work(2), INC, 2, 2)
        .unwrap();
    check("work queued");
    store.ack_work_through(1).unwrap();
    check("work acknowledged");

    for id in ["m-1", "m-2"] {
        org_sync::messages::enqueue(&store, message(id), 1_700_000_000_000).unwrap();
    }
    check("messages queued");
    assert!(store.retire_undeliverable_message("m-1", SAT).unwrap());
    check("a message retired");

    append_event(&store, b"three");
    store.record_replicated_op_id("op-1").unwrap();
    check("an event and an unprojected field");

    store
        .reset_for_re_enrollment(
            OfficeId::new("0195a000-0000-7000-8000-000000000009").unwrap(),
            Digest::from_bytes([0x43; 32]),
            Epoch::new(2).unwrap(),
        )
        .unwrap();
    check("events cleared and the work outbox rebased");
    assert!(dump(&runtime)
        .iter()
        .all(|row| !row.contains("replication_org_events ORDER")));
}

/// Every file under `root`, by its path below it, with its bytes.
fn tree(root: &Path) -> std::collections::BTreeMap<String, Vec<u8>> {
    let mut found = std::collections::BTreeMap::new();
    let mut pending = vec![root.to_owned()];
    while let Some(folder) = pending.pop() {
        for entry in fs::read_dir(&folder).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                pending.push(path);
            } else {
                let name = path
                    .strip_prefix(root)
                    .unwrap()
                    .to_str()
                    .unwrap()
                    .to_owned();
                found.insert(name, fs::read(&path).unwrap());
            }
        }
    }
    found
}

/// TASK-172 with the event log: the memory and SQLite engines leave byte-identical durable
/// folders through appends, an unprojected change and a clear.
#[test]
fn memory_and_sqlite_engines_leave_identical_durable_replication_files() {
    let run = |sqlite: bool| {
        let dir = tempfile::tempdir().unwrap();
        let durable = dir.path().join("durable");
        let runtime = dir.path().join("runtime");
        fs::create_dir_all(&durable).unwrap();
        fs::create_dir_all(&runtime).unwrap();
        let store = if sqlite {
            open_sqlite(&runtime, hq_params(&durable))
        } else {
            DurableReplicationStore::open_unprojected(hq_params(&durable)).unwrap()
        };
        let mut snapshots = vec![tree(&durable)];
        append_event(&store, b"one");
        append_event(&store, b"two");
        snapshots.push(tree(&durable));
        store.record_replicated_op_id("op-1").unwrap();
        snapshots.push(tree(&durable));
        store
            .reset_for_re_enrollment(
                OfficeId::new("0195a000-0000-7000-8000-000000000009").unwrap(),
                Digest::from_bytes([0x43; 32]),
                Epoch::new(2).unwrap(),
            )
            .unwrap();
        snapshots.push(tree(&durable));
        snapshots
    };
    let memory = run(false);
    let sqlite = run(true);
    assert_eq!(memory, sqlite);
    let names: Vec<&String> = memory[1].keys().collect();
    assert_eq!(
        names,
        [
            &format!("local/offices/{HQ}/replication/events.v1.ndjson"),
            &format!("local/offices/{HQ}/replication/state.v1.json"),
        ]
    );
    assert_eq!(memory[0].len(), 1, "no log before the first event");
}
