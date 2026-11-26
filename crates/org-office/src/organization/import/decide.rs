//! Building Entry 1 for a new HQ office from a validated import: the next office code, the
//! owner person and the rewritten organization, repositories, offices and codes.

use std::collections::{BTreeMap, BTreeSet};
use std::io::Write;

use org_store::{
    Ack, Actor, Change, DurablePath, Id, OfficeIdentity, Operation, Refusal, Sha256, Stream,
    Timestamp, UnitOfWork,
};
use serde::Serialize;

use crate::organization::{
    features::{create_organization::HQ_SLUG, operation, Creator, Op},
    ports::OrganizationRead,
    records::{
        self, By, Employee, EmployeeKind, EmployeeStatus, Kind, Meta, Office, OfficeCode,
        OfficeRole,
    },
};

use super::ImportedOrganization;

/// Allocates the next unused valid office code (`A`..`Z` excluding `I` and `O`, then `AA`..`ZZ`)
/// so a newly opened HQ that cannot prove its last task number takes a fresh office code
/// (`doc-91` "Opening an existing organization").
pub fn next_office_code(existing: &[OfficeCode]) -> Option<String> {
    const LETTERS: &[u8] = b"ABCDEFGHJKLMNPQRSTUVWXYZ";
    let used: BTreeSet<&str> = existing.iter().map(|c| c.code.as_str()).collect();
    for &b in LETTERS {
        let code = (b as char).to_string();
        if !used.contains(code.as_str()) {
            return Some(code);
        }
    }
    for &a in LETTERS {
        for &b in LETTERS {
            let mut code = String::with_capacity(2);
            code.push(a as char);
            code.push(b as char);
            if !used.contains(code.as_str()) {
                return Some(code);
            }
        }
    }
    None
}

/// Command describing Entry 1 of an HQ opened from existing Git files (`R1`).
#[derive(Clone, Debug)]
pub struct ImportOrganization {
    pub organization_id: Id,
    pub office_id: Id,
    pub office_name: String,
    pub owner_member_id: Id,
    /// Optional new person to add into the root team when the administrator creates a fresh
    /// person record instead of selecting an existing person from Git.
    pub new_owner_person: Option<Creator>,
    pub imported: ImportedOrganization,
}

#[derive(Serialize)]
struct ImportCreatorFingerprint<'a> {
    employee_id: &'a Id,
    name: &'a str,
    handle: &'a str,
}

#[derive(Serialize)]
struct ImportFingerprint<'a> {
    organization_id: &'a Id,
    office_id: &'a Id,
    owner_member_id: &'a Id,
    repository_remote_url: &'a str,
    state_branch: &'a str,
    commit_oid: Option<&'a str>,
    content_sha256: String,
    new_owner_person: Option<ImportCreatorFingerprint<'a>>,
}

fn canonical_content_sha256(files: &BTreeMap<DurablePath, Vec<u8>>) -> String {
    let mut manifest = Vec::new();
    for (path, bytes) in files {
        let digest = Sha256::of(bytes);
        let _ = writeln!(manifest, "{}\0{}\0{}", path.as_str(), bytes.len(), digest);
    }
    Sha256::of(&manifest).to_string()
}

impl ImportOrganization {
    pub fn operation(&self, id: Id) -> Operation {
        let actor = Actor::Employee(self.owner_member_id.clone());
        let body = ImportFingerprint {
            organization_id: &self.organization_id,
            office_id: &self.office_id,
            owner_member_id: &self.owner_member_id,
            repository_remote_url: &self.imported.repository_remote_url,
            state_branch: &self.imported.state_branch,
            commit_oid: self.imported.commit_oid.as_deref(),
            content_sha256: canonical_content_sha256(&self.imported.files),
            new_owner_person: self
                .new_owner_person
                .as_ref()
                .map(|c| ImportCreatorFingerprint {
                    employee_id: &c.employee_id,
                    name: &c.name,
                    handle: &c.handle,
                }),
        };
        operation(Op::ImportOrganization, id, actor, &body)
    }
}

/// Builds Entry 1 for an HQ opened from Git files (`doc-91`, `doc-61` D25).
pub fn decide_import<E: org_store::Engine>(
    uow: &mut UnitOfWork<'_, E>,
    here: &OfficeIdentity,
    command: &ImportOrganization,
) -> Result<Change, Refusal>
where
    E::Tx: crate::organization::ports::OrganizationRead,
{
    if command.organization_id != here.organization_id || command.office_id != here.office_id {
        return Err(Refusal::invalid(
            "this store is another organization's or office's",
        ));
    }
    if command.imported.summary.organization_id != here.organization_id {
        return Err(Refusal::invalid("imported organization_id does not match"));
    }
    if uow
        .view()
        .organization()
        .map_err(|_| Refusal::invalid("read failed"))?
        .is_some()
    {
        return Err(Refusal::invalid("the organization already exists"));
    }
    let generation = uow.next(Stream::Organization)?;
    if generation != 1 {
        return Err(Refusal::invalid(
            "import is the new HQ journal's first entry",
        ));
    }
    let now = uow.now();
    let by = By::Employee(command.owner_member_id.clone());
    let mut files = command.imported.files.clone();
    // Old HQ escrow is replaced by the new HQ's fresh CA escrow written by the caller.
    files.remove(&records::ca_escrow_path());

    ensure_owner_person(&mut files, command, &by, now)?;
    rewrite_org_and_repo(&mut files, command, &by, now)?;
    rewrite_offices_and_codes(&mut files, command, &by, now)?;

    let mut change = Change::new(Ack::Durable);
    for (path, bytes) in files {
        change = change.create(path, bytes);
    }
    let reply = serde_json::json!({
        "organization_id": command.organization_id.as_str(),
        "office_id": command.office_id.as_str(),
        "owner_member_id": command.owner_member_id.as_str(),
        "generation": generation,
    });
    Ok(change.reply(serde_json::to_vec(&reply).expect("reply JSON encodes")))
}

fn ensure_owner_person(
    files: &mut BTreeMap<DurablePath, Vec<u8>>,
    command: &ImportOrganization,
    by: &By,
    now: Timestamp,
) -> Result<(), Refusal> {
    let existing_active = command
        .imported
        .summary
        .people
        .iter()
        .any(|p| p.id == command.owner_member_id.as_str());
    if existing_active {
        return Ok(());
    }
    let Some(creator) = &command.new_owner_person else {
        return Err(Refusal::invalid(
            "owner_member_id must name an active person in the organization",
        ));
    };
    if creator.employee_id != command.owner_member_id {
        return Err(Refusal::invalid("new_owner_person employee_id mismatch"));
    }
    if !records::is_line(&creator.name) {
        return Err(Refusal::invalid("new_owner_person name is invalid"));
    }
    if !org_proto::selected_bootstrap::valid_handle(&creator.handle)
        || creator.handle == org_proto::selected_bootstrap::HR_HANDLE
    {
        return Err(Refusal::invalid("new_owner_person handle is invalid"));
    }
    if command
        .imported
        .summary
        .reserved_handles
        .iter()
        .any(|h| h == &creator.handle)
    {
        return Err(Refusal::invalid("new_owner_person handle already exists"));
    }
    if command
        .imported
        .summary
        .reserved_entity_ids
        .iter()
        .any(|id| id == creator.employee_id.as_str())
    {
        return Err(Refusal::invalid(
            "new_owner_person employee_id collides with an existing record",
        ));
    }
    let Some(root_team_id) = command
        .imported
        .summary
        .has_active_root_team
        .then_some(command.imported.summary.root_team_id.as_ref())
        .flatten()
    else {
        return Err(Refusal::invalid(
            "imported organization has no active root team",
        ));
    };
    let emp = Employee {
        id: creator.employee_id.clone(),
        handle: creator.handle.clone(),
        kind: EmployeeKind::Person,
        name: creator.name.clone(),
        home_team_id: root_team_id.clone(),
        role_id: None,
        reports_to_employee_id: None,
        status: EmployeeStatus::Active,
        contact: None,
        title: None,
        focus: None,
        role_text: None,
        title_in_team: None,
        offboarded_at: None,
        may_hire: true,
        builtin_key: None,
        meta: Meta::created(by, now),
        body: String::new(),
    };
    let path = records::employee_path(
        std::slice::from_ref(root_team_id),
        EmployeeKind::Person,
        &emp.id,
    )
    .map_err(|_| Refusal::invalid("invalid owner person path"))?;
    files.insert(path, records::encode_employee(&emp));
    Ok(())
}

fn rewrite_org_and_repo(
    files: &mut BTreeMap<DurablePath, Vec<u8>>,
    command: &ImportOrganization,
    by: &By,
    now: Timestamp,
) -> Result<(), Refusal> {
    let org_path = records::organization_path();
    let org_bytes = files
        .get(&org_path)
        .ok_or_else(|| Refusal::invalid("missing organization.yaml"))?;
    let mut org =
        records::decode_organization(org_bytes).map_err(|_| Refusal::invalid("invalid org"))?;
    let repo_id = org.organization_repository_id.clone();
    if org.state_branch != command.imported.state_branch {
        org.state_branch = command.imported.state_branch.clone();
        org.meta.revision = org.meta.revision.saturating_add(1);
        org.meta.updated_at = Some(records::time_text(now));
        org.meta.updated_by = by.clone();
        files.insert(org_path, records::encode_organization(&org));
    }
    let remote = command.imported.repository_remote_url.as_str();
    if !remote.starts_with("https://") && !remote.starts_with("ssh://") {
        return Ok(());
    }
    let repos_path = records::repositories_path();
    let Some(repo_bytes) = files.get(&repos_path) else {
        return Ok(());
    };
    let mut repos = records::decode_repositories(repo_bytes)
        .map_err(|_| Refusal::invalid("invalid repositories.yaml"))?;
    let Some(repo) = repos.iter_mut().find(|r| r.id == repo_id) else {
        return Ok(());
    };
    if repo.remote_url.as_deref() == Some(remote) {
        return Ok(());
    }
    repo.remote_url = Some(remote.to_owned());
    repo.meta.revision = repo.meta.revision.saturating_add(1);
    repo.meta.updated_at = Some(records::time_text(now));
    repo.meta.updated_by = by.clone();
    files.insert(repos_path, records::encode_repositories(&repos));
    Ok(())
}

fn rewrite_offices_and_codes(
    files: &mut BTreeMap<DurablePath, Vec<u8>>,
    command: &ImportOrganization,
    by: &By,
    now: Timestamp,
) -> Result<(), Refusal> {
    let now_text = records::time_text(now);
    let mut used_slugs = BTreeSet::new();
    let office_paths: Vec<(DurablePath, Id)> = files
        .keys()
        .filter_map(|p| match records::kind_of(p) {
            Some(Kind::Office(id)) => Some((p.clone(), id)),
            _ => None,
        })
        .collect();
    let mut old_hq_id: Option<Id> = None;
    for (path, id) in &office_paths {
        let office = records::decode_office(&files[path])
            .map_err(|_| Refusal::invalid("invalid office record"))?;
        if office.role == OfficeRole::Hq {
            old_hq_id = Some(id.clone());
        } else {
            used_slugs.insert(office.slug);
        }
    }
    if let Some(old_id) = &old_hq_id {
        let old_path = records::office_path(old_id);
        let mut old_office = records::decode_office(&files[&old_path])
            .map_err(|_| Refusal::invalid("invalid old HQ office"))?;
        old_office.role = OfficeRole::Satellite;
        if old_office.slug == HQ_SLUG {
            old_office.slug = unique_replaced_slug(&used_slugs, old_id);
        }
        used_slugs.insert(old_office.slug.clone());
        old_office.meta.revision = old_office.meta.revision.saturating_add(1);
        old_office.meta.updated_at = Some(now_text.clone());
        old_office.meta.updated_by = by.clone();
        files.insert(old_path, records::encode_office(&old_office));
    }
    let new_hq = Office {
        id: command.office_id.clone(),
        slug: HQ_SLUG.to_owned(),
        name: command.office_name.clone(),
        role: OfficeRole::Hq,
        meta: Meta::created(by, now),
    };
    files.insert(
        records::office_path(&command.office_id),
        records::encode_office(&new_hq),
    );

    let codes_path = records::office_codes_path();
    let mut codes = match files.get(&codes_path) {
        Some(bytes) => records::decode_office_codes(bytes)
            .map_err(|_| Refusal::invalid("invalid office-codes.yaml"))?,
        None => Vec::new(),
    };
    if let Some(old_id) = &old_hq_id {
        for entry in &mut codes {
            if entry.office_id == *old_id && entry.retired_at.is_none() {
                entry.retired_at = Some(now_text.clone());
            }
        }
    }
    let next_code = next_office_code(&codes)
        .ok_or_else(|| Refusal::invalid("no office codes left to assign"))?;
    codes.push(OfficeCode {
        code: next_code,
        office_id: command.office_id.clone(),
        issued_at: now_text,
        retired_at: None,
    });
    files.insert(codes_path, records::encode_office_codes(&codes));
    Ok(())
}

fn unique_replaced_slug(used: &BTreeSet<String>, old_id: &Id) -> String {
    let prefix: String = old_id
        .as_str()
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .take(8)
        .collect();
    let base = format!("hq-previous-{prefix}");
    if !used.contains(&base) {
        return base;
    }
    for n in 2u32..1000 {
        let candidate = format!("{base}-{n}");
        if !used.contains(&candidate) {
            return candidate;
        }
    }
    format!("hq-prev-{prefix}")
}
