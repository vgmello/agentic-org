//! A snapshot is an encrypted, allowlisted organization projection. Install it inside
//! the writer transaction that advances the cursor. Local office data lives elsewhere.

use std::collections::{HashMap, HashSet};
use std::fs::{self, DirBuilder};
use std::io::{self, Read, Write};
use std::os::unix::fs::DirBuilderExt;
use std::path::PathBuf;

use org_proto::{Area, Member, Membership, Project, ProjectTeam, Team};
use org_sync::error::{Refusal, StoreError, SyncError};
use org_sync::organization::{OrgCheckpoint, OrgTxn, PeerIdentity, PeerRole, ValidatedSnapshot};
use org_sync_proto::{Codec, Digest, Epoch, OfficeId, OrgId, Position};
use rusqlite::config::DbConfig;
use rusqlite::types::{ToSqlOutput, ValueRef};
use rusqlite::{params, params_from_iter, Connection, OptionalExtension, Transaction};
use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};

use super::replication::{int, OrganizationTxn, PROTOCOL};
use super::{Answer, SCHEMA};
use crate::error::OfficeError;
use crate::key::Key;
use crate::organization::{is_offboarded, valid_owner};
use crate::private_fs::private_file;
use crate::store::{connect, sync::storage_error, Store};
use crate::ulid;

pub(super) const CODEC: &str = "org.native.snapshot";
const ARTIFACT_SCHEMA: &str = "CREATE TABLE snapshot (body BLOB NOT NULL) STRICT";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OrganizationSnapshotManifest {
    pub org_id: OrgId,
    pub hq_office: OfficeId,
    pub position: Position,
    pub digest: Digest,
    pub protocol_level: u32,
    pub schema: i64,
    pub ciphertext_bytes: u64,
    pub ciphertext_digest: Digest,
}

/// The one-time key is supplied by the trusted transport. The artifact contains only ciphertext.
pub struct OrganizationSnapshotExport {
    pub manifest: OrganizationSnapshotManifest,
    pub ciphertext: Vec<u8>,
}

/// Local staging authorization supplied by the worker, never decoded from the peer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SnapshotStageMode {
    Normal,
    RecoveryWarningShown {
        authority: Epoch,
        restored_digest: Digest,
    },
}

struct Scratch(PathBuf);

impl Scratch {
    fn new() -> io::Result<Self> {
        let name = format!("agentic-org-snapshot-{}", ulid::generate()?);
        let directory = std::path::absolute(std::env::temp_dir())?.join(name);
        DirBuilder::new().mode(0o700).create(&directory)?;
        Ok(Self(directory))
    }

    fn database(&self) -> PathBuf {
        self.0.join("snapshot.db")
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[derive(Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct Header {
    org_id: String,
    hq_office: String,
    epoch: u64,
    sequence: u64,
    digest: [u8; 32],
    protocol: u32,
    schema: i64,
}

impl From<&OrganizationSnapshotManifest> for Header {
    fn from(manifest: &OrganizationSnapshotManifest) -> Self {
        Self {
            org_id: manifest.org_id.as_str().to_owned(),
            hq_office: manifest.hq_office.as_str().to_owned(),
            epoch: manifest.position.epoch.get(),
            sequence: manifest.position.seq,
            digest: *manifest.digest.as_bytes(),
            protocol: manifest.protocol_level,
            schema: manifest.schema,
        }
    }
}

#[derive(Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(untagged)]
enum Cell {
    Null,
    Integer(i64),
    Text(String),
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Projection {
    header: Header,
    native_seq: u64,
    rows: [Vec<Vec<Cell>>; TABLES.len()],
}

// Ordered to install the owning rows before their relationships. The list is an allowlist,
// independent of both the sender's schema and unrelated local tables.
const TABLES: [(&str, &str, &str); 7] = [
    (
        "members",
        "id, handle_key, reports_to_member_id, body",
        "?1, ?2, ?3, ?4",
    ),
    (
        "teams",
        "id, owner_team_id, name_key, lead_member_id, body",
        "?1, ?2, ?3, ?4, ?5",
    ),
    ("projects", "id, body", "?1, ?2"),
    ("areas", "id, project_id, body", "?1, ?2, ?3"),
    (
        "memberships",
        "id, team_id, member_id, body",
        "?1, ?2, ?3, ?4",
    ),
    (
        "project_teams",
        "id, project_id, team_id, body",
        "?1, ?2, ?3, ?4",
    ),
    (
        "commands",
        "command_id, epoch, seq, expires_at, result",
        "?1, ?2, ?3, ?4, ?5",
    ),
];

pub(super) fn create_table(tx: &Transaction<'_>) -> rusqlite::Result<()> {
    tx.execute_batch(
        "CREATE TABLE org_snapshot_stage (
             singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
             stage_id TEXT NOT NULL, epoch INTEGER NOT NULL CHECK (epoch > 0),
             seq INTEGER NOT NULL CHECK (seq >= 0),
             digest BLOB NOT NULL CHECK (length(digest) = 32), body BLOB NOT NULL
         ) STRICT;
         CREATE TABLE org_snapshot_install (
             singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
             stage_id TEXT NOT NULL, epoch INTEGER NOT NULL CHECK (epoch > 0),
             seq INTEGER NOT NULL CHECK (seq >= 0),
             digest BLOB NOT NULL CHECK (length(digest) = 32)
         ) STRICT;",
    )
}

fn bounded_rows(db: &Connection, header: Header, limit: u64) -> Result<Projection, OfficeError> {
    let mut projection = Projection {
        header,
        native_seq: super::head(db)?.seq,
        rows: std::array::from_fn(|_| Vec::new()),
    };
    let mut remaining = limit;
    for ((table, columns, _), output) in TABLES.into_iter().zip(&mut projection.rows) {
        let mut query = db
            .prepare(&format!("SELECT {columns} FROM {table} ORDER BY 1"))
            .map_err(OfficeError::storage)?;
        let width = query.column_count();
        let mut rows = query.query([]).map_err(OfficeError::storage)?;
        while let Some(row) = rows.next().map_err(OfficeError::storage)? {
            let mut cells = Vec::with_capacity(width);
            for column in 0..width {
                let value = row.get_ref(column).map_err(OfficeError::storage)?;
                let cost = match value {
                    ValueRef::Text(text) => text.len() as u64,
                    _ => 0,
                };
                remaining =
                    remaining
                        .checked_sub(cost.saturating_add(24))
                        .ok_or(OfficeError::Invalid(
                            "organization snapshot exceeds its byte limit",
                        ))?;
                cells.push(match value {
                    ValueRef::Null => Cell::Null,
                    ValueRef::Integer(value) => Cell::Integer(value),
                    ValueRef::Text(value) => Cell::Text(
                        std::str::from_utf8(value)
                            .map_err(OfficeError::storage)?
                            .to_owned(),
                    ),
                    _ => {
                        return Err(OfficeError::Invalid(
                            "organization snapshot has an invalid cell",
                        ))
                    }
                });
            }
            output.push(cells);
        }
    }
    Ok(projection)
}

fn as_office(error: StoreError) -> OfficeError {
    OfficeError::storage(error)
}

pub(super) fn legacy_anchor(
    db: &Connection,
    org: &OrgId,
    hq: &OfficeId,
    position: Position,
    limit: u64,
) -> Result<Digest, SyncError> {
    let header = Header {
        org_id: org.as_str().to_owned(),
        hq_office: hq.as_str().to_owned(),
        epoch: position.epoch.get(),
        sequence: position.seq,
        digest: *Digest::ZERO.as_bytes(),
        protocol: PROTOCOL,
        schema: SCHEMA.version,
    };
    let projection = bounded_rows(db, header, limit).map_err(storage_error)?;
    let mut digest = Sha256::new();
    digest.update(b"agentic-org:native-snapshot-base:v1\0");
    serde_json::to_writer(&mut digest, &projection).map_err(storage_error)?;
    Ok(Digest::from_bytes(digest.finalize().into()))
}

pub(crate) fn export_replication_snapshot(
    store: &Store,
    transport_key: &Key,
    max_bytes: u64,
) -> Result<OrganizationSnapshotExport, OfficeError> {
    if *transport_key == store.key {
        return Err(OfficeError::Invalid(
            "snapshot transport requires a one-time key",
        ));
    }
    let (mut manifest, projection) = store.read(|db| {
        let tx = db.unchecked_transaction().map_err(OfficeError::storage)?;
        let checkpoint = OrganizationTxn(&tx).checkpoint().map_err(as_office)?;
        if checkpoint.local_office != checkpoint.hq_office || checkpoint.restore_pending {
            return Err(OfficeError::Invalid(
                "organization snapshot requires unfenced HQ authority",
            ));
        }
        let manifest = OrganizationSnapshotManifest {
            org_id: checkpoint.org_id,
            hq_office: checkpoint.hq_office,
            position: checkpoint.position,
            digest: checkpoint.digest,
            protocol_level: checkpoint.protocol_level,
            schema: SCHEMA.version,
            ciphertext_bytes: 0,
            ciphertext_digest: Digest::ZERO,
        };
        let projection = bounded_rows(&tx, Header::from(&manifest), max_bytes)?;
        Ok((manifest, projection))
    })?;
    let body = serde_json::to_vec(&projection).map_err(OfficeError::storage)?;
    if body.len() as u64 > max_bytes {
        return Err(OfficeError::Invalid(
            "organization snapshot exceeds its byte limit",
        ));
    }

    let scratch = Scratch::new().map_err(OfficeError::storage)?;
    let path = scratch.database();
    drop(private_file(&path).map_err(OfficeError::storage)?);
    let mut db = connect(&path, transport_key, true, false)?;
    let tx = db.transaction().map_err(OfficeError::storage)?;
    tx.execute_batch(ARTIFACT_SCHEMA)
        .map_err(OfficeError::storage)?;
    tx.execute("INSERT INTO snapshot VALUES (?1)", [body])
        .map_err(OfficeError::storage)?;
    tx.commit().map_err(OfficeError::storage)?;
    drop(db);

    let length = fs::metadata(&path).map_err(OfficeError::storage)?.len();
    if length > max_bytes {
        return Err(OfficeError::Invalid(
            "organization snapshot exceeds its byte limit",
        ));
    }
    let ciphertext = fs::read(&path).map_err(OfficeError::storage)?;
    manifest.ciphertext_bytes = length;
    manifest.ciphertext_digest = Digest::from_bytes(Sha256::digest(&ciphertext).into());
    Ok(OrganizationSnapshotExport {
        manifest,
        ciphertext,
    })
}

pub(crate) fn stage_replication_snapshot(
    store: &Store,
    peer: PeerIdentity,
    manifest: &OrganizationSnapshotManifest,
    transport_key: &Key,
    input: impl Read,
    max_bytes: u64,
    mode: SnapshotStageMode,
) -> Result<ValidatedSnapshot, SyncError> {
    if peer.role != PeerRole::Hq
        || peer.org_id != manifest.org_id
        || peer.office_id != manifest.hq_office
        || peer.authority != manifest.position.epoch
    {
        return Err(Refusal::NotAuthorized.into());
    }
    let checkpoint = super::replication::staging_checkpoint(store)?;
    check_stage(&checkpoint, &peer, mode, manifest.position, manifest.digest)?;
    if manifest.schema != SCHEMA.version || manifest.protocol_level != PROTOCOL {
        return Err(Refusal::UnsupportedVersion.into());
    }
    if manifest.ciphertext_bytes == 0 || manifest.ciphertext_bytes > max_bytes {
        return Err(Refusal::QuotaExceeded.into());
    }
    if *transport_key == store.key {
        return Err(Refusal::InvalidInput.into());
    }
    let scratch = Scratch::new().map_err(storage_error)?;
    let path = scratch.database();
    let mut file = private_file(&path).map_err(storage_error)?;
    let mut input = input.take(manifest.ciphertext_bytes.saturating_add(1));
    let mut checksum = Sha256::new();
    let mut bytes = 0;
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let read = input.read(&mut buffer).map_err(storage_error)?;
        if read == 0 {
            break;
        }
        bytes += read as u64;
        if bytes > manifest.ciphertext_bytes {
            return Err(Refusal::IntegrityFailure.into());
        }
        checksum.update(&buffer[..read]);
        file.write_all(&buffer[..read]).map_err(storage_error)?;
    }
    drop(file);
    if bytes != manifest.ciphertext_bytes
        || Digest::from_bytes(checksum.finalize().into()) != manifest.ciphertext_digest
    {
        return Err(Refusal::IntegrityFailure.into());
    }

    let db = connect(&path, transport_key, false, false).map_err(|error| match error {
        OfficeError::KeyMissing => SyncError::from(Refusal::IntegrityFailure),
        error => storage_error(error).into(),
    })?;
    db.set_db_config(DbConfig::SQLITE_DBCONFIG_DEFENSIVE, true)
        .map_err(storage_error)?;
    db.execute_batch("PRAGMA trusted_schema = OFF; PRAGMA query_only = ON;")
        .map_err(storage_error)?;
    let schema_matches: bool = db
        .query_row(
            "SELECT count(*) = 1 AND min(type = 'table' AND name = 'snapshot' AND sql = ?1)
         FROM sqlite_schema",
            [ARTIFACT_SCHEMA],
            |row| row.get(0),
        )
        .map_err(|_| Refusal::IntegrityFailure)?;
    if !schema_matches {
        return Err(Refusal::IntegrityFailure.into());
    }
    let body: Vec<u8> = db
        .query_row(
            "SELECT body FROM snapshot WHERE rowid = 1 AND length(body) <= ?1
         AND (SELECT count(*) FROM snapshot) = 1",
            [int(max_bytes.min(i64::MAX as u64))?],
            |row| row.get(0),
        )
        .map_err(|_| Refusal::IntegrityFailure)?;
    let projection = decode(&body)?;
    if projection.header != Header::from(manifest) {
        return Err(Refusal::IntegrityFailure.into());
    }

    let mut staged = ValidatedSnapshot {
        org_id: manifest.org_id.clone(),
        authority: manifest.position.epoch,
        position: manifest.position,
        digest: manifest.digest,
        codec: Codec::new(CODEC, 1).map_err(storage_error)?,
        staging_id: ulid::generate().map_err(storage_error)?,
        protocol_level: manifest.protocol_level,
    };
    let id = staged.staging_id.clone();
    let position = staged.position;
    let digest = staged.digest;
    staged.staging_id = store.sync_write(move |db| {
        let current = OrganizationTxn(db).checkpoint()?;
        check_stage(&current, &peer, mode, position, digest)?;
        check_native_sequence(db, &current, &projection)?;
        let epoch = int(position.epoch.get())?;
        let seq = int(position.seq)?;
        if let Some(committed) = installed_stage_id(db, position, digest)? {
            return Ok(committed);
        }
        let existing: Option<(String, i64, [u8; 32])> = db
            .query_row(
                "SELECT stage_id, seq, digest FROM org_snapshot_stage WHERE epoch = ?1",
                [epoch],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .optional()
            .map_err(storage_error)?;
        if let Some((existing_id, existing_seq, existing_digest)) = existing {
            if seq < existing_seq || (seq == existing_seq && digest.as_bytes() != &existing_digest)
            {
                return Err(Refusal::Divergence.into());
            }
            if seq == existing_seq {
                return Ok(existing_id);
            }
        }
        db.execute(
            "INSERT INTO org_snapshot_stage (singleton, stage_id, epoch, seq, digest, body)
             VALUES (1, ?1, ?2, ?3, ?4, ?5) ON CONFLICT(singleton) DO UPDATE SET
             stage_id = excluded.stage_id, epoch = excluded.epoch, seq = excluded.seq,
             digest = excluded.digest, body = excluded.body",
            params![id, epoch, seq, digest.as_bytes(), body],
        )
        .map_err(storage_error)?;
        Ok(id.clone())
    })?;
    Ok(staged)
}

fn check_stage(
    current: &OrgCheckpoint,
    peer: &PeerIdentity,
    mode: SnapshotStageMode,
    position: Position,
    digest: Digest,
) -> Result<(), Refusal> {
    if current.org_id != peer.org_id
        || current.hq_office != peer.office_id
        || current.highest_epoch != peer.authority
        || current.local_office == current.hq_office
    {
        return Err(Refusal::NotAuthorized);
    }
    match (current.restore_pending, mode) {
        (false, SnapshotStageMode::Normal) => {
            if position.seq < current.position.seq
                || (position == current.position && digest != current.digest)
            {
                return Err(Refusal::Divergence);
            }
            Ok(())
        }
        (
            true,
            SnapshotStageMode::RecoveryWarningShown {
                authority,
                restored_digest,
            },
        ) if current.highest_epoch == authority
            && current.pending_restore_base == Some(restored_digest) =>
        {
            if position.seq == 0 && digest != restored_digest {
                return Err(Refusal::Divergence);
            }
            Ok(())
        }
        _ => Err(Refusal::RestorePending),
    }
}

fn check_native_sequence(
    db: &Connection,
    current: &OrgCheckpoint,
    incoming: &Projection,
) -> Result<(), SyncError> {
    if incoming.header.epoch != current.position.epoch.get()
        || super::replication::awaiting_snapshot(db)?
    {
        return Ok(());
    }
    let local = super::head(db).map_err(storage_error)?;
    if incoming.native_seq < local.seq
        || (incoming.header.sequence == current.position.seq && incoming.native_seq != local.seq)
    {
        return Err(Refusal::Divergence.into());
    }
    Ok(())
}

fn decode(body: &[u8]) -> Result<Projection, Refusal> {
    let value: Projection = serde_json::from_slice(body).map_err(|_| Refusal::IntegrityFailure)?;
    if value.native_seq > value.header.sequence {
        return Err(Refusal::IntegrityFailure);
    }
    for ((table, _, binds), rows) in TABLES.into_iter().zip(&value.rows) {
        let mut ids = HashSet::with_capacity(rows.len());
        for row in rows {
            if row.len() != binds.split(',').count()
                || !valid_row(table, row)
                || !ids.insert(&row[0])
            {
                return Err(Refusal::IntegrityFailure);
            }
        }
    }
    validate_relationships(&value)?;
    Ok(value)
}

fn validate_relationships(value: &Projection) -> Result<(), Refusal> {
    let [members, teams, projects, areas, memberships, links, commands] = &value.rows;
    let mut member_by_id = HashMap::with_capacity(members.len());
    for row in members {
        let [Cell::Text(id), _, _, Cell::Text(body)] = row.as_slice() else {
            return Err(Refusal::IntegrityFailure);
        };
        let member: Member = serde_json::from_str(body).map_err(|_| Refusal::IntegrityFailure)?;
        member_by_id.insert(id.as_str(), member);
    }
    let mut owners = member_by_id.values().filter(|member| member.owner);
    let owner = owners.next().ok_or(Refusal::IntegrityFailure)?;
    if owners.next().is_some() || !valid_owner(owner) {
        return Err(Refusal::IntegrityFailure);
    }
    let valid_team_member = |cell: &Cell| match cell {
        Cell::Text(id) => member_by_id
            .get(id.as_str())
            .is_some_and(|member| !member.owner && !is_offboarded(member)),
        _ => false,
    };
    let invalid_reporting = member_by_id.values().any(|member| {
        member.reports_to_member_id.as_deref().is_some_and(|id| {
            member_by_id
                .get(id)
                .is_none_or(|manager| is_offboarded(manager) && !is_offboarded(member))
        })
    });
    let team_ids = row_ids(teams);
    let project_ids = row_ids(projects);
    let handles = members
        .iter()
        .map(|row| &row[1])
        .filter(|cell| !matches!(cell, Cell::Null));
    let team_members: HashSet<_> = memberships.iter().map(|row| (&row[1], &row[2])).collect();
    if !distinct(handles)
        || !distinct(teams.iter().map(|row| (&row[1], &row[2])))
        || team_members.len() != memberships.len()
        || !distinct(links.iter().map(|row| (&row[1], &row[2])))
        || invalid_reporting
        || teams.iter().any(|row| {
            !references(&row[1], &team_ids)
                || (!matches!(row[3], Cell::Null)
                    && (!valid_team_member(&row[3]) || !team_members.contains(&(&row[0], &row[3]))))
        })
        || areas.iter().any(|row| !references(&row[1], &project_ids))
        || memberships
            .iter()
            .any(|row| !references(&row[1], &team_ids) || !valid_team_member(&row[2]))
        || links
            .iter()
            .any(|row| !references(&row[1], &project_ids) || !references(&row[2], &team_ids))
        || commands.iter().any(|row| {
            let (Cell::Integer(epoch), Cell::Integer(seq)) = (&row[1], &row[2]) else {
                return true;
            };
            *epoch as u64 > value.header.epoch
                || (*epoch as u64 == value.header.epoch && *seq as u64 > value.native_seq)
        })
        || !acyclic(members, 2)
        || !acyclic(teams, 1)
    {
        return Err(Refusal::IntegrityFailure);
    }
    Ok(())
}

fn acyclic(rows: &[Vec<Cell>], parent_column: usize) -> bool {
    let parents: HashMap<_, _> = rows
        .iter()
        .filter_map(|row| match (&row[0], &row[parent_column]) {
            (Cell::Text(child), Cell::Text(parent)) => Some((child.as_str(), parent.as_str())),
            _ => None,
        })
        .collect();
    let mut complete = HashSet::new();
    let mut visiting = HashSet::new();
    for mut current in parents.keys().copied() {
        while let Some(parent) = parents.get(current) {
            if complete.contains(current) {
                break;
            }
            if !visiting.insert(current) {
                return false;
            }
            current = parent;
        }
        complete.extend(visiting.drain());
    }
    true
}

fn distinct<T: Eq + std::hash::Hash>(values: impl IntoIterator<Item = T>) -> bool {
    let mut seen = HashSet::new();
    values.into_iter().all(|value| seen.insert(value))
}

fn row_ids(rows: &[Vec<Cell>]) -> HashSet<&str> {
    rows.iter()
        .filter_map(|row| match row.first() {
            Some(Cell::Text(id)) => Some(id.as_str()),
            _ => None,
        })
        .collect()
}

fn references(cell: &Cell, ids: &HashSet<&str>) -> bool {
    match cell {
        Cell::Null => true,
        Cell::Text(id) => ids.contains(id.as_str()),
        Cell::Integer(_) => false,
    }
}

fn valid_row(table: &str, row: &[Cell]) -> bool {
    if table == "commands" {
        return valid_command_row(row);
    }
    let Some(Cell::Text(id)) = row.first() else {
        return false;
    };
    let Some(Cell::Text(body)) = row.last() else {
        return false;
    };
    match table {
        "members" => serde_json::from_str::<Member>(body).is_ok_and(|member| {
            member.id == *id
                && matches_text(&row[1], super::handle_key(&member).as_deref())
                && matches_text(&row[2], member.reports_to_member_id.as_deref())
        }),
        "teams" => serde_json::from_str::<Team>(body).is_ok_and(|team| {
            team.id == *id
                && matches_text(&row[1], team.owner_team_id.as_deref())
                && matches_text(&row[2], Some(&crate::rules::name_key(&team.name)))
                && matches_text(&row[3], team.lead_member_id.as_deref())
        }),
        "projects" => serde_json::from_str::<Project>(body).is_ok_and(|project| project.id == *id),
        "areas" => serde_json::from_str::<Area>(body)
            .is_ok_and(|area| area.id == *id && matches_text(&row[1], Some(&area.project_id))),
        "memberships" => serde_json::from_str::<Membership>(body).is_ok_and(|membership| {
            membership.id == *id
                && matches_text(&row[1], Some(&membership.team_id))
                && matches_text(&row[2], Some(&membership.member_id))
        }),
        "project_teams" => serde_json::from_str::<ProjectTeam>(body).is_ok_and(|link| {
            link.id == *id
                && matches_text(&row[1], Some(&link.project_id))
                && matches_text(&row[2], Some(&link.team_id))
        }),
        _ => false,
    }
}

fn valid_command_row(row: &[Cell]) -> bool {
    let [Cell::Text(id), Cell::Integer(epoch), Cell::Integer(seq), Cell::Text(expires), answer] =
        row
    else {
        return false;
    };
    if ulid::time_ms(id).is_none()
        || crate::time::parse_rfc3339_ms(expires).is_none()
        || *epoch < 0
        || *seq < 0
        || (*epoch == 0) != (*seq == 0)
    {
        return false;
    }
    match answer {
        // Older operation rows may predate the persisted result body.
        Cell::Null => true,
        Cell::Text(body) => serde_json::from_str::<Answer>(body).is_ok_and(|answer| {
            super::valid_fingerprint(&answer.params_hash)
                && answer.refusal.is_some() == (*epoch == 0)
        }),
        Cell::Integer(_) => false,
    }
}

fn matches_text(cell: &Cell, expected: Option<&str>) -> bool {
    match (cell, expected) {
        (Cell::Null, None) => true,
        (Cell::Text(actual), Some(expected)) => actual == expected,
        _ => false,
    }
}

pub(super) fn install(
    db: &Connection,
    staged: &ValidatedSnapshot,
    next: &OrgCheckpoint,
) -> Result<bool, SyncError> {
    if staged.codec.id() != CODEC || staged.codec.version() != 1 {
        return Err(Refusal::UnsupportedVersion.into());
    }
    if installed_stage_id(db, next.position, next.digest)?.as_deref() == Some(&staged.staging_id) {
        return Ok(false);
    }
    let body: Vec<u8> = db
        .query_row(
            "SELECT body FROM org_snapshot_stage WHERE stage_id = ?1",
            [&staged.staging_id],
            |row| row.get(0),
        )
        .optional()
        .map_err(storage_error)?
        .ok_or(Refusal::InvalidInput)?;
    let projection = decode(&body)?;
    if projection.header
        != (Header {
            org_id: next.org_id.as_str().to_owned(),
            hq_office: next.hq_office.as_str().to_owned(),
            epoch: next.position.epoch.get(),
            sequence: next.position.seq,
            digest: *next.digest.as_bytes(),
            protocol: next.protocol_level,
            schema: SCHEMA.version,
        })
    {
        return Err(Refusal::Divergence.into());
    }

    let current = OrganizationTxn(db).checkpoint()?;
    check_native_sequence(db, &current, &projection)?;
    // An exact materialized checkpoint needs only a commit receipt. Keep its
    // native replay history and its older federation base intact.
    let replace = current.position != next.position
        || current.digest != next.digest
        || super::replication::awaiting_snapshot(db)?;
    if replace {
        replace_projection(db, next.protocol_level, projection)?;
    }
    db.execute(
        "INSERT INTO org_snapshot_install VALUES (1, ?1, ?2, ?3, ?4)
         ON CONFLICT(singleton) DO UPDATE SET stage_id = excluded.stage_id,
             epoch = excluded.epoch, seq = excluded.seq, digest = excluded.digest",
        params![
            staged.staging_id,
            int(next.position.epoch.get())?,
            int(next.position.seq)?,
            next.digest.as_bytes()
        ],
    )
    .map_err(storage_error)?;
    db.execute(
        "DELETE FROM org_snapshot_stage WHERE stage_id = ?1",
        [&staged.staging_id],
    )
    .map_err(storage_error)?;
    Ok(replace)
}

fn installed_stage_id(
    db: &Connection,
    position: Position,
    digest: Digest,
) -> Result<Option<String>, StoreError> {
    db.query_row(
        "SELECT stage_id FROM org_snapshot_install WHERE (epoch, seq, digest) = (?1, ?2, ?3)",
        (
            int(position.epoch.get())?,
            int(position.seq)?,
            digest.as_bytes(),
        ),
        |row| row.get(0),
    )
    .optional()
    .map_err(storage_error)
}

fn replace_projection(
    db: &Connection,
    protocol_level: u32,
    projection: Projection,
) -> Result<(), StoreError> {
    clear_projection(db)?;
    for ((table, columns, binds), rows) in TABLES.into_iter().zip(projection.rows) {
        let mut insert = db
            .prepare(&format!("INSERT INTO {table} ({columns}) VALUES ({binds})"))
            .map_err(storage_error)?;
        for row in rows {
            let values = row.iter().map(|value| {
                ToSqlOutput::Borrowed(match value {
                    Cell::Null => ValueRef::Null,
                    Cell::Integer(value) => ValueRef::Integer(*value),
                    Cell::Text(value) => ValueRef::Text(value.as_bytes()),
                })
            });
            insert
                .execute(params_from_iter(values))
                .map_err(storage_error)?;
        }
    }
    db.execute(
        "UPDATE org_replica SET protocol_level = ?1, awaiting_snapshot = 0, native_seq = ?2",
        (protocol_level, int(projection.native_seq)?),
    )
    .map_err(storage_error)?;
    Ok(())
}

pub(super) fn clear_projection(db: &Connection) -> Result<(), StoreError> {
    db.execute_batch("DELETE FROM org_replica_events; DELETE FROM events;")
        .map_err(storage_error)?;
    for (table, _, _) in TABLES.iter().rev() {
        db.execute(&format!("DELETE FROM {table}"), [])
            .map_err(storage_error)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_row_requires_the_columns_the_receiver_will_insert() {
        let mut row = vec![
            Cell::Text("01ARZ3NDEKTSV4RRFFQ69G5FAV".into()),
            Cell::Integer(1),
            Cell::Integer(2),
            Cell::Null,
            Cell::Text(format!(r#"{{"params_hash":"{}"}}"#, "12".repeat(32))),
        ];
        assert!(!valid_row("commands", &row));
        row[3] = Cell::Text("2016-08-29T00:00:00.000Z".into());
        assert!(valid_row("commands", &row));
        row[4] = Cell::Null;
        assert!(
            valid_row("commands", &row),
            "legacy result body can be NULL"
        );
    }
}
