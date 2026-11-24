//! Organization office membership registration (HQ-only) and satellite replicated `state/`
//! file application (`TASK-52` / `S1`).

use org_store::{
    Actor, Change, DurablePath, Engine, Id, OfficeIdentity, Operation, Refusal, Stream, UnitOfWork,
};

use super::{invalid, operation, reply, shared, Feature, Op};
use crate::{
    organization::{
        ports::OrganizationRead,
        records::{self, Meta, Office, OfficeCode, OfficeRole},
    },
    projects::ports::ProjectsRead,
};

/// Registers (or idempotently refreshes) an enrolled satellite office in `state/offices/`
/// and allocates a unique office code in `state/identity/office-codes.yaml`.
#[derive(Clone, PartialEq, Eq, Debug, serde::Serialize, serde::Deserialize)]
pub struct RegisterSatelliteOffice {
    pub office_id: Id,
    pub office_name: String,
}

#[derive(Clone, PartialEq, Eq, Debug, serde::Serialize, serde::Deserialize)]
pub struct SatelliteRegistered {
    pub office_id: Id,
    pub slug: String,
    pub office_code: String,
    pub generation: u64,
}

impl Feature for RegisterSatelliteOffice {
    const OP: Op = Op::RegisterSatelliteOffice;

    fn operation(&self, id: Id, actor: &Actor) -> Operation {
        operation(Self::OP, id, actor.clone(), self)
    }

    fn decide<E: Engine>(
        &self,
        uow: &mut UnitOfWork<'_, E>,
        here: &OfficeIdentity,
        owner: &Id,
        by: &Actor,
    ) -> Result<Change, Refusal>
    where
        E::Tx: ProjectsRead,
    {
        let admitted = shared::admit(uow, here, owner, by)?;
        if !records::is_stable_id(self.office_id.as_str()) {
            return invalid("satellite office_id must be a canonical lowercase UUIDv7");
        }
        let name = self.office_name.trim();
        if !records::is_line(name) {
            return invalid("satellite office_name is invalid");
        }
        let offices = uow.view().offices()?;
        let mut codes = uow.view().office_codes()?;

        // Idempotent refresh when this satellite is already registered (e.g. D25 re-enrollment).
        if let Some(existing) = offices.iter().find(|o| o.id == self.office_id) {
            if existing.role != OfficeRole::Satellite {
                return invalid("cannot register HQ office as satellite");
            }
            let code = codes
                .iter()
                .find(|c| c.office_id == self.office_id && c.retired_at.is_none())
                .map(|c| c.code.clone())
                .unwrap_or_else(|| next_office_code(&codes));
            let updated = Office {
                name: name.to_owned(),
                meta: shared::touched(&existing.meta, &admitted.by, admitted.now),
                ..existing.clone()
            };
            let result = SatelliteRegistered {
                office_id: updated.id.clone(),
                slug: updated.slug.clone(),
                office_code: code,
                generation: admitted.generation,
            };
            return Ok(Change::new(Self::OP.ack())
                .put(
                    records::office_path(&updated.id),
                    records::encode_office(&updated),
                )
                .reply(reply(&result)));
        }

        let slug = allocate_office_slug(name, &offices);
        let code_str = next_office_code(&codes);
        let meta = Meta::created(&admitted.by, admitted.now);
        let office = Office {
            id: self.office_id.clone(),
            slug: slug.clone(),
            name: name.to_owned(),
            role: OfficeRole::Satellite,
            meta,
        };
        codes.push(OfficeCode {
            code: code_str.clone(),
            office_id: self.office_id.clone(),
            issued_at: records::time_text(admitted.now),
            retired_at: None,
        });
        let result = SatelliteRegistered {
            office_id: office.id.clone(),
            slug,
            office_code: code_str,
            generation: admitted.generation,
        };
        Ok(Change::new(Self::OP.ack())
            .put(
                records::office_path(&office.id),
                records::encode_office(&office),
            )
            .put(
                records::office_codes_path(),
                records::encode_office_codes(&codes),
            )
            .reply(reply(&result)))
    }
}

/// Applies authoritative `state/` file updates replicated from HQ onto a satellite's own
/// `org-store` acceptance journal and projection (`TASK-52` / `S1`).
#[derive(Clone, PartialEq, Eq, Debug, serde::Serialize, serde::Deserialize)]
pub struct ApplyReplicatedFiles {
    /// List of `(durable_path_str, Some(bytes) for put / None for delete)`.
    pub files: Vec<(String, Option<Vec<u8>>)>,
}

impl ApplyReplicatedFiles {
    pub fn operation(&self, id: Id, actor: Actor) -> Operation {
        operation(Op::ApplyReplicatedFiles, id, actor, self)
    }

    pub fn decide<E: Engine>(&self, uow: &mut UnitOfWork<'_, E>) -> Result<Change, Refusal>
    where
        E::Tx: OrganizationRead + crate::tasks::ports::TasksRead,
    {
        let _ = uow.next(Stream::Organization)?;
        let stale_tasks = collect_stale_replicated_tasks(uow.view(), &self.files);
        let mut change = Change::new(Op::ApplyReplicatedFiles.ack());
        let mut deferred_hq_offices: Vec<(DurablePath, Vec<u8>)> = Vec::new();
        for (raw_path, content) in &self.files {
            if !raw_path.starts_with("state/") && !raw_path.starts_with("messaging/") {
                return invalid("replicated files must stay strictly under state/ or messaging/");
            }
            let path = DurablePath::parse(raw_path)
                .map_err(|_| Refusal::invalid("invalid replicated durable path"))?;
            match content {
                Some(bytes) => {
                    if should_skip_stale_work_file(uow.view(), &path, bytes, &stale_tasks) {
                        continue;
                    }
                    // During D25 HQ replacement, a new HQ office file may sort lexicographically
                    // before the demoted previous HQ office file. Defer `role == Hq` office writes
                    // so the demoted office releases `slug = "hq"` before the new HQ claims it.
                    let is_hq_office =
                        matches!(records::kind_of(&path), Some(records::Kind::Office(_)))
                            && records::decode_office(bytes)
                                .is_ok_and(|o| o.role == OfficeRole::Hq);
                    if is_hq_office {
                        deferred_hq_offices.push((path, bytes.clone()));
                    } else {
                        change = change.put(path, bytes.clone());
                    }
                }
                None => change = change.remove(path),
            }
        }
        for (path, bytes) in deferred_hq_offices {
            change = change.put(path, bytes);
        }
        Ok(change)
    }
}

/// Identifies task IDs whose committed local revision is already >= the incoming replicated
/// task revision for the same owning `office_code` so an older replacement-HQ snapshot never
/// downgrades a local task while leaving higher-numbered immutable events behind (`TASK-52` P10-#4).
fn collect_stale_replicated_tasks(
    view: &impl crate::tasks::ports::TasksRead,
    files: &[(String, Option<Vec<u8>>)],
) -> std::collections::BTreeSet<Id> {
    let mut stale = std::collections::BTreeSet::new();
    for (raw_path, content) in files {
        let Some(bytes) = content else {
            continue;
        };
        let Ok(path) = DurablePath::parse(raw_path) else {
            continue;
        };
        let Some(crate::tasks::records::Kind::Task(tid)) = crate::tasks::records::kind_of(&path)
        else {
            continue;
        };
        let Ok(incoming) = serde_json::from_slice::<crate::tasks::records::TaskRecord>(bytes)
        else {
            continue;
        };
        if let Ok(Some(local)) = view.task(&tid) {
            if local.office_code == incoming.office_code
                && local.meta.revision > incoming.meta.revision
            {
                stale.insert(tid);
            }
        }
    }
    stale
}

fn should_skip_stale_work_file(
    view: &impl crate::tasks::ports::TasksRead,
    path: &DurablePath,
    bytes: &[u8],
    stale_tasks: &std::collections::BTreeSet<Id>,
) -> bool {
    match crate::tasks::records::kind_of(path) {
        Some(crate::tasks::records::Kind::OfficeTaskCounter(code)) => {
            let incoming_num = serde_json::from_slice::<serde_json::Value>(bytes)
                .ok()
                .and_then(|v| v.get("last_number").and_then(serde_json::Value::as_u64));
            matches!(
                (incoming_num, view.office_task_counter(&code)),
                (Some(in_num), Ok(Some(local))) if local.last_number >= in_num
            )
        }
        Some(
            crate::tasks::records::Kind::Task(tid)
            | crate::tasks::records::Kind::Checklist(tid)
            | crate::tasks::records::Kind::Comment(tid, _)
            | crate::tasks::records::Kind::Watchers(tid)
            | crate::tasks::records::Kind::Links(tid)
            | crate::tasks::records::Kind::Event(tid, _),
        ) => stale_tasks.contains(&tid),
        None => false,
    }
}

fn allocate_office_slug(name: &str, existing: &[Office]) -> String {
    let mut base = String::new();
    let mut prev_dash = true;
    for ch in name.chars() {
        if ch.is_ascii_alphanumeric() {
            base.push(ch.to_ascii_lowercase());
            prev_dash = false;
        } else if !prev_dash {
            base.push('-');
            prev_dash = true;
        }
    }
    while base.ends_with('-') {
        base.pop();
    }
    if base.is_empty() {
        base.push_str("office");
    }
    base.truncate(56);
    while base.ends_with('-') {
        base.pop();
    }
    if !existing.iter().any(|o| o.slug == base) {
        return base;
    }
    for suffix in 2u32..10_000 {
        let candidate = format!("{base}-{suffix}");
        if !existing.iter().any(|o| o.slug == candidate) {
            return candidate;
        }
    }
    format!("office-{}", existing.len() + 1)
}

fn next_office_code(existing: &[OfficeCode]) -> String {
    const ALPHABET: &[u8] = b"ABCDEFGHJKLMNPQRSTUVWXYZ"; // 24 letters: excludes I and O
    for &b in ALPHABET {
        let candidate = (b as char).to_string();
        if !existing.iter().any(|c| c.code == candidate) {
            return candidate;
        }
    }
    for &a in ALPHABET {
        for &b in ALPHABET {
            let candidate = format!("{}{}", a as char, b as char);
            if !existing.iter().any(|c| c.code == candidate) {
                return candidate;
            }
        }
    }
    "ZZ".to_owned()
}
