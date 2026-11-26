use super::*;
use crate::replication::ReplicationStore;
use org_sync::organization::{append_hq, OrgStore};
use org_sync_proto::Codec;
use std::sync::Arc;

#[test]
fn sqlite_replication_store_passes_eng3_conformance_and_rebuilds_after_runtime_delete() {
    let mut holders = Vec::new();
    crate::replication::conformance::run_eng3_replication_suite(|spec| {
        let dir = tempfile::tempdir().unwrap();
        let durable = dir.path().join("durable");
        let runtime = dir.path().join("runtime");
        fs::create_dir_all(&durable).unwrap();
        fs::create_dir_all(&runtime).unwrap();
        let store = SqliteReplicationStore::open(OpenReplicationParams {
            durable_root: &durable,
            runtime_root: &runtime,
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
    {
        let store = SqliteReplicationStore::open(OpenReplicationParams {
            durable_root: &durable,
            runtime_root: &runtime,
            org_id: org_id.clone(),
            local_office: hq_id.clone(),
            hq_office: hq_id.clone(),
            ca,
            epoch: Epoch::new(1).unwrap(),
            incarnation: inc.clone(),
            initial_snapshot_required: false,
        })
        .unwrap();
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
    let reopened = SqliteReplicationStore::open(OpenReplicationParams {
        durable_root: &durable,
        runtime_root: &runtime,
        org_id,
        local_office: hq_id.clone(),
        hq_office: hq_id,
        ca,
        epoch: Epoch::new(1).unwrap(),
        incarnation: inc,
        initial_snapshot_required: false,
    })
    .unwrap();
    assert_eq!(reopened.sync_status().unwrap().seq, 1);
    assert_eq!(reopened.org_events_after(1, 0).unwrap().len(), 1);
}
