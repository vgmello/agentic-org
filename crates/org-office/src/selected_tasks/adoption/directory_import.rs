//! Whole canonical public directory admission. YAML values are used only for
//! known facts; the exact sources, including extensions and bodies, are retained.
use super::*;
use crate::selected_tasks::directory;
use org_proto::selected_tasks::adoption::{DirectoryKind as Kind, DirectoryManifest};
use serde_json::Map;

pub(super) struct Prepared {
    source: String,
    files: BTreeMap<String, (Vec<u8>, bool)>,
}
fn field<'a>(m: &'a Map<String, Value>, key: &str) -> Result<&'a str, OfficeError> {
    m.get(key)
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty() && !s.chars().any(char::is_control))
        .ok_or_else(invalid)
}
fn positive(m: &Map<String, Value>, key: &str) -> Result<i64, OfficeError> {
    m.get(key)
        .and_then(Value::as_i64)
        .filter(|n| *n > 0)
        .ok_or_else(invalid)
}
fn metadata(m: &Map<String, Value>, org: &str) -> Result<(), OfficeError> {
    let id = field(m, "id")?;
    directory::path_component(id).map_err(|_| invalid())?;
    if positive(m, "schema_version")? != 1
        || m.get("organization_id")
            .is_some_and(|v| v.as_str() != Some(org))
    {
        return Err(invalid());
    }
    positive(m, "revision")?;
    for prefix in ["created", "updated"] {
        if let Some(v) = m.get(&format!("{prefix}_at")) {
            if !v.is_null() && !v.as_str().is_some_and(history::utc_timestamp) {
                return Err(invalid());
            }
        }
        let kind = m.get(&format!("{prefix}_by_kind")).and_then(Value::as_str);
        let actor = m.get(&format!("{prefix}_by_id"));
        match (kind, actor) {
            (None, None) => {} // Unknown legacy provenance is not fabricated.
            (Some("employee"), Some(Value::String(id))) if !id.is_empty() => {}
            (Some("owner" | "system" | "unknown"), None | Some(Value::Null)) => {}
            _ => return Err(invalid()),
        }
    }
    Ok(())
}
fn source_map(bytes: &[u8], markdown: bool) -> Result<Map<String, Value>, OfficeError> {
    if markdown {
        if bytes.len() > org_docs::MAX_FRONT_MATTER_BYTES + org_docs::MAX_SOURCE_BODY_BYTES + 64 {
            return Err(invalid());
        }
        let doc = org_docs::parse(std::str::from_utf8(bytes).map_err(|_| invalid())?)
            .map_err(|_| invalid())?;
        if doc.body.len() > org_docs::MAX_SOURCE_BODY_BYTES {
            return Err(invalid());
        }
        doc.front_matter.ok_or_else(invalid)
    } else {
        yaml(bytes)
    }
}
fn project_known(m: &Map<String, Value>, org: &str, keys: &[&str], hash: &str) -> Value {
    let mut known = Map::new();
    for key in ["id", "schema_version", "revision", "name"]
        .into_iter()
        .chain(keys.iter().copied())
    {
        // Explicit nulls preserve the optional relationship vocabulary; they
        // never alter the retained source or manufacture a security binding.
        known.insert(key.into(), m.get(key).cloned().unwrap_or(Value::Null));
    }
    known.insert("organization_id".into(), org.into());
    known.insert("source_sha256".into(), hash.into());
    Value::Object(known)
}

#[derive(Default)]
struct DirectoryRecords {
    organization: Option<Map<String, Value>>,
    repositories: Option<Map<String, Value>>,
    numbered: BTreeMap<(String, i64), String>,
    teams: Map<String, Value>,
    employees: Map<String, Value>,
    projects: Map<String, Value>,
    roles: BTreeSet<String>,
}
impl DirectoryRecords {
    fn add(
        &mut self,
        db: &Connection,
        file: &org_proto::selected_tasks::adoption::DirectoryFile,
        value: &Map<String, Value>,
        bytes: &[u8],
        org: &str,
    ) -> Result<(), OfficeError> {
        let entity = value.get("id").and_then(Value::as_str).unwrap_or("");
        match file.kind {
            Kind::Organization => {
                if entity != org || self.organization.replace(value.clone()).is_some() {
                    return Err(invalid());
                }
                for key in ["name", "slug", "state_branch", "organization_repository_id"] {
                    field(value, key)?;
                }
            }
            Kind::Repositories => {
                if positive(value, "schema_version")? != 1
                    || self.repositories.replace(value.clone()).is_some()
                {
                    return Err(invalid());
                }
                // Repository edits are a separate workflow. Existing registry
                // bytes are retained; this admission cannot drop registrations.
                verify_same_if_present(db, &file.canonical_path, bytes)?;
            }
            Kind::Team | Kind::Project => {
                self.add_numbered(file, value, org)?;
            }
            Kind::Employee => {
                self.add_employee(value, org, &file.blob.sha256)?;
            }
            Kind::Role => {
                field(value, "name")?;
                if !self.roles.insert(entity.to_owned()) {
                    return Err(invalid());
                }
            }
            Kind::Counter | Kind::Allocation => {}
        }
        Ok(())
    }
    fn add_numbered(
        &mut self,
        file: &org_proto::selected_tasks::adoption::DirectoryFile,
        value: &Map<String, Value>,
        org: &str,
    ) -> Result<(), OfficeError> {
        let entity = value.get("id").and_then(Value::as_str).unwrap_or("");
        field(value, "name")?;
        let kind = if file.kind == Kind::Team {
            "team"
        } else {
            "project"
        };
        let number = positive(value, "number")?;
        if self
            .numbered
            .insert((kind.to_owned(), number), entity.to_owned())
            .is_some()
        {
            return Err(invalid());
        }
        let keys: &[&str] = if file.kind == Kind::Team {
            &["number", "status", "parent_team_id", "lead_employee_id"]
        } else {
            field(value, "slug")?;
            if !value.get("archived").is_some_and(Value::is_boolean) {
                return Err(invalid());
            }
            &["number", "slug", "archived", "owner_team_id"]
        };
        let collection = if file.kind == Kind::Team {
            &mut self.teams
        } else {
            &mut self.projects
        };
        if collection
            .insert(
                entity.into(),
                project_known(value, org, keys, &file.blob.sha256),
            )
            .is_some()
        {
            return Err(invalid());
        }
        Ok(())
    }
    fn add_employee(
        &mut self,
        value: &Map<String, Value>,
        org: &str,
        hash: &str,
    ) -> Result<(), OfficeError> {
        let entity = value.get("id").and_then(Value::as_str).unwrap_or("");
        if value.get("is_owner").is_some_and(|v| !v.is_boolean()) {
            return Err(invalid());
        }
        let keys = [
            "handle",
            "kind",
            "status",
            "home_team_id",
            "reports_to_employee_id",
            "role_id",
        ];
        if self
            .employees
            .insert(entity.into(), project_known(value, org, &keys, hash))
            .is_some()
        {
            return Err(invalid());
        }
        Ok(())
    }
    fn validate_relationships(&self) -> Result<(), OfficeError> {
        let mut slugs = BTreeSet::new();
        for project in self.projects.values() {
            if !slugs.insert(
                project
                    .get("slug")
                    .and_then(Value::as_str)
                    .ok_or_else(invalid)?,
            ) {
                return Err(invalid());
            }
        }
        for (team, fact) in &self.teams {
            if let Some(lead) = fact.get("lead_employee_id").and_then(Value::as_str) {
                if self
                    .employees
                    .get(lead)
                    .and_then(|e| e.get("home_team_id"))
                    .and_then(Value::as_str)
                    != Some(team)
                {
                    return Err(invalid());
                }
            }
        }
        for employee in self.employees.keys() {
            let mut visited = BTreeSet::new();
            let mut next = Some(employee.as_str());
            while let Some(id) = next {
                if !visited.insert(id) {
                    return Err(invalid());
                }
                next = self
                    .employees
                    .get(id)
                    .and_then(|e| e.get("reports_to_employee_id"))
                    .and_then(Value::as_str);
            }
        }
        Ok(())
    }
    fn validate_paths(
        &self,
        facts: &Facts<'_>,
        records: &[(Kind, String, Map<String, Value>)],
        org: &str,
    ) -> Result<(), OfficeError> {
        for (kind, path, record) in records {
            let expected = match kind {
                Kind::Organization => "organization.yaml".into(),
                Kind::Repositories => "repositories.yaml".into(),
                Kind::Team => format!(
                    "{}/team.yaml",
                    facts
                        .team_path(field(record, "id")?)
                        .map_err(|_| invalid())?
                ),
                Kind::Employee => {
                    if let Some(role) = record.get("role_id").filter(|v| !v.is_null()) {
                        if !role.as_str().is_some_and(|r| self.roles.contains(r)) {
                            return Err(invalid());
                        }
                    }
                    let directory = match field(record, "kind")? {
                        "person" => "people",
                        "agent" => "agents",
                        _ => return Err(invalid()),
                    };
                    format!(
                        "{}/{directory}/{}/profile.md",
                        facts
                            .team_path(field(record, "home_team_id")?)
                            .map_err(|_| invalid())?,
                        field(record, "id")?
                    )
                }
                Kind::Project => format!("projects/{}/project.yaml", field(record, "id")?),
                Kind::Role => format!("roles/{}.md", field(record, "id")?),
                Kind::Counter | Kind::Allocation => number_path(record, *kind, org)?,
            };
            if path != &expected {
                return Err(invalid());
            }
            for prefix in ["created", "updated"] {
                if record
                    .get(&format!("{prefix}_by_kind"))
                    .and_then(Value::as_str)
                    == Some("employee")
                    && !record
                        .get(&format!("{prefix}_by_id"))
                        .and_then(Value::as_str)
                        .is_some_and(|actor| self.employees.contains_key(actor))
                {
                    return Err(invalid());
                }
            }
        }
        Ok(())
    }
}

pub(super) fn prepare(
    db: &Connection,
    id: &AcceptanceIdentity,
    owner: &OwnerAccount,
    root: &Path,
    stage: &str,
    manifest: &str,
) -> Result<Prepared, OfficeError> {
    let raw = staging::manifest_source(root, id, owner, stage, manifest)?;
    let parsed = codec::parse(&raw).map_err(|_| invalid())?;
    let Some(Json::Array(files)) = parsed.get("files") else {
        return Err(invalid());
    };
    if !matches!(parsed.get("source_position"), Some(Json::Object(_)))
        || files.iter().any(|f| {
            !matches!(f, Json::Object(_)) || !matches!(f.get("blob"), Some(Json::Object(_)))
        })
    {
        return Err(invalid());
    }
    let m: DirectoryManifest = serde_json::from_slice(&raw).map_err(|_| invalid())?;
    if m.version != 1
        || m.resource_profile != codec::RESOURCE_PROFILE
        || m.source_position.epoch > i64::MAX as u64
        || m.source_position.seq > i64::MAX as u64
    {
        return Err(invalid());
    }
    // Aggregate admission bounds apply before loading any source payload.
    codec::check_blob_lengths(
        std::iter::once(raw.len() as u64).chain(m.files.iter().map(|f| f.blob.byte_length)),
    )
    .map_err(|_| invalid())?;
    let mut sources = BTreeMap::new();
    let mut records = Vec::new();
    let mut directory = DirectoryRecords::default();
    for file in m.files {
        let markdown = matches!(file.kind, Kind::Employee | Kind::Role);
        let max = if markdown {
            org_docs::MAX_FRONT_MATTER_BYTES + org_docs::MAX_SOURCE_BODY_BYTES + 64
        } else {
            org_docs::MAX_FRONT_MATTER_BYTES
        };
        let bytes = staging::source(root, id, owner, stage, manifest, &file.blob, max)?;
        let value = source_map(&bytes, markdown)?;
        if sources.contains_key(&file.canonical_path) {
            return Err(invalid());
        }
        let immutable = file.kind == Kind::Allocation;
        if !matches!(
            file.kind,
            Kind::Repositories | Kind::Counter | Kind::Allocation
        ) {
            metadata(&value, &id.organization_id)?;
            verify_revision(db, &file.canonical_path, &bytes, &value, markdown)?;
        }
        directory.add(db, &file, &value, &bytes, &id.organization_id)?;
        records.push((file.kind, file.canonical_path.clone(), value));
        sources.insert(file.canonical_path, (bytes, immutable));
    }
    validate_repository(
        &directory.organization.take().ok_or_else(invalid)?,
        &directory.repositories.take().ok_or_else(invalid)?,
        &id.organization_id,
    )?;
    let source = serde_json::to_string(&json!({"schema_version":1,"organization_id":id.organization_id,"source_position":{"epoch":m.source_position.epoch,"seq":m.source_position.seq},"teams":directory.teams,"employees":directory.employees,"projects":directory.projects})).map_err(storage)?;
    let parsed = codec::parse(source.as_bytes()).map_err(|_| invalid())?;
    let facts = Facts::validate(&parsed, &id.organization_id).map_err(|_| invalid())?;
    directory.validate_relationships()?;
    directory::validate_adopted_paths(db, id, &facts).map_err(|_| invalid())?;
    retain_previous_identities(db, id, &parsed)?;
    directory.validate_paths(&facts, &records, &id.organization_id)?;
    validate_numbering(db, &id.organization_id, &directory.numbered, &sources)?;
    Ok(Prepared {
        source,
        files: sources,
    })
}

fn verify_same_if_present(db: &Connection, path: &str, bytes: &[u8]) -> Result<(), OfficeError> {
    let old: Option<Vec<u8>> = db
        .query_row(
            "SELECT content FROM selected_artifacts WHERE namespace='public' AND path=?1",
            [path],
            |r| r.get(0),
        )
        .optional()
        .map_err(storage)?;
    if old.is_some_and(|old| old != bytes) {
        return Err(invalid());
    }
    Ok(())
}
fn verify_revision(
    db: &Connection,
    path: &str,
    bytes: &[u8],
    record: &Map<String, Value>,
    markdown: bool,
) -> Result<(), OfficeError> {
    let old: Option<Vec<u8>> = db
        .query_row(
            "SELECT content FROM selected_artifacts WHERE namespace='public' AND path=?1",
            [path],
            |r| r.get(0),
        )
        .optional()
        .map_err(storage)?;
    if let Some(old) = old {
        let prior = source_map(&old, markdown)?;
        let a = positive(&prior, "revision")?;
        let b = positive(record, "revision")?;
        if b < a || (a == b && old != bytes) || prior.get("id") != record.get("id") {
            return Err(invalid());
        }
    }
    Ok(())
}
fn validate_repository(
    org: &Map<String, Value>,
    registry: &Map<String, Value>,
    organization: &str,
) -> Result<(), OfficeError> {
    let rows = registry
        .get("repositories")
        .and_then(Value::as_array)
        .ok_or_else(invalid)?;
    let mut ids = BTreeSet::new();
    let mut selected = false;
    for row in rows {
        let row = row.as_object().ok_or_else(invalid)?;
        metadata(row, organization)?;
        let id = field(row, "id")?;
        if !ids.insert(id) {
            return Err(invalid());
        }
        match field(row, "kind")? {
            "remote" => {
                field(row, "remote_url")?;
            }
            "local" => {
                if row.get("remote_url").is_some_and(|v| !v.is_null()) {
                    return Err(invalid());
                }
            }
            _ => return Err(invalid()),
        }
        if id == field(org, "organization_repository_id")? {
            selected = field(row, "kind")? == "remote";
        }
    }
    if !selected {
        return Err(invalid());
    }
    Ok(())
}
fn retain_previous_identities(
    db: &Connection,
    id: &AcceptanceIdentity,
    new: &Json,
) -> Result<(), OfficeError> {
    let old:Option<String>=db.query_row("SELECT data FROM selected_directory d JOIN selected_directory_head h USING(organization_id,generation) WHERE organization_id=?1",[&id.organization_id],|r|r.get(0)).optional().map_err(storage)?;
    if let Some(old) = old {
        let old = codec::parse(old.as_bytes()).map_err(|_| invalid())?;
        for key in ["teams", "employees", "projects"] {
            let Some(Json::Object(prior)) = old.get(key) else {
                return Err(invalid());
            };
            for entity in prior.keys() {
                if new.get(key).and_then(|c| c.get(entity)).is_none() {
                    return Err(invalid());
                }
                let before = prior.get(entity).ok_or_else(invalid)?;
                let after = new
                    .get(key)
                    .and_then(|c| c.get(entity))
                    .ok_or_else(invalid)?;
                // Relocation is not an import update: it must atomically move
                // every canonical source/reference through a separate workflow.
                // Refuse even without an adopted task, before querying the new
                // path for revisions (which could otherwise hide old identity).
                let fixed: &[&str] = match key {
                    "teams" => &["parent_team_id"],
                    "employees" => &["home_team_id", "kind"],
                    "projects" => &["owner_team_id"],
                    _ => unreachable!(),
                };
                if fixed
                    .iter()
                    .any(|field| before.get(field) != after.get(field))
                {
                    return Err(invalid());
                }
            }
        }
        let position = |j: &Json| -> Result<(u64, u64), OfficeError> {
            let pos = j.get("source_position").ok_or_else(invalid)?;
            let n = |key: &str| match pos.get(key) {
                Some(Json::Number(n)) => n.as_str().parse::<u64>().map_err(|_| invalid()),
                _ => Err(invalid()),
            };
            Ok((n("epoch")?, n("seq")?))
        };
        if position(new)? < position(&old)? {
            return Err(invalid());
        }
    }
    Ok(())
}
fn number_path(record: &Map<String, Value>, kind: Kind, org: &str) -> Result<String, OfficeError> {
    if field(record, "scope_kind")? != "organization"
        || field(record, "scope_id")? != org
        || !matches!(field(record, "entity_kind")?, "team" | "project")
    {
        return Err(invalid());
    }
    let entity = field(record, "entity_kind")?;
    if kind == Kind::Counter {
        positive(record, "last_number")?;
        Ok(format!(
            "identity/counters/organization/{org}/{entity}.yaml"
        ))
    } else {
        let number = positive(record, "number")?;
        directory::path_component(field(record, "entity_id")?).map_err(|_| invalid())?;
        if !field(record, "allocated_at").is_ok_and(history::utc_timestamp) {
            return Err(invalid());
        }
        Ok(format!(
            "identity/allocations/organization/{org}/{entity}/{number}.yaml"
        ))
    }
}
fn validate_numbering(
    db: &Connection,
    org: &str,
    numbered: &BTreeMap<(String, i64), String>,
    files: &BTreeMap<String, (Vec<u8>, bool)>,
) -> Result<(), OfficeError> {
    let mut high = BTreeMap::<String, i64>::new();
    for ((kind, number), entity) in numbered {
        high.entry(kind.clone())
            .and_modify(|n| *n = (*n).max(*number))
            .or_insert(*number);
        let path = format!("identity/allocations/organization/{org}/{kind}/{number}.yaml");
        let bytes = &files.get(&path).ok_or_else(invalid)?.0;
        if field(&yaml(bytes)?, "entity_id")? != entity {
            return Err(invalid());
        }
    }
    for (path, (bytes, immutable)) in files {
        if !path.starts_with("identity/") {
            continue;
        }
        if *immutable {
            verify_same_if_present(db, path, bytes)?;
        }
        let value = yaml(bytes)?;
        let kind = field(&value, "entity_kind")?.to_owned();
        if *immutable {
            let n = positive(&value, "number")?;
            high.entry(kind)
                .and_modify(|v| *v = (*v).max(n))
                .or_insert(n);
        }
    }
    // Stream retained allocation/counter rows. A new import's bounds must not
    // become an aggregate cap on already acknowledged numbering history.
    let prefix = format!("identity/allocations/organization/{org}/");
    let mut q=db.prepare("SELECT path,content FROM selected_artifacts WHERE namespace='public' AND substr(path,1,length(?1))=?1").map_err(storage)?;
    let mut rows = q.query([prefix]).map_err(storage)?;
    while let Some(row) = rows.next().map_err(storage)? {
        let path: String = row.get(0).map_err(storage)?;
        let bytes: Vec<u8> = row.get(1).map_err(storage)?;
        let record = yaml(&bytes)?;
        retain_allocation_high_water(&record, &path, org, numbered, &mut high)?;
    }
    for (kind, required) in high {
        let path = format!("identity/counters/organization/{org}/{kind}.yaml");
        let incoming = &files.get(&path).ok_or_else(invalid)?.0;
        let n = positive(&yaml(incoming)?, "last_number")?;
        let old: Option<Vec<u8>> = db
            .query_row(
                "SELECT content FROM selected_artifacts WHERE namespace='public' AND path=?1",
                [&path],
                |r| r.get(0),
            )
            .optional()
            .map_err(storage)?;
        if n < required
            || old
                .map(|b| yaml(&b).and_then(|v| positive(&v, "last_number")))
                .transpose()?
                .is_some_and(|old| n < old)
        {
            return Err(invalid());
        }
    }
    Ok(())
}
fn retain_allocation_high_water(
    record: &Map<String, Value>,
    path: &str,
    org: &str,
    numbered: &BTreeMap<(String, i64), String>,
    high: &mut BTreeMap<String, i64>,
) -> Result<(), OfficeError> {
    let kind = field(record, "entity_kind")?;
    if !matches!(kind, "team" | "project") {
        return Ok(());
    }
    if number_path(record, Kind::Allocation, org)? != path {
        return Err(invalid());
    }
    let number = positive(record, "number")?;
    if let Some(entity) = numbered.get(&(kind.to_owned(), number)) {
        if field(record, "entity_id")? != entity {
            return Err(invalid());
        }
    }
    if numbered.iter().any(|((k, n), entity)| {
        k == kind && *n != number && record.get("entity_id").and_then(Value::as_str) == Some(entity)
    }) {
        return Err(invalid());
    }
    high.entry(kind.into())
        .and_modify(|n| *n = (*n).max(number))
        .or_insert(number);
    Ok(())
}

pub(super) fn apply(
    db: &Connection,
    id: &AcceptanceIdentity,
    current: Option<i64>,
    p: Prepared,
) -> Result<Outcome, OfficeError> {
    let next = current.unwrap_or(0).checked_add(1).ok_or_else(invalid)?;
    for (path, (bytes, immutable)) in p.files {
        service::put_artifact(db, "public", &path, &bytes, immutable)?;
    }
    db.execute(
        "INSERT INTO selected_directory VALUES(?1,?2,?3,?4)",
        params![
            id.organization_id,
            next,
            p.source,
            codec::sha256(p.source.as_bytes())
        ],
    )
    .map_err(storage)?;
    db.execute("INSERT INTO selected_directory_head VALUES(?1,?2) ON CONFLICT(organization_id) DO UPDATE SET generation=excluded.generation",params![id.organization_id,next]).map_err(storage)?;
    Ok(Outcome::DirectoryCompleted { generation: next })
}
