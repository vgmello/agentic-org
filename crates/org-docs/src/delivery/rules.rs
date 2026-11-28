//! Pure schema-aligned validation rules, DAG cycle guards, flag value-type checks,
//! used-flag immutability checks, and release/milestone progress calculations.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use org_proto::{format_rfc3339_ms, is_uuid_v7, parse_rfc3339_ms};
use serde_json::Value;

use super::records::{
    DeploymentOperation, DeploymentRecord, DeploymentStatus, EntityLinkRecord, FeatureFlagRecord,
    FeatureRecord, FeatureStatus, FlagStatus, FlagValueType, LinkEntityKind, LinkKind,
    MilestoneProgress, ReleaseProgress,
};
use crate::is_invisible_or_break;
use crate::project_tree::{is_body, DocStatus};

/// Maximum length of a slug (`local_slug` or flag `key`).
pub const SLUG_MAX_BYTES: usize = 64;
/// Maximum length of an environment or target name.
pub const ENV_TARGET_MAX_BYTES: usize = 64;
/// Maximum length of a title or label line in Unicode scalar values.
pub const LINE_MAX_CHARS: usize = 200;
/// Maximum length of a commit SHA string.
pub const COMMIT_SHA_MAX_BYTES: usize = 128;
/// Maximum length of an external URL or retired reason.
pub const DETAIL_TEXT_MAX_BYTES: usize = 1024;

/// A pure validation failure carrying `(field, reason)`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuleError {
    /// Offending field name.
    pub field: String,
    /// Human-readable reason.
    pub reason: String,
}

impl RuleError {
    /// Constructs a new rule error.
    pub fn new(field: impl Into<String>, reason: impl Into<String>) -> Self {
        Self {
            field: field.into(),
            reason: reason.into(),
        }
    }
}

impl fmt::Display for RuleError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.field.is_empty() {
            f.write_str(&self.reason)
        } else {
            write!(f, "{}: {}", self.field, self.reason)
        }
    }
}

impl std::error::Error for RuleError {}

/// Checks `schema.sql` slug constraint: `1..=64` bytes of `[a-z0-9-]`, starts and ends with
/// `[a-z0-9]`, and contains no `--`.
pub fn is_valid_slug(text: &str) -> bool {
    let bytes = text.as_bytes();
    if bytes.is_empty() || bytes.len() > SLUG_MAX_BYTES {
        return false;
    }
    let starts_ends_alnum = |b: u8| b.is_ascii_lowercase() || b.is_ascii_digit();
    starts_ends_alnum(bytes[0])
        && starts_ends_alnum(bytes[bytes.len() - 1])
        && bytes.iter().all(|&b| starts_ends_alnum(b) || b == b'-')
        && !text.contains("--")
}

/// Derives a valid lowercase slug from a title (`"feature"` fallback).
pub fn slug_from_title(title: &str) -> String {
    let mut out = String::new();
    for ch in title.chars() {
        if ch.is_ascii_alphanumeric() {
            if out.len() >= SLUG_MAX_BYTES {
                break;
            }
            out.push(ch.to_ascii_lowercase());
        } else if !out.is_empty() && !out.ends_with('-') && out.len() + 1 < SLUG_MAX_BYTES {
            out.push('-');
        }
    }
    while out.ends_with('-') {
        out.pop();
    }
    if out.is_empty() {
        "feature".to_owned()
    } else {
        out
    }
}

/// Formats the canonical qualified feature slug (`<project.slug>/<local_slug>`).
pub fn feature_qualified_slug(project_slug: &str, local_slug: &str) -> String {
    format!("{project_slug}/{local_slug}")
}

/// Checks one visible trimmed single-line text field (`1..=200` chars).
pub fn is_valid_line(text: &str) -> bool {
    !text.is_empty()
        && text.trim() == text
        && text.chars().count() <= LINE_MAX_CHARS
        && !text
            .chars()
            .any(|c| c.is_control() || is_invisible_or_break(c))
}

/// Checks a non-empty environment or target name (`1..=64` bytes, trimmed, printable single line).
pub fn is_valid_env_or_target(text: &str) -> bool {
    !text.is_empty()
        && text.len() <= ENV_TARGET_MAX_BYTES
        && text.trim() == text
        && !text
            .chars()
            .any(|c| c.is_control() || is_invisible_or_break(c))
}

/// Checks a repository commit SHA (`1..=128` ASCII graphic chars, no whitespace).
pub fn is_valid_commit_sha(text: &str) -> bool {
    !text.is_empty()
        && text.len() <= COMMIT_SHA_MAX_BYTES
        && text.bytes().all(|b| b.is_ascii_graphic())
}

/// Checks optional detail text (`external_url`, `retired_reason`).
pub fn is_valid_detail_text(text: &str) -> bool {
    !text.is_empty()
        && text.len() <= DETAIL_TEXT_MAX_BYTES
        && text.trim() == text
        && !text
            .chars()
            .any(|c| c.is_control() || is_invisible_or_break(c))
}

/// Parses an RFC 3339 timestamp and canonicalizes it to UTC millisecond precision
/// (`YYYY-MM-DDTHH:MM:SS.mmmZ`) so SQLite lexicographic CHECKs and UTC epoch-ms comparisons agree.
pub fn canonicalize_timestamp(field: &str, raw: &str) -> Result<String, RuleError> {
    let ms = parse_rfc3339_ms(raw.trim())
        .ok_or_else(|| RuleError::new(field, "must be an RFC 3339 instant"))?;
    format_rfc3339_ms(ms).ok_or_else(|| RuleError::new(field, "timestamp out of supported range"))
}

/// Canonicalizes an optional RFC 3339 timestamp (`None` or blank clears to `None`).
pub fn canonicalize_opt_timestamp(
    field: &str,
    raw: Option<&str>,
) -> Result<Option<String>, RuleError> {
    let Some(trimmed) = raw.map(str::trim).filter(|s| !s.is_empty()) else {
        return Ok(None);
    };
    canonicalize_timestamp(field, trimmed).map(Some)
}

/// Validates a JSON value against a feature flag's `value_type` (`schema.sql` CHECK & triggers).
pub fn validate_flag_value(value_type: FlagValueType, value: &Value) -> Result<(), RuleError> {
    let ok = match value_type {
        FlagValueType::Boolean => value.is_boolean(),
        FlagValueType::String => value.is_string(),
        FlagValueType::Number => value.is_number(),
        FlagValueType::Json => true,
    };
    if ok {
        Ok(())
    } else {
        Err(RuleError::new(
            "value",
            format!("flag value type mismatch: expected {}", value_type.as_str()),
        ))
    }
}

/// Validates optional `rollout_rules` (must be absent or a JSON object).
pub fn validate_rollout_rules(rules: Option<&Value>) -> Result<(), RuleError> {
    match rules {
        None | Some(Value::Object(_)) => Ok(()),
        Some(_) => Err(RuleError::new(
            "rollout_rules",
            "rollout_rules must be a JSON object",
        )),
    }
}

/// Enforces `used_flag_identity_immutable`: once referenced by a release or deployment snapshot,
/// a flag's `id`, `project_id`, `key`, and `value_type` cannot change.
pub fn check_used_flag_identity_unchanged(
    before: &FeatureFlagRecord,
    after: &FeatureFlagRecord,
    is_used: bool,
) -> Result<(), RuleError> {
    if !is_used {
        return Ok(());
    }
    if before.id != after.id
        || before.project_id != after.project_id
        || before.key != after.key
        || before.value_type != after.value_type
    {
        return Err(RuleError::new(
            "flag",
            "used flag identity (key, value_type) is immutable once referenced by a snapshot",
        ));
    }
    Ok(())
}

/// Validates single-record invariants on a canonical feature (`schema.sql` `features` CHECKs).
pub fn validate_feature_record(feature: &FeatureRecord) -> Result<(), RuleError> {
    if !is_uuid_v7(&feature.id) {
        return Err(RuleError::new("id", "must be a lowercase UUIDv7"));
    }
    if !is_uuid_v7(&feature.project_id) {
        return Err(RuleError::new("project_id", "must be a lowercase UUIDv7"));
    }
    if feature.number == 0 {
        return Err(RuleError::new("number", "must be greater than 0"));
    }
    if !is_valid_slug(&feature.local_slug) {
        return Err(RuleError::new(
            "local_slug",
            "must be 1..=64 lowercase letters, digits, or single `-`, starting and ending with a letter or digit",
        ));
    }
    if !is_valid_line(&feature.title) {
        return Err(RuleError::new(
            "title",
            "must be one non-empty trimmed line of at most 200 characters",
        ));
    }
    if !is_body(&feature.body_markdown) {
        return Err(RuleError::new(
            "body_markdown",
            "must be at most 256 KiB and contain no NUL bytes",
        ));
    }
    if matches!(
        feature.status,
        FeatureStatus::InProgress | FeatureStatus::Live
    ) && feature.owner_employee_id.is_none()
    {
        return Err(RuleError::new(
            "owner_employee_id",
            "an in_progress or live feature requires an owner_employee_id",
        ));
    }
    if let Some(ref ts) = feature.delivered_at {
        if parse_rfc3339_ms(ts).is_none() {
            return Err(RuleError::new(
                "delivered_at",
                "must be an RFC 3339 instant",
            ));
        }
    }
    if let Some(ref reason) = feature.retired_reason {
        if !is_valid_detail_text(reason) {
            return Err(RuleError::new(
                "retired_reason",
                "must be trimmed visible text of at most 1024 bytes",
            ));
        }
    }
    if feature.replaced_by_feature_id.as_deref() == Some(feature.id.as_str()) {
        return Err(RuleError::new(
            "replaced_by_feature_id",
            "a feature cannot replace itself",
        ));
    }
    Ok(())
}

/// Checks that `replaced_by_feature_id` pointers across all features form a DAG (no cycles).
pub fn check_feature_replacement_acyclic(features: &[FeatureRecord]) -> Result<(), RuleError> {
    let next_map: BTreeMap<&str, &str> = features
        .iter()
        .filter_map(|f| {
            f.replaced_by_feature_id
                .as_deref()
                .map(|target| (f.id.as_str(), target))
        })
        .collect();
    for &start in next_map.keys() {
        let mut seen = BTreeSet::new();
        let mut cur = start;
        while let Some(&next) = next_map.get(cur) {
            if !seen.insert(cur) || next == start {
                return Err(RuleError::new(
                    "replaced_by_feature_id",
                    "feature replacement cycle is not allowed",
                ));
            }
            cur = next;
        }
    }
    Ok(())
}

fn validate_deployment_operation(dep: &DeploymentRecord) -> Result<(), RuleError> {
    if dep.rollback_of_id.as_deref() == Some(dep.id.as_str()) {
        return Err(RuleError::new(
            "rollback_of_id",
            "a deployment cannot roll back itself",
        ));
    }
    if dep.baseline_deployment_id.as_deref() == Some(dep.id.as_str()) {
        return Err(RuleError::new(
            "baseline_deployment_id",
            "a deployment cannot be its own baseline",
        ));
    }
    match (dep.operation, dep.rollback_of_id.is_some()) {
        (DeploymentOperation::Deploy, true) => Err(RuleError::new(
            "rollback_of_id",
            "a deploy operation must not set rollback_of_id",
        )),
        (DeploymentOperation::Rollback, false) => Err(RuleError::new(
            "rollback_of_id",
            "a rollback operation requires rollback_of_id",
        )),
        _ => Ok(()),
    }
}

fn validate_deployment_timestamps(dep: &DeploymentRecord) -> Result<(), RuleError> {
    let parse_opt = |ts: &Option<String>, field: &'static str| {
        ts.as_deref()
            .map(|s| {
                let canonical = canonicalize_timestamp(field, s)?;
                let ms = parse_rfc3339_ms(&canonical)
                    .ok_or_else(|| RuleError::new(field, "must be an RFC 3339 instant"))?;
                Ok::<(u64, String), RuleError>((ms, canonical))
            })
            .transpose()
    };
    let started = parse_opt(&dep.started_at, "started_at")?;
    let finished = parse_opt(&dep.finished_at, "finished_at")?;
    let timestamps_ok = match dep.status {
        DeploymentStatus::Queued => started.is_none() && finished.is_none(),
        DeploymentStatus::Running => started.is_some() && finished.is_none(),
        DeploymentStatus::Succeeded | DeploymentStatus::Failed => {
            started.is_some() && finished.is_some()
        }
        DeploymentStatus::Cancelled => finished.is_some(),
    };
    if !timestamps_ok {
        return Err(RuleError::new(
            "status",
            "started_at/finished_at do not match deployment status requirements",
        ));
    }
    if let (Some((start_ms, start_utc)), Some((finish_ms, finish_utc))) = (&started, &finished) {
        if finish_ms < start_ms || finish_utc < start_utc {
            return Err(RuleError::new(
                "finished_at",
                "finished_at must not be earlier than started_at",
            ));
        }
    }
    Ok(())
}

/// Validates single-record invariants on a deployment (`schema.sql` `deployments` CHECKs).
pub fn validate_deployment_record(dep: &DeploymentRecord) -> Result<(), RuleError> {
    for (field, val) in [
        ("id", dep.id.as_str()),
        ("project_id", dep.project_id.as_str()),
        ("release_id", dep.release_id.as_str()),
    ] {
        if !is_uuid_v7(val) {
            return Err(RuleError::new(field, "must be a lowercase UUIDv7"));
        }
    }
    for (field, val) in [
        ("environment", dep.environment.as_str()),
        ("target", dep.target.as_str()),
    ] {
        if !is_valid_env_or_target(val) {
            return Err(RuleError::new(
                field,
                "must be 1..=64 bytes of trimmed single-line text",
            ));
        }
    }
    if !is_body(&dep.body_markdown) {
        return Err(RuleError::new(
            "body_markdown",
            "must be at most 256 KiB and contain no NUL bytes",
        ));
    }
    validate_deployment_operation(dep)?;
    validate_deployment_timestamps(dep)?;
    if let Some(ref url) = dep.external_url {
        if !is_valid_detail_text(url) {
            return Err(RuleError::new(
                "external_url",
                "must be trimmed visible text of at most 1024 bytes",
            ));
        }
    }
    Ok(())
}

/// Checks that `rollback_of_id` pointers across deployments form a DAG (no cycles).
pub fn check_deployment_rollback_acyclic(
    deployments: &[DeploymentRecord],
) -> Result<(), RuleError> {
    let next_map: BTreeMap<&str, &str> = deployments
        .iter()
        .filter_map(|d| {
            d.rollback_of_id
                .as_deref()
                .map(|target| (d.id.as_str(), target))
        })
        .collect();
    for &start in next_map.keys() {
        let mut seen = BTreeSet::new();
        let mut cur = start;
        while let Some(&next) = next_map.get(cur) {
            if !seen.insert(cur) || next == start {
                return Err(RuleError::new(
                    "rollback_of_id",
                    "deployment rollback cycle is not allowed",
                ));
            }
            cur = next;
        }
    }
    Ok(())
}

/// Validates single-record invariants on a feature flag (`schema.sql` `feature_flags` CHECKs).
pub fn validate_flag_record(flag: &FeatureFlagRecord) -> Result<(), RuleError> {
    if !is_uuid_v7(&flag.id) {
        return Err(RuleError::new("id", "must be a lowercase UUIDv7"));
    }
    if !is_uuid_v7(&flag.project_id) {
        return Err(RuleError::new("project_id", "must be a lowercase UUIDv7"));
    }
    if !is_valid_slug(&flag.key) {
        return Err(RuleError::new(
            "key",
            "must be 1..=64 lowercase letters, digits, or single `-`, starting and ending with a letter or digit",
        ));
    }
    if flag.status == FlagStatus::Active && flag.owner_employee_id.is_none() {
        return Err(RuleError::new(
            "owner_employee_id",
            "an active feature flag requires an owner_employee_id",
        ));
    }
    validate_flag_value(flag.value_type, &flag.default_value)?;
    if !is_body(&flag.body_markdown) {
        return Err(RuleError::new(
            "body_markdown",
            "must be at most 256 KiB and contain no NUL bytes",
        ));
    }
    Ok(())
}

/// Validates a scoped release/deployment changelog (`documents` CHECKs).
pub fn validate_changelog_invariants(
    title: &str,
    status: DocStatus,
    owner_employee_id: Option<&str>,
    body_markdown: &str,
    has_release: bool,
    has_deployment: bool,
) -> Result<(), RuleError> {
    if has_release == has_deployment {
        return Err(RuleError::new(
            "scope",
            "a changelog must belong to exactly one release or deployment",
        ));
    }
    if !is_valid_line(title) {
        return Err(RuleError::new(
            "title",
            "must be one non-empty trimmed line of at most 200 characters",
        ));
    }
    if status == DocStatus::Current && owner_employee_id.is_none() {
        return Err(RuleError::new(
            "owner_employee_id",
            "a current changelog requires an owner_employee_id",
        ));
    }
    if !is_body(body_markdown) {
        return Err(RuleError::new(
            "body_markdown",
            "must be at most 256 KiB and contain no NUL bytes",
        ));
    }
    Ok(())
}

/// Validates a Why link against `schema.sql` `links` CHECK constraints.
pub fn validate_entity_link(link: &EntityLinkRecord) -> Result<(), RuleError> {
    if !is_uuid_v7(&link.id) {
        return Err(RuleError::new("id", "must be a lowercase UUIDv7"));
    }
    if !is_uuid_v7(&link.project_id) {
        return Err(RuleError::new("project_id", "must be a lowercase UUIDv7"));
    }
    if !link.source_kind.is_valid_source() {
        return Err(RuleError::new(
            "source_kind",
            "must be one of: project, feature, task, document",
        ));
    }
    if !is_uuid_v7(&link.source_id) {
        return Err(RuleError::new("source_id", "must be a lowercase UUIDv7"));
    }
    if !is_uuid_v7(&link.target_id) {
        return Err(RuleError::new("target_id", "must be a lowercase UUIDv7"));
    }
    if link.source_kind == link.target_kind && link.source_id == link.target_id {
        return Err(RuleError::new(
            "target_id",
            "an entity cannot link to itself",
        ));
    }
    if matches!(link.kind, LinkKind::DependsOn | LinkKind::Fixes)
        && !(link.source_kind == LinkEntityKind::Task && link.target_kind == LinkEntityKind::Task)
    {
        return Err(RuleError::new(
            "kind",
            "depends_on and fixes links require source_kind=task and target_kind=task",
        ));
    }
    Ok(())
}

/// Computes `milestone_progress` from an iterator of `(is_done_or_cancelled)` flags for milestone tasks.
pub fn compute_milestone_progress<I>(task_done_flags: I) -> MilestoneProgress
where
    I: IntoIterator<Item = bool>,
{
    let mut task_count = 0u64;
    let mut done_task_count = 0u64;
    for done in task_done_flags {
        task_count += 1;
        if done {
            done_task_count += 1;
        }
    }
    MilestoneProgress {
        task_count,
        done_task_count,
        is_done: task_count > 0 && task_count == done_task_count,
    }
}

/// Computes `release_progress` matching `schema.sql` `release_progress` view:
/// deduplicates tasks across `release_tasks` and milestone-assigned tasks, and requires all
/// milestones (if any) to be done and `task_count > 0 && task_count == done_task_count`.
pub fn compute_release_progress(
    milestone_progresses: &[MilestoneProgress],
    member_tasks: &[(String, bool)],
) -> ReleaseProgress {
    let mut unique_tasks: BTreeMap<&str, bool> = BTreeMap::new();
    for (task_id, is_done) in member_tasks {
        unique_tasks.insert(task_id.as_str(), *is_done);
    }
    let task_count = unique_tasks.len() as u64;
    let done_task_count = unique_tasks.values().filter(|&&d| d).count() as u64;
    let milestone_count = milestone_progresses.len() as u64;
    let all_milestones_done = milestone_progresses.iter().all(|m| m.is_done);
    ReleaseProgress {
        milestone_count,
        task_count,
        done_task_count,
        is_done: task_count > 0 && task_count == done_task_count && all_milestones_done,
    }
}
