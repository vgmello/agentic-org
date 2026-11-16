//! Immutable frozen work transactions on the office's sole writer. Preparation
//! is rolled back before publication; recovery never invokes the decision again.

#[cfg(test)]
pub(crate) mod crash;
pub(crate) mod durable;
mod rows;
pub(crate) mod schema;

use std::{
    collections::{BTreeMap, BTreeSet},
    panic::{catch_unwind, AssertUnwindSafe},
    path::PathBuf,
};

use org_work::selected_codec::{self as codec, Json, NumberToken};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

use super::{origin, Store, WriteJob};
use crate::OfficeError;

/// Supplied only by protected host/HQ admission, not a browser task command.
/// A local lock alone is not sufficient writer fencing for copied roots.
pub use org_proto::selected_tasks::{JournalPosition, WriterIdentity as AcceptanceIdentity};

/// Implemented by the authenticated host control channel. Confirmation persists
/// outside the organization root before completion; EOF fails closed.
pub trait WriterAuthority: Send + Sync {
    fn checkpoint(
        &self,
        identity: &AcceptanceIdentity,
    ) -> Result<Option<JournalPosition>, OfficeError>;
    fn confirm(
        &self,
        identity: &AcceptanceIdentity,
        before: Option<&JournalPosition>,
        after: &JournalPosition,
    ) -> Result<(), OfficeError>;
}

#[derive(Clone)]
pub struct AcceptanceConfig {
    pub identity: AcceptanceIdentity,
    pub protected_root: PathBuf,
    pub public_root: PathBuf,
    /// Required in production. Unit fixtures may exercise storage independently.
    pub authority: Option<std::sync::Arc<dyn WriterAuthority>>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Envelope {
    schema_version: i64,
    json_codec: i64,
    resource_profile: String,
    journal_kind: String,
    identity: AcceptanceIdentity,
    kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    commit_ordinal: Option<i64>,
    previous_sha256: Option<String>,
    projection_schema_sha256: String,
    origin_incarnation: String,
    origin_before: i64,
    origin_after: i64,
    origin_entries: Vec<origin::OriginEntry>,
    mutations: Vec<rows::Mutation>,
    payload_sha256: String,
}

impl Envelope {
    fn position(&self) -> JournalPosition {
        JournalPosition {
            ordinal: self.commit_ordinal.unwrap_or(0),
            sha256: self.payload_sha256.clone(),
            previous_sha256: self.previous_sha256.clone(),
            origin_incarnation: self.origin_incarnation.clone(),
            origin_high_water: self.origin_after,
        }
    }
}

struct Publication {
    bytes: Vec<u8>,
    blobs: BTreeMap<String, Vec<u8>>,
    hash: String,
}

pub(crate) struct Journal {
    config: AcceptanceConfig,
    head: String,
    ordinal: i64,
    origin: String,
    high_water: i64,
    position: Option<JournalPosition>,
    ready: bool,
}

fn storage(e: impl std::error::Error + Send + Sync + 'static) -> OfficeError {
    OfficeError::storage(e)
}
fn invalid() -> OfficeError {
    OfficeError::Invalid("work acceptance recovery required")
}

impl Store {
    pub(crate) fn install_acceptance(&self, config: AcceptanceConfig) -> Result<(), OfficeError> {
        self.control(move |state| {
            if state.acceptance.is_some() {
                return Err(OfficeError::Invalid("work acceptance already installed"));
            }
            // Once admission starts, a failed startup cannot fall through to the
            // legacy batching path. The host must recover/reopen this worker.
            state.recovery_required = true;
            rows::install_capture(&state.db)?;
            let opened = catch_unwind(AssertUnwindSafe(|| Journal::open(&state.db, config)))
                .unwrap_or_else(|panic| Err(super::panicked(panic)));
            if !state.db.is_autocommit() {
                state.db.execute_batch("ROLLBACK").map_err(storage)?;
            }
            rows::mode(&state.db, 0)?;
            let journal = opened?;
            state.acceptance = Some(journal);
            state.recovery_required = false;
            Ok(())
        })
    }

    /// New selected operations are FnOnce jobs, serialized at the same barrier as
    /// legacy work. SQLite and filesystem projection stay inside this module.
    #[cfg(test)]
    pub(crate) fn selected_write<R: Send + 'static>(
        &self,
        work: impl FnOnce(&Connection) -> Result<R, OfficeError> + Send + 'static,
    ) -> Result<R, OfficeError> {
        self.selected_transaction(move |db, _| work(db))
    }

    pub(crate) fn selected_transaction<R: Send + 'static>(
        &self,
        work: impl FnOnce(&Connection, &AcceptanceIdentity) -> Result<R, OfficeError> + Send + 'static,
    ) -> Result<R, OfficeError> {
        let identity = self.selected_read(|_, id| Ok(id.clone()))?;
        let mut work = Some(work);
        let (job, answer) = Self::job_with_batch_abort(
            move |db| {
                let f = work
                    .take()
                    .ok_or(OfficeError::Invalid("selected decision cannot rerun"))?;
                f(db, &identity).map(|r| (r, None))
            },
            |_| false,
        );
        self.control(move |state| {
            let Some(journal) = state.acceptance.as_mut() else {
                return Err(OfficeError::Invalid("selected acceptance unavailable"));
            };
            run_job(&mut state.db, journal, job);
            Ok(())
        })?;
        answer.recv().map_err(|_| invalid())?
    }

    pub(crate) fn selected_read<R: Send + 'static>(
        &self,
        read: impl FnOnce(&Connection, &AcceptanceIdentity) -> Result<R, OfficeError> + Send + 'static,
    ) -> Result<R, OfficeError> {
        self.control(move |state| {
            let journal = state
                .acceptance
                .as_ref()
                .filter(|j| j.ready)
                .ok_or_else(invalid)?;
            journal.verify_cursor(&state.db)?;
            journal.verify_authority()?;
            verify_artifacts(&state.db, &journal.config)?;
            read(&state.db, &journal.config.identity)
        })
    }
}

fn validate_acceptance_config(
    db: &Connection,
    config: &AcceptanceConfig,
) -> Result<(), OfficeError> {
    for s in [
        &config.identity.instance_id,
        &config.identity.organization_id,
        &config.identity.office_id,
        &config.identity.journal_incarnation,
        &config.identity.machine_id,
    ] {
        if s.is_empty() {
            return Err(invalid());
        }
    }
    durable::component(&config.identity.office_id).map_err(storage)?;
    durable::component(&config.identity.journal_incarnation).map_err(storage)?;
    if config.identity.writer_epoch <= 0
        || !config.protected_root.is_absolute()
        || !config.public_root.is_absolute()
        || config.protected_root.starts_with(&config.public_root)
        || config.public_root.starts_with(&config.protected_root)
    {
        return Err(invalid());
    }
    for (key, expected) in [
        ("office_id", config.identity.office_id.as_str()),
        ("instance_id", config.identity.instance_id.as_str()),
    ] {
        let actual: Option<String> = db
            .query_row("SELECT value FROM meta WHERE key=?1", [key], |r| r.get(0))
            .optional()
            .map_err(storage)?;
        if actual.as_deref() != Some(expected) {
            return Err(invalid());
        }
    }
    Ok(())
}

fn create_anchor(
    db: &Connection,
    journal: &Journal,
    admitted: Option<&JournalPosition>,
) -> Result<Envelope, OfficeError> {
    if admitted.is_some() {
        return Err(invalid());
    }
    let cursor: bool = db
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM selected_journal_cursor)",
            [],
            |r| r.get(0),
        )
        .map_err(storage)?;
    if cursor {
        return Err(invalid());
    }
    rows::mode(db, 2)?;
    db.execute_batch("BEGIN IMMEDIATE").map_err(storage)?;
    let prepared = (|| {
        let incarnation = origin::incarnation(db)?;
        let high = i64::try_from(origin::high_water(db, &incarnation)?).map_err(|_| invalid())?;
        let entries = read_origins(db, &incarnation, 0)?;
        validate_origins(&journal.config.identity, &incarnation, 0, high, &entries)?;
        Ok(Envelope {
            schema_version: 1,
            json_codec: 2,
            resource_profile: codec::RESOURCE_PROFILE.into(),
            journal_kind: "work-acceptance".into(),
            identity: journal.config.identity.clone(),
            kind: "anchor".into(),
            commit_ordinal: None,
            previous_sha256: None,
            projection_schema_sha256: rows::schema_digest(db)?,
            origin_incarnation: incarnation,
            origin_before: high,
            origin_after: high,
            origin_entries: Vec::new(),
            mutations: rows::checkpoint(db)?,
            payload_sha256: String::new(),
        })
    })();
    db.execute_batch("ROLLBACK").map_err(storage)?;
    rows::mode(db, 0)?;
    let mut anchor = prepared?;
    journal.publish(&mut anchor, &journal.anchor_path())?;
    Ok(anchor)
}

fn journal_ordinals(journal: &Journal) -> Result<Vec<i64>, OfficeError> {
    let directory = journal
        .config
        .protected_root
        .join(format!("{}/transactions", journal.prefix()));
    let mut ordinals = Vec::new();
    match std::fs::read_dir(&directory) {
        Ok(entries) => {
            for entry in entries {
                let entry = entry.map_err(storage)?;
                let name = entry.file_name().into_string().map_err(|_| invalid())?;
                if name.starts_with(".acceptance-") && name.ends_with(".tmp") {
                    continue;
                }
                if entry.file_type().map_err(storage)?.is_symlink() {
                    return Err(invalid());
                }
                let number = name
                    .strip_suffix(".json")
                    .and_then(|s| s.parse::<i64>().ok())
                    .filter(|n| *n > 0)
                    .ok_or_else(invalid)?;
                if name != format!("{number}.json") {
                    return Err(invalid());
                }
                ordinals.push(number);
            }
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(storage(e)),
    }
    ordinals.sort();
    Ok(ordinals)
}

struct RecoverySource {
    expected: rows::FrozenState,
    allowed_files: ArtifactHashes,
    verified_prefix: Vec<(i64, String)>,
}

fn verify_recovery_source(
    journal: &mut Journal,
    anchor: &Envelope,
    admitted: Option<&JournalPosition>,
) -> Result<RecoverySource, OfficeError> {
    // Retained source rows establish the initial high-water; no fabricated T0.
    let mut expected = rows::FrozenState::default();
    expected.extend(&anchor.mutations)?;
    let mut allowed_files = BTreeMap::new();
    retain_artifact_hashes(&mut allowed_files, &anchor.mutations)?;
    journal.head = anchor.payload_sha256.clone();
    journal.origin = anchor.origin_incarnation.clone();
    journal.high_water = anchor.origin_after;
    journal.position = Some(anchor.position());
    let mut checkpoint_seen = admitted == journal.position.as_ref();
    let ordinals = journal_ordinals(journal)?;
    let mut verified_prefix = Vec::new();
    for number in ordinals {
        if journal.ordinal.checked_add(1) != Some(number) {
            return Err(invalid());
        }
        let bytes = durable::read(
            &journal.config.protected_root,
            &journal.transaction_path(number),
            codec::MAX_JSON_BYTES,
        )
        .map_err(storage)?;
        let envelope = journal.decode(&bytes)?;
        journal.verify_next(&envelope)?;
        expected.extend(&envelope.mutations)?;
        retain_artifact_hashes(&mut allowed_files, &envelope.mutations)?;
        verified_prefix.push((number, envelope.payload_sha256.clone()));
        journal.advance(&envelope);
        checkpoint_seen |= admitted == journal.position.as_ref();
    }
    if journal.config.authority.is_some() {
        let maximum = match &admitted {
            Some(p) => p.ordinal.saturating_add(1),
            None => 0,
        };
        if (admitted.is_some() && !checkpoint_seen) || journal.ordinal > maximum {
            return Err(invalid());
        }
    }
    Ok(RecoverySource {
        expected,
        allowed_files,
        verified_prefix,
    })
}

fn projection_needed(db: &Connection, e: &Envelope, anchor: bool) -> Result<bool, OfficeError> {
    let ordinal = e.commit_ordinal.unwrap_or(0);
    let cursor: Option<(i64, String)> = db
        .query_row(
            "SELECT ordinal,digest FROM selected_journal_cursor WHERE singleton=1",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()
        .map_err(storage)?;
    if let Some((applied, digest)) = &cursor {
        if *applied > ordinal {
            return Ok(false);
        } // Full chain still validated by open.
        if *applied == ordinal {
            return if digest == &e.payload_sha256 {
                Ok(false)
            } else {
                Err(invalid())
            };
        }
        if anchor
            || *applied + 1 != ordinal
            || Some(digest.as_str()) != e.previous_sha256.as_deref()
        {
            return Err(invalid());
        }
    } else if !anchor {
        return Err(invalid());
    }
    Ok(true)
}

impl Journal {
    fn checkpoint(&self) -> Result<Option<JournalPosition>, OfficeError> {
        match &self.config.authority {
            Some(authority) => authority.checkpoint(&self.config.identity),
            #[cfg(test)]
            None => Ok(None),
            #[cfg(not(test))]
            None => Err(OfficeError::Invalid(
                "protected writer admission unavailable",
            )),
        }
    }

    fn verify_authority(&self) -> Result<(), OfficeError> {
        let expected = self.checkpoint()?;
        if self.config.authority.is_some() && expected != self.position {
            return Err(invalid());
        }
        Ok(())
    }

    fn confirm(
        &self,
        before: Option<&JournalPosition>,
        after: &JournalPosition,
    ) -> Result<(), OfficeError> {
        if let Some(authority) = &self.config.authority {
            authority.confirm(&self.config.identity, before, after)?;
        }
        Ok(())
    }
    fn verify_cursor(&self, db: &Connection) -> Result<(), OfficeError> {
        let cursor:(String,i64,String,String,i64)=db.query_row("SELECT identity,ordinal,digest,origin_incarnation,origin_high_water FROM selected_journal_cursor WHERE singleton=1",[],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?))).map_err(storage)?;
        let identity: AcceptanceIdentity =
            serde_json::from_str(&cursor.0).map_err(|_| invalid())?;
        if identity != self.config.identity
            || (cursor.1, cursor.2, cursor.3, cursor.4)
                != (
                    self.ordinal,
                    self.head.clone(),
                    self.origin.clone(),
                    self.high_water,
                )
        {
            return Err(invalid());
        }
        Ok(())
    }
    fn prefix(&self) -> String {
        format!(
            "offices/{}/journals/work-acceptance/{}",
            self.config.identity.office_id, self.config.identity.journal_incarnation
        )
    }
    fn anchor_path(&self) -> String {
        format!("{}/anchor.json", self.prefix())
    }
    fn transaction_path(&self, n: i64) -> String {
        format!("{}/transactions/{n}.json", self.prefix())
    }

    fn open(db: &Connection, config: AcceptanceConfig) -> Result<Self, OfficeError> {
        validate_acceptance_config(db, &config)?;
        durable::directories(&config.protected_root, &config.protected_root).map_err(storage)?;
        durable::directories(&config.public_root, &config.public_root).map_err(storage)?;
        let mut journal = Self {
            config,
            head: String::new(),
            ordinal: 0,
            origin: String::new(),
            high_water: 0,
            position: None,
            ready: false,
        };
        let admitted = journal.checkpoint()?;
        if admitted.as_ref().is_some_and(|p| !p.valid()) {
            return Err(invalid());
        }
        let anchor = match durable::read(
            &journal.config.protected_root,
            &journal.anchor_path(),
            codec::MAX_JSON_BYTES,
        ) {
            Ok(bytes) => journal.decode(&bytes)?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                create_anchor(db, &journal, admitted.as_ref())?
            }
            Err(e) => return Err(storage(e)),
        };
        if anchor.kind != "anchor"
            || anchor.commit_ordinal.is_some()
            || anchor.previous_sha256.is_some()
            || anchor.origin_before != anchor.origin_after
            || !anchor.origin_entries.is_empty()
        {
            return Err(invalid());
        }
        let recovered = verify_recovery_source(&mut journal, &anchor, admitted.as_ref())?;
        // Verify the complete retained source before mutating projections. SQL
        // can be rebuilt while canonical files are already at a later revision;
        // never rewind those files through intermediate historical snapshots.
        journal.project(db, &anchor, true, true)?;
        for (number, digest) in recovered.verified_prefix {
            let bytes = durable::read(
                &journal.config.protected_root,
                &journal.transaction_path(number),
                codec::MAX_JSON_BYTES,
            )
            .map_err(storage)?;
            let envelope = journal.decode(&bytes)?;
            if envelope.payload_sha256 != digest {
                return Err(invalid());
            }
            journal.project(db, &envelope, false, true)?;
        }
        // A cursor ahead of the retained source cannot prove acknowledged history.
        let cursor: (i64, String) = db
            .query_row(
                "SELECT ordinal,digest FROM selected_journal_cursor WHERE singleton=1",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .map_err(storage)?;
        if cursor != (journal.ordinal, journal.head.clone()) {
            return Err(invalid());
        }
        recovered.expected.verify(db)?;
        repair_artifacts(db, &journal.config, &recovered.allowed_files)?;
        rows::mode(db, 0)?;
        journal.confirm(
            admitted.as_ref(),
            journal.position.as_ref().ok_or_else(invalid)?,
        )?;
        journal.ready = true;
        Ok(journal)
    }

    fn decode(&self, bytes: &[u8]) -> Result<Envelope, OfficeError> {
        let mut json = codec::parse(bytes).map_err(storage)?;
        codec::canonical(&json).map_err(storage)?;
        if json.get("resource_profile").and_then(Json::as_str) != Some(codec::RESOURCE_PROFILE) {
            return Err(invalid());
        }
        let stored = json
            .get("payload_sha256")
            .and_then(Json::as_str)
            .ok_or_else(invalid)?;
        if codec::payload_sha256(&json).map_err(storage)? != stored {
            return Err(invalid());
        }
        let mut lengths = Vec::new();
        self.hydrate(&mut json, &mut lengths)?;
        codec::check_blob_lengths(lengths).map_err(storage)?;
        // Hydrated strings may exceed the manifest's single-token budget by
        // design; serde handles only known typed cells, not event payload numbers.
        let value = to_serde(json)?;
        let envelope: Envelope = serde_json::from_value(value).map_err(|_| invalid())?;
        if envelope.schema_version != 1
            || envelope.json_codec != 2
            || envelope.journal_kind != "work-acceptance"
            || envelope.identity != self.config.identity
            || envelope.projection_schema_sha256.is_empty()
        {
            return Err(invalid());
        }
        Ok(envelope)
    }

    fn hydrate(&self, json: &mut Json, lengths: &mut Vec<u64>) -> Result<(), OfficeError> {
        match json {
            Json::Object(map) if map.contains_key("$journal_blob") => {
                if map.len() != 3 {
                    return Err(invalid());
                }
                let digest = map
                    .get("$journal_blob")
                    .and_then(Json::as_str)
                    .ok_or_else(invalid)?;
                if !digest_ok(digest) {
                    return Err(invalid());
                }
                let length = match map.get("byte_length") {
                    Some(Json::Number(n)) if n.as_str() == "0" => 0,
                    Some(n) => n.positive_i64().ok_or_else(invalid)? as u64,
                    _ => return Err(invalid()),
                };
                lengths.push(length);
                codec::check_blob_lengths(lengths.iter().copied()).map_err(storage)?;
                let data = durable::read(
                    &self.config.protected_root,
                    &format!("blobs/sha256/{digest}"),
                    codec::MAX_BLOB_BYTES as usize,
                )
                .map_err(storage)?;
                if data.len() as u64 != length || codec::sha256(&data) != digest {
                    return Err(invalid());
                }
                let value = match map.get("encoding").and_then(Json::as_str) {
                    Some("utf-8") => String::from_utf8(data).map_err(|_| invalid())?,
                    Some("hex") => rows::hex(&data),
                    _ => return Err(invalid()),
                };
                *json = Json::String(value);
            }
            Json::Object(map) => {
                for v in map.values_mut() {
                    self.hydrate(v, lengths)?;
                }
            }
            Json::Array(values) => {
                for v in values {
                    self.hydrate(v, lengths)?;
                }
            }
            _ => {}
        }
        Ok(())
    }

    fn publish(&self, envelope: &mut Envelope, path: &str) -> Result<(), OfficeError> {
        let frozen = Self::freeze(envelope)?;
        self.publish_frozen(envelope, path, frozen)
    }

    fn freeze(envelope: &Envelope) -> Result<Publication, OfficeError> {
        let mut json = from_serde(serde_json::to_value(envelope).map_err(storage)?)?;
        let mut blobs = BTreeMap::new();
        let mut lengths = Vec::new();
        externalize(&mut json, &mut blobs, &mut lengths)?;
        codec::check_blob_lengths(lengths).map_err(storage)?;
        let hash = codec::payload_sha256(&json).map_err(storage)?;
        if let Json::Object(map) = &mut json {
            map.insert("payload_sha256".into(), Json::String(hash.clone()));
        }
        let bytes = codec::canonical(&json).map_err(storage)?;
        Ok(Publication { bytes, blobs, hash })
    }

    fn publish_frozen(
        &self,
        envelope: &mut Envelope,
        path: &str,
        frozen: Publication,
    ) -> Result<(), OfficeError> {
        for (digest, data) in frozen.blobs {
            durable::publish(
                &self.config.protected_root,
                &format!("blobs/sha256/{digest}"),
                &data,
                true,
                None,
            )
            .map_err(storage)?;
        }
        durable::publish(&self.config.protected_root, path, &frozen.bytes, true, None)
            .map_err(storage)?;
        envelope.payload_sha256 = frozen.hash;
        Ok(())
    }

    fn verify_next(&self, e: &Envelope) -> Result<(), OfficeError> {
        if e.kind != "transaction"
            || e.commit_ordinal != self.ordinal.checked_add(1)
            || e.previous_sha256.as_deref() != Some(&self.head)
            || e.origin_incarnation != self.origin
            || e.origin_before != self.high_water
        {
            return Err(invalid());
        }
        validate_origins(
            &self.config.identity,
            &self.origin,
            e.origin_before,
            e.origin_after,
            &e.origin_entries,
        )
    }

    fn advance(&mut self, e: &Envelope) {
        self.ordinal = e.commit_ordinal.expect("verified transaction ordinal");
        self.head = e.payload_sha256.clone();
        self.high_water = e.origin_after;
        self.position = Some(e.position());
    }

    fn project(
        &self,
        db: &Connection,
        e: &Envelope,
        anchor: bool,
        recovering: bool,
    ) -> Result<(), OfficeError> {
        project_envelope(&self.config, db, e, anchor, recovering)
    }
}

fn verify_projected_origin(db: &Connection, e: &Envelope, anchor: bool) -> Result<(), OfficeError> {
    if origin::stored_incarnation(db)?.as_deref() != Some(e.origin_incarnation.as_str())
        || origin::high_water(db, &e.origin_incarnation)? != e.origin_after as u64
    {
        return Err(invalid());
    }
    if !anchor
        && read_origins(db, &e.origin_incarnation, e.origin_before as u64)? != e.origin_entries
    {
        return Err(invalid());
    }
    Ok(())
}

fn project_envelope(
    config: &AcceptanceConfig,
    db: &Connection,
    e: &Envelope,
    anchor: bool,
    recovering: bool,
) -> Result<(), OfficeError> {
    if rows::schema_digest(db)? != e.projection_schema_sha256 {
        return Err(invalid());
    }
    let ordinal = e.commit_ordinal.unwrap_or(0);
    if !projection_needed(db, e, anchor)? {
        return Ok(());
    }
    let existing_origin = origin::stored_incarnation(db)?;
    if !anchor
        && (existing_origin.as_deref() != Some(e.origin_incarnation.as_str())
            || origin::high_water(db, &e.origin_incarnation)? != e.origin_before as u64)
    {
        return Err(invalid());
    }
    rows::mode(db, 2)?;
    db.execute_batch("BEGIN IMMEDIATE").map_err(storage)?;
    let result = (|| {
        if !recovering {
            materialize_final(config, &e.mutations)?;
        }
        for mutation in &e.mutations {
            rows::apply(db, mutation, anchor)?;
        }
        verify_projected_origin(db, e, anchor)?;
        let identity = serde_json::to_string(&config.identity).map_err(storage)?;
        db.execute("INSERT INTO selected_journal_cursor VALUES(1,?1,?2,?3,?4,?5) ON CONFLICT(singleton) DO UPDATE SET identity=excluded.identity,ordinal=excluded.ordinal,digest=excluded.digest,origin_incarnation=excluded.origin_incarnation,origin_high_water=excluded.origin_high_water",
            params![identity,ordinal,e.payload_sha256,e.origin_incarnation,e.origin_after]).map_err(storage)?;
        if !recovering {
            verify_artifacts(db, config)?;
        }
        Ok(())
    })();
    match result {
        Ok(()) => db.execute_batch("COMMIT").map_err(storage)?,
        Err(e) => {
            let _ = db.execute_batch("ROLLBACK");
            rows::mode(db, 0)?;
            return Err(e);
        }
    }
    #[cfg(test)]
    crash::hit(crash::Point::SqlCommitted);
    rows::mode(db, 0)?;
    Ok(())
}

pub(super) fn run_job(db: &mut Connection, journal: &mut Journal, mut job: WriteJob) {
    if !journal.ready {
        (job.reply)(Err(invalid()));
        return;
    }
    // Known pre-publication exhaustion is a write refusal, not uncertain
    // publication. Keep coherent reads and original-receipt lookup available.
    if journal.ordinal == i64::MAX {
        (job.reply)(Err(OfficeError::Invalid("acceptance journal exhausted")));
        return;
    }
    // Any panic/uncertain filesystem publication fences subsequent work. A fresh
    // verified replay is required; a failed response never reruns the decision.
    journal.ready = false;
    let result = catch_unwind(AssertUnwindSafe(|| {
        prepare_and_accept(db, journal, &mut job)
    }))
    .unwrap_or_else(|panic| Err(super::panicked(panic)));
    if !db.is_autocommit() {
        let _ = db.execute_batch("ROLLBACK");
    }
    let _ = rows::mode(db, 0);
    #[cfg(test)]
    crash::hit(crash::Point::BeforeReply);
    (job.reply)(result);
}

fn prepare_and_accept(
    db: &Connection,
    journal: &mut Journal,
    job: &mut WriteJob,
) -> Result<(), OfficeError> {
    let next = journal.ordinal.checked_add(1).ok_or_else(invalid)?;
    journal.verify_authority()?;
    rows::clear(db)?;
    rows::mode(db, 1)?;
    db.execute_batch("BEGIN IMMEDIATE").map_err(storage)?;
    let prepared = (|| {
        journal.verify_cursor(db)?;
        if origin::stored_incarnation(db)?.as_deref() != Some(&journal.origin)
            || origin::high_water(db, &journal.origin)? != journal.high_water as u64
        {
            return Err(invalid());
        }
        let feed_before = rows::feed_high_water(db)?;
        let undo = (job.attempt)(db)?;
        if let Some(undo) = undo {
            super::run_undo(undo);
            return Err(OfficeError::Invalid(
                "work preparation cannot publish external effects",
            ));
        }
        let mut mutations = rows::captured(db)?;
        if let Some(m) = rows::freeze_feed(feed_before, rows::feed_high_water(db)?) {
            mutations.push(m);
        }
        let after =
            i64::try_from(origin::high_water(db, &journal.origin)?).map_err(|_| invalid())?;
        let entries = read_origins(db, &journal.origin, journal.high_water as u64)?;
        validate_origins(
            &journal.config.identity,
            &journal.origin,
            journal.high_water,
            after,
            &entries,
        )?;
        if origin::stored_incarnation(db)?.as_deref() != Some(&journal.origin) {
            return Err(invalid());
        }
        Ok(Envelope {
            schema_version: 1,
            json_codec: 2,
            resource_profile: codec::RESOURCE_PROFILE.into(),
            journal_kind: "work-acceptance".into(),
            identity: journal.config.identity.clone(),
            kind: "transaction".into(),
            commit_ordinal: Some(next),
            previous_sha256: Some(journal.head.clone()),
            projection_schema_sha256: rows::schema_digest(db)?,
            origin_incarnation: journal.origin.clone(),
            origin_before: journal.high_water,
            origin_after: after,
            origin_entries: entries,
            mutations,
            payload_sha256: String::new(),
        })
    })();
    db.execute_batch("ROLLBACK").map_err(storage)?;
    rows::mode(db, 0)?;
    #[cfg(test)]
    crash::hit(crash::Point::Prepared);
    let mut envelope = match prepared {
        Ok(e) => e,
        Err(e) => {
            journal.ready = true;
            return Err(e);
        }
    };
    if envelope.mutations.is_empty() {
        journal.ready = true;
        return Ok(());
    }
    let frozen = match Journal::freeze(&envelope) {
        Ok(f) => f,
        Err(e) => {
            journal.ready = true;
            return Err(e);
        }
    };
    journal.publish_frozen(&mut envelope, &journal.transaction_path(next), frozen)?;
    journal.verify_next(&envelope)?;
    journal.project(db, &envelope, false, false)?;
    journal.confirm(journal.position.as_ref(), &envelope.position())?;
    journal.advance(&envelope);
    journal.ready = true;
    Ok(())
}

fn read_origins(
    db: &Connection,
    incarnation: &str,
    mut after: u64,
) -> Result<Vec<origin::OriginEntry>, OfficeError> {
    let mut result = Vec::new();
    loop {
        let page = origin::entries_after(db, incarnation, after, 1024)?;
        if page.is_empty() {
            break;
        }
        after = page.last().expect("nonempty page").seq;
        result.extend(page);
        if result.len() > codec::MAX_NODES {
            return Err(invalid());
        }
    }
    Ok(result)
}

fn validate_origins(
    identity: &AcceptanceIdentity,
    incarnation: &str,
    before: i64,
    after: i64,
    entries: &[origin::OriginEntry],
) -> Result<(), OfficeError> {
    if before < 0
        || before.checked_add(i64::try_from(entries.len()).map_err(|_| invalid())?) != Some(after)
    {
        return Err(invalid());
    }
    for (i, e) in entries.iter().enumerate() {
        if e.origin.office_id != identity.office_id
            || e.origin.incarnation != incarnation
            || e.seq != (before as u64) + (i as u64) + 1
            || e.feed_seq == 0
            || e.target.generation == 0
            || e.target.target_id.is_empty()
            || [
                e.seq,
                e.feed_seq,
                e.target.generation,
                e.target.version,
                e.org_position.epoch,
                e.org_position.seq,
                e.committed_at_ms,
            ]
            .iter()
            .any(|v| *v > i64::MAX as u64)
        {
            return Err(invalid());
        }
    }
    Ok(())
}

fn digest_ok(s: &str) -> bool {
    s.len() == 64
        && s.bytes()
            .all(|b| b.is_ascii_digit() || matches!(b, b'a'..=b'f'))
}

fn externalize(
    json: &mut Json,
    blobs: &mut BTreeMap<String, Vec<u8>>,
    lengths: &mut Vec<u64>,
) -> Result<(), OfficeError> {
    match json {
        Json::Object(map)
            if matches!(
                map.get("kind").and_then(Json::as_str),
                Some("text" | "blob")
            ) && map
                .get("value")
                .and_then(Json::as_str)
                .is_some_and(|s| s.len() > 64 * 1024) =>
        {
            let hex = map.get("kind").and_then(Json::as_str) == Some("blob");
            let text = map
                .get("value")
                .and_then(Json::as_str)
                .ok_or_else(invalid)?;
            let data = if hex {
                rows::unhex(text)?
            } else {
                text.as_bytes().to_vec()
            };
            lengths.push(data.len() as u64);
            codec::check_blob_lengths(lengths.iter().copied()).map_err(storage)?;
            let digest = codec::sha256(&data);
            let value = Json::Object(
                [
                    ("$journal_blob".into(), Json::String(digest.clone())),
                    (
                        "byte_length".into(),
                        Json::Number(NumberToken::new(&data.len().to_string()).map_err(storage)?),
                    ),
                    (
                        "encoding".into(),
                        Json::String(if hex { "hex" } else { "utf-8" }.into()),
                    ),
                ]
                .into(),
            );
            blobs.entry(digest).or_insert(data);
            map.insert("value".into(), value);
        }
        Json::Object(map) => {
            for v in map.values_mut() {
                externalize(v, blobs, lengths)?;
            }
        }
        Json::Array(values) => {
            for v in values {
                externalize(v, blobs, lengths)?;
            }
        }
        _ => {}
    }
    Ok(())
}

fn from_serde(v: serde_json::Value) -> Result<Json, OfficeError> {
    Ok(match v {
        serde_json::Value::Null => Json::Null,
        serde_json::Value::Bool(b) => Json::Bool(b),
        serde_json::Value::Number(n) => {
            Json::Number(NumberToken::new(&n.to_string()).map_err(storage)?)
        }
        serde_json::Value::String(s) => Json::String(s),
        serde_json::Value::Array(a) => {
            Json::Array(a.into_iter().map(from_serde).collect::<Result<_, _>>()?)
        }
        serde_json::Value::Object(o) => Json::Object(
            o.into_iter()
                .map(|(k, v)| Ok((k, from_serde(v)?)))
                .collect::<Result<_, OfficeError>>()?,
        ),
    })
}

fn to_serde(v: Json) -> Result<serde_json::Value, OfficeError> {
    Ok(match v {
        Json::Null => serde_json::Value::Null,
        Json::Bool(b) => serde_json::Value::Bool(b),
        Json::String(s) => serde_json::Value::String(s),
        Json::Number(n) => serde_json::Value::Number(n.as_str().parse().map_err(|_| invalid())?),
        Json::Array(a) => {
            serde_json::Value::Array(a.into_iter().map(to_serde).collect::<Result<_, _>>()?)
        }
        Json::Object(o) => serde_json::Value::Object(
            o.into_iter()
                .map(|(k, v)| Ok((k, to_serde(v)?)))
                .collect::<Result<_, OfficeError>>()?,
        ),
    })
}

fn artifact(
    m: &rows::Mutation,
    row: &[rows::Cell],
) -> Result<(String, String, Vec<u8>, bool), OfficeError> {
    let get = |key: &str| {
        m.columns
            .iter()
            .position(|s| s == key)
            .and_then(|i| row.get(i))
            .ok_or_else(invalid)
    };
    let namespace = match get("namespace")? {
        rows::Cell::Text(s) => s.clone(),
        _ => return Err(invalid()),
    };
    let path = match get("path")? {
        rows::Cell::Text(s) => s.clone(),
        _ => return Err(invalid()),
    };
    let content = match get("content")? {
        rows::Cell::Blob(s) => rows::unhex(s)?,
        _ => return Err(invalid()),
    };
    let immutable = match get("immutable")? {
        rows::Cell::Integer(0) => false,
        rows::Cell::Integer(1) => true,
        _ => return Err(invalid()),
    };
    Ok((namespace, path, content, immutable))
}

fn materialize(config: &AcceptanceConfig, m: &rows::Mutation) -> Result<(), OfficeError> {
    if m.table != "selected_artifacts" {
        return Ok(());
    }
    let after = m.after.as_ref().ok_or_else(invalid)?;
    let (namespace, path, content, immutable) = artifact(m, after)?;
    let root = match namespace.as_str() {
        "public" => &config.public_root,
        "protected" => &config.protected_root,
        _ => return Err(invalid()),
    };
    let before = m.before.as_ref().map(|r| artifact(m, r)).transpose()?;
    if before
        .as_ref()
        .is_some_and(|(n, p, _, i)| n != &namespace || p != &path || *i)
    {
        return Err(invalid());
    }
    let digest = before.map(|(_, _, b, _)| codec::sha256(&b));
    durable::publish(root, &path, &content, immutable, digest.as_deref()).map_err(storage)
}

fn materialize_final(
    config: &AcceptanceConfig,
    mutations: &[rows::Mutation],
) -> Result<(), OfficeError> {
    let mut final_files: BTreeMap<(String, String), rows::Mutation> = BTreeMap::new();
    for m in mutations.iter().filter(|m| m.table == "selected_artifacts") {
        let after = m.after.as_ref().ok_or_else(invalid)?;
        let (ns, path, _, _) = artifact(m, after)?;
        if let Some(first) = final_files.get_mut(&(ns.clone(), path.clone())) {
            if first.after != m.before {
                return Err(invalid());
            }
            first.after = m.after.clone();
        } else {
            final_files.insert((ns, path), m.clone());
        }
    }
    for m in final_files.values() {
        materialize(config, m)?;
    }
    Ok(())
}

type ArtifactHashes = BTreeMap<(String, String), BTreeSet<String>>;

fn retain_artifact_hashes(
    hashes: &mut ArtifactHashes,
    mutations: &[rows::Mutation],
) -> Result<(), OfficeError> {
    for m in mutations.iter().filter(|m| m.table == "selected_artifacts") {
        for row in [m.before.as_ref(), m.after.as_ref()].into_iter().flatten() {
            let (ns, path, bytes, _) = artifact(m, row)?;
            hashes
                .entry((ns, path))
                .or_default()
                .insert(codec::sha256(&bytes));
        }
    }
    Ok(())
}

fn repair_artifacts(
    db: &Connection,
    config: &AcceptanceConfig,
    allowed: &ArtifactHashes,
) -> Result<(), OfficeError> {
    let cols = rows::columns(db, "selected_artifacts")?;
    let mut query = db
        .prepare("SELECT namespace,path,content,immutable FROM selected_artifacts")
        .map_err(storage)?;
    let mut records = query.query([]).map_err(storage)?;
    while let Some(row) = records.next().map_err(storage)? {
        let mut m = rows::Mutation {
            table: "selected_artifacts".into(),
            columns: cols.clone(),
            before: None,
            after: Some(vec![
                rows::Cell::Text(row.get(0).map_err(storage)?),
                rows::Cell::Text(row.get(1).map_err(storage)?),
                rows::Cell::Blob(rows::hex(&row.get::<_, Vec<u8>>(2).map_err(storage)?)),
                rows::Cell::Integer(row.get(3).map_err(storage)?),
            ]),
        };
        let (ns, path, content, immutable) = artifact(&m, m.after.as_ref().ok_or_else(invalid)?)?;
        let root = match ns.as_str() {
            "public" => &config.public_root,
            "protected" => &config.protected_root,
            _ => return Err(invalid()),
        };
        match durable::read(root, &path, codec::MAX_BLOB_BYTES as usize) {
            Ok(actual) if actual != content => {
                if immutable
                    || !allowed
                        .get(&(ns.clone(), path.clone()))
                        .is_some_and(|h| h.contains(&codec::sha256(&actual)))
                {
                    return Err(invalid());
                }
                m.before = Some(vec![
                    rows::Cell::Text(ns),
                    rows::Cell::Text(path),
                    rows::Cell::Blob(rows::hex(&actual)),
                    rows::Cell::Integer(0),
                ]);
            }
            Ok(_) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(storage(e)),
        }
        // A mutable projection can advance from any verified frozen predecessor,
        // or be recreated if absent. Outside bytes are never silently replaced.
        materialize(config, &m)?;
    }
    verify_artifacts(db, config)
}

fn verify_artifacts(db: &Connection, config: &AcceptanceConfig) -> Result<(), OfficeError> {
    let mut query = db
        .prepare("SELECT namespace,path,content FROM selected_artifacts")
        .map_err(storage)?;
    let mut rows = query.query([]).map_err(storage)?;
    while let Some(row) = rows.next().map_err(storage)? {
        let namespace: String = row.get(0).map_err(storage)?;
        let path: String = row.get(1).map_err(storage)?;
        let content: Vec<u8> = row.get(2).map_err(storage)?;
        let root = match namespace.as_str() {
            "public" => &config.public_root,
            "protected" => &config.protected_root,
            _ => return Err(invalid()),
        };
        let actual = durable::read(root, &path, codec::MAX_BLOB_BYTES as usize).map_err(storage)?;
        if actual != content {
            return Err(invalid());
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
