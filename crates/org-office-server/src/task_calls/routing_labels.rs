//! Caller authorization and validation for `model:*` / `effort:*` task routing labels (`TASK-136`).

use std::collections::BTreeSet;

use org_office::organization_family::records::{EmployeeKind, EmployeeStatus};
use org_office::Office;
use org_proto::tasks::TasksUpdateParams;
use org_proto::{ApiError, ErrorCode};

const ROUTING_LABEL_DENIED: &str =
    "Only a person may set or change model:* or effort:* task routing labels.";

fn caller_may_edit_routing_labels(office: &Office, member: &str, via_grant: bool) -> bool {
    via_grant
        && office
            .load_employee(member)
            .ok()
            .flatten()
            .is_some_and(|e| e.kind == EmployeeKind::Person && e.status == EmployeeStatus::Active)
}

fn has_any_routing_label(labels: &[String]) -> bool {
    labels
        .iter()
        .any(|l| org_office_sessions::is_task_routing_label(l))
}

fn normalized_routing_labels(labels: &[String]) -> Vec<String> {
    labels
        .iter()
        .filter(|l| org_office_sessions::is_task_routing_label(l))
        .map(|l| l.trim().to_ascii_lowercase())
        .collect()
}

fn load_existing_task_labels(office: &Office, task_ref: &str) -> Option<(u64, Vec<String>)> {
    office
        .task_inspect(task_ref)
        .ok()
        .and_then(Result::ok)
        .map(|inspected| (inspected.task.revision, inspected.task.labels))
}

fn routing_labels_changed(
    has_existing: bool,
    existing_routing: &[String],
    rep_routing: &[String],
) -> bool {
    let existing_set: BTreeSet<&str> = existing_routing.iter().map(String::as_str).collect();
    let rep_set: BTreeSet<&str> = rep_routing.iter().map(String::as_str).collect();
    rep_set != existing_set
        || rep_routing.len() != existing_routing.len()
        || (!has_existing && !rep_routing.is_empty())
}

fn merge_incremental_labels(
    existing: &[String],
    add_labels: &[String],
    remove_labels: &[String],
) -> Vec<String> {
    let removed: BTreeSet<String> = remove_labels
        .iter()
        .map(|l| l.trim().to_ascii_lowercase())
        .collect();
    let mut merged: BTreeSet<String> = existing
        .iter()
        .map(|l| l.trim().to_ascii_lowercase())
        .filter(|l| !removed.contains(l))
        .collect();
    for add in add_labels {
        merged.insert(add.trim().to_ascii_lowercase());
    }
    merged.into_iter().collect()
}

fn validate_replacement_labels(
    existing_labels: Option<&[String]>,
    replacement: &[String],
    can_edit: bool,
) -> Result<(), ApiError> {
    let rep_routing = normalized_routing_labels(replacement);
    let existing_routing = normalized_routing_labels(existing_labels.unwrap_or(&[]));
    let changed =
        routing_labels_changed(existing_labels.is_some(), &existing_routing, &rep_routing);
    if changed && !can_edit {
        return Err(ApiError::new(
            ErrorCode::NotAuthorized,
            ROUTING_LABEL_DENIED,
        ));
    }
    let preserving_legacy = !changed
        && existing_labels
            .is_some_and(|ex| org_office_sessions::parse_task_routing_labels(ex).is_err());
    if !preserving_legacy {
        org_office_sessions::parse_task_routing_labels(replacement)?;
    }
    Ok(())
}

fn validate_incremental_labels(
    existing_labels: Option<&[String]>,
    add_labels: &[String],
    remove_labels: &[String],
) -> Result<(), ApiError> {
    if !has_any_routing_label(add_labels) {
        return Ok(());
    }
    org_office_sessions::parse_task_routing_labels(add_labels)?;
    if let Some(existing) = existing_labels {
        let merged = merge_incremental_labels(existing, add_labels, remove_labels);
        org_office_sessions::parse_task_routing_labels(&merged)?;
    }
    Ok(())
}

pub(super) fn validate_create_routing_labels(
    office: &Office,
    member: &str,
    via_grant: bool,
    labels: &[String],
) -> Result<(), ApiError> {
    if has_any_routing_label(labels) && !caller_may_edit_routing_labels(office, member, via_grant) {
        return Err(ApiError::new(
            ErrorCode::NotAuthorized,
            ROUTING_LABEL_DENIED,
        ));
    }
    org_office_sessions::parse_task_routing_labels(labels)?;
    Ok(())
}

pub(super) fn validate_update_routing_labels(
    office: &Office,
    member: &str,
    via_grant: bool,
    p: &TasksUpdateParams,
) -> Result<(), ApiError> {
    if p.labels.is_none() && p.add_labels.is_empty() && p.remove_labels.is_empty() {
        return Ok(());
    }
    let can_edit = caller_may_edit_routing_labels(office, member, via_grant);
    if (has_any_routing_label(&p.add_labels) || has_any_routing_label(&p.remove_labels))
        && !can_edit
    {
        return Err(ApiError::new(
            ErrorCode::NotAuthorized,
            ROUTING_LABEL_DENIED,
        ));
    }
    let existing = load_existing_task_labels(office, &p.task);
    let existing_labels = existing.as_ref().map(|(_, labels)| labels.as_slice());
    match p.labels.as_deref() {
        Some(rep) => validate_replacement_labels(existing_labels, rep, can_edit)?,
        None => validate_incremental_labels(existing_labels, &p.add_labels, &p.remove_labels)?,
    }
    // Pin the pre-check revision against `p.expected_revision` whenever a full label replacement
    // or routing-label addition depends on the existing label snapshot (`TASK-136` Pass 3):
    // - Missing task fails closed with `NotFound` (prevents racing a predicted task key at rev 1).
    // - `existing_rev < p.expected_revision` fails closed with `Conflict` (prevents guessing
    //   `R + 1` to strip a concurrent person's routing label or bypass duplicate-label checks).
    // - `existing_rev > p.expected_revision` still flows to `office.task_update` for its
    //   structured `stale_revision_refusal` (monotonically increasing revisions).
    if p.labels.is_some() || has_any_routing_label(&p.add_labels) {
        let Some((existing_rev, _)) = existing else {
            return Err(ApiError::new(ErrorCode::NotFound, "task not found"));
        };
        if existing_rev < p.expected_revision {
            return Err(ApiError::new(
                ErrorCode::Conflict,
                "expected revision is ahead of the current task revision; inspect the task and retry",
            ));
        }
    }
    Ok(())
}
