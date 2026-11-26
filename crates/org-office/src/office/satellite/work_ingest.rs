//! HQ-side validation of satellite-originated work batches against HQ's organization snapshot.

use org_store::{DurablePath, Id};

use crate::error::OfficeError;
use crate::office::{storage, Office};

use super::is_immutable_state_path;

pub(super) fn validate_work_file_paths(
    files: &[(String, Option<Vec<u8>>)],
) -> Result<(), OfficeError> {
    for (path, bytes) in files {
        let dp = DurablePath::parse(path)
            .map_err(|_| OfficeError::Invalid("invalid replicated work path"))?;
        let task_kind = crate::tasks::records::kind_of(&dp);
        let msg_kind = crate::messaging::records::kind_of(&dp);
        if task_kind.is_none() && msg_kind.is_none() {
            return Err(OfficeError::Invalid(
                "satellite work change may only touch tasks or messaging records",
            ));
        }
        if bytes.is_none() && is_immutable_state_path(path) {
            return Err(OfficeError::Invalid(
                "cannot delete immutable task or messaging history file",
            ));
        }
    }
    Ok(())
}

pub(super) struct ReadOfficeCodesOp;

impl crate::store::open::ReadOp for ReadOfficeCodesOp {
    type Out = Vec<crate::organization::records::OfficeCode>;

    fn read<S: crate::tasks::ports::TasksRead>(
        self,
        snapshot: &S,
    ) -> Result<Self::Out, org_store::StoreError> {
        crate::organization::ports::OrganizationRead::office_codes(snapshot)
            .map_err(org_store::StoreError::Engine)
    }
}

struct IncomingTaskCheck<'a> {
    expected_record_id: &'a str,
    origin_codes: &'a std::collections::BTreeSet<String>,
    team_ids: &'a std::collections::BTreeSet<&'a str>,
    /// The revision the batch brings each task to (`task.md`'s, or a later event's).
    revisions: &'a std::collections::BTreeMap<Id, u64>,
}

fn validate_incoming_task_record(
    office: &Office,
    tid: &Id,
    opt_bytes: Option<&[u8]>,
    check: &IncomingTaskCheck<'_>,
) -> Result<String, OfficeError> {
    let IncomingTaskCheck {
        expected_record_id,
        origin_codes,
        team_ids,
        revisions,
    } = check;
    let expected_record_id = *expected_record_id;
    let Some(bytes) = opt_bytes else {
        return Err(OfficeError::Invalid(
            "satellite work change cannot delete task records",
        ));
    };
    let rec = crate::tasks::records::decode_task_md(bytes)
        .map_err(|_| OfficeError::Invalid("malformed task.md in satellite work"))?
        .task;
    if rec.id != *tid {
        return Err(OfficeError::Invalid("TaskRecord id does not match path"));
    }
    let incoming_revision = revisions
        .get(tid)
        .copied()
        .unwrap_or(rec.meta.revision)
        .max(rec.meta.revision);
    if org_proto::is_uuid_v7(expected_record_id) && rec.id.as_str() != expected_record_id {
        return Err(OfficeError::Invalid(
            "TaskRecord id does not match WorkChange record_id",
        ));
    }
    if !origin_codes.contains(&rec.office_code) {
        return Err(OfficeError::Invalid(
            "satellite cannot write task belonging to another office",
        ));
    }
    if !team_ids.contains(rec.team_id.as_str()) {
        return Err(OfficeError::Invalid(
            "TaskRecord references unknown organization team",
        ));
    }
    let actor_id = rec
        .meta
        .updated_by_id
        .as_ref()
        .or(rec.meta.created_by_id.as_ref())
        .ok_or(OfficeError::Invalid(
            "TaskRecord missing writer attribution",
        ))?;
    let actor_active = office
        .load_employee(actor_id.as_str())?
        .is_some_and(|e| e.status == crate::organization::records::EmployeeStatus::Active);
    if !actor_active {
        return Err(OfficeError::Invalid(
            "TaskRecord writer is not an active organization member",
        ));
    }
    if let Ok(Ok(existing)) = office.task_inspect(rec.id.as_str()) {
        if !origin_codes.contains(&existing.task.office_code) {
            return Err(OfficeError::Invalid(
                "satellite cannot overwrite task owned by another office",
            ));
        }
        // `task.md` carries the revision of the task's last core change; the task's
        // effective revision may be past it by child-only changes (`doc-79`), so only the
        // batch's own revision for the task is compared.
        if incoming_revision < existing.task.revision {
            return Err(OfficeError::Invalid(
                "stale TaskRecord revision rejected by HQ",
            ));
        }
    }
    Ok(rec.office_code)
}

pub(super) fn validate_typed_work_files_against_hq(
    office: &Office,
    origin_office: &str,
    expected_record_id: &str,
    files: &[(String, Option<Vec<u8>>)],
) -> Result<(), OfficeError> {
    // Caller (`ingest_validated_satellite_work`) already holds `office.federation_io_lock`.
    if crate::office::invitations::is_satellite_office_removed_locked(office, origin_office)? {
        return Err(OfficeError::Invalid(
            "removed satellite office no longer holds task write authority",
        ));
    }
    let snap = office.org_snapshot()?;
    let team_ids: std::collections::BTreeSet<&str> =
        snap.teams.iter().map(|t| t.id.as_str()).collect();
    let (codes, _) = office.store.read(ReadOfficeCodesOp).map_err(storage)?;
    let origin_codes: std::collections::BTreeSet<String> = codes
        .into_iter()
        .filter(|c| c.office_id.as_str() == origin_office)
        .map(|c| c.code)
        .collect();
    if origin_codes.is_empty() {
        return Err(OfficeError::Invalid(
            "origin satellite office has no assigned office code on HQ",
        ));
    }
    let mut incoming_task_codes: std::collections::BTreeMap<String, String> =
        std::collections::BTreeMap::new();
    let mut deferred_sidecars: Vec<Id> = Vec::new();
    let refs = files.iter().map(|(p, b)| (p.as_str(), b.as_deref()));
    let revisions: std::collections::BTreeMap<Id, u64> =
        crate::tasks::replica::incoming_tasks(refs)
            .map_err(|_| OfficeError::Invalid("malformed task.md in satellite work"))?
            .into_iter()
            .map(|(id, incoming)| (id, incoming.revision))
            .collect();
    let check = IncomingTaskCheck {
        expected_record_id,
        origin_codes: &origin_codes,
        team_ids: &team_ids,
        revisions: &revisions,
    };
    // Pass 1: validate `task.md` records before sidecars regardless of file order (`TASK-52`
    // P8-#2). The office's task counter is office-local and never in a work change (`doc-80`).
    for (path, opt_bytes) in files {
        let dp = DurablePath::parse(path).map_err(|_| OfficeError::Invalid("invalid path"))?;
        let Some(kind) = crate::tasks::records::kind_of(&dp) else {
            return Err(OfficeError::Invalid(
                "satellite work change must only contain task records",
            ));
        };
        match kind {
            crate::tasks::records::Kind::Task(tid) => {
                let code =
                    validate_incoming_task_record(office, &tid, opt_bytes.as_deref(), &check)?;
                incoming_task_codes.insert(tid.as_str().to_owned(), code);
            }
            crate::tasks::records::Kind::Checklist(tid)
            | crate::tasks::records::Kind::Comment(tid, _)
            | crate::tasks::records::Kind::Operations(tid)
            | crate::tasks::records::Kind::Event(tid, _) => {
                deferred_sidecars.push(tid);
            }
        }
    }
    // Pass 2: validate task sidecars now that every incoming `Task` record has been registered.
    for tid in deferred_sidecars {
        if org_proto::is_uuid_v7(expected_record_id) && tid.as_str() != expected_record_id {
            return Err(OfficeError::Invalid(
                "task sidecar does not match WorkChange record_id",
            ));
        }
        if !incoming_task_codes.contains_key(tid.as_str()) {
            let existing = office
                .task_inspect(tid.as_str())
                .ok()
                .and_then(|r| r.ok())
                .ok_or(OfficeError::Invalid(
                    "task sidecar references unknown task on HQ",
                ))?;
            if !origin_codes.contains(&existing.task.office_code) {
                return Err(OfficeError::Invalid(
                    "satellite cannot mutate sidecar of another office's task",
                ));
            }
        }
    }
    Ok(())
}
