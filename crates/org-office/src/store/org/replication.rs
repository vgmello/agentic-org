//! The organization log, materialized state, and its federation cursor share a writer job.

use std::sync::Arc;

use org_proto::org::OrgChange;
use org_sync::error::{Refusal, StoreError, SyncError};
use org_sync::organization::{
    OrgAttempt, OrgCheckpoint, OrgEvent, OrgOutput, OrgTxn, ValidatedSnapshot,
};
use org_sync_proto::{Codec, Digest, Epoch, OfficeId, OrgId, Position};
use rusqlite::{params, Connection, OptionalExtension, Transaction};
use serde::Deserialize;

use crate::error::OfficeError;
use crate::store::sync::storage_error;
use crate::store::Store;
use crate::{time, ulid};

use super::{apply_changes, json, Answer, KEEP_MS};

const CODEC: &str = "org.native.changes";
const PROTOCOL: u32 = 1;

pub(super) fn create_tables(tx: &Transaction<'_>) -> rusqlite::Result<()> {
    tx.execute_batch(
        "CREATE TABLE org_replica (
             singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
             org_id TEXT NOT NULL, local_office TEXT NOT NULL, hq_office TEXT NOT NULL,
             ca BLOB NOT NULL CHECK (length(ca) = 32),
             epoch INTEGER NOT NULL CHECK (epoch > 0), seq INTEGER NOT NULL CHECK (seq >= 0),
             digest BLOB NOT NULL CHECK (length(digest) = 32),
             highest_epoch INTEGER NOT NULL CHECK (highest_epoch >= epoch),
             restore_pending INTEGER NOT NULL CHECK (restore_pending IN (0, 1)),
             pending_base BLOB CHECK (length(pending_base) = 32),
             protocol_level INTEGER NOT NULL CHECK (protocol_level > 0),
             CHECK ((epoch < highest_epoch) = (pending_base IS NOT NULL)),
             CHECK (epoch = highest_epoch OR restore_pending = 1)
         ) STRICT;
         CREATE TABLE org_replica_events (
             epoch INTEGER NOT NULL CHECK (epoch > 0), seq INTEGER NOT NULL CHECK (seq > 0),
             digest BLOB NOT NULL CHECK (length(digest) = 32),
             previous BLOB NOT NULL CHECK (length(previous) = 32),
             codec TEXT NOT NULL, codec_version INTEGER NOT NULL, payload BLOB NOT NULL,
             PRIMARY KEY (epoch, seq)
         ) STRICT;",
    )
}

/// Enrollment initializes the pin and checkpoint. An unbound office has no federation
/// authority, even if it has a legacy native organization log.
pub(crate) fn transact_replication(
    store: &Store,
    attempt: Arc<OrgAttempt>,
) -> Result<OrgOutput, SyncError> {
    store.sync_write(move |db| attempt(&mut OrganizationTxn(db)))
}

struct OrganizationTxn<'a>(&'a Connection);

fn epoch(value: i64) -> Result<Epoch, StoreError> {
    Epoch::new(u64::try_from(value).map_err(storage_error)?).map_err(storage_error)
}

fn int(value: u64) -> Result<i64, StoreError> {
    i64::try_from(value).map_err(storage_error)
}

impl OrgTxn for OrganizationTxn<'_> {
    fn checkpoint(&mut self) -> Result<OrgCheckpoint, StoreError> {
        let (
            org,
            local,
            hq,
            ca,
            current_epoch,
            seq,
            digest,
            highest,
            restore_pending,
            pending,
            protocol_level,
        ) = self
            .0
            .query_row(
                "SELECT org_id, local_office, hq_office, ca, epoch, seq, digest, highest_epoch,
                    restore_pending, pending_base, protocol_level
             FROM org_replica WHERE singleton = 1",
                [],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get(3)?,
                        row.get(4)?,
                        row.get::<_, i64>(5)?,
                        row.get(6)?,
                        row.get(7)?,
                        row.get(8)?,
                        row.get::<_, Option<[u8; 32]>>(9)?,
                        row.get(10)?,
                    ))
                },
            )
            .map_err(storage_error)?;
        Ok(OrgCheckpoint {
            org_id: OrgId::new(org).map_err(storage_error)?,
            local_office: OfficeId::new(local).map_err(storage_error)?,
            hq_office: OfficeId::new(hq).map_err(storage_error)?,
            ca: Digest::from_bytes(ca),
            position: Position {
                epoch: epoch(current_epoch)?,
                seq: u64::try_from(seq).map_err(storage_error)?,
            },
            digest: Digest::from_bytes(digest),
            highest_epoch: epoch(highest)?,
            restore_pending,
            pending_restore_base: pending.map(Digest::from_bytes),
            protocol_level,
        })
    }

    fn digest_at(&mut self, position: Position) -> Result<Option<Digest>, StoreError> {
        self.0
            .query_row(
                "SELECT digest FROM org_replica_events WHERE (epoch, seq) = (?1, ?2)",
                (int(position.epoch.get())?, int(position.seq)?),
                |row| row.get(0).map(Digest::from_bytes),
            )
            .optional()
            .map_err(storage_error)
    }

    fn supports_codec(&mut self, codec: &Codec) -> Result<bool, StoreError> {
        Ok(codec.id() == CODEC && codec.version() == 1)
    }

    fn supports_protocol(&mut self, level: u32) -> Result<bool, StoreError> {
        Ok(level == PROTOCOL)
    }

    fn apply_event(&mut self, event: &OrgEvent) -> Result<(), SyncError> {
        let record: NativeEvent =
            serde_json::from_slice(&event.payload).map_err(|_| Refusal::IntegrityFailure)?;
        record.apply(self.0, event.position)
    }

    fn record_event(&mut self, event: &OrgEvent, next: &OrgCheckpoint) -> Result<(), StoreError> {
        self.0
            .execute(
                "INSERT INTO org_replica_events VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![
                    int(event.position.epoch.get())?,
                    int(event.position.seq)?,
                    event.digest.as_bytes(),
                    event.previous.as_bytes(),
                    event.codec.id(),
                    event.codec.version(),
                    &*event.payload
                ],
            )
            .map_err(storage_error)?;
        self.set_authority(next)
    }

    fn install_snapshot(
        &mut self,
        _snapshot: &ValidatedSnapshot,
        _next: &OrgCheckpoint,
    ) -> Result<(), SyncError> {
        // Only the event codec is enabled until receiver-key snapshot staging is wired.
        Err(Refusal::UnsupportedVersion.into())
    }

    fn set_authority(&mut self, next: &OrgCheckpoint) -> Result<(), StoreError> {
        let changed = self
            .0
            .execute(
                "UPDATE org_replica SET epoch = ?1, seq = ?2, digest = ?3,
                    highest_epoch = ?4, restore_pending = ?5, pending_base = ?6
             WHERE singleton = 1",
                params![
                    int(next.position.epoch.get())?,
                    int(next.position.seq)?,
                    next.digest.as_bytes(),
                    int(next.highest_epoch.get())?,
                    next.restore_pending,
                    next.pending_restore_base.as_ref().map(Digest::as_bytes)
                ],
            )
            .map_err(storage_error)?;
        if changed != 1 {
            return Err(storage_error(rusqlite::Error::QueryReturnedNoRows));
        }
        Ok(())
    }
}

/// Version-one native log content, including the retained outcome if this is a command.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct NativeEvent {
    command_id: String,
    at: String,
    changes: Vec<OrgChange>,
    answer: Option<NativeAnswer>,
}

// The provisional codec carries pre-X-02 outcomes. Refuse extra fields so newer
// principal data cannot be accepted and dropped by this adapter.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct NativeAnswer {
    params_hash: String,
    refusal: Option<org_proto::ApiError>,
}

impl NativeEvent {
    fn apply(self, db: &Connection, position: Position) -> Result<(), SyncError> {
        if time::parse_rfc3339_ms(&self.at).is_none() {
            return Err(Refusal::IntegrityFailure.into());
        }
        let expires = match &self.answer {
            Some(answer) => {
                if answer.params_hash.len() != 64
                    || !answer
                        .params_hash
                        .bytes()
                        .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
                    || (answer.refusal.is_some() && !self.changes.is_empty())
                {
                    return Err(Refusal::IntegrityFailure.into());
                }
                Some(ulid::time_ms(&self.command_id).ok_or(Refusal::IntegrityFailure)? + KEEP_MS)
            }
            None if self.command_id == "genesis"
                && position.epoch.get() == 1
                && position.seq == 1 =>
            {
                None
            }
            None => return Err(Refusal::IntegrityFailure.into()),
        };

        apply_changes(db, &self.changes).map_err(|error| match error {
            OfficeError::Invalid(_) => SyncError::from(Refusal::IntegrityFailure),
            OfficeError::Storage(ref cause) if matches!(cause.downcast_ref(), Some(rusqlite::Error::SqliteFailure(failure, _)) if failure.code == rusqlite::ErrorCode::ConstraintViolation) => SyncError::from(Refusal::InvariantFailure),
            error => storage_error(error).into(),
        })?;

        let mut kind = "org.changed";
        let mut body = json(&self.changes);
        let (epoch, seq) = (int(position.epoch.get())?, int(position.seq)?);
        if let Some((answer, expires)) = self.answer.zip(expires) {
            let result_position = match &answer.refusal {
                Some(refusal) => {
                    kind = "org.rejected";
                    body = json(refusal);
                    (0, 0)
                }
                None => (epoch, seq),
            };
            let inserted = db
                .execute(
                    "INSERT INTO commands (command_id, epoch, seq, expires_at, result)
                 VALUES (?1, ?2, ?3, ?4, ?5) ON CONFLICT(command_id) DO NOTHING",
                    (
                        &self.command_id,
                        result_position.0,
                        result_position.1,
                        time::format_epoch_ms(expires),
                        json(&Answer {
                            params_hash: answer.params_hash,
                            refusal: answer.refusal,
                        }),
                    ),
                )
                .map_err(storage_error)?;
            if inserted != 1 {
                return Err(Refusal::Divergence.into());
            }
        }
        db.execute(
            "INSERT INTO events (epoch, seq, kind, command_id, at, body)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            (epoch, seq, kind, self.command_id, self.at, body),
        )
        .map_err(storage_error)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use org_sync::error::MAX_STORE_RETRIES;
    use org_sync::organization::{self, PeerIdentity, PeerRole, VerifiedPeer};
    use serde_json::json;
    use tempfile::TempDir;

    use super::*;
    use crate::{Key, NewOffice, Office, OfficeRole};

    const COMMAND: &str = "01ARZ3NDEKTSV4RRFFQ69G5FAV";
    const SECOND: &str = "01ARZ3NDEKTSV4RRFFQ69G5FAW";

    struct Hq;

    impl VerifiedPeer for Hq {
        fn identity(&self) -> PeerIdentity {
            PeerIdentity {
                org_id: OrgId::new("ORG").unwrap(),
                office_id: OfficeId::new("HQ").unwrap(),
                role: PeerRole::Hq,
            }
        }
    }

    fn key() -> Key {
        Key::from_bytes([3; 32])
    }

    fn replica() -> (TempDir, Office) {
        let dir = TempDir::new().unwrap();
        let office = Office::create(
            dir.path().join("SAT"),
            &NewOffice {
                office_id: "SAT".into(),
                office_name: "Satellite".into(),
                role: OfficeRole::Hq,
                org_id: "ORG".into(),
                org_name: "Organization".into(),
            },
            key(),
        )
        .unwrap();
        // Simulate trusted enrollment at the already materialized genesis checkpoint.
        office.org_store().run_sql_for_test(
            "INSERT INTO org_replica VALUES (1, 'ORG', 'SAT', 'HQ', zeroblob(32),
                                            1, 1, zeroblob(32), 1, 0, NULL, 1)",
        );
        (dir, office)
    }

    fn event(seq: u64, previous: Digest, id: &str, name: &str) -> OrgEvent {
        OrgEvent::new(
            OrgId::new("ORG").unwrap(),
            Position { epoch: Epoch::new(1).unwrap(), seq },
            previous,
            Codec::new(CODEC, 1).unwrap(),
            serde_json::to_vec(&json!({
                "command_id": id,
                "at": "2026-11-08T12:00:00.000Z",
                "changes": [{"type": "team_upserted", "team": {"id": name, "name": name, "version": 1}}],
                "answer": {"params_hash": "12".repeat(32)},
            })).unwrap(),
        ).unwrap()
    }

    #[test]
    fn applied_change_outcome_and_digest_survive_reopen_and_replay() {
        let (dir, office) = replica();
        let first = event(2, Digest::ZERO, COMMAND, "first");
        let second = event(3, first.digest, SECOND, "second");
        for incoming in [&first, &second] {
            assert!(
                matches!(organization::apply(&office, &Hq, incoming.clone()).unwrap(),
                OrgOutput::Ack { position, digest, duplicate: false }
                    if position == incoming.position && digest == incoming.digest)
            );
        }
        drop(office);

        let office = Office::open(dir.path().join("SAT"), key()).unwrap();
        assert_eq!(office.org_snapshot().unwrap().teams.len(), 2);
        assert!(matches!(organization::apply(&office, &Hq, first).unwrap(),
            OrgOutput::Ack { position, digest, duplicate: true }
                if position == second.position && digest == second.digest));
        let answer = office
            .org_store()
            .read(|db| {
                db.query_row(
                    "SELECT seq, result FROM commands WHERE command_id = ?1",
                    [COMMAND],
                    |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?)),
                )
                .map_err(OfficeError::storage)
            })
            .unwrap();
        assert_eq!(answer.0, 2);
        assert_eq!(
            serde_json::from_str::<Answer>(&answer.1)
                .unwrap()
                .params_hash,
            "12".repeat(32)
        );
    }

    #[test]
    fn failed_log_insert_rolls_back_domain_and_outcome_before_acknowledgment() {
        let (_dir, office) = replica();
        office.org_store().run_sql_for_test(
            "CREATE TRIGGER refuse_log BEFORE INSERT ON org_replica_events
             BEGIN SELECT RAISE(ABORT, 'injected failure'); END;",
        );
        let incoming = event(2, Digest::ZERO, COMMAND, "first");
        let seen = office.org_change_token();
        assert!(
            matches!(organization::apply(&office, &Hq, incoming.clone()),
            Err(SyncError::Storage(error)) if !error.is_retryable())
        );
        assert_eq!(office.org_change_token(), seen);
        let snapshot = office.org_snapshot().unwrap();
        assert_eq!(snapshot.position.seq, 1);
        assert!(snapshot.teams.is_empty());
        let count = office
            .org_store()
            .read(|db| {
                db.query_row("SELECT count(*) FROM commands", [], |row| {
                    row.get::<_, i64>(0)
                })
                .map_err(OfficeError::storage)
            })
            .unwrap();
        assert_eq!(count, 0);
        office
            .org_store()
            .run_sql_for_test("DROP TRIGGER refuse_log;");
        assert!(matches!(
            organization::apply(&office, &Hq, incoming).unwrap(),
            OrgOutput::Ack {
                duplicate: false,
                ..
            }
        ));
    }

    #[test]
    fn busy_replays_the_entire_job_up_to_the_retry_limit() {
        let (_dir, office) = replica();
        let attempts = Arc::new(AtomicUsize::new(0));
        let seen = Arc::clone(&attempts);
        let result = office.org_store().sync_write(move |db| {
            seen.fetch_add(1, Ordering::Relaxed);
            db.execute("INSERT INTO meta VALUES ('retry_test', 'partial')", [])
                .map_err(storage_error)?;
            Err::<(), _>(
                storage_error(rusqlite::Error::SqliteFailure(
                    rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_BUSY),
                    None,
                ))
                .into(),
            )
        });
        assert!(matches!(result, Err(SyncError::Storage(error)) if error.is_retryable()));
        assert_eq!(attempts.load(Ordering::Relaxed), MAX_STORE_RETRIES + 1);
        assert_eq!(office.org_store().meta("retry_test").unwrap(), None);
    }

    #[test]
    fn the_replication_schema_upgrade_preserves_an_existing_organization() {
        let (dir, office) = replica();
        let original = office.org_snapshot().unwrap();
        office.org_store().run_sql_for_test(
            "DROP TABLE org_replica_events; DROP TABLE org_replica; PRAGMA user_version = 1;",
        );
        drop(office);

        let office = Office::open(dir.path().join("SAT"), key()).unwrap();
        assert_eq!(office.org_snapshot().unwrap(), original);
        office.org_store().run_sql_for_test(
            "INSERT INTO org_replica VALUES (1, 'ORG', 'SAT', 'HQ', zeroblob(32),
                                            1, 1, zeroblob(32), 1, 0, NULL, 1)",
        );
        assert!(matches!(
            organization::apply(&office, &Hq, event(2, Digest::ZERO, COMMAND, "first")).unwrap(),
            OrgOutput::Ack {
                duplicate: false,
                ..
            }
        ));
    }
}
