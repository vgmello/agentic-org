//! Domain-public operation primitives for the `tasks` family (`TASK-49`, `TASK-77`, `TASK-218`):
//! single-office write authority enforced atomically inside `UnitOfWork`, task lookup,
//! deterministic derived IDs, and durable `task.md`/child-file writers.
//!
//! Lives outside `tasks::features` so sibling domains and store adapters (`store/open/git_ops`,
//! `office/satellite`) never reach into `tasks::features`.

use std::collections::BTreeSet;

use org_proto::attachments::{AttachmentTargetKind, TASK_ATTACHMENTS_MAX};
use org_proto::ErrorCode;
use org_store::{Ack, Change, EngineError, Id, Refusal, Sha256};
use serde_json::Value;

use crate::attachments::records::AttachmentRecord;
use crate::error::{api_refusal, storage_api_refusal};
use crate::organization::ports::OrganizationRead;
use crate::organization::records::OfficeRole;
use crate::tasks::ports::TasksRead;
use crate::tasks::records::{self, EventInput, Location, TaskLinkRecord, TaskRecord};

/// Single-office task writer authority verified atomically inside a task mutation's `UnitOfWork`
/// against the live transaction view (`TASK-52` P3-#9 / P4-#6 / P5-#7, `TASK-192`, `TASK-218`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TaskWriteAuthority {
    /// Enforce local write authority for `office_id` against the live `UnitOfWork` view's
    /// `offices` and `office_codes` tables (`removed_offices` carries permanently revoked
    /// satellite IDs on HQ).
    Local {
        office_id: Id,
        role: OfficeRole,
        removed_offices: BTreeSet<String>,
    },
    /// No local-office restriction (test-only: for direct store-level tests without an `Office`).
    #[cfg(any(test, feature = "testkit"))]
    Unrestricted,
}

/// Evaluates whether `office_id` holds local write authority for a task numbered with
/// `task_office_code` against one consistent [`OrganizationRead`] view (live `UnitOfWork` or
/// snapshot).
///
/// - A satellite may only locally modify tasks whose `office_code` belongs to that satellite.
/// - HQ may modify its own tasks and recovered/orphaned tasks whose `office_code` is retired or
///   belongs to a removed satellite, refusing only tasks owned by an active enrolled satellite.
pub(crate) fn has_local_task_write_authority_in_view<R: OrganizationRead>(
    view: &R,
    office_id: &Id,
    role: OfficeRole,
    removed_offices: &BTreeSet<String>,
    task_office_code: &str,
) -> Result<bool, EngineError> {
    let codes = view.office_codes()?;
    if role == OfficeRole::Satellite {
        return Ok(codes
            .iter()
            .any(|c| c.office_id == *office_id && c.code == task_office_code));
    }
    let offices = view.offices()?;
    if offices.len() <= 1 {
        return Ok(true);
    }
    let owned_by_active_satellite = codes.iter().any(|c| {
        c.code == task_office_code
            && c.retired_at.is_none()
            && c.office_id != *office_id
            && offices.iter().any(|o| o.id == c.office_id)
            && !removed_offices.contains(c.office_id.as_str())
    });
    Ok(!owned_by_active_satellite)
}

/// Atomically enforces [`TaskWriteAuthority`] against `view` for `task` inside a `UnitOfWork`.
pub(crate) fn require_local_task_write_authority<R: OrganizationRead>(
    view: &R,
    authority: &TaskWriteAuthority,
    task: &TaskRecord,
) -> Result<(), Refusal> {
    let (office_id, role, removed_offices) = match authority {
        TaskWriteAuthority::Local {
            office_id,
            role,
            removed_offices,
        } => (office_id, *role, removed_offices),
        #[cfg(any(test, feature = "testkit"))]
        TaskWriteAuthority::Unrestricted => return Ok(()),
    };
    let allowed = has_local_task_write_authority_in_view(
        view,
        office_id,
        role,
        removed_offices,
        &task.office_code,
    )
    .map_err(storage_refusal)?;
    if allowed {
        return Ok(());
    }
    Err(api_refusal(
        ErrorCode::NotAuthorized,
        format!(
            "office {} cannot locally modify task {} owned by office code {}; submit edits to the owning office",
            office_id.as_str(),
            task.key(),
            task.office_code
        ),
    ))
}

pub(crate) fn storage_refusal(err: impl std::fmt::Display) -> Refusal {
    storage_api_refusal("task storage error", err)
}

pub(crate) fn validate_i64_u64(value: u64, field: &str) -> Result<u64, Refusal> {
    if value > i64::MAX as u64 {
        return Err(api_refusal(
            ErrorCode::InvalidRequest,
            format!("`{field}` exceeds maximum supported integer value"),
        ));
    }
    Ok(value)
}

pub(crate) fn require_task<R: TasksRead>(view: &R, reference: &str) -> Result<TaskRecord, Refusal> {
    view.resolve_task(reference)
        .map_err(storage_refusal)?
        .ok_or_else(|| api_refusal(ErrorCode::NotFound, format!("task `{reference}` not found")))
}

/// A UUIDv7-shaped ID derived from `parts` alone, so a retry derives the same one.
pub(crate) fn derived_id(parts: &[&[u8]]) -> Id {
    let digest = Sha256::of_parts(parts);
    let mut b = [0u8; 16];
    b.copy_from_slice(&digest.as_bytes()[..16]);
    b[6] = (b[6] & 0x0f) | 0x70;
    b[8] = (b[8] & 0x3f) | 0x80;
    let formatted = format!(
        "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7], b[8], b[9], b[10], b[11], b[12], b[13], b[14], b[15]
    );
    Id::parse(&formatted).expect("derived UUIDv7 id")
}

pub(crate) fn derive_link_id(source: &Id, target: &Id, salt: u32) -> Id {
    let salt_bytes = salt.to_be_bytes();
    derived_id(&[
        b"task-link-v1:",
        source.as_str().as_bytes(),
        b"->",
        target.as_str().as_bytes(),
        &salt_bytes,
    ])
}

/// Who changes a task, when, and for which command.
pub(crate) struct Writer<'a> {
    pub organization_id: Id,
    pub actor_id: &'a Id,
    pub at: &'a str,
    pub command_id: &'a str,
}

impl<'a> Writer<'a> {
    pub(crate) fn new<R: TasksRead>(
        view: &R,
        actor_id: &'a Id,
        at: &'a str,
        command_id: &'a str,
    ) -> Result<Self, Refusal> {
        let organization = view.organization().map_err(storage_refusal)?;
        let organization_id = organization
            .map(|o| o.id)
            .ok_or_else(|| api_refusal(ErrorCode::StorageError, "the organization is missing"))?;
        Ok(Self {
            organization_id,
            actor_id,
            at,
            command_id,
        })
    }

    fn event(&self, task_id: &Id, revision: u64, change: Value) -> Result<Vec<u8>, Refusal> {
        let (_, bytes) = records::encode_event(EventInput {
            organization_id: &self.organization_id,
            task_id,
            revision,
            occurred_at: self.at,
            actor_id: self.actor_id,
            operation_id: self.command_id,
            change,
        })
        .map_err(|e| api_refusal(ErrorCode::StorageError, format!("encode task event: {e}")))?;
        Ok(bytes)
    }

    /// `task.md` at `task.meta.revision` (already advanced), with `links` and `attachments`,
    /// and the event of that revision.
    pub(crate) fn task_and_event(
        &self,
        change: Change,
        location: &Location,
        task: &TaskRecord,
        links: &[TaskLinkRecord],
        attachments: &[AttachmentRecord],
        event: Value,
    ) -> Result<Change, Refusal> {
        let path = records::task_md_path(location, &task.id).map_err(too_deep)?;
        let bytes = records::encode_task_md(task, links, attachments)
            .map_err(|e| api_refusal(ErrorCode::TooLarge, e))?;
        let event = self.event(&task.id, task.meta.revision, event)?;
        Ok(change
            .put(path, bytes)
            .create(records::event_path(&task.id, task.meta.revision), event))
    }

    /// The next history event of a child-only change, without `task.md`: the task's
    /// effective revision advances to it. Returns the change and that revision.
    pub(crate) fn child_event(
        &self,
        change: Change,
        task: &TaskRecord,
        event: Value,
    ) -> Result<(Change, u64), Refusal> {
        let revision = validate_i64_u64(task.meta.revision + 1, "revision")?;
        let bytes = self.event(&task.id, revision, event)?;
        let path = records::event_path(&task.id, revision);
        Ok((change.create(path, bytes), revision))
    }
}

pub(crate) fn too_deep(_: org_store::StoreError) -> Refusal {
    api_refusal(
        ErrorCode::InvalidRequest,
        "the owning team is too deep in the team tree for task files",
    )
}

/// Where `task`'s folder is: its project's, or its owning team's chain.
pub(crate) fn task_location<R: TasksRead>(
    view: &R,
    task: &TaskRecord,
) -> Result<Location, Refusal> {
    if let Some(project) = &task.project_id {
        return Ok(Location::Project(project.clone()));
    }
    let chain = view.team_chain(&task.team_id).map_err(storage_refusal)?;
    chain.map(Location::Team).ok_or_else(|| {
        api_refusal(
            ErrorCode::StorageError,
            "the task's owning team has no chain to a top-level team",
        )
    })
}

/// The task's links as they stand, for a `task.md` rewrite that does not change them.
pub(crate) fn current_links<R: TasksRead>(
    view: &R,
    task: &TaskRecord,
) -> Result<Vec<TaskLinkRecord>, Refusal> {
    view.task_links_from(&task.id).map_err(storage_refusal)
}

/// The task's attachments as they stand, for a `task.md` rewrite that preserves or mutates them.
pub(crate) fn current_attachments<R: TasksRead>(
    view: &R,
    task: &TaskRecord,
) -> Result<Vec<AttachmentRecord>, Refusal> {
    view.attachments_for_target(
        AttachmentTargetKind::Task,
        Some(&task.id),
        None,
        TASK_ATTACHMENTS_MAX,
    )
    .map_err(storage_refusal)
}

/// A child file's bytes, or the refusal for a file over its bound.
pub(crate) fn child_bytes(encoded: Result<Vec<u8>, String>) -> Result<Vec<u8>, Refusal> {
    encoded.map_err(|e| api_refusal(ErrorCode::TooLarge, e))
}

/// A core mutation of `task`, already advanced to its next revision: `task.md` with the
/// task's links and attachments as they stand and that revision's event, answered `durable`
/// (`doc-79`).
pub(crate) fn rewrite_task<R: TasksRead>(
    view: &R,
    task: &TaskRecord,
    actor_id: &Id,
    at: &str,
    command_id: &str,
    event: Value,
) -> Result<Change, Refusal> {
    let location = task_location(view, task)?;
    let links = current_links(view, task)?;
    let attachments = current_attachments(view, task)?;
    Writer::new(view, actor_id, at, command_id)?.task_and_event(
        Change::new(Ack::Durable),
        &location,
        task,
        &links,
        &attachments,
        event,
    )
}
