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
