//! Initial public canonical state and trusted directory share one frozen journal
//! transaction. This is not an employee/profile-based owner grant.
use super::{
    admission::{self, OwnerAuthority},
    canonical_directory::Facts,
};
use crate::{Office, OfficeError};
use org_proto::selected_bootstrap::PreparedV1;
use org_work::selected_codec as codec;
use rusqlite::{params, OptionalExtension};
use serde_json::{json, Value};
use std::{collections::BTreeMap, sync::Arc};

fn invalid() -> OfficeError {
    OfficeError::Invalid("selected bootstrap requires recovery")
}
fn storage(e: impl std::error::Error + Send + Sync + 'static) -> OfficeError {
    OfficeError::storage(e)
}

impl Office {
    /// Called after protected writer/owner installation, before selected readiness.
    /// A completed bootstrap is read/verified on retry; later directory changes
    /// never get reset to the original initial generation.
    pub fn finish_selected_bootstrap(
        &self,
        authority: Arc<dyn OwnerAuthority>,
    ) -> Result<(), OfficeError> {
        let Some(binding) = self.retained_hq_creation_binding()? else {
            return Ok(());
        };
        let Some(prepared) = binding.bootstrap else {
            return Ok(());
        };
        if !prepared.valid()
            || prepared.creator.employee_id != binding.owner_member_id
            || prepared.organization_id != self.info().org_id
            || prepared.office_id != self.info().office_id
        {
            return Err(invalid());
        }
        let marker = format!("bootstrap/{}/receipt.v1.json", binding.operation_id);
        let identity = serde_json::to_vec(&prepared).map_err(storage)?;
        let expected = codec::sha256(&identity);
        let read_authority = Arc::clone(&authority);
        let m = marker.clone();
        let digest = expected.clone();
        let retained_plan = prepared.clone();
        let allocated_at = self.info().created_at.clone();
        if self.local_store().selected_read(move|db,id| {
            admission::owner(db,id,&*read_authority)?;
            let prior:Option<Vec<u8>>=db.query_row("SELECT content FROM selected_artifacts WHERE namespace='protected' AND path=?1",[m],|r|r.get(0)).optional().map_err(storage)?;
            match prior {
                Some(bytes) if bytes==digest.as_bytes()=>{
                    verify_initial_numbering(db,&retained_plan,&allocated_at)?;
                    Ok(true)
                },
                Some(_)=>Err(invalid()),None=>Ok(false)
            }
        })? {return Ok(());}
        let (source, artifacts) =
            materialize(&prepared, &self.info().office_name, &self.info().created_at)?;
        let parsed = codec::parse(&source).map_err(storage)?;
        let source = String::from_utf8(source).map_err(storage)?;
        self.local_store().selected_transaction(move |db, id| {
            admission::owner(db, id, &*authority)?;
            Facts::validate(&parsed, &id.organization_id)?;
            let installed: bool = db
                .query_row(
                    "SELECT EXISTS(SELECT 1 FROM selected_directory_head WHERE organization_id=?1)",
                    [&id.organization_id],
                    |r| r.get(0),
                )
                .map_err(storage)?;
            if installed {
                return Err(invalid());
            }
            // This is the first allocation in a newly prepared organization,
            // not a MAX+1 allocator or adoption of an existing counter. Any
            // retained identity state without this bootstrap receipt requires
            // explicit recovery; never reset its high-water to one.
            let identity_exists: bool = db.query_row(
                "SELECT EXISTS(SELECT 1 FROM selected_artifacts WHERE namespace='public' AND path LIKE 'identity/%')",[],|r|r.get(0),
            ).map_err(storage)?;
            if identity_exists { return Err(invalid()); }
            db.execute(
                "INSERT INTO selected_directory VALUES(?1,1,?2,?3)",
                params![id.organization_id, source, codec::sha256(source.as_bytes())],
            )
            .map_err(storage)?;
            db.execute(
                "INSERT INTO selected_directory_head VALUES(?1,1)",
                [&id.organization_id],
            )
            .map_err(storage)?;
            for (path, content) in &artifacts {
                db.execute(
                    "INSERT INTO selected_artifacts VALUES('public',?1,?2,?3)",
                    params![path, content, path.starts_with("identity/allocations/")],
                )
                .map_err(storage)?;
            }
            db.execute(
                "INSERT INTO selected_artifacts VALUES('protected',?1,?2,1)",
                params![marker, expected.as_bytes()],
            )
            .map_err(storage)?;
            Ok(())
        })
    }
}
/// Every bootstrap record is created by the organization's creator, a person employee.
/// No record is attributed to a nameless owner.
fn provenance(at: &str, creator: &str) -> Value {
    json!({"schema_version":1,"revision":1,"created_at":at,"created_by_kind":"employee","created_by_id":creator,"updated_at":at,"updated_by_kind":"employee","updated_by_id":creator})
}
fn with(mut base: Value, fields: Value) -> Value {
    base.as_object_mut()
        .expect("object")
        .extend(fields.as_object().expect("object").clone());
    base
}
fn yaml(v: &Value) -> Result<Vec<u8>, OfficeError> {
    let map = v.as_object().ok_or_else(invalid)?;
    let mut bytes = String::new();
    for (key, value) in map {
        bytes.push_str(key);
        bytes.push_str(": ");
        bytes.push_str(&serde_json::to_string(value).map_err(storage)?);
        bytes.push('\n');
    }
    Ok(bytes.into_bytes())
}
type Materialized = (Vec<u8>, BTreeMap<String, Vec<u8>>);
fn counter_path(p: &PreparedV1) -> String {
    format!(
        "identity/counters/organization/{}/team.yaml",
        p.organization_id
    )
}
fn allocation_path(p: &PreparedV1) -> String {
    format!(
        "identity/allocations/organization/{}/team/1.yaml",
        p.organization_id
    )
}
fn initial_allocation(p: &PreparedV1, at: &str) -> Result<Vec<u8>, OfficeError> {
    yaml(
        &json!({"scope_kind":"organization","scope_id":p.organization_id,"entity_kind":"team","number":1,"entity_id":p.home_team_id,"allocated_at":at}),
    )
}
fn verify_initial_numbering(
    db: &rusqlite::Connection,
    p: &PreparedV1,
    at: &str,
) -> Result<(), OfficeError> {
    let (allocation, immutable): (Vec<u8>, bool) = db
        .query_row(
            "SELECT content,immutable FROM selected_artifacts WHERE namespace='public' AND path=?1",
            [allocation_path(p)],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .map_err(storage)?;
    if !immutable || allocation != initial_allocation(p, at)? {
        return Err(invalid());
    }
    let counter: Vec<u8> = db
        .query_row(
            "SELECT content FROM selected_artifacts WHERE namespace='public' AND path=?1",
            [counter_path(p)],
            |r| r.get(0),
        )
        .map_err(storage)?;
    if counter.len() > org_docs::MAX_FRONT_MATTER_BYTES {
        return Err(invalid());
    }
    let text = std::str::from_utf8(&counter).map_err(storage)?;
    let document = format!("---\n{text}\n---\n");
    let parsed = org_docs::parse(&document).map_err(storage)?;
    let counter = parsed.front_matter.ok_or_else(invalid)?;
    if counter.get("scope_kind").and_then(Value::as_str) != Some("organization")
        || counter.get("scope_id").and_then(Value::as_str) != Some(&p.organization_id)
        || counter.get("entity_kind").and_then(Value::as_str) != Some("team")
        || counter
            .get("last_number")
            .and_then(Value::as_i64)
            .is_none_or(|n| n < 1)
    {
        return Err(invalid());
    }
    Ok(())
}
fn materialize(p: &PreparedV1, office_name: &str, at: &str) -> Result<Materialized, OfficeError> {
    if !p.valid() {
        return Err(invalid());
    }
    let mut files = BTreeMap::new();
    let creator = p.creator.employee_id.as_str();
    let org = with(
        provenance(at, creator),
        json!({"id":p.organization_id,"slug":p.plan.slug,"name":p.plan.name,"organization_repository_id":p.repository_id,"state_branch":p.plan.state_branch}),
    );
    let repo = with(
        provenance(at, creator),
        json!({"id":p.repository_id,"kind":"remote","remote_url":p.plan.repository_remote_url}),
    );
    let office = with(
        provenance(at, creator),
        json!({"id":p.office_id,"slug":"hq","name":office_name,"role":"hq"}),
    );
    // One top-level team, named after the organization and led by its creator.
    let team = with(
        provenance(at, creator),
        json!({"id":p.home_team_id,"name":p.plan.name,"number":1,"parent_team_id":null,"lead_employee_id":creator,"status":"active"}),
    );
    let person = with(
        provenance(at, creator),
        json!({"id":creator,"name":p.creator.name,"handle":p.creator.handle,"kind":"person","home_team_id":p.home_team_id,"role_id":null,"reports_to_employee_id":null,"status":"active","offboarded_at":null}),
    );
    // The data contract's employee record: `status` only. Whether the agent runs is not a
    // field of its profile (docs/design/organization-data).
    let hr = with(
        provenance(at, creator),
        json!({"id":p.hr_employee_id,"name":"HR","handle":org_proto::selected_bootstrap::HR_HANDLE,"kind":"agent","home_team_id":p.home_team_id,"role_id":null,"reports_to_employee_id":null,"status":"active","offboarded_at":null}),
    );
    files.insert("organization.yaml".into(), yaml(&org)?);
    files.insert(
        "repositories.yaml".into(),
        yaml(&json!({"schema_version":1,"repositories":[repo]}))?,
    );
    files.insert(format!("offices/{}.yaml", p.office_id), yaml(&office)?);
    files.insert(format!("teams/{}/team.yaml", p.home_team_id), yaml(&team)?);
    // The entity, counter and immutable allocation enter one frozen acceptance
    // envelope. The protected work-acceptance journal covers unpublished
    // numbering state; no separate competing allocator/journal is introduced.
    files.insert(counter_path(p),yaml(&json!({"scope_kind":"organization","scope_id":p.organization_id,"entity_kind":"team","last_number":1}))?);
    files.insert(allocation_path(p), initial_allocation(p, at)?);
    let mut profile = b"---\n".to_vec();
    profile.extend(yaml(&hr)?);
    profile.extend(
        b"---\nHR bootstrap identity. Execution requires a separate verified configuration.\n",
    );
    files.insert(
        format!(
            "teams/{}/agents/{}/profile.md",
            p.home_team_id, p.hr_employee_id
        ),
        profile,
    );
    let mut profile = b"---\n".to_vec();
    profile.extend(yaml(&person)?);
    profile.extend(b"---\n");
    files.insert(
        format!("teams/{}/people/{creator}/profile.md", p.home_team_id),
        profile,
    );
    let team = with(team, json!({"organization_id":p.organization_id}));
    let hr = with(hr, json!({"organization_id":p.organization_id}));
    let person = with(person, json!({"organization_id":p.organization_id}));
    let source = json!({"schema_version":1,"organization_id":p.organization_id,"source_position":{"epoch":0,"seq":0},"teams":{p.home_team_id.clone():team},"employees":{p.hr_employee_id.clone():hr,creator:person},"projects":{}});
    Ok((serde_json::to_vec(&source).map_err(storage)?, files))
}

#[cfg(test)]
mod tests;
