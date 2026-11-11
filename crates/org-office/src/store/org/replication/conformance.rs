//! Real SQLCipher adapter contract, including durable writer COMMIT failures.
use std::sync::{
    atomic::{AtomicBool, AtomicU32, Ordering},
    Arc, Mutex,
};

use org_proto::org::TeamCreateParams;
use org_sync::organization::{OrgStore, PeerIdentity, PeerRole, VerifiedPeer};
use org_sync::testkit::conformance::{organization_adapter, OrganizationHarness};
use tempfile::TempDir;

use super::*;
use crate::rules::OrgCommand;
use crate::{Key, NewOffice, Office, OfficeRole, SnapshotStageMode};

const LIMIT: u64 = 4 * 1024 * 1024;

fn key() -> Key {
    Key::from_bytes([3; 32])
}

struct HqAt(Epoch);

impl VerifiedPeer for HqAt {
    fn identity(&self) -> PeerIdentity {
        PeerIdentity {
            authority: self.0,
            org_id: OrgId::new("ORG").unwrap(),
            office_id: OfficeId::new("HQ").unwrap(),
            role: PeerRole::Hq,
        }
    }
}

#[derive(Clone)]
struct SqlCipherHarness {
    directory: Arc<TempDir>,
    local: &'static str,
    office: Arc<Mutex<Option<Office>>>,
    protocol: Arc<AtomicU32>,
    fail_commit: Arc<AtomicBool>,
}

impl SqlCipherHarness {
    fn new(local: &'static str) -> Self {
        let directory = Arc::new(TempDir::new().unwrap());
        let office = Office::create(
            directory.path().join(local),
            &NewOffice {
                office_id: local.into(),
                office_name: local.into(),
                role: OfficeRole::Hq,
                org_id: "ORG".into(),
                org_name: "Organization".into(),
            },
            key(),
        )
        .unwrap();
        if local == "HQ" {
            office.bind_replication_hq(Digest::ZERO, LIMIT).unwrap();
        } else {
            office
                .bind_replication_satellite(
                    OfficeId::new("HQ").unwrap(),
                    Digest::ZERO,
                    Epoch::new(1).unwrap(),
                    Digest::ZERO,
                )
                .unwrap();
        }
        Self {
            directory,
            local,
            office: Arc::new(Mutex::new(Some(office))),
            protocol: Arc::new(AtomicU32::new(1)),
            fail_commit: Arc::new(AtomicBool::new(false)),
        }
    }
}

impl OrgStore for SqlCipherHarness {
    fn transact(&self, attempt: Arc<OrgAttempt>) -> Result<OrgOutput, SyncError> {
        let guard = self.office.lock().unwrap();
        let store = guard.as_ref().unwrap().org_store();
        let protocol = self.protocol.load(Ordering::SeqCst);
        let result = store.sync_write(move |db| {
            attempt(&mut FixtureTxn {
                native: OrganizationTxn(db),
                protocol,
            })
        });
        if self.fail_commit.swap(false, Ordering::SeqCst) {
            store.run_sql_for_test("DROP TRIGGER fail_conformance_commit");
        }
        result
    }
}

impl OrganizationHarness for SqlCipherHarness {
    fn publish_fixture(&self, tag: u8) -> OrgEvent {
        let guard = self.office.lock().unwrap();
        let office = guard.as_ref().unwrap();
        let before = office.replication_checkpoint().unwrap();
        office
            .submit_org(OrgCommand::TeamCreate(TeamCreateParams {
                command_id: ulid::generate().unwrap(),
                team_id: ulid::generate().unwrap(),
                name: format!("Conformance {tag}"),
                owner_team_id: None,
            }))
            .unwrap()
            .unwrap();
        let mut events = office
            .replication_events_after(before.position, before.digest, 1)
            .unwrap()
            .unwrap();
        assert_eq!(events.len(), 1);
        events.pop().unwrap()
    }

    fn inspect_checkpoint(&self) -> OrgCheckpoint {
        let guard = self.office.lock().unwrap();
        staging_checkpoint(guard.as_ref().unwrap().org_store())
            .unwrap()
            .0
    }

    fn domain_application_count(&self) -> usize {
        self.office
            .lock()
            .unwrap()
            .as_ref()
            .unwrap()
            .org_store()
            .read(|db| {
                db.query_row("SELECT count(*) FROM teams", [], |row| {
                    row.get::<_, u32>(0).map(|count| count as usize)
                })
                .map_err(OfficeError::storage)
            })
            .unwrap()
    }

    fn fail_next_commit(&self) {
        assert!(!self.fail_commit.swap(true, Ordering::SeqCst));
        self.office
            .lock()
            .unwrap()
            .as_ref()
            .unwrap()
            .org_store()
            .run_sql_for_test(
                "CREATE TABLE IF NOT EXISTS conformance_parent (id INTEGER PRIMARY KEY);
             CREATE TABLE IF NOT EXISTS conformance_child (
                 id INTEGER REFERENCES conformance_parent(id) DEFERRABLE INITIALLY DEFERRED
             );
             CREATE TRIGGER fail_conformance_commit AFTER UPDATE ON org_replica
             BEGIN INSERT INTO conformance_child VALUES (999); END;",
            );
    }

    fn reopen(&self) -> Self {
        let mut guard = self.office.lock().unwrap();
        drop(guard.take().unwrap());
        *guard = Some(Office::open(self.directory.path().join(self.local), key()).unwrap());
        self.clone()
    }

    fn require_initial_snapshot(&self) {
        // Enrollment performed the write; assert it instead of manufacturing a flag.
        assert!(self.initial_snapshot_required());
    }

    fn initial_snapshot_required(&self) -> bool {
        self.office
            .lock()
            .unwrap()
            .as_ref()
            .unwrap()
            .org_store()
            .read(|db| awaiting_snapshot(db).map_err(OfficeError::storage))
            .unwrap()
    }

    fn stage_snapshot(&self, source: &Self) -> ValidatedSnapshot {
        let transport_key = Key::from_bytes([9; 32]);
        let exported = source
            .office
            .lock()
            .unwrap()
            .as_ref()
            .unwrap()
            .export_replication_snapshot(&transport_key, LIMIT)
            .unwrap();
        self.office
            .lock()
            .unwrap()
            .as_ref()
            .unwrap()
            .stage_replication_snapshot(
                &HqAt(exported.manifest.position.epoch),
                &exported.manifest,
                &transport_key,
                &*exported.ciphertext,
                LIMIT,
                SnapshotStageMode::Normal,
            )
            .unwrap()
    }

    fn support_protocol_fixture(&self, level: u32) {
        self.protocol.store(level, Ordering::SeqCst);
    }

    fn publish_protocol_raise_fixture(&self) -> OrgEvent {
        let native = NativeEvent {
            command_id: ulid::generate().unwrap(),
            at: time::format_epoch_ms(time::now_ms()),
            changes: vec![OrgChange::TeamUpserted {
                team: org_proto::org::Team {
                    id: ulid::generate().unwrap(),
                    name: "Conformance 255".into(),
                    owner_team_id: None,
                    lead_member_id: None,
                    version: 1,
                },
            }],
            answer: Some(NativeAnswer {
                params_hash: "ab".repeat(32),
                refusal: None,
            }),
        };
        let payload: Arc<[u8]> = serde_json::to_vec(&native).unwrap().into();
        let OrgOutput::Published(event) = self
            .transact(Arc::new(move |tx| {
                organization::append_hq(tx, Codec::new(CODEC, 1).unwrap(), payload.clone())
            }))
            .unwrap()
        else {
            panic!("HQ fixture did not publish an event");
        };
        assert_eq!(self.inspect_checkpoint().protocol_level, 2);
        event
    }
}

// Native v1 has no protocol-upgrade command. This fixture delegates native decoding,
// domain apply, event recording and authority persistence to production. It injects
// only binary support and a policy write in the same transaction, as the suite requests.
struct FixtureTxn<'a> {
    native: OrganizationTxn<'a>,
    protocol: u32,
}

impl OrgTxn for FixtureTxn<'_> {
    fn checkpoint(&mut self) -> Result<OrgCheckpoint, StoreError> {
        self.native.checkpoint()
    }
    fn requires_initial_snapshot(&mut self) -> Result<bool, StoreError> {
        self.native.requires_initial_snapshot()
    }
    fn digest_at(&mut self, position: Position) -> Result<Option<Digest>, StoreError> {
        self.native.digest_at(position)
    }
    fn supports_codec(&mut self, codec: &Codec) -> Result<bool, StoreError> {
        self.native.supports_codec(codec)
    }
    fn supports_protocol(&mut self, level: u32) -> Result<bool, StoreError> {
        Ok(level > 0 && level <= self.protocol)
    }
    fn apply_event(&mut self, event: &OrgEvent) -> Result<(), SyncError> {
        self.native.apply_event(event)?;
        if NativeEvent::decode(&event.payload)?.changes.iter().any(
            |change| matches!(change, OrgChange::TeamUpserted { team } if team.name == "Conformance 255"),
        ) {
            assert!(self.protocol >= 2);
            self.native.0.execute("UPDATE org_replica SET protocol_level = 2", [])
                .map_err(storage_error)?;
        }
        Ok(())
    }
    fn record_event(&mut self, event: &OrgEvent, next: &OrgCheckpoint) -> Result<(), StoreError> {
        self.native.record_event(event, next)
    }
    fn install_snapshot(
        &mut self,
        snapshot: &ValidatedSnapshot,
        next: &OrgCheckpoint,
    ) -> Result<(), SyncError> {
        self.native.install_snapshot(snapshot, next)
    }
    fn set_authority(&mut self, next: &OrgCheckpoint) -> Result<(), StoreError> {
        self.native.set_authority(next)
    }
}

#[test]
fn sqlcipher_implements_the_organization_adapter_contract() {
    organization_adapter(&SqlCipherHarness::new("HQ"), &SqlCipherHarness::new("SAT"));
}
