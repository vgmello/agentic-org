//! The organization log, materialized state, and its federation cursor share a writer job.

mod relationships;

use std::sync::Arc;

use org_proto::org::OrgChange;
use org_sync::error::{Refusal, StoreError, SyncError};
use org_sync::organization::{
    self, OrgAttempt, OrgCheckpoint, OrgEvent, OrgOutput, OrgTxn, ValidatedSnapshot,
};
use org_sync_proto::{event_digest_v1, Codec, Digest, Epoch, OfficeId, OrgId, Position};
use rusqlite::{params, Connection, OptionalExtension, Transaction};
use serde::{Deserialize, Serialize};

use crate::error::OfficeError;
use crate::store::sync::{storage_error, sync_error};
use crate::store::Store;
use crate::{time, ulid};

use super::{apply_changes, json, snapshot, Answer, KEEP_MS};

const CODEC: &str = "org.native.changes";
// X-05 limits the encoded JSON frame to 1 MiB. Reserve 1 KiB for its envelope,
// whose IDs, digests, position and codec fields have bounded representations.
const MAX_LIVE_PAYLOAD_BYTES: usize = (1024 * 1024 - 1024) / 4 * 3;
pub(super) const PROTOCOL: u32 = 1;

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
             awaiting_snapshot INTEGER NOT NULL CHECK (awaiting_snapshot IN (0, 1)),
             native_seq INTEGER NOT NULL CHECK (native_seq >= 0),
             CHECK (awaiting_snapshot = 0 OR local_office <> hq_office),
             CHECK ((epoch < highest_epoch) = (pending_base IS NOT NULL)),
             CHECK (epoch = highest_epoch OR restore_pending = 1)
         ) STRICT;
         CREATE TABLE org_replica_events (
             epoch INTEGER NOT NULL CHECK (epoch > 0), seq INTEGER NOT NULL CHECK (seq > 0),
             digest BLOB NOT NULL CHECK (length(digest) = 32),
             previous BLOB NOT NULL CHECK (length(previous) = 32),
             codec TEXT NOT NULL, codec_version INTEGER NOT NULL, payload BLOB NOT NULL,
             PRIMARY KEY (epoch, seq)
         ) STRICT;
         CREATE TABLE org_replica_base (
             singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
             epoch INTEGER NOT NULL CHECK (epoch > 0), seq INTEGER NOT NULL CHECK (seq >= 0),
             digest BLOB NOT NULL CHECK (length(digest) = 32)
         ) STRICT;",
    )?;
    snapshot::create_table(tx)
}

/// Enrollment initializes the pin and checkpoint. An unbound office has no federation
/// authority, even if it has a legacy native organization log.
pub(crate) fn transact_replication(
    store: &Store,
    attempt: Arc<OrgAttempt>,
) -> Result<OrgOutput, SyncError> {
    store.sync_write(move |db| attempt(&mut OrganizationTxn(db)))
}

pub(crate) enum ReplicationBootstrap {
    Hq {
        max_snapshot_bytes: u64,
    },
    Satellite {
        authority: Epoch,
        epoch_base: Digest,
    },
}

pub(crate) fn bind_replication(
    store: &Store,
    org: OrgId,
    local: OfficeId,
    hq: OfficeId,
    ca: Digest,
    bootstrap: ReplicationBootstrap,
) -> Result<(), SyncError> {
    store.sync_write(move |db| {
        if native_binding(db).map_err(storage_error)?.is_some() {
            let current = OrganizationTxn(db).checkpoint()?;
            return if (current.org_id, current.local_office, current.hq_office, current.ca)
                == (org.clone(), local.clone(), hq.clone(), ca)
            {
                Ok(())
            } else {
                Err(Refusal::NotAuthorized.into())
            };
        }
        let stored_org: String = db.query_row(
            "SELECT value FROM meta WHERE key = 'organization_id'", [], |row| row.get(0),
        ).map_err(storage_error)?;
        if stored_org != org.as_str() {
            return Err(Refusal::InvariantFailure.into());
        }
        let (position, digest) = match bootstrap {
            ReplicationBootstrap::Hq { max_snapshot_bytes } if local == hq => {
                let native = super::head(db).map_err(storage_error)?;
                let position = Position {
                    epoch: Epoch::new(native.epoch).map_err(storage_error)?, seq: native.seq,
                };
                let digest = snapshot::legacy_anchor(db, &org, &hq, position, max_snapshot_bytes)?;
                (position, digest)
            }
            ReplicationBootstrap::Satellite { authority, epoch_base } if local != hq => {
                // An enrolled replica starts from the verified HQ's snapshot or genesis.
                // Only a fresh office's synthetic local genesis may be replaced here.
                let fresh: bool = db.query_row(
                    "SELECT (SELECT count(*) FROM events) = 1
                        AND EXISTS (SELECT 1 FROM events WHERE epoch = 1 AND seq = 1 AND command_id = 'genesis')
                        AND NOT EXISTS (SELECT 1 FROM commands)", [], |row| row.get(0),
                ).map_err(storage_error)?;
                if !fresh {
                    return Err(Refusal::InvalidInput.into());
                }
                snapshot::clear_projection(db)?;
                (Position { epoch: authority, seq: 0 }, epoch_base)
            }
            _ => return Err(Refusal::InvalidInput.into()),
        };
        db.execute(
            "INSERT INTO org_replica (singleton, org_id, local_office, hq_office, ca,
                    epoch, seq, digest, highest_epoch, restore_pending, pending_base, protocol_level,
                    awaiting_snapshot, native_seq)
             VALUES (1, ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?5, 0, NULL, ?8, ?9, ?6)",
            params![org.as_str(), local.as_str(), hq.as_str(), ca.as_bytes(),
                int(position.epoch.get())?, int(position.seq)?, digest.as_bytes(), PROTOCOL, local != hq],
        ).map_err(storage_error)?;
        store_snapshot_base(db, position, digest)?;
        Ok(())
    })
}

pub(crate) fn replication_checkpoint(store: &Store) -> Result<OrgCheckpoint, SyncError> {
    read_checkpoint(store, false)
}

pub(super) fn staging_checkpoint(store: &Store) -> Result<OrgCheckpoint, SyncError> {
    read_checkpoint(store, true)
}

fn read_checkpoint(store: &Store, allow_unmaterialized: bool) -> Result<OrgCheckpoint, SyncError> {
    store
        .read(|db| {
            let tx = db.unchecked_transaction().map_err(OfficeError::storage)?;
            if !allow_unmaterialized && awaiting_snapshot(&tx).map_err(OfficeError::storage)? {
                return Err(OfficeError::storage(SyncError::from(
                    Refusal::RecoveryRequired,
                )));
            }
            OrganizationTxn(&tx)
                .checkpoint()
                .map_err(OfficeError::storage)
        })
        .map_err(sync_error)
}

/// `None` means the sender no longer has a chain starting at `after`.
pub(crate) fn replication_events_after(
    store: &Store,
    after: Position,
    digest: Digest,
    limit: usize,
) -> Result<Option<Vec<OrgEvent>>, SyncError> {
    store
        .read(|db| {
            let tx = db.unchecked_transaction().map_err(OfficeError::storage)?;
            events_after(&tx, after, digest, limit).map_err(OfficeError::storage)
        })
        .map_err(sync_error)
}

fn events_after(
    db: &Connection,
    after: Position,
    digest: Digest,
    limit: usize,
) -> Result<Option<Vec<OrgEvent>>, SyncError> {
    let head = OrganizationTxn(db).checkpoint()?;
    if head.local_office != head.hq_office {
        return Err(Refusal::NotAuthorized.into());
    }
    if head.restore_pending {
        return Err(Refusal::RestorePending.into());
    }
    if after.epoch != head.position.epoch || after.seq > head.position.seq {
        return Ok(None);
    }
    if after == head.position {
        return if digest == head.digest {
            Ok(Some(Vec::new()))
        } else {
            Err(Refusal::Divergence.into())
        };
    }
    let mut expected_seq = after.seq + 1;
    let first: Option<[u8; 32]> = db
        .query_row(
            "SELECT previous FROM org_replica_events WHERE epoch = ?1 AND seq = ?2",
            (int(after.epoch.get())?, int(expected_seq)?),
            |row| row.get(0),
        )
        .optional()
        .map_err(storage_error)?;
    let Some(first) = first else {
        return Ok(None);
    };
    if Digest::from_bytes(first) != digest {
        return Err(Refusal::Divergence.into());
    }

    let mut read = db
        .prepare(
            "SELECT seq, previous, digest, codec, codec_version, payload
         FROM org_replica_events WHERE epoch = ?1 AND seq >= ?2 ORDER BY seq LIMIT ?3",
        )
        .map_err(storage_error)?;
    let mut rows = read
        .query((
            int(after.epoch.get())?,
            int(expected_seq)?,
            i64::try_from(limit).map_err(storage_error)?,
        ))
        .map_err(storage_error)?;
    let mut events = Vec::new();
    let mut previous = digest;
    while let Some(row) = rows.next().map_err(storage_error)? {
        let seq =
            u64::try_from(row.get::<_, i64>(0).map_err(storage_error)?).map_err(storage_error)?;
        let stored_previous = Digest::from_bytes(row.get(1).map_err(storage_error)?);
        let stored_digest = Digest::from_bytes(row.get(2).map_err(storage_error)?);
        let codec_id: String = row.get(3).map_err(storage_error)?;
        let codec =
            Codec::new(codec_id, row.get(4).map_err(storage_error)?).map_err(storage_error)?;
        let payload: Vec<u8> = row.get(5).map_err(storage_error)?;
        if seq != expected_seq || stored_previous != previous || seq > head.position.seq {
            return Err(Refusal::InvariantFailure.into());
        }
        let event = OrgEvent::new(
            head.org_id.clone(),
            Position {
                epoch: after.epoch,
                seq,
            },
            previous,
            codec,
            payload,
        )
        .map_err(|_| Refusal::InvariantFailure)?;
        if event.digest != stored_digest
            || (event.position == head.position && event.digest != head.digest)
        {
            return Err(Refusal::IntegrityFailure.into());
        }
        previous = event.digest;
        expected_seq += 1;
        events.push(event);
    }
    if events.len() < limit && expected_seq <= head.position.seq {
        return Err(Refusal::InvariantFailure.into());
    }
    Ok(Some(events))
}

pub(super) struct OrganizationTxn<'a>(pub(super) &'a Connection);

fn epoch(value: i64) -> Result<Epoch, StoreError> {
    Epoch::new(u64::try_from(value).map_err(storage_error)?).map_err(storage_error)
}

pub(super) fn int(value: u64) -> Result<i64, StoreError> {
    i64::try_from(value).map_err(storage_error)
}

pub(super) fn native_position(db: &Connection) -> Result<Option<org_proto::Position>, OfficeError> {
    let row = db
        .query_row(
            "SELECT epoch, native_seq, awaiting_snapshot FROM org_replica WHERE singleton = 1",
            [],
            |row| {
                Ok((
                    org_proto::Position {
                        epoch: row.get::<_, i64>(0)? as u64,
                        seq: row.get::<_, i64>(1)? as u64,
                    },
                    row.get::<_, bool>(2)?,
                ))
            },
        )
        .optional()
        .map_err(OfficeError::storage)?;
    match row {
        Some((_, true)) => Err(OfficeError::NotFound),
        Some((position, false)) => Ok(Some(position)),
        None => Ok(None),
    }
}

pub(super) fn awaiting_snapshot(db: &Connection) -> Result<bool, StoreError> {
    db.query_row(
        "SELECT awaiting_snapshot FROM org_replica WHERE singleton = 1",
        [],
        |row| row.get(0),
    )
    .map_err(storage_error)
}

pub(super) fn native_binding(db: &Connection) -> Result<Option<(bool, bool)>, OfficeError> {
    db.query_row(
        "SELECT local_office = hq_office, restore_pending FROM org_replica WHERE singleton = 1",
        [],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )
    .optional()
    .map_err(OfficeError::storage)
}

pub(super) fn publish_command(
    db: &Connection,
    command_id: &str,
    at: String,
    changes: Vec<OrgChange>,
    answer: Answer,
) -> Result<org_proto::Position, OfficeError> {
    let record = NativeEvent {
        command_id: command_id.to_owned(),
        at,
        changes,
        answer: Some(NativeAnswer {
            params_hash: answer.params_hash,
            refusal: answer.refusal,
        }),
    };
    let payload = serde_json::to_vec(&record).map_err(OfficeError::storage)?;
    let codec = Codec::new(CODEC, 1).map_err(OfficeError::storage)?;
    if payload.len() > MAX_LIVE_PAYLOAD_BYTES {
        publish_snapshot_checkpoint(db, record, &codec, &payload).map_err(OfficeError::storage)?;
    } else {
        organization::append_hq(&mut OrganizationTxn(db), codec, payload.into())
            .map_err(OfficeError::storage)?;
    }
    super::head(db)
}

fn publish_snapshot_checkpoint(
    db: &Connection,
    record: NativeEvent,
    codec: &Codec,
    payload: &[u8],
) -> Result<(), SyncError> {
    let mut txn = OrganizationTxn(db);
    let mut head = txn.checkpoint()?;
    if head.local_office != head.hq_office {
        return Err(Refusal::NotAuthorized.into());
    }
    if head.restore_pending {
        return Err(Refusal::RestorePending.into());
    }
    if head.position.epoch != head.highest_epoch || head.protocol_level != PROTOCOL {
        return Err(Refusal::InvariantFailure.into());
    }
    head.position.seq = head
        .position
        .seq
        .checked_add(1)
        .ok_or(Refusal::InvariantFailure)?;
    head.digest = event_digest_v1(&head.org_id, head.position, head.digest, codec, payload)
        .map_err(storage_error)?;
    // This valid native command cannot fit in an event. Commit it at a new snapshot
    // boundary so no peer can resume across the omitted delta.
    record.apply(db, head.position)?;
    db.execute("DELETE FROM org_replica_events", [])
        .map_err(storage_error)?;
    store_snapshot_base(db, head.position, head.digest)?;
    txn.set_authority(&head)?;
    Ok(())
}

fn store_snapshot_base(
    db: &Connection,
    position: Position,
    digest: Digest,
) -> Result<(), StoreError> {
    db.execute(
        "INSERT INTO org_replica_base (singleton, epoch, seq, digest) VALUES (1, ?1, ?2, ?3)
         ON CONFLICT(singleton) DO UPDATE SET epoch = excluded.epoch, seq = excluded.seq, digest = excluded.digest",
        (int(position.epoch.get())?, int(position.seq)?, digest.as_bytes()),
    ).map_err(storage_error)?;
    Ok(())
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

    fn requires_initial_snapshot(&mut self) -> Result<bool, StoreError> {
        awaiting_snapshot(self.0)
    }

    fn digest_at(&mut self, position: Position) -> Result<Option<Digest>, StoreError> {
        self.0
            .query_row(
                "SELECT digest FROM org_replica_events WHERE (epoch, seq) = (?1, ?2)
                 UNION ALL SELECT digest FROM org_replica_base WHERE (epoch, seq) = (?1, ?2)",
                (int(position.epoch.get())?, int(position.seq)?),
                |row| row.get(0).map(Digest::from_bytes),
            )
            .optional()
            .map_err(storage_error)
    }

    fn supports_codec(&mut self, codec: &Codec) -> Result<bool, StoreError> {
        Ok(matches!(codec.id(), CODEC | snapshot::CODEC) && codec.version() == 1)
    }

    fn supports_protocol(&mut self, level: u32) -> Result<bool, StoreError> {
        Ok(level == PROTOCOL)
    }

    fn apply_event(&mut self, event: &OrgEvent) -> Result<(), SyncError> {
        if event.codec.id() != CODEC || event.codec.version() != 1 {
            return Err(Refusal::IntegrityFailure.into());
        }
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
        staged: &ValidatedSnapshot,
        next: &OrgCheckpoint,
    ) -> Result<(), SyncError> {
        if snapshot::install(self.0, staged, next)? {
            store_snapshot_base(self.0, next.position, next.digest)?;
            self.set_authority(next)?;
        }
        Ok(())
    }

    fn set_authority(&mut self, next: &OrgCheckpoint) -> Result<(), StoreError> {
        let changed = self
            .0
            .execute(
                "UPDATE org_replica SET epoch = ?1, seq = ?2, digest = ?3,
                    highest_epoch = ?4, restore_pending = ?5, pending_base = ?6,
                    native_seq = CASE WHEN epoch <> ?1 AND ?2 = 0 THEN 0 ELSE native_seq END
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
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct NativeEvent {
    command_id: String,
    at: String,
    changes: Vec<OrgChange>,
    answer: Option<NativeAnswer>,
}

// The provisional codec carries pre-X-02 outcomes. Refuse extra fields so newer
// principal data cannot be accepted and dropped by this adapter.
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct NativeAnswer {
    pub(super) params_hash: String,
    pub(super) refusal: Option<org_proto::ApiError>,
}

impl NativeEvent {
    fn apply(self, db: &Connection, position: Position) -> Result<(), SyncError> {
        if time::parse_rfc3339_ms(&self.at).is_none() {
            return Err(Refusal::IntegrityFailure.into());
        }
        let expires = match &self.answer {
            Some(answer) => {
                if !super::valid_fingerprint(&answer.params_hash)
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

        let previous_teams = relationships::before_apply(db, &self.changes, self.answer.is_none())?;
        apply_changes(db, &self.changes).map_err(|error| match error {
            OfficeError::Invalid(_) => SyncError::from(Refusal::IntegrityFailure),
            OfficeError::Storage(ref cause) if matches!(cause.downcast_ref(), Some(rusqlite::Error::SqliteFailure(failure, _)) if failure.code == rusqlite::ErrorCode::ConstraintViolation) => SyncError::from(Refusal::InvariantFailure),
            error => storage_error(error).into(),
        })?;
        relationships::after_apply(db, &self.changes, previous_teams)?;

        // Federation keeps every outcome. The native feed advances only for changes,
        // so a copied refusal cannot introduce a gap in existing subscribers' positions.
        let changed = self
            .answer
            .as_ref()
            .is_none_or(|answer| answer.refusal.is_none());
        let (epoch, seq) = if changed {
            let next = super::append_event(db, &self.command_id, &self.at, &self.changes)
                .map_err(storage_error)?;
            let seq = int(next.seq)?;
            db.execute("UPDATE org_replica SET native_seq = ?1", [seq])
                .map_err(storage_error)?;
            (int(next.epoch)?, seq)
        } else {
            (0, 0)
        };
        if let Some((answer, expires)) = self.answer.zip(expires) {
            let inserted = db
                .execute(
                    "INSERT INTO commands (command_id, epoch, seq, expires_at, result)
                 VALUES (?1, ?2, ?3, ?4, ?5) ON CONFLICT(command_id) DO NOTHING",
                    (
                        &self.command_id,
                        epoch,
                        seq,
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
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use org_sync::error::MAX_STORE_RETRIES;
    use org_sync::organization::{
        self, AuthorityRecord, AuthorityVerifier, PeerIdentity, PeerRole, VerifiedPeer,
    };
    use serde_json::json;
    use tempfile::TempDir;

    use super::*;
    use crate::{Key, NewOffice, Office, OfficeRole, SnapshotStageMode};

    const COMMAND: &str = "01ARZ3NDEKTSV4RRFFQ69G5FAV";
    const SECOND: &str = "01ARZ3NDEKTSV4RRFFQ69G5FAW";

    struct Hq;

    impl VerifiedPeer for Hq {
        fn identity(&self) -> PeerIdentity {
            PeerIdentity {
                org_id: OrgId::new("ORG").unwrap(),
                office_id: OfficeId::new("HQ").unwrap(),
                role: PeerRole::Hq,
                authority: Epoch::new(1).unwrap(),
            }
        }
    }

    struct HqAt(Epoch);

    impl VerifiedPeer for HqAt {
        fn identity(&self) -> PeerIdentity {
            PeerIdentity {
                authority: self.0,
                ..Hq.identity()
            }
        }
    }

    struct VerifiedSignature;

    impl AuthorityVerifier for VerifiedSignature {
        fn verify(&self, _: &AuthorityRecord) -> Result<(), Refusal> {
            Ok(())
        }
    }

    fn key() -> Key {
        Key::from_bytes([3; 32])
    }

    fn replica() -> (TempDir, Office) {
        let dir = TempDir::new().unwrap();
        let office = bound_office(&dir, "SAT");
        (dir, office)
    }

    fn new_office(dir: &TempDir, local_id: &str) -> Office {
        Office::create(
            dir.path().join(local_id),
            &NewOffice {
                office_id: local_id.into(),
                office_name: "Satellite".into(),
                role: OfficeRole::Hq,
                org_id: "ORG".into(),
                org_name: "Organization".into(),
            },
            key(),
        )
        .unwrap()
    }

    fn bound_office(dir: &TempDir, local_id: &'static str) -> Office {
        let office = new_office(dir, local_id);
        // Simulate trusted enrollment at the already materialized genesis checkpoint.
        office
            .org_store()
            .write(move |db| {
                db.execute(
                    "INSERT INTO org_replica VALUES (1, 'ORG', ?1, 'HQ', zeroblob(32),
                                            1, 1, zeroblob(32), 1, 0, NULL, 1, 0, 1)",
                    [local_id],
                )
                .map_err(OfficeError::storage)?;
                Ok(((), None))
            })
            .unwrap();
        office
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
    fn authenticated_delta_cannot_remove_the_owner_or_create_invalid_team_relationships() {
        use org_proto::org::{Membership, Team};

        let (_dir, office) = replica();
        let before = office.org_snapshot().unwrap();
        let checkpoint = office.replication_checkpoint().unwrap();
        let owner = before.members.iter().find(|member| member.owner).unwrap();
        let team = |id: &str, parent: Option<&str>| Team {
            id: id.into(),
            owner_team_id: parent.map(str::to_owned),
            lead_member_id: None,
            name: id.into(),
            version: 1,
        };
        let team_id = "team".to_owned();
        let missing_membership = OrgChange::MembershipUpserted {
            membership: Membership {
                id: "membership".into(),
                team_id: team_id.clone(),
                member_id: "missing".into(),
                title_in_team: None,
                version: 1,
            },
        };
        let delta = |seq, previous, command_id: &str, changes| {
            OrgEvent::new(
                checkpoint.org_id.clone(),
                Position {
                    epoch: checkpoint.position.epoch,
                    seq,
                },
                previous,
                Codec::new(CODEC, 1).unwrap(),
                serde_json::to_vec(&NativeEvent {
                    command_id: command_id.to_owned(),
                    at: "2026-11-08T12:00:00.000Z".into(),
                    changes,
                    answer: Some(NativeAnswer {
                        params_hash: "12".repeat(32),
                        refusal: None,
                    }),
                })
                .unwrap(),
            )
            .unwrap()
        };
        for changes in [
            vec![OrgChange::MemberRemoved {
                member_id: owner.id.clone(),
            }],
            vec![
                OrgChange::TeamUpserted {
                    team: team(&team_id, None),
                },
                missing_membership,
            ],
            vec![
                OrgChange::TeamUpserted {
                    team: team("parent", Some("child")),
                },
                OrgChange::TeamUpserted {
                    team: team("child", Some("parent")),
                },
            ],
        ] {
            let incoming = delta(2, checkpoint.digest, COMMAND, changes);
            assert!(matches!(
                organization::apply(&office, &Hq, incoming),
                Err(SyncError::Refused(Refusal::IntegrityFailure))
            ));
            assert_eq!(office.org_snapshot().unwrap(), before);
            assert_eq!(office.replication_checkpoint().unwrap(), checkpoint);
        }
        let mut member = owner.clone();
        member.id = "person".into();
        member.owner = false;
        let mut led = team(&team_id, None);
        led.lead_member_id = Some(member.id.clone());
        let valid = delta(
            2,
            checkpoint.digest,
            COMMAND,
            vec![
                OrgChange::MemberUpserted {
                    member: member.clone(),
                },
                OrgChange::TeamUpserted { team: led },
                OrgChange::MembershipUpserted {
                    membership: Membership {
                        id: "membership".into(),
                        team_id: team_id.clone(),
                        member_id: member.id,
                        title_in_team: None,
                        version: 1,
                    },
                },
            ],
        );
        let prior = valid.digest;
        organization::apply(&office, &Hq, valid).unwrap();
        let remove = OrgChange::MembershipRemoved {
            membership_id: "membership".into(),
        };
        assert!(matches!(
            organization::apply(&office, &Hq, delta(3, prior, SECOND, vec![remove.clone()])),
            Err(SyncError::Refused(Refusal::IntegrityFailure))
        ));
        assert!(matches!(
            organization::apply(
                &office,
                &Hq,
                delta(
                    3,
                    prior,
                    SECOND,
                    vec![
                        remove,
                        OrgChange::TeamUpserted {
                            team: Team {
                                version: 2,
                                ..team(&team_id, None)
                            }
                        },
                    ]
                )
            ),
            Ok(OrgOutput::Ack {
                duplicate: false,
                ..
            })
        ));
        assert!(office.org_snapshot().unwrap().teams[0]
            .lead_member_id
            .is_none());
    }

    fn agent_member(
        id: &str,
        status: org_proto::org::AgentStatus,
        manager: Option<&str>,
    ) -> org_proto::org::Member {
        serde_json::from_value(json!({
            "id": id, "kind": "agent", "name": id, "office_id": "HQ", "version": 1,
            "reports_to_member_id": manager,
            "agent": { "role": "engineer", "title": "Engineer", "runtime": "codex", "messaging_name": id, "status": status },
        })).unwrap()
    }

    #[test]
    fn replicated_offboarding_preserves_only_historical_reports() {
        use org_proto::org::AgentStatus::{Active, Offboarded};
        let (_dir, satellite) = replica();
        let current = satellite.replication_checkpoint().unwrap();
        let delta = |seq, previous, id: &str, members: Vec<org_proto::org::Member>| {
            OrgEvent::new(
                current.org_id.clone(),
                Position {
                    epoch: current.position.epoch,
                    seq,
                },
                previous,
                Codec::new(CODEC, 1).unwrap(),
                serde_json::to_vec(&NativeEvent {
                    command_id: id.into(),
                    at: "2026-11-08T12:00:00.000Z".into(),
                    changes: members
                        .into_iter()
                        .map(|member| OrgChange::MemberUpserted { member })
                        .collect(),
                    answer: Some(NativeAnswer {
                        params_hash: "12".repeat(32),
                        refusal: None,
                    }),
                })
                .unwrap(),
            )
            .unwrap()
        };
        let seed = delta(
            2,
            current.digest,
            COMMAND,
            vec![
                agent_member("manager", Active, None),
                agent_member("current", Active, Some("manager")),
                agent_member("history", Offboarded, Some("manager")),
            ],
        );
        let prior = seed.digest;
        organization::apply(&satellite, &Hq, seed).unwrap();
        let manager = agent_member("manager", Offboarded, None);
        assert!(matches!(
            organization::apply(
                &satellite,
                &Hq,
                delta(3, prior, SECOND, vec![manager.clone()])
            ),
            Err(SyncError::Refused(Refusal::IntegrityFailure))
        ));
        let valid = delta(
            3,
            prior,
            SECOND,
            vec![manager, agent_member("current", Active, None)],
        );
        let prior = valid.digest;
        organization::apply(&satellite, &Hq, valid).unwrap();
        let invalid = delta(
            4,
            prior,
            "01ARZ3NDEKTSV4RRFFQ69G5FAX",
            vec![agent_member("current", Active, Some("manager"))],
        );
        assert!(matches!(
            organization::apply(&satellite, &Hq, invalid),
            Err(SyncError::Refused(Refusal::IntegrityFailure))
        ));
        let reactivated = delta(
            4,
            prior,
            "01ARZ3NDEKTSV4RRFFQ69G5FAX",
            vec![agent_member("manager", Active, None)],
        );
        assert!(matches!(
            organization::apply(&satellite, &Hq, reactivated),
            Err(SyncError::Refused(Refusal::IntegrityFailure))
        ));
        let mut renamed = agent_member("manager", Offboarded, None);
        renamed.agent.as_mut().unwrap().messaging_name = "released-handle".into();
        assert!(matches!(
            organization::apply(
                &satellite,
                &Hq,
                delta(4, prior, "01ARZ3NDEKTSV4RRFFQ69G5FAX", vec![renamed])
            ),
            Err(SyncError::Refused(Refusal::IntegrityFailure))
        ));
        let state = satellite.org_snapshot().unwrap();
        assert_eq!(
            state
                .members
                .iter()
                .find(|member| member.id == "history")
                .unwrap()
                .reports_to_member_id
                .as_deref(),
            Some("manager")
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
            "DROP TABLE org_snapshot_install; DROP TABLE org_snapshot_stage; DROP TABLE org_replica_events;
             DROP TABLE org_replica_base; DROP TABLE org_replica; PRAGMA user_version = 1;",
        );
        drop(office);

        let office = Office::open(dir.path().join("SAT"), key()).unwrap();
        assert_eq!(office.org_snapshot().unwrap(), original);
        office.org_store().run_sql_for_test(
            "INSERT INTO org_replica VALUES (1, 'ORG', 'SAT', 'HQ', zeroblob(32),
                                            1, 1, zeroblob(32), 1, 0, NULL, 1, 0, 1)",
        );
        assert!(matches!(
            organization::apply(&office, &Hq, event(2, Digest::ZERO, COMMAND, "first")).unwrap(),
            OrgOutput::Ack {
                duplicate: false,
                ..
            }
        ));
    }

    #[test]
    fn snapshot_staging_verifies_ciphertext_and_installs_only_the_org_projection() {
        const MAX_BYTES: u64 = 1024 * 1024;
        let dir = TempDir::new().unwrap();
        let satellite = new_office(&dir, "SAT");
        let hq = new_office(&dir, "HQ");
        let ca = Digest::from_bytes([6; 32]);
        hq.bind_replication_hq(ca, MAX_BYTES).unwrap();
        satellite
            .bind_replication_satellite(
                OfficeId::new("HQ").unwrap(),
                ca,
                Epoch::new(1).unwrap(),
                Digest::ZERO,
            )
            .unwrap();
        let anchor = hq.replication_checkpoint().unwrap();
        let transport_key = Key::from_bytes([9; 32]);
        let older = hq
            .export_replication_snapshot(&transport_key, MAX_BYTES)
            .unwrap();
        let first = event(2, anchor.digest, COMMAND, "first");
        let next = first.clone();
        assert_eq!(
            org_sync::organization::OrgStore::transact(
                &hq,
                Arc::new(move |txn| {
                    organization::append_hq(txn, next.codec.clone(), Arc::clone(&next.payload))
                })
            )
            .unwrap(),
            OrgOutput::Published(first.clone()),
        );
        hq.org_store()
            .run_sql_for_test("INSERT INTO meta VALUES ('sender_local', 'private')");
        satellite
            .local_store()
            .run_sql_for_test("INSERT INTO meta VALUES ('outbox', 'pending')");
        assert!(hq.export_replication_snapshot(&key(), MAX_BYTES).is_err());
        let exported = hq
            .export_replication_snapshot(&transport_key, MAX_BYTES)
            .unwrap();
        let bytes = exported.ciphertext.as_slice();
        assert!(!bytes.starts_with(b"SQLite format 3\0"));
        assert!(matches!(
            satellite.stage_replication_snapshot(
                &Hq,
                &exported.manifest,
                &transport_key,
                &bytes[..bytes.len() - 1],
                MAX_BYTES,
                SnapshotStageMode::Normal,
            ),
            Err(SyncError::Refused(Refusal::IntegrityFailure))
        ));
        assert!(matches!(
            satellite.stage_replication_snapshot(
                &Hq,
                &exported.manifest,
                &Key::from_bytes([4; 32]),
                bytes,
                MAX_BYTES,
                SnapshotStageMode::Normal,
            ),
            Err(SyncError::Refused(Refusal::IntegrityFailure))
        ));

        let staged = satellite
            .stage_replication_snapshot(
                &Hq,
                &exported.manifest,
                &transport_key,
                bytes,
                MAX_BYTES,
                SnapshotStageMode::Normal,
            )
            .unwrap();
        assert!(matches!(
            satellite.stage_replication_snapshot(
                &Hq,
                &older.manifest,
                &transport_key,
                older.ciphertext.as_slice(),
                MAX_BYTES,
                SnapshotStageMode::Normal
            ),
            Err(SyncError::Refused(Refusal::Divergence))
        ));
        drop(satellite);
        let satellite = Office::open(dir.path().join("SAT"), key()).unwrap();
        satellite.org_store().run_sql_for_test(
            "CREATE TRIGGER fail_copy BEFORE INSERT ON teams
             BEGIN SELECT RAISE(ABORT, 'injected failure'); END;",
        );
        assert!(organization::install_snapshot(&satellite, staged.clone()).is_err());
        assert!(satellite.org_snapshot().is_err());
        let old = org_proto::Position { epoch: 1, seq: 0 };
        satellite
            .org_store()
            .run_sql_for_test("DROP TRIGGER fail_copy;");
        assert_eq!(
            organization::install_snapshot(&satellite, staged.clone()).unwrap(),
            OrgOutput::SnapshotInstalled
        );
        drop(satellite);
        let satellite = Office::open(dir.path().join("SAT"), key()).unwrap();
        assert_eq!(
            organization::install_snapshot(&satellite, staged).unwrap(),
            OrgOutput::SnapshotInstalled
        );
        let installed = satellite.org_snapshot().unwrap();
        assert_eq!(installed.position.seq, 2);
        assert_eq!(installed.teams[0].name, "first");
        assert_eq!(
            satellite.local_store().meta("outbox").unwrap().as_deref(),
            Some("pending")
        );
        assert_eq!(satellite.org_store().meta("sender_local").unwrap(), None);
        assert!(!satellite.org_can_resume_from(old).unwrap());
        assert!(satellite.org_can_resume_from(installed.position).unwrap());
        let second = event(3, first.digest, SECOND, "second");
        assert!(matches!(
            organization::apply(&satellite, &Hq, second).unwrap(),
            OrgOutput::Ack {
                duplicate: false,
                ..
            }
        ));
        assert!(satellite.org_changes_after(old, 10).is_err());
        assert!(matches!(
            organization::apply(&satellite, &Hq, first).unwrap(),
            OrgOutput::Ack {
                duplicate: true,
                ..
            }
        ));
    }

    #[test]
    fn snapshot_with_no_owner_or_with_dangling_membership_is_refused() {
        const MAX_BYTES: u64 = 1024 * 1024;
        let dir = TempDir::new().unwrap();
        let satellite = new_office(&dir, "SAT");
        satellite
            .bind_replication_satellite(
                OfficeId::new("HQ").unwrap(),
                Digest::ZERO,
                Epoch::new(1).unwrap(),
                Digest::ZERO,
            )
            .unwrap();
        let hq = new_office(&dir, "HQ");
        hq.bind_replication_hq(Digest::ZERO, MAX_BYTES).unwrap();
        let transport_key = Key::from_bytes([8; 32]);
        hq.org_store().run_sql_for_test("INSERT INTO memberships (id, team_id, member_id, body)
            VALUES ('m', 'missing-team', 'missing-member',
                    '{\"id\":\"m\",\"team_id\":\"missing-team\",\"member_id\":\"missing-member\",\"version\":1}')");
        let stage = || {
            let export = hq
                .export_replication_snapshot(&transport_key, MAX_BYTES)
                .unwrap();
            satellite.stage_replication_snapshot(
                &Hq,
                &export.manifest,
                &transport_key,
                export.ciphertext.as_slice(),
                MAX_BYTES,
                SnapshotStageMode::Normal,
            )
        };
        assert!(matches!(
            stage(),
            Err(SyncError::Refused(Refusal::IntegrityFailure))
        ));
        hq.org_store()
            .run_sql_for_test("DELETE FROM memberships; DELETE FROM members");
        assert!(matches!(
            stage(),
            Err(SyncError::Refused(Refusal::IntegrityFailure))
        ));
    }

    #[test]
    fn snapshot_refuses_team_cycles_and_invalid_team_roles() {
        const MAX_BYTES: u64 = 1024 * 1024;
        let dir = TempDir::new().unwrap();
        let satellite = new_office(&dir, "SAT");
        satellite
            .bind_replication_satellite(
                OfficeId::new("HQ").unwrap(),
                Digest::ZERO,
                Epoch::new(1).unwrap(),
                Digest::ZERO,
            )
            .unwrap();
        let hq = new_office(&dir, "HQ");
        hq.bind_replication_hq(Digest::ZERO, MAX_BYTES).unwrap();
        let transport_key = Key::from_bytes([8; 32]);
        let stage = || {
            let export = hq
                .export_replication_snapshot(&transport_key, MAX_BYTES)
                .unwrap();
            satellite.stage_replication_snapshot(
                &Hq,
                &export.manifest,
                &transport_key,
                export.ciphertext.as_slice(),
                MAX_BYTES,
                SnapshotStageMode::Normal,
            )
        };
        hq.org_store().run_sql_for_test("INSERT INTO teams VALUES
            ('a', 'b', 'a', NULL, '{\"id\":\"a\",\"owner_team_id\":\"b\",\"name\":\"a\",\"version\":1}'),
            ('b', 'a', 'b', NULL, '{\"id\":\"b\",\"owner_team_id\":\"a\",\"name\":\"b\",\"version\":1}')");
        assert!(matches!(
            stage(),
            Err(SyncError::Refused(Refusal::IntegrityFailure))
        ));
        hq.org_store().run_sql_for_test("DELETE FROM teams;");
        let mut person = hq.org_snapshot().unwrap().members.remove(0);
        let owner = person.id.clone();
        person.id = "person".into();
        person.owner = false;
        hq.org_store()
            .write(move |db| {
                let team = org_proto::org::Team {
                    id: "team".into(),
                    owner_team_id: None,
                    name: "team".into(),
                    lead_member_id: Some(person.id.clone()),
                    version: 1,
                };
                apply_changes(
                    db,
                    &[
                        OrgChange::MemberUpserted {
                            member: person.clone(),
                        },
                        OrgChange::TeamUpserted { team },
                    ],
                )?;
                Ok(((), None))
            })
            .unwrap();
        assert!(matches!(
            stage(),
            Err(SyncError::Refused(Refusal::IntegrityFailure))
        ));
        hq.org_store()
            .write(move |db| {
                let membership = org_proto::org::Membership {
                    id: "membership".into(),
                    team_id: "team".into(),
                    member_id: owner.clone(),
                    title_in_team: None,

                    version: 1,
                };
                let team = org_proto::org::Team {
                    id: "team".into(),
                    owner_team_id: None,
                    name: "team".into(),
                    lead_member_id: Some(owner.clone()),
                    version: 2,
                };
                apply_changes(
                    db,
                    &[
                        OrgChange::TeamUpserted { team },
                        OrgChange::MembershipUpserted { membership },
                    ],
                )?;
                Ok(((), None))
            })
            .unwrap();
        assert!(matches!(
            stage(),
            Err(SyncError::Refused(Refusal::IntegrityFailure))
        ));
    }

    #[test]
    fn snapshot_refuses_live_relationships_with_offboarded_agents() {
        use org_proto::org::AgentStatus::{Active, Offboarded};
        const MAX_BYTES: u64 = 1024 * 1024;
        let dir = TempDir::new().unwrap();
        let satellite = new_office(&dir, "SAT");
        satellite
            .bind_replication_satellite(
                OfficeId::new("HQ").unwrap(),
                Digest::ZERO,
                Epoch::new(1).unwrap(),
                Digest::ZERO,
            )
            .unwrap();
        let hq = new_office(&dir, "HQ");
        hq.bind_replication_hq(Digest::ZERO, MAX_BYTES).unwrap();
        let transport_key = Key::from_bytes([8; 32]);
        let stage = || {
            let export = hq
                .export_replication_snapshot(&transport_key, MAX_BYTES)
                .unwrap();
            satellite.stage_replication_snapshot(
                &Hq,
                &export.manifest,
                &transport_key,
                export.ciphertext.as_slice(),
                MAX_BYTES,
                SnapshotStageMode::Normal,
            )
        };
        let inject = |changes: Vec<OrgChange>| {
            hq.org_store()
                .write(move |db| apply_changes(db, &changes).map(|()| ((), None)))
                .unwrap();
        };
        inject(vec![
            OrgChange::MemberUpserted {
                member: agent_member("manager", Offboarded, None),
            },
            OrgChange::MemberUpserted {
                member: agent_member("report", Active, Some("manager")),
            },
        ]);
        assert!(matches!(
            stage(),
            Err(SyncError::Refused(Refusal::IntegrityFailure))
        ));
        inject(vec![
            OrgChange::MemberUpserted {
                member: agent_member("report", Offboarded, Some("manager")),
            },
            OrgChange::TeamUpserted {
                team: org_proto::org::Team {
                    id: "team".into(),
                    name: "Team".into(),
                    owner_team_id: None,
                    lead_member_id: Some("manager".into()),
                    version: 1,
                },
            },
            OrgChange::MembershipUpserted {
                membership: org_proto::org::Membership {
                    id: "membership".into(),
                    team_id: "team".into(),
                    member_id: "manager".into(),
                    title_in_team: None,
                    version: 1,
                },
            },
        ]);
        assert!(matches!(
            stage(),
            Err(SyncError::Refused(Refusal::IntegrityFailure))
        ));
        inject(vec![
            OrgChange::MembershipRemoved {
                membership_id: "membership".into(),
            },
            OrgChange::TeamDeleted {
                team_id: "team".into(),
            },
        ]);
        assert!(stage().is_ok());
    }

    #[test]
    fn same_epoch_snapshot_preserves_owner_and_offboarded_members() {
        use crate::rules::OrgCommand;
        use org_proto::org::{AgentStatus, TeamCreateParams};
        const MAX_BYTES: u64 = 1024 * 1024;
        let (dir, satellite) = replica();
        let hq = new_office(&dir, "HQ");
        hq.bind_replication_hq(Digest::ZERO, MAX_BYTES).unwrap();
        hq.submit_org(OrgCommand::TeamCreate(TeamCreateParams {
            command_id: ulid::generate().unwrap(),
            team_id: "team".into(),
            name: "Team".into(),
            owner_team_id: None,
        }))
        .unwrap()
        .unwrap();
        let key = Key::from_bytes([8; 32]);
        let stage = || {
            let snapshot = hq.export_replication_snapshot(&key, MAX_BYTES).unwrap();
            satellite.stage_replication_snapshot(
                &Hq,
                &snapshot.manifest,
                &key,
                snapshot.ciphertext.as_slice(),
                MAX_BYTES,
                SnapshotStageMode::Normal,
            )
        };
        assert!(matches!(
            stage(),
            Err(SyncError::Refused(Refusal::Divergence))
        ));

        let owner = satellite.org_snapshot().unwrap().members.remove(0);
        let original_hq_owner = hq.org_snapshot().unwrap().members.remove(0).id;
        let history = agent_member("history", AgentStatus::Offboarded, None);
        let copied = history.clone();
        satellite
            .org_store()
            .write(move |db| {
                apply_changes(
                    db,
                    &[OrgChange::MemberUpserted {
                        member: copied.clone(),
                    }],
                )
                .map(|()| ((), None))
            })
            .unwrap();
        hq.org_store()
            .write(move |db| {
                apply_changes(
                    db,
                    &[
                        OrgChange::MemberRemoved {
                            member_id: original_hq_owner.clone(),
                        },
                        OrgChange::MemberUpserted {
                            member: owner.clone(),
                        },
                    ],
                )
                .map(|()| ((), None))
            })
            .unwrap();
        assert!(matches!(
            stage(),
            Err(SyncError::Refused(Refusal::Divergence))
        ));

        let inject_history = |member: org_proto::org::Member| {
            hq.org_store()
                .write(move |db| {
                    apply_changes(
                        db,
                        &[OrgChange::MemberUpserted {
                            member: member.clone(),
                        }],
                    )
                    .map(|()| ((), None))
                })
                .unwrap();
        };
        inject_history(agent_member("history", AgentStatus::Active, None));
        assert!(matches!(
            stage(),
            Err(SyncError::Refused(Refusal::Divergence))
        ));
        let mut renamed = history.clone();
        renamed.agent.as_mut().unwrap().messaging_name = "new-handle".into();
        inject_history(renamed);
        assert!(matches!(
            stage(),
            Err(SyncError::Refused(Refusal::Divergence))
        ));
        inject_history(history);
        assert!(stage().is_ok());
    }

    #[test]
    fn newly_enrolled_satellite_requires_a_snapshot_at_a_restored_epoch_zero() {
        let dir = TempDir::new().unwrap();
        let satellite = new_office(&dir, "SAT");
        let epoch = Epoch::new(2).unwrap();
        let base = Digest::from_bytes([7; 32]);
        satellite
            .bind_replication_satellite(OfficeId::new("HQ").unwrap(), Digest::ZERO, epoch, base)
            .unwrap();
        drop(satellite);
        let satellite = Office::open(dir.path().join("SAT"), key()).unwrap();
        assert!(matches!(
            satellite.replication_checkpoint(),
            Err(SyncError::Refused(Refusal::RecoveryRequired))
        ));
        assert!(satellite.org_snapshot().is_err());
        let stale = crate::OrganizationSnapshotManifest {
            org_id: OrgId::new("ORG").unwrap(),
            hq_office: OfficeId::new("HQ").unwrap(),
            position: Position {
                epoch: Epoch::new(1).unwrap(),
                seq: 0,
            },
            digest: Digest::ZERO,
            protocol_level: 99,
            schema: 99,
            ciphertext_bytes: 0,
            ciphertext_digest: Digest::ZERO,
        };
        assert!(matches!(
            satellite.stage_replication_snapshot(
                &Hq,
                &stale,
                &key(),
                &[][..],
                0,
                SnapshotStageMode::Normal,
            ),
            Err(SyncError::Refused(Refusal::NotAuthorized))
        ));
        for seq in [2, 1] {
            let sample = event(seq, base, COMMAND, "incomplete");
            let incoming = OrgEvent::new(
                sample.org_id,
                Position { epoch, seq },
                base,
                sample.codec,
                sample.payload,
            )
            .unwrap();
            assert_eq!(
                organization::apply(&satellite, &HqAt(epoch), incoming).unwrap(),
                OrgOutput::SnapshotRequired
            );
        }
        let record = |authority, restored_epoch| AuthorityRecord {
            org_id: OrgId::new("ORG").unwrap(),
            hq_office: OfficeId::new("HQ").unwrap(),
            ca: Digest::ZERO,
            epoch: authority,
            restored_position: Position {
                epoch: restored_epoch,
                seq: 0,
            },
            restored_digest: base,
            signature: Arc::from([1]),
        };
        for signed in [
            record(epoch, Epoch::new(1).unwrap()),
            record(epoch.checked_next().unwrap(), epoch),
        ] {
            assert_eq!(
                organization::accept_authority(&satellite, signed, Arc::new(VerifiedSignature))
                    .unwrap(),
                OrgOutput::AuthorityAccepted {
                    snapshot_required: true
                }
            );
        }
        assert!(satellite.org_snapshot().is_err());
    }

    #[test]
    fn oversized_native_change_commits_and_requires_a_federation_snapshot() {
        use crate::rules::OrgCommand;
        use org_proto::org::{Member, MemberKind, MemberRemoveParams, TeamCreateParams};
        const MAX_BYTES: u64 = 8 * 1024 * 1024;
        let dir = TempDir::new().unwrap();
        let hq = new_office(&dir, "HQ");
        let manager = "00000000000000000000099999";
        hq.org_store()
            .write(move |db| {
                let mut changes = Vec::new();
                for index in 0..=875 {
                    let member = Member {
                        id: if index == 0 {
                            manager.to_owned()
                        } else {
                            format!("{index:026}")
                        },
                        kind: MemberKind::Person,
                        name: format!("Person {index}"),
                        contact: Some("😀".repeat(org_proto::org::AGENT_TEXT_MAX_CHARS)),
                        owner: false,
                        office_id: None,
                        agent: None,
                        reports_to_member_id: (index != 0).then(|| manager.to_owned()),
                        created_at: None,
                        version: 1,
                    };
                    changes.push(OrgChange::MemberUpserted { member });
                }
                apply_changes(db, &changes)?;
                Ok(((), None))
            })
            .unwrap();
        hq.bind_replication_hq(Digest::ZERO, MAX_BYTES).unwrap();
        let before = hq.replication_checkpoint().unwrap();
        let command = OrgCommand::MemberRemove(MemberRemoveParams {
            command_id: ulid::generate().unwrap(),
            member_id: manager.to_owned(),
            expected_version: 1,
        });
        let answer = hq.submit_org(command.clone()).unwrap().unwrap();
        assert_eq!(answer.position.unwrap().seq, before.position.seq + 1);
        assert_eq!(hq.submit_org(command).unwrap().unwrap(), answer);
        let after = hq.replication_checkpoint().unwrap();
        assert_eq!(after.position.seq, before.position.seq + 1);
        assert!(hq
            .replication_events_after(before.position, before.digest, 8)
            .unwrap()
            .is_none());
        assert!(hq
            .replication_events_after(after.position, after.digest, 8)
            .unwrap()
            .unwrap()
            .is_empty());
        let state = hq.org_snapshot().unwrap();
        assert!(state
            .members
            .iter()
            .all(|member| member.id != manager && member.reports_to_member_id.is_none()));
        hq.submit_org(OrgCommand::TeamCreate(TeamCreateParams {
            command_id: ulid::generate().unwrap(),
            team_id: "after".into(),
            name: "After".into(),
            owner_team_id: None,
        }))
        .unwrap()
        .unwrap();
        let resumed = hq
            .replication_events_after(after.position, after.digest, 8)
            .unwrap()
            .unwrap();
        assert_eq!(resumed.len(), 1);
        assert_eq!(resumed[0].previous, after.digest);
        assert_eq!(resumed[0].position.seq, after.position.seq + 1);
    }

    #[test]
    fn accepting_a_higher_authority_persists_the_fence_and_rejects_stale_sessions() {
        const MAX_BYTES: u64 = 1024 * 1024;

        let (dir, satellite) = replica();
        let old = satellite.replication_checkpoint().unwrap();
        let hq = new_office(&dir, "HQ");
        hq.bind_replication_hq(old.ca, MAX_BYTES).unwrap();
        let source = hq.replication_checkpoint().unwrap();
        let next = Epoch::new(2).unwrap();
        let restored_digest = source.digest;
        let record = AuthorityRecord {
            org_id: old.org_id.clone(),
            hq_office: old.hq_office.clone(),
            ca: old.ca,
            epoch: next,
            restored_position: source.position,
            restored_digest,
            signature: Arc::from([1]),
        };
        assert_eq!(
            organization::accept_authority(&hq, record.clone(), Arc::new(VerifiedSignature))
                .unwrap(),
            OrgOutput::AuthorityAccepted {
                snapshot_required: false
            }
        );
        let output =
            organization::accept_authority(&satellite, record, Arc::new(VerifiedSignature))
                .unwrap();
        assert_eq!(
            output,
            OrgOutput::AuthorityAccepted {
                snapshot_required: true
            }
        );
        drop(satellite);
        let satellite = Office::open(dir.path().join("SAT"), key()).unwrap();
        let recovered = satellite.replication_checkpoint().unwrap();
        assert_eq!(recovered.position, old.position);
        assert_eq!(recovered.highest_epoch, next);
        assert_eq!(recovered.pending_restore_base, Some(restored_digest));
        assert!(recovered.restore_pending);
        assert!(matches!(
            organization::apply(&satellite, &Hq, event(2, old.digest, COMMAND, "stale")),
            Err(SyncError::Refused(Refusal::NotAuthorized))
        ));

        let transport_key = Key::from_bytes([8; 32]);
        let exported = hq
            .export_replication_snapshot(&transport_key, MAX_BYTES)
            .unwrap();
        let stage = |mode| {
            satellite.stage_replication_snapshot(
                &HqAt(next),
                &exported.manifest,
                &transport_key,
                exported.ciphertext.as_slice(),
                MAX_BYTES,
                mode,
            )
        };
        assert!(matches!(
            stage(SnapshotStageMode::Normal),
            Err(SyncError::Refused(Refusal::RestorePending))
        ));
        assert!(matches!(
            stage(SnapshotStageMode::RecoveryWarningShown {
                authority: next,
                restored_digest: Digest::ZERO
            }),
            Err(SyncError::Refused(Refusal::RestorePending))
        ));
        hq.org_store()
            .run_sql_for_test("UPDATE org_replica SET digest = zeroblob(32);");
        let wrong = hq
            .export_replication_snapshot(&transport_key, MAX_BYTES)
            .unwrap();
        assert!(matches!(
            satellite.stage_replication_snapshot(
                &HqAt(next),
                &wrong.manifest,
                &transport_key,
                wrong.ciphertext.as_slice(),
                MAX_BYTES,
                SnapshotStageMode::RecoveryWarningShown {
                    authority: next,
                    restored_digest
                }
            ),
            Err(SyncError::Refused(Refusal::Divergence))
        ));
        let staged = stage(SnapshotStageMode::RecoveryWarningShown {
            authority: next,
            restored_digest,
        })
        .unwrap();
        assert_eq!(
            organization::install_snapshot(&satellite, staged).unwrap(),
            OrgOutput::SnapshotInstalled
        );
        let installed = satellite.replication_checkpoint().unwrap();
        assert_eq!(
            installed.position,
            Position {
                epoch: next,
                seq: 0
            }
        );
        assert_eq!(installed.digest, restored_digest);
        assert_eq!(installed.pending_restore_base, None);
        assert!(!installed.restore_pending);
    }

    #[test]
    fn native_hq_submission_advances_the_federation_log_once_per_outcome() {
        use crate::rules::OrgCommand;
        use org_proto::org::TeamCreateParams;

        const MAX_BYTES: u64 = 1024 * 1024;
        let dir = TempDir::new().unwrap();
        let hq = new_office(&dir, "HQ");
        let ca = Digest::from_bytes([6; 32]);
        hq.bind_replication_hq(ca, MAX_BYTES).unwrap();
        let anchor = hq.replication_checkpoint().unwrap();
        assert_ne!(anchor.digest, Digest::ZERO);
        let new_team = |team_id: &str, name: &str| {
            OrgCommand::TeamCreate(TeamCreateParams {
                command_id: ulid::generate().unwrap(),
                team_id: team_id.into(),
                name: name.into(),
                owner_team_id: None,
            })
        };
        let accepted = new_team("teamA", "Design");
        let committed = hq.submit_org(accepted.clone()).unwrap().unwrap();
        assert_eq!(committed.position.unwrap().seq, 2);
        assert_eq!(hq.org_head().unwrap().seq, 2);
        assert_eq!(hq.submit_org(accepted).unwrap().unwrap(), committed);
        let refused = new_team("teamB", "Design");
        let error = hq.submit_org(refused.clone()).unwrap().unwrap_err();
        assert_eq!(error.code, org_proto::ErrorCode::AlreadyExists);
        assert_eq!(hq.submit_org(refused).unwrap().unwrap_err(), error);
        let satellite = new_office(&dir, "SAT");
        satellite
            .bind_replication_satellite(
                OfficeId::new("HQ").unwrap(),
                ca,
                anchor.position.epoch,
                Digest::ZERO,
            )
            .unwrap();
        let transport_key = Key::from_bytes([8; 32]);
        let export = hq
            .export_replication_snapshot(&transport_key, MAX_BYTES)
            .unwrap();
        assert_eq!(export.manifest.position.seq, 3);
        let staged = satellite
            .stage_replication_snapshot(
                &Hq,
                &export.manifest,
                &transport_key,
                export.ciphertext.as_slice(),
                MAX_BYTES,
                SnapshotStageMode::Normal,
            )
            .unwrap();
        assert_eq!(
            organization::install_snapshot(&satellite, staged).unwrap(),
            OrgOutput::SnapshotInstalled
        );
        assert_eq!(satellite.org_head().unwrap().seq, 2);
        let next = hq
            .submit_org(new_team("teamC", "Engineering"))
            .unwrap()
            .unwrap();
        assert_eq!(
            hq.org_changes_after(org_proto::Position { epoch: 1, seq: 1 }, 8)
                .unwrap()
                .iter()
                .map(|change| change.position.seq)
                .collect::<Vec<_>>(),
            [2, 3]
        );
        assert_eq!(next.position.unwrap().seq, 3);
        assert!(matches!(
            hq.replication_events_after(anchor.position, Digest::ZERO, 8),
            Err(SyncError::Refused(Refusal::Divergence))
        ));
        let events = hq
            .replication_events_after(anchor.position, anchor.digest, 8)
            .unwrap()
            .unwrap();
        assert_eq!(events.len(), 3);
        assert_eq!(events[1].previous, events[0].digest);
        assert_eq!(events[2].previous, events[1].digest);
        assert_eq!(
            events[2].digest,
            hq.replication_checkpoint().unwrap().digest
        );
        assert!(matches!(
            organization::apply(&satellite, &Hq, events[2].clone()).unwrap(),
            OrgOutput::Ack { position, duplicate: false, .. } if position.seq == 4
        ));
        assert_eq!(
            satellite.org_snapshot().unwrap(),
            hq.org_snapshot().unwrap()
        );
        let prior = org_proto::Position { epoch: 1, seq: 2 };
        let before = satellite.org_changes_after(prior, 8).unwrap();
        let at_head = hq
            .export_replication_snapshot(&transport_key, MAX_BYTES)
            .unwrap();
        let same_head = satellite
            .stage_replication_snapshot(
                &Hq,
                &at_head.manifest,
                &transport_key,
                at_head.ciphertext.as_slice(),
                MAX_BYTES,
                SnapshotStageMode::Normal,
            )
            .unwrap();
        assert_eq!(
            organization::install_snapshot(&satellite, same_head).unwrap(),
            OrgOutput::SnapshotInstalled
        );
        assert_eq!(satellite.org_changes_after(prior, 8).unwrap(), before);
        hq.submit_org(new_team("teamD", "Design"))
            .unwrap()
            .unwrap_err();
        let stage_latest = || {
            let export = hq
                .export_replication_snapshot(&transport_key, MAX_BYTES)
                .unwrap();
            satellite.stage_replication_snapshot(
                &Hq,
                &export.manifest,
                &transport_key,
                export.ciphertext.as_slice(),
                MAX_BYTES,
                SnapshotStageMode::Normal,
            )
        };
        hq.org_store()
            .run_sql_for_test("UPDATE org_replica SET native_seq = 5");
        assert!(matches!(
            stage_latest(),
            Err(SyncError::Refused(Refusal::Divergence))
        ));
        hq.org_store().run_sql_for_test(r#"UPDATE org_replica SET native_seq = 3;
            INSERT INTO teams VALUES ('extra', NULL, 'extra', NULL, '{"id":"extra","name":"extra","version":1}');"#);
        assert!(matches!(
            stage_latest(),
            Err(SyncError::Refused(Refusal::Divergence))
        ));
        hq.org_store()
            .run_sql_for_test("DELETE FROM teams WHERE id = 'extra'");
        let pending = stage_latest().unwrap();
        assert_eq!(
            organization::install_snapshot(&satellite, pending).unwrap(),
            OrgOutput::SnapshotInstalled
        );
        assert_eq!(
            satellite.replication_checkpoint().unwrap().position,
            hq.replication_checkpoint().unwrap().position
        );
        assert_eq!(satellite.org_changes_after(prior, 8).unwrap(), before);
        assert!(hq.bind_replication_hq(Digest::ZERO, MAX_BYTES).is_err());
        hq.bind_replication_hq(ca, MAX_BYTES).unwrap();
        assert_eq!(hq.org_head().unwrap().seq, 3);
    }
}
