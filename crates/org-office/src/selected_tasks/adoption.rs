//! Canonical selected-task adoption through the existing accepting writer.
//! Canonical task and directory imports retain their explicitly public source bytes.
mod directory_import;
mod legacy_import;
use super::{
    directory::Facts, service, staging, AcceptanceIdentity, OwnerAccount, OwnerAuthority,
    TaskSnapshot,
};
use crate::{store::Store, Office, OfficeError};
use org_proto::selected_tasks::{
    adoption::{Action, HistoryOrigin, Manifest, Outcome, METHOD},
    staging::Blob,
};
use org_work::{
    selected_codec::{self as codec, Json},
    selected_history::{self as history, Admission, EventFields},
    TaskHistoryOrigin,
};
use rusqlite::{params, Connection, OptionalExtension};
use serde_json::{json, Value};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
    sync::Arc,
};

fn storage(e: impl std::error::Error + Send + Sync + 'static) -> OfficeError {
    OfficeError::storage(e)
}
fn invalid() -> OfficeError {
    OfficeError::Invalid("unsupported selected adoption source")
}
fn staged(code: &str) -> Outcome {
    Outcome::Staged {
        reason_code: code.into(),
    }
}
fn operation(action: &Action) -> &str {
    match action {
        Action::Commit { operation_id, .. }
        | Action::DirectoryCommit { operation_id, .. }
        | Action::Lookup { operation_id } => operation_id.as_str(),
    }
}
fn fingerprint(id: &AcceptanceIdentity, action: &Action) -> Result<String, OfficeError> {
    Ok(codec::sha256(
        &serde_json::to_vec(&(
            METHOD,
            1,
            id.instance_id.as_str(),
            id.organization_id.as_str(),
            id.office_id.as_str(),
            action,
        ))
        .map_err(storage)?,
    ))
}
fn receipt(
    db: &Connection,
    id: &AcceptanceIdentity,
    owner: &OwnerAccount,
    operation: &str,
    hash: Option<&str>,
) -> Result<Option<Outcome>, OfficeError> {
    type Row = (String, String, String, i64, String, Vec<u8>, i64);
    let row:Option<Row>=db.query_row("SELECT instance_id,account_id,method,command_version,fingerprint,result,expires_at_ms FROM selected_receipts WHERE organization_id=?1 AND operation_id=?2",params![id.organization_id,operation],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?,r.get(5)?,r.get(6)?))).optional().map_err(storage)?;
    if let Some((instance, account, method, version, actual, bytes, expiry)) = row {
        if instance != id.instance_id
            || account != owner.account_id
            || method != METHOD
            || version != 1
            || hash.is_some_and(|h| h != actual)
        {
            return Err(OfficeError::Invalid("operation reused"));
        }
        if service::now(db)? >= u64::try_from(expiry).map_err(|_| invalid())? {
            return Err(OfficeError::Invalid("operation expired"));
        }
        return serde_json::from_slice(&bytes).map(Some).map_err(storage);
    }
    if db
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM work_change_results WHERE change_id=?1)",
            [operation],
            |r| r.get::<_, bool>(0),
        )
        .map_err(storage)?
    {
        return Err(OfficeError::Invalid("operation reused"));
    }
    Ok(None)
}
impl Office {
    pub fn adopt_selected_task(
        &self,
        authority: Arc<dyn OwnerAuthority>,
        action: Action,
    ) -> Result<Outcome, OfficeError> {
        run_with_org(
            self.local_store(),
            self.folder().join("selected-adoption"),
            authority,
            action,
            Some(self.work_org_source()),
        )
    }
}
#[cfg(test)]
pub(super) fn run(
    store: &Store,
    root: PathBuf,
    authority: Arc<dyn OwnerAuthority>,
    action: Action,
) -> Result<Outcome, OfficeError> {
    run_with_org(store, root, authority, action, None)
}
pub(super) fn run_with_org(
    store: &Store,
    root: PathBuf,
    authority: Arc<dyn OwnerAuthority>,
    action: Action,
    legacy_org: Option<Arc<dyn crate::store::work::assembly::OrgSource>>,
) -> Result<Outcome, OfficeError> {
    let intent = Arc::new(action);
    let check = intent.clone();
    let auth = authority.clone();
    if let Some(prior) = store.selected_read(move |db, id| {
        let owner = service::owner(db, id, &*auth)?;
        let hash = (!matches!(&*check, Action::Lookup { .. }))
            .then(|| fingerprint(id, &check))
            .transpose()?;
        receipt(db, id, &owner, operation(&check), hash.as_deref())
    })? {
        return Ok(prior);
    }
    if matches!(&*intent, Action::Lookup { .. }) {
        return Err(OfficeError::NotFound);
    }
    store.selected_transaction(move|db,id| {
        let owner=service::owner(db,id,&*authority)?;let hash=fingerprint(id,&intent)?;let op=operation(&intent);
        if let Some(prior)=receipt(db,id,&owner,op,Some(&hash))?{return Ok(prior);}
        let now=service::now(db)?;let time=crate::ulid::time_ms(op).ok_or_else(invalid)?;const DAY:u64=86_400_000;
        if time>now.saturating_add(300_000)||now.saturating_sub(time)>29*DAY{return Err(OfficeError::Invalid("operation outside retry window"));}
        let current:Option<i64>=db.query_row("SELECT generation FROM selected_directory_head WHERE organization_id=?1",[&id.organization_id],|r|r.get(0)).optional().map_err(storage)?;
        let result = match &*intent {
            Action::Commit{stage_id,manifest_sha256,expected_directory_generation,..} => {
                if current!=Some(expected_directory_generation.get()) {staged("directory_conflict")} else {
                    match prepare(db,id,&owner,&root,stage_id.as_str(),manifest_sha256,ImportAt {operation:op,now,legacy_org:legacy_org.as_deref()}) {
                        Ok(p)=>apply(db,id,manifest_sha256,p)?,
                        Err(OfficeError::Invalid("unsupported selected adoption source"))=>staged("unsupported_or_inconsistent_source"),
                        Err(e)=>return Err(e),
                    }
                }
            }
            Action::DirectoryCommit{stage_id,manifest_sha256,expected_directory_generation,..} => {
                if current!=expected_directory_generation.map(|r|r.get()) {staged("directory_conflict")} else {
                    match directory_import::prepare(db,id,&owner,&root,stage_id.as_str(),manifest_sha256) {
                        Ok(p)=>directory_import::apply(db,id,current,p)?,
                        Err(OfficeError::Invalid("unsupported selected adoption source"))=>staged("unsupported_or_inconsistent_source"),
                        Err(e)=>return Err(e),
                    }
                }
            }
            Action::Lookup{..}=>unreachable!(),
        };
        let expiry=time.checked_add(30*DAY).and_then(|n|i64::try_from(n).ok()).ok_or_else(invalid)?;
        let bytes=serde_json::to_vec(&result).map_err(storage)?;
        db.execute("INSERT INTO selected_receipts VALUES(?1,?2,?3,?4,'owner',NULL,?5,1,?6,?7,?8)",params![id.organization_id,op,id.instance_id,owner.account_id,METHOD,hash,bytes,expiry]).map_err(storage)?;
        let protected=serde_json::to_vec(&json!({"receipt_version":1,"instance_id":id.instance_id,"organization_id":id.organization_id,"office_id":id.office_id,"account_id":owner.account_id,"operation_id":op,"method":METHOD,"command_version":1,"fingerprint":hash,"result":result,"expires_at_ms":expiry})).map_err(storage)?;
        service::put_artifact(db,"protected",&format!("offices/{}/receipts/{op}.json",id.office_id),&protected,true)?;
        db.execute("INSERT INTO work_clock VALUES(1,?1) ON CONFLICT(id) DO UPDATE SET high_water_ms=MAX(high_water_ms,excluded.high_water_ms)",[now as i64]).map_err(storage)?;
        Ok(result)
    })
}
struct Prepared {
    source: Vec<u8>,
    task: String,
    path: String,
    revision: i64,
    team: String,
    project: Option<String>,
    assigned: Option<String>,
    assignee: Option<String>,
    events: Vec<(i64, Vec<u8>, String)>,
    artifacts: BTreeMap<String, (Vec<u8>, bool)>,
    coverage: String,
    legacy: bool,
    origin_generation: i64,
}

struct ImportAt<'a> {
    operation: &'a str,
    now: u64,
    legacy_org: Option<&'a dyn crate::store::work::assembly::OrgSource>,
}

fn prepare(
    db: &Connection,
    id: &AcceptanceIdentity,
    owner: &OwnerAccount,
    root: &Path,
    stage: &str,
    manifest: &str,
    at: ImportAt<'_>,
) -> Result<Prepared, OfficeError> {
    let raw = staging::manifest_source(root, id, owner, stage, manifest)?;
    let m = parse_task_manifest(&raw)?;
    let source = staging::source(
        root,
        id,
        owner,
        stage,
        manifest,
        &m.public_snapshot,
        org_docs::MAX_FRONT_MATTER_BYTES + 8 * 1024 * 1024 + 64,
    )?;
    let text = std::str::from_utf8(&source).map_err(|_| invalid())?;
    let snapshot = TaskSnapshot::parse(&id.organization_id, text).map_err(|_| invalid())?;
    let facts = snapshot.immutable();
    if facts.task_id != m.task_id {
        return Err(invalid());
    }
    let accepted: bool = db
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM selected_tasks WHERE task_id=?1)",
            [&m.task_id],
            |r| r.get(0),
        )
        .map_err(storage)?;
    if accepted {
        return Err(invalid());
    }
    let (directory, path) = validate_task_directory(db, id, &snapshot, &m)?;
    let mut artifacts = BTreeMap::new();
    let mut events = Vec::new();
    retain_public_blobs(
        root,
        id,
        owner,
        stage,
        manifest,
        &m.public_blobs,
        &mut artifacts,
    )?;
    let (origin, baseline) = prepare_history_origin(db, id, &snapshot, &m, &at)?;
    if let Some(event) = baseline {
        artifacts.insert(
            format!("work-history/blobs/sha256/{}", m.public_snapshot.sha256),
            (source.clone(), true),
        );
        events.push((snapshot.revision(), event, String::new()));
    }
    let mut revisions = BTreeSet::new();
    for event in &m.events {
        if !revisions.insert(event.revision.get()) {
            return Err(invalid());
        }
        let bytes = staging::source(
            root,
            id,
            owner,
            stage,
            manifest,
            &event.blob,
            codec::MAX_JSON_BYTES,
        )?;
        events.push((event.revision.get(), bytes, String::new()));
    }
    let wrapped = legacy_import::wrap(db, id, &snapshot, &directory, &m, &events, (owner, at))?;
    let legacy = wrapped.is_some();
    let mut origin_generation = 1;
    if let Some((bytes, generation)) = wrapped {
        // This profile constructs the FIRST selected event at import time. It
        // never mutates a previously accepted selected identity or legacy row.
        events[0].1 = bytes;
        origin_generation = generation;
    }
    let verified = verify_imported_events(&mut events, id, &snapshot, &m, &source, &artifacts)?;
    let headers = verified
        .iter()
        .map(|e| e.coverage_header())
        .collect::<Vec<_>>();
    let coverage = org_work::check_task_history_coverage(
        &id.organization_id,
        &m.task_id,
        snapshot.revision() as u64,
        origin,
        &headers,
    )
    .map_err(|_| invalid())?;
    let coverage = match coverage {
        org_work::TaskHistoryCoverage::FromCreation { .. } => "complete",
        org_work::TaskHistoryCoverage::EarlierHistoryUnavailable { .. } => "snapshot_baseline",
    }
    .to_owned();
    numbering(
        db,
        id,
        root,
        owner,
        stage,
        manifest,
        &snapshot,
        m.number_counter.as_ref(),
        m.number_allocation.as_ref(),
        &mut artifacts,
    )?;
    Ok(Prepared {
        task: m.task_id,
        path,
        revision: snapshot.revision(),
        team: facts.team_id.clone(),
        project: facts.project_id.clone(),
        assigned: snapshot.assignment().assigned_team_id().map(str::to_owned),
        assignee: snapshot.assignment().assignee_id().map(str::to_owned),
        source,
        events,
        artifacts,
        coverage,
        legacy,
        origin_generation,
    })
}

fn validate_task_directory(
    db: &Connection,
    id: &AcceptanceIdentity,
    snapshot: &TaskSnapshot<'_>,
    m: &Manifest,
) -> Result<(Json, String), OfficeError> {
    let directory:String=db.query_row("SELECT data FROM selected_directory d JOIN selected_directory_head h USING(organization_id,generation) WHERE organization_id=?1",[&id.organization_id],|r|r.get(0)).map_err(storage)?;
    let directory = codec::parse(directory.as_bytes()).map_err(storage)?;
    let path = Facts::validate(&directory, &id.organization_id)?
        .task_path(snapshot)
        .map_err(|_| invalid())?;
    if path != m.canonical_path {
        return Err(invalid());
    }
    for (key, target) in [
        ("teams", snapshot.assignment().assigned_team_id()),
        ("employees", snapshot.assignment().assignee_id()),
    ] {
        if target.is_some_and(|target| directory.get(key).and_then(|d| d.get(target)).is_none()) {
            return Err(invalid());
        }
    }
    Ok((directory, path))
}

fn verify_imported_events(
    events: &mut [(i64, Vec<u8>, String)],
    id: &AcceptanceIdentity,
    snapshot: &TaskSnapshot<'_>,
    m: &Manifest,
    source: &[u8],
    artifacts: &BTreeMap<String, (Vec<u8>, bool)>,
) -> Result<Vec<history::VerifiedEvent>, OfficeError> {
    let mut verified = Vec::new();
    for (revision, bytes, hash) in events {
        let event = history::verify(
            bytes,
            history::ExpectedIdentity {
                organization_id: &id.organization_id,
                task_id: &m.task_id,
                filename: &format!("{revision}.json"),
            },
            Admission::Import,
        )
        .map_err(|_| invalid())?;
        // Codec/hash verification does not classify retained source. Apply the
        // same provenance guard to every event, including nonlegacy imports and
        // unknown change types, before any public artifact can be installed.
        legacy_import::public_tree(event.value())?;
        verify_public_snapshot(
            event.value().get("change").ok_or_else(invalid)?,
            &id.organization_id,
            &m.task_id,
            *revision,
            artifacts,
        )?;
        validate_latest_event(event.value(), *revision, snapshot, m, source)?;
        *hash = event
            .value()
            .get("payload_sha256")
            .and_then(Json::as_str)
            .ok_or_else(invalid)?
            .into();
        verified.push(event);
    }
    Ok(verified)
}

fn parse_task_manifest(raw: &[u8]) -> Result<Manifest, OfficeError> {
    let parsed = codec::parse(raw).map_err(|_| invalid())?;
    // Reject positional record arrays before typed serde can accept them.
    for key in ["public_snapshot", "history_origin"] {
        if !matches!(parsed.get(key), Some(Json::Object(_))) {
            return Err(invalid());
        }
    }
    for key in ["number_counter", "number_allocation"] {
        if !matches!(parsed.get(key), Some(Json::Null | Json::Object(_))) {
            return Err(invalid());
        }
    }
    for key in ["events", "public_blobs"] {
        let Some(Json::Array(items)) = parsed.get(key) else {
            return Err(invalid());
        };
        if items.iter().any(|v| {
            !matches!(v, Json::Object(_))
                || (key == "events" && !matches!(v.get("blob"), Some(Json::Object(_))))
        }) {
            return Err(invalid());
        }
    }
    let m: Manifest = serde_json::from_slice(raw).map_err(|_| invalid())?;
    if m.version != 1 || m.resource_profile != codec::RESOURCE_PROFILE {
        return Err(invalid());
    }
    let refs = std::iter::once(&m.public_snapshot)
        .chain(m.events.iter().map(|e| &e.blob))
        .chain(&m.public_blobs)
        .chain(m.number_counter.iter())
        .chain(m.number_allocation.iter())
        .collect::<Vec<_>>();
    codec::check_blob_lengths(refs.iter().map(|r| r.byte_length)).map_err(|_| invalid())?;
    Ok(m)
}

fn retain_public_blobs(
    root: &Path,
    id: &AcceptanceIdentity,
    owner: &OwnerAccount,
    stage: &str,
    manifest: &str,
    blobs: &[Blob],
    artifacts: &mut BTreeMap<String, (Vec<u8>, bool)>,
) -> Result<(), OfficeError> {
    let mut seen = BTreeSet::new();
    for blob in blobs {
        if !seen.insert(&blob.sha256) {
            return Err(invalid());
        }
        let bytes = staging::source(
            root,
            id,
            owner,
            stage,
            manifest,
            blob,
            codec::MAX_BLOB_BYTES as usize,
        )?;
        artifacts.insert(
            format!("work-history/blobs/sha256/{}", blob.sha256),
            (bytes, true),
        );
    }
    Ok(())
}

fn prepare_history_origin(
    db: &Connection,
    id: &AcceptanceIdentity,
    snapshot: &TaskSnapshot<'_>,
    m: &Manifest,
    at: &ImportAt<'_>,
) -> Result<(TaskHistoryOrigin, Option<Vec<u8>>), OfficeError> {
    let facts = snapshot.immutable();
    let now = at.now;
    let op = at.operation;
    let origin = match &m.history_origin {
        HistoryOrigin::Creation => (TaskHistoryOrigin::Creation, None),
        HistoryOrigin::Baseline { revision } => (
            TaskHistoryOrigin::Migration {
                baseline_revision: revision.get() as u64,
            },
            None,
        ),
        HistoryOrigin::SnapshotOnly => {
            let known:bool=db.query_row("SELECT EXISTS(SELECT 1 FROM work_events WHERE target_id=?1) OR EXISTS(SELECT 1 FROM selected_artifacts WHERE namespace='public' AND path LIKE ?2)",params![m.task_id,format!("work-history/tasks/{}/%",m.task_id)],|r|r.get(0)).map_err(storage)?;
            if known || !m.events.is_empty() {
                return Err(invalid());
            }
            let mut metadata = BTreeMap::new();
            metadata.insert(
                "history_coverage".into(),
                Json::String("snapshot_baseline".into()),
            );
            metadata.insert(
                "source_snapshot_sha256".into(),
                Json::String(m.public_snapshot.sha256.clone()),
            );
            let change=codec::parse(&serde_json::to_vec(&json!({"type":"history_baseline","known_facts":{"id":facts.task_id,"team_id":facts.team_id,"project_id":facts.project_id,"revision":snapshot.revision(),"assigned_team_id":snapshot.assignment().assigned_team_id(),"assignee_id":snapshot.assignment().assignee_id()},"public_snapshot":{"encoding":"utf-8-markdown","sha256":m.public_snapshot.sha256,"byte_length":m.public_snapshot.byte_length,"resource_profile":codec::RESOURCE_PROFILE}})).map_err(storage)?).map_err(storage)?;
            let event = history::encode(
                EventFields {
                    organization_id: id.organization_id.clone(),
                    task_id: facts.task_id.clone(),
                    revision: snapshot.revision(),
                    occurred_at: Some(crate::time::format_epoch_ms(now)),
                    actor_kind: "owner".into(),
                    actor_id: None,
                    operation_id: Some(op.into()),
                    change,
                    source_metadata: metadata,
                    extensions: BTreeMap::new(),
                },
                Admission::Import,
            )
            .map_err(|_| invalid())?;
            (
                TaskHistoryOrigin::Migration {
                    baseline_revision: snapshot.revision() as u64,
                },
                Some(event),
            )
        }
    };
    Ok(origin)
}

fn validate_latest_event(
    value: &Json,
    revision: i64,
    snapshot: &TaskSnapshot<'_>,
    m: &Manifest,
    source: &[u8],
) -> Result<(), OfficeError> {
    if revision == snapshot.revision() {
        let change = value.get("change").ok_or_else(invalid)?;
        match change.get("type").and_then(Json::as_str) {
            Some("created" | "history_baseline") => {
                let reference = change.get("public_snapshot").ok_or_else(invalid)?;
                if let Some(text) = reference.get("text").and_then(Json::as_str) {
                    if text.as_bytes() != source {
                        return Err(invalid());
                    }
                } else if reference.get("sha256").and_then(Json::as_str)
                    != Some(&m.public_snapshot.sha256)
                {
                    return Err(invalid());
                }
            }
            Some("assignment_changed") => {
                let to = change.get("to").ok_or_else(invalid)?;
                let expected=codec::parse(&serde_json::to_vec(&json!({"assigned_team_id":snapshot.assignment().assigned_team_id(),"assignee_id":snapshot.assignment().assignee_id()})).map_err(storage)?).map_err(storage)?;
                if to != &expected {
                    return Err(invalid());
                }
            }
            _ => {} // Unknown changes remain intact; no unsupported reducer runs.
        }
    }
    Ok(())
}

fn verify_public_snapshot(
    change: &Json,
    org: &str,
    task: &str,
    revision: i64,
    artifacts: &BTreeMap<String, (Vec<u8>, bool)>,
) -> Result<(), OfficeError> {
    if !matches!(
        change.get("type").and_then(Json::as_str),
        Some("created" | "history_baseline")
    ) {
        return Ok(());
    }
    if let Some(reference) = change.get("public_snapshot") {
        let text = if let Some(Json::String(text)) = reference.get("text") {
            text.as_str()
        } else {
            let digest = reference
                .get("sha256")
                .and_then(Json::as_str)
                .ok_or_else(invalid)?;
            let Some((bytes, _)) = artifacts.get(&format!("work-history/blobs/sha256/{digest}"))
            else {
                return Err(invalid());
            };
            if reference.get("byte_length").and_then(Json::positive_i64) != Some(bytes.len() as i64)
                || codec::sha256(bytes) != digest
            {
                return Err(invalid());
            }
            std::str::from_utf8(bytes).map_err(|_| invalid())?
        };
        let snapshot = TaskSnapshot::parse(org, text).map_err(|_| invalid())?;
        if snapshot.immutable().task_id != task || snapshot.revision() != revision {
            return Err(invalid());
        }
        let facts = snapshot.immutable();
        let known = change.get("known_facts").ok_or_else(invalid)?;
        let expected=codec::parse(&serde_json::to_vec(&json!({"id":task,"team_id":facts.team_id,"project_id":facts.project_id,"revision":revision,"assigned_team_id":snapshot.assignment().assigned_team_id(),"assignee_id":snapshot.assignment().assignee_id()})).map_err(storage)?).map_err(storage)?;
        for key in [
            "id",
            "team_id",
            "project_id",
            "revision",
            "assigned_team_id",
            "assignee_id",
        ] {
            if known.get(key) != expected.get(key) {
                return Err(invalid());
            }
        }
    } else {
        return Err(invalid());
    }
    Ok(())
}

pub(super) fn yaml(bytes: &[u8]) -> Result<serde_json::Map<String, Value>, OfficeError> {
    if bytes.len() > org_docs::MAX_FRONT_MATTER_BYTES {
        return Err(invalid());
    }
    let text = std::str::from_utf8(bytes).map_err(|_| invalid())?;
    let wrapped = format!("---\n{text}\n---\n");
    let doc = org_docs::parse(&wrapped).map_err(|_| invalid())?;
    // These inputs are YAML files, not Markdown. An embedded fence must not
    // turn a second YAML document into an unchecked Markdown body.
    if !doc.body.is_empty() {
        return Err(invalid());
    }
    doc.front_matter.ok_or_else(invalid)
}
#[allow(clippy::too_many_arguments)]
fn numbering(
    db: &Connection,
    _id: &AcceptanceIdentity,
    root: &Path,
    owner: &OwnerAccount,
    stage: &str,
    manifest: &str,
    snapshot: &TaskSnapshot<'_>,
    counter: Option<&Blob>,
    allocation: Option<&Blob>,
    artifacts: &mut BTreeMap<String, (Vec<u8>, bool)>,
) -> Result<(), OfficeError> {
    let f = snapshot.immutable();
    let Some(number) = f.number else {
        if counter.is_some() || allocation.is_some() {
            return Err(invalid());
        }
        return Ok(());
    };
    let scope = if f.project_id.is_some() {
        "project"
    } else {
        "team"
    };
    let scope_id = f.project_id.as_ref().unwrap_or(&f.team_id);
    let c = staging::source(
        root,
        _id,
        owner,
        stage,
        manifest,
        counter.ok_or_else(invalid)?,
        org_docs::MAX_FRONT_MATTER_BYTES,
    )?;
    let a = staging::source(
        root,
        _id,
        owner,
        stage,
        manifest,
        allocation.ok_or_else(invalid)?,
        org_docs::MAX_FRONT_MATTER_BYTES,
    )?;
    let cm = yaml(&c)?;
    let am = yaml(&a)?;
    for map in [&cm, &am] {
        if !task_number_scope(map, scope, scope_id) {
            return Err(invalid());
        }
    }
    let high = cm
        .get("last_number")
        .and_then(Value::as_i64)
        .filter(|n| *n >= number)
        .ok_or_else(invalid)?;
    validate_task_allocation(&am, number, &f.task_id)?;
    let cp = format!("identity/counters/{scope}/{scope_id}/task.yaml");
    let ap = format!("identity/allocations/{scope}/{scope_id}/task/{number}.yaml");
    let prefix = format!("identity/allocations/{scope}/{scope_id}/task/");
    let mut q=db.prepare("SELECT path,content FROM selected_artifacts WHERE namespace='public' AND substr(path,1,length(?1))=?1").map_err(storage)?;
    let mut rows = q.query([prefix]).map_err(storage)?;
    let mut required = high;
    while let Some(r) = rows.next().map_err(storage)? {
        let path: String = r.get(0).map_err(storage)?;
        let bytes: Vec<u8> = r.get(1).map_err(storage)?;
        let m = yaml(&bytes)?;
        if !task_number_scope(&m, scope, scope_id) {
            return Err(invalid());
        }
        let n = m
            .get("number")
            .and_then(Value::as_i64)
            .filter(|n| *n > 0)
            .ok_or_else(invalid)?;
        required = required.max(n);
        if path != format!("identity/allocations/{scope}/{scope_id}/task/{n}.yaml") {
            return Err(invalid());
        }
        if (n == number || m.get("entity_id").and_then(Value::as_str) == Some(&f.task_id))
            && (path != ap || bytes != a)
        {
            return Err(invalid());
        }
    }
    let old: Option<Vec<u8>> = db
        .query_row(
            "SELECT content FROM selected_artifacts WHERE namespace='public' AND path=?1",
            [&cp],
            |r| r.get(0),
        )
        .optional()
        .map_err(storage)?;
    let counter = select_task_counter(old, c, required, high, scope, scope_id)?;
    artifacts.insert(cp, (counter, false));
    artifacts.insert(ap, (a, true));
    Ok(())
}
fn validate_task_allocation(
    am: &serde_json::Map<String, Value>,
    number: i64,
    task_id: &str,
) -> Result<(), OfficeError> {
    if am.get("number").and_then(Value::as_i64) != Some(number)
        || am.get("entity_id").and_then(Value::as_str) != Some(task_id)
        || !am
            .get("allocated_at")
            .and_then(Value::as_str)
            .is_some_and(history::utc_timestamp)
    {
        return Err(invalid());
    }
    Ok(())
}

fn task_number_scope(map: &serde_json::Map<String, Value>, scope: &str, scope_id: &str) -> bool {
    map.get("scope_kind").and_then(Value::as_str) == Some(scope)
        && map.get("scope_id").and_then(Value::as_str) == Some(scope_id)
        && map.get("entity_kind").and_then(Value::as_str) == Some("task")
}

fn select_task_counter(
    old: Option<Vec<u8>>,
    c: Vec<u8>,
    required: i64,
    high: i64,
    scope: &str,
    scope_id: &str,
) -> Result<Vec<u8>, OfficeError> {
    let counter = if let Some(old) = old {
        let map = yaml(&old)?;
        if !task_number_scope(&map, scope, scope_id) {
            return Err(invalid());
        }
        let n = map
            .get("last_number")
            .and_then(Value::as_i64)
            .filter(|n| *n >= 0)
            .ok_or_else(invalid)?;
        if n >= required {
            old
        } else if high >= required {
            c
        } else {
            return Err(invalid());
        }
    } else {
        if high < required {
            return Err(invalid());
        }
        c
    };
    Ok(counter)
}

fn apply(
    db: &Connection,
    id: &AcceptanceIdentity,
    manifest: &str,
    p: Prepared,
) -> Result<Outcome, OfficeError> {
    db.execute(
        "INSERT INTO selected_tasks VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)",
        params![
            id.organization_id,
            p.task,
            p.revision,
            p.team,
            p.project,
            p.assigned,
            p.assignee,
            p.path,
            p.source,
            codec::sha256(&p.source)
        ],
    )
    .map_err(storage)?;
    service::put_artifact(db, "public", &p.path, &p.source, false)?;
    for (revision, bytes, hash) in &p.events {
        db.execute(
            "INSERT INTO selected_events VALUES(?1,?2,?3,2,?4,?5,?6)",
            params![
                id.organization_id,
                p.task,
                revision,
                codec::RESOURCE_PROFILE,
                hash,
                bytes
            ],
        )
        .map_err(storage)?;
        service::put_artifact(
            db,
            "public",
            &format!("work-history/tasks/{}/{revision}.json", p.task),
            bytes,
            true,
        )?;
    }
    for (path, (bytes, immutable)) in &p.artifacts {
        service::put_artifact(db, "public", path, bytes, *immutable)?;
    }
    let metadata=serde_json::to_string(&json!({"resource_profile":codec::RESOURCE_PROFILE,"history_coverage":p.coverage,"source_kind":if p.legacy {"legacy_work_v1"} else {"canonical_selected"},"manifest_sha256":manifest})).map_err(storage)?;
    db.execute(
        "INSERT INTO selected_adoptions VALUES(?1,?2,?3,?4,?5,?6)",
        params![
            id.organization_id,
            p.task,
            id.writer_epoch,
            p.origin_generation,
            manifest,
            metadata
        ],
    )
    .map_err(storage)?;
    Ok(Outcome::Completed {
        task_id: p.task,
        revision: p.revision,
        history_coverage: p.coverage,
    })
}
