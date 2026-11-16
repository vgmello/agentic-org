//! LEGACY-ADOPT-1. Compare classified raw originals inside the sole writer;
//! never update legacy rows, identities, feeds or origin allocation.
use super::*;
use org_proto::work::{ItemKind, ItemStatus, WorkEvent, WorkEventKind};

fn text<'a>(j: &'a Json, key: &str) -> Result<&'a str, OfficeError> {
    j.get(key).and_then(Json::as_str).ok_or_else(invalid)
}
fn fields(j: &Json, expected: &[&str]) -> Result<(), OfficeError> {
    let Json::Object(map) = j else {
        return Err(invalid());
    };
    if map.len() != expected.len() || expected.iter().any(|k| !map.contains_key(*k)) {
        return Err(invalid());
    }
    Ok(())
}
fn nullable<'a>(j: &'a Json, key: &str) -> Result<Option<&'a str>, OfficeError> {
    match j.get(key) {
        Some(Json::Null) => Ok(None),
        Some(Json::String(s)) if !s.is_empty() => Ok(Some(s)),
        _ => Err(invalid()),
    }
}
fn number(j: &Json, key: &str, expected: i64) -> Result<(), OfficeError> {
    if j.get(key).and_then(Json::positive_i64) != Some(expected) {
        return Err(invalid());
    }
    Ok(())
}
fn bytes(db: &Connection, sql: &str, task: &str) -> Result<String, OfficeError> {
    // SQL rejects an oversized TEXT before allocating its Rust representation.
    db.query_row(sql, params![task, codec::MAX_JSON_BYTES as i64], |r| {
        r.get(0)
    })
    .optional()
    .map_err(storage)?
    .ok_or_else(invalid)
}
fn member(org: &dyn org_work::OrgView, id: &str) -> Result<org_proto::Member, OfficeError> {
    let member = org.member(id).map_err(storage)?.ok_or_else(invalid)?;
    if member.id != id
        || member.owner != (member.id == org.owner_id().map_err(storage)?)
        || !matches!(
            member.kind,
            org_proto::MemberKind::Person | org_proto::MemberKind::Agent
        )
    {
        return Err(invalid());
    }
    Ok(member)
}
fn employee<'a>(
    directory: &'a Json,
    id: &str,
    source: &org_proto::Member,
) -> Result<&'a Json, OfficeError> {
    if source.owner {
        return Err(invalid());
    }
    let e = directory
        .get("employees")
        .and_then(|v| v.get(id))
        .ok_or_else(invalid)?;
    let kind = match source.kind {
        org_proto::MemberKind::Person => "person",
        org_proto::MemberKind::Agent => "agent",
        _ => return Err(invalid()),
    };
    if e.get("kind").and_then(Json::as_str) != Some(kind) {
        return Err(invalid());
    }
    Ok(e)
}
fn public_tree(j: &Json) -> Result<(), OfficeError> {
    match j {
        Json::Object(map) => {
            for (key, value) in map {
                let key = key.to_ascii_lowercase();
                if matches!(
                    key.as_str(),
                    "private"
                        | "private_note"
                        | "private_notes"
                        | "private_memory"
                        | "agent_memory"
                        | "credential"
                        | "credentials"
                        | "password"
                        | "secret"
                        | "secrets"
                        | "private_key"
                        | "api_key"
                        | "authorization"
                        | "cookie"
                ) {
                    return Err(invalid());
                }
                if matches!(
                    key.as_str(),
                    "source_kind" | "visibility" | "classification" | "namespace" | "record_kind"
                ) && matches!(
                    value.as_str(),
                    Some(
                        "private"
                            | "private_note"
                            | "private_notes"
                            | "agent_memory"
                            | "credentials"
                            | "credential"
                            | "secret"
                    )
                ) {
                    return Err(invalid());
                }
                public_tree(value)?;
            }
        }
        Json::Array(items) => {
            for value in items {
                public_tree(value)?;
            }
        }
        _ => {}
    }
    Ok(())
}
fn classification(raw: &Json, known: &Json, declared: &Json) -> Result<(), OfficeError> {
    let Json::Array(paths) = declared else {
        return Err(invalid());
    };
    let mut pending = BTreeSet::new();
    for path in paths {
        let path = path.as_str().ok_or_else(invalid)?;
        if !path.starts_with('/') || !pending.insert(path) {
            return Err(invalid());
        }
    }
    fn walk(
        raw: &Json,
        known: &Json,
        path: &mut String,
        pending: &mut BTreeSet<&str>,
    ) -> Result<(), OfficeError> {
        match (raw, known) {
            (Json::Object(r), Json::Object(k)) => {
                for (key, value) in r {
                    let start = path.len();
                    path.push('/');
                    for c in key.chars() {
                        match c {
                            '~' => path.push_str("~0"),
                            '/' => path.push_str("~1"),
                            _ => path.push(c),
                        }
                    }
                    if let Some(known) = k.get(key) {
                        walk(value, known, path, pending)?;
                    } else if !pending.remove(path.as_str()) {
                        return Err(invalid());
                    }
                    path.truncate(start);
                }
            }
            (Json::Array(r), Json::Array(k)) => {
                for (i, value) in r.iter().enumerate() {
                    let start = path.len();
                    path.push('/');
                    path.push_str(&i.to_string());
                    walk(value, k.get(i).ok_or_else(invalid)?, path, pending)?;
                    path.truncate(start);
                }
            }
            _ => {}
        }
        Ok(())
    }
    public_tree(raw)?;
    walk(raw, known, &mut String::new(), &mut pending)?;
    if !pending.is_empty() {
        return Err(invalid());
    }
    Ok(())
}

fn verify_source_position(directory: &Json, epoch: u64, seq: u64) -> Result<(), OfficeError> {
    let source = directory.get("source_position").ok_or_else(invalid)?;
    let position_number = |key: &str| -> Result<u64, OfficeError> {
        match source.get(key) {
            Some(Json::Number(n)) => n.as_str().parse().map_err(|_| invalid()),
            _ => Err(invalid()),
        }
    };
    if (position_number("epoch")?, position_number("seq")?) != (epoch, seq) {
        return Err(invalid());
    }
    Ok(())
}

fn validate_initial_legacy(
    item: &org_work::StoredItem,
    event: &WorkEvent,
    id: &AcceptanceIdentity,
    task: &str,
    epoch: u64,
    seq: u64,
) -> Result<(), OfficeError> {
    let initial = match item.kind {
        ItemKind::Task => ItemStatus::Todo,
        ItemKind::Bug | ItemKind::Request => ItemStatus::New,
        _ => return Err(invalid()),
    };
    if item.deleted
        || item.version != 1
        || item.status != initial
        || item.resolution.is_some()
        || item.owner_office_id != id.office_id
    {
        return Err(invalid());
    }
    validate_initial_event(item, event, task, epoch, seq)
}

fn validate_initial_event(
    item: &org_work::StoredItem,
    event: &WorkEvent,
    task: &str,
    epoch: u64,
    seq: u64,
) -> Result<(), OfficeError> {
    if event.target_id != task
        || event.version != 1
        || (event.org_position.epoch, event.org_position.seq) > (epoch, seq)
        || !matches!(event.change, WorkEventKind::Created)
        || event.actor_id.as_deref() != Some(item.created_by.as_str())
        || org_work::parse_ms(&event.at) != Some(item.created_at_ms)
        || item.updated_at_ms != item.created_at_ms
    {
        return Err(invalid());
    }
    Ok(())
}

fn validate_mapping_source(
    m: &Json,
    item: &org_work::StoredItem,
    original: &str,
    original_event: &str,
) -> Result<(), OfficeError> {
    fields(
        m,
        &[
            "version",
            "source_kind",
            "source_snapshot_sha256",
            "source_event_sha256",
            "source_generation",
            "source_version",
            "original_kind",
            "original_reference",
            "accountable_team_id",
            "project_id",
            "scope_kind",
            "scope_id",
            "number",
            "reporter",
            "assignee",
            "public_extensions",
        ],
    )?;
    number(m, "version", 1)?;
    number(m, "source_version", 1)?;
    number(
        m,
        "source_generation",
        i64::try_from(item.generation).map_err(|_| invalid())?,
    )?;
    if text(m, "source_kind")? != "public_legacy_work"
        || text(m, "source_snapshot_sha256")? != codec::sha256(original.as_bytes())
        || text(m, "source_event_sha256")? != codec::sha256(original_event.as_bytes())
    {
        return Err(invalid());
    }
    Ok(())
}

fn validate_task_mapping(
    m: &Json,
    item: &org_work::StoredItem,
    snapshot: &TaskSnapshot<'_>,
    event: &WorkEvent,
) -> Result<(), OfficeError> {
    let f = snapshot.immutable();
    let (kind, prefix, selected) = match item.kind {
        ItemKind::Task => ("task", "T", "chore"),
        ItemKind::Bug => ("bug", "B", "bug"),
        ItemKind::Request => ("request", "REQ", "request"),
        _ => return Err(invalid()),
    };
    let n = item
        .number
        .and_then(|n| i64::try_from(n).ok())
        .filter(|n| *n > 0)
        .ok_or_else(invalid)?;
    if text(m, "original_kind")? != kind
        || text(m, "original_reference")? != format!("{prefix}-{n}")
        || f.task_type != selected
        || f.number != Some(n)
    {
        return Err(invalid());
    }
    validate_task_facts(m, item, snapshot, event, n)
}

fn validate_task_facts(
    m: &Json,
    item: &org_work::StoredItem,
    snapshot: &TaskSnapshot<'_>,
    event: &WorkEvent,
    n: i64,
) -> Result<(), OfficeError> {
    let f = snapshot.immutable();
    if f.title != item.title
        || f.status
            != serde_json::to_value(item.status)
                .map_err(storage)?
                .as_str()
                .ok_or_else(invalid)?
        || f.created_at.as_deref() != Some(event.at.as_str())
        || f.project_id != item.project_id
        || text(m, "accountable_team_id")? != f.team_id
        || nullable(m, "project_id")? != f.project_id.as_deref()
    {
        return Err(invalid());
    }
    let (scope, scope_id) = f
        .project_id
        .as_ref()
        .map(|p| ("project", p.as_str()))
        .unwrap_or(("team", f.team_id.as_str()));
    if text(m, "scope_kind")? != scope || text(m, "scope_id")? != scope_id {
        return Err(invalid());
    }
    number(m, "number", n)?;
    Ok(())
}

fn validate_people_mapping(
    m: &Json,
    org: &dyn org_work::OrgView,
    item: &org_work::StoredItem,
    snapshot: &TaskSnapshot<'_>,
    directory: &Json,
    owner: &OwnerAccount,
) -> Result<(), OfficeError> {
    let f = snapshot.immutable();
    let reporter = m.get("reporter").ok_or_else(invalid)?;
    fields(reporter, &["legacy_id", "kind", "id"])?;
    let legacy = member(org, &item.created_by)?;
    if text(reporter, "legacy_id")? != item.created_by
        || text(reporter, "kind")? != f.created_by_kind
        || nullable(reporter, "id")? != f.created_by_id.as_deref()
    {
        return Err(invalid());
    }
    match f.created_by_kind.as_str() {
        "owner"
            if legacy.owner
                && legacy.id == owner.member_id
                && org.owner_id().map_err(storage)? == legacy.id
                && f.created_by_id.is_none() => {}
        "employee" => {
            employee(
                directory,
                f.created_by_id.as_deref().ok_or_else(invalid)?,
                &legacy,
            )?;
        }
        _ => return Err(invalid()),
    }
    let assignee = m.get("assignee").ok_or_else(invalid)?;
    fields(assignee, &["legacy_id", "employee_id"])?;
    if nullable(assignee, "legacy_id")? != item.assignee_id.as_deref()
        || nullable(assignee, "employee_id")? != snapshot.assignment().assignee_id()
        || snapshot.assignment().assigned_team_id().is_some()
    {
        return Err(invalid());
    }
    match (&item.assignee_id, snapshot.assignment().assignee_id()) {
        (None, None) => {}
        (Some(old), Some(new)) => {
            employee(directory, new, &member(org, old)?)?;
        }
        _ => return Err(invalid()),
    }
    Ok(())
}

fn legacy_envelope_extensions(value: &Json) -> Result<BTreeMap<String, Json>, OfficeError> {
    let Json::Object(envelope) = value else {
        return Err(invalid());
    };
    let known = [
        "schema_version",
        "resource_profile",
        "organization_id",
        "task_kind",
        "task_id",
        "revision",
        "occurred_at",
        "actor_kind",
        "actor_id",
        "operation_id",
        "change",
        "source_metadata",
        "payload_sha256",
    ];
    let extensions = envelope
        .iter()
        .filter(|(k, _)| !known.contains(&k.as_str()))
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    Ok(extensions)
}

/// Return a freshly wrapped event only for an existing legacy item. Canonical
/// selected sources are not rewritten. Caller subsequently verifies the final
/// event, numbering and complete history through normal admission.
pub(super) fn wrap(
    db: &Connection,
    id: &AcceptanceIdentity,
    snapshot: &TaskSnapshot<'_>,
    directory: &Json,
    manifest: &Manifest,
    events: &[(i64, Vec<u8>, String)],
    import: (&OwnerAccount, ImportAt<'_>),
) -> Result<Option<(Vec<u8>, i64)>, OfficeError> {
    let (owner, at) = import;
    let task = &manifest.task_id;
    let present:bool=db.query_row("SELECT EXISTS(SELECT 1 FROM work_ids WHERE id=?1) OR EXISTS(SELECT 1 FROM work_items WHERE id=?1)",[task],|r|r.get(0)).map_err(storage)?;
    if !present {
        return Ok(None);
    }
    // This is the same org.db snapshot port used by ordinary native work. Never
    // read imaginary organization tables from office.db or trust a staged
    // caller-provided member/owner flag. Hold one source view for all mappings.
    let org = at
        .legacy_org
        .ok_or_else(invalid)?
        .snapshot()
        .map_err(storage)?;
    let position = org.applied_position().map_err(storage)?;
    verify_source_position(directory, position.epoch, position.seq)?;
    if snapshot.revision() != 1
        || !matches!(&manifest.history_origin,HistoryOrigin::Baseline {revision} if revision.get()==1)
        || events.len() != 1
        || events[0].0 != 1
    {
        return Err(invalid());
    }
    let count: i64 = db
        .query_row(
            "SELECT count(*) FROM (SELECT version FROM work_events WHERE target_id=?1 LIMIT 2)",
            [task],
            |r| r.get(0),
        )
        .map_err(storage)?;
    if count != 1 {
        return Err(invalid());
    }
    let original = bytes(
        db,
        "SELECT data FROM work_items WHERE id=?1 AND version=1 AND length(CAST(data AS BLOB))<=?2",
        task,
    )?;
    let original_event=bytes(db,"SELECT data FROM work_events WHERE target_id=?1 AND version=1 AND length(CAST(data AS BLOB))<=?2",task)?;
    let raw = codec::parse(original.as_bytes()).map_err(|_| invalid())?;
    let raw_event = codec::parse(original_event.as_bytes()).map_err(|_| invalid())?;
    // Reuse actual legacy validation, including generation/ID index/high-water
    // consistency, only after the aggregate per-source allocation bound above.
    let item = crate::store::work::get_in(db, task)?.ok_or_else(invalid)?;
    let event: WorkEvent = serde_json::from_str(&original_event).map_err(|_| invalid())?;
    validate_initial_legacy(&item, &event, id, task, position.epoch, position.seq)?;
    let generation = i64::try_from(item.generation).map_err(|_| invalid())?;
    let origins: i64 = db
        .query_row(
            "SELECT count(*) FROM work_origin_log WHERE target_id=?1 AND version=1",
            [task],
            |r| r.get(0),
        )
        .map_err(storage)?;
    let mapping:bool=db.query_row("SELECT EXISTS(SELECT 1 FROM work_origin_log WHERE target_id=?1 AND version=1 AND generation=?2 AND org_epoch=?3 AND org_seq=?4)",params![task,generation,i64::try_from(event.org_position.epoch).map_err(|_|invalid())?,i64::try_from(event.org_position.seq).map_err(|_|invalid())?],|r|r.get(0)).map_err(storage)?;
    if origins != 1 || !mapping {
        return Err(invalid());
    }
    let template = history::verify(
        &events[0].1,
        history::ExpectedIdentity {
            organization_id: &id.organization_id,
            task_id: task,
            filename: "1.json",
        },
        Admission::Import,
    )
    .map_err(|_| invalid())?;
    let value = template.value();
    public_tree(value)?;
    let change = value.get("change").ok_or_else(invalid)?;
    if change.get("type").and_then(Json::as_str) != Some("history_baseline") {
        return Err(invalid());
    }
    let metadata = value.get("source_metadata").ok_or_else(invalid)?;
    if metadata.get("legacy_snapshot") != Some(&raw)
        || metadata.get("legacy_event") != Some(&raw_event)
    {
        return Err(invalid());
    }
    let m = metadata.get("legacy_mapping").ok_or_else(invalid)?;
    validate_mapping_source(m, &item, &original, &original_event)?;
    validate_task_mapping(m, &item, snapshot, &event)?;
    // No pre-existing legacy team fact is silently made the selected owner.
    // The explicit accountable mapping is checked by Facts::task_path outside.
    validate_people_mapping(m, &*org, &item, snapshot, directory, owner)?;
    let public = m.get("public_extensions").ok_or_else(invalid)?;
    fields(public, &["snapshot", "event"])?;
    let known_snapshot =
        codec::parse(&serde_json::to_vec(&item).map_err(storage)?).map_err(|_| invalid())?;
    let known_event =
        codec::parse(&serde_json::to_vec(&event).map_err(storage)?).map_err(|_| invalid())?;
    classification(
        &raw,
        &known_snapshot,
        public.get("snapshot").ok_or_else(invalid)?,
    )?;
    classification(
        &raw_event,
        &known_event,
        public.get("event").ok_or_else(invalid)?,
    )?;
    let Json::Object(metadata) = metadata else {
        return Err(invalid());
    };
    let extensions = legacy_envelope_extensions(value)?;
    history::encode(
        EventFields {
            organization_id: id.organization_id.clone(),
            task_id: task.clone(),
            revision: 1,
            occurred_at: Some(org_work::format_ms(at.now)),
            actor_kind: "owner".into(),
            actor_id: None,
            operation_id: Some(at.operation.into()),
            change: change.clone(),
            source_metadata: metadata.clone(),
            extensions,
        },
        Admission::New,
    )
    .map(|bytes| Some((bytes, generation)))
    .map_err(|_| invalid())
}
