//! Engine-independent port and shared helpers for pure delivery reads, writes, and validation.

use std::collections::BTreeSet;

use org_proto::delivery::{FlagUsageRefWire, RepoCommitWire};
use org_proto::{ApiError, ErrorCode};
use serde::Serialize;

use super::records::{
    ActorRef, DeploymentRecord, EntityLinkRecord, FeatureFlagRecord, FeatureRecord, LinkEntityKind,
    MilestoneProgress, MilestoneRecord, NumberAllocation, NumberCounterRecord, RecordMeta,
    ReleaseProgress, ReleaseRecord, RepoCommitRecord, ScopedChangelogRecord,
};
use super::rules::{
    canonicalize_timestamp, compute_milestone_progress, compute_release_progress,
    is_valid_commit_sha, RuleError,
};
use crate::project_tree::DocStatus;

/// Result of a pure delivery write decision, ready to commit into any store journal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeliveryMutation {
    /// Durable `(path, bytes)` files to write.
    pub puts: Vec<(String, Vec<u8>)>,
    /// Durable paths to remove.
    pub removes: Vec<String>,
    /// Serialized JSON reply payload.
    pub reply: Vec<u8>,
}

/// Authorized caller & project context passed into every delivery operation.
#[derive(Debug, Clone, Copy)]
pub struct CallerContext<'a> {
    /// Active caller employee UUIDv7.
    pub caller_id: &'a str,
    /// Authorized project UUIDv7.
    pub project_id: &'a str,
    /// Authorized project slug.
    pub project_slug: &'a str,
    /// RFC 3339 timestamp for mutations.
    pub now_rfc3339: &'a str,
    /// Milliseconds since epoch for deterministic UUIDv7 derivation.
    pub now_ms: u64,
}

/// Minimal task summary needed by delivery rollups and release/milestone progress.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskSummary {
    /// Task UUIDv7.
    pub id: String,
    /// Optional owning project UUIDv7.
    pub project_id: Option<String>,
    /// Optional canonical feature UUIDv7.
    pub feature_id: Option<String>,
    /// Optional release milestone UUIDv7.
    pub milestone_id: Option<String>,
    /// Optional affected feature UUIDv7 (`task_details.affects_feature_id`).
    pub affects_feature_id: Option<String>,
    /// True iff task status is `done` or `cancelled`.
    pub is_done: bool,
}

/// Failure from a delivery read or write decision.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeliveryOpError {
    /// Caller-facing API refusal.
    Refused(ApiError),
    /// Underlying storage error message.
    Storage(String),
}

impl From<ApiError> for DeliveryOpError {
    fn from(err: ApiError) -> Self {
        Self::Refused(err)
    }
}

/// Pure storage view consumed by `org-docs` delivery operations and rebuild validator.
pub trait DeliveryStoreView {
    /// All canonical features in `project_id`.
    fn project_features(&self, project_id: &str) -> Result<Vec<FeatureRecord>, String>;
    /// One feature by UUIDv7.
    fn feature(&self, id: &str) -> Result<Option<FeatureRecord>, String>;
    /// All releases in `project_id`.
    fn project_releases(&self, project_id: &str) -> Result<Vec<ReleaseRecord>, String>;
    /// One release by UUIDv7.
    fn release(&self, id: &str) -> Result<Option<ReleaseRecord>, String>;
    /// All milestones belonging to `release_id`.
    fn release_milestones(&self, release_id: &str) -> Result<Vec<MilestoneRecord>, String>;
    /// All milestones belonging to `project_id` (including orphans for rebuild checks).
    fn project_milestones(&self, project_id: &str) -> Result<Vec<MilestoneRecord>, String>;
    /// One milestone by UUIDv7.
    fn milestone(&self, id: &str) -> Result<Option<MilestoneRecord>, String>;
    /// All deployments in `project_id`.
    fn project_deployments(&self, project_id: &str) -> Result<Vec<DeploymentRecord>, String>;
    /// One deployment by UUIDv7.
    fn deployment(&self, id: &str) -> Result<Option<DeploymentRecord>, String>;
    /// All feature flags in `project_id`.
    fn project_flags(&self, project_id: &str) -> Result<Vec<FeatureFlagRecord>, String>;
    /// Flag IDs currently referenced by any release or deployment snapshot in `project_id`.
    fn used_flag_ids(&self, project_id: &str) -> Result<BTreeSet<String>, String>;
    /// Optional release changelog document.
    fn release_changelog(&self, release_id: &str) -> Result<Option<ScopedChangelogRecord>, String>;
    /// Optional deployment changelog document.
    fn deployment_changelog(
        &self,
        deployment_id: &str,
    ) -> Result<Option<ScopedChangelogRecord>, String>;
    /// All scoped release/deployment changelogs in `project_id` (including orphans).
    fn project_changelogs(&self, project_id: &str) -> Result<Vec<ScopedChangelogRecord>, String>;
    /// All Why links in `project_id`.
    fn project_links(&self, project_id: &str) -> Result<Vec<EntityLinkRecord>, String>;
    /// Scoped monotonic counter for `(scope_kind, scope_id, entity_kind)`.
    fn counter(
        &self,
        scope_kind: &str,
        scope_id: &str,
        entity_kind: &str,
    ) -> Result<Option<NumberCounterRecord>, String>;
    /// Whether employee `id` exists (for historical attribution during rebuild).
    fn employee_exists(&self, id: &str) -> Result<bool, String>;
    /// Whether employee `id` exists and has `active` status (for assigning new owners).
    fn employee_is_active(&self, id: &str) -> Result<bool, String>;
    /// Whether repository `id` exists.
    fn repository_exists(&self, id: &str) -> Result<bool, String>;
    /// All tasks belonging to `project_id`.
    fn project_tasks(&self, project_id: &str) -> Result<Vec<TaskSummary>, String>;
    /// Whether an entity `(kind, id)` exists within `project_id` (or org for processes).
    fn entity_exists_in_project(
        &self,
        project_id: &str,
        kind: LinkEntityKind,
        id: &str,
    ) -> Result<bool, String>;
    /// Derives a deterministic UUIDv7 from `(domain, command_id, now_ms)`.
    fn derive_uuid_v7(&self, domain: &str, command_id: &str, now_ms: u64) -> String;
}

/// Constructs a refused [`DeliveryOpError`].
pub fn refused(code: ErrorCode, msg: impl Into<String>) -> DeliveryOpError {
    DeliveryOpError::Refused(ApiError::new(code, msg.into()))
}

/// Constructs an `InvalidRequest` refusal naming `field`.
pub fn invalid(field: &str, msg: impl Into<String>) -> DeliveryOpError {
    refused(
        ErrorCode::InvalidRequest,
        format!("{field}: {}", msg.into()),
    )
}

/// Converts a [`RuleError`] to `InvalidRequest`.
pub fn rule_err(err: RuleError) -> DeliveryOpError {
    refused(ErrorCode::InvalidRequest, err.to_string())
}

/// Maps a storage string error into [`DeliveryOpError::Storage`].
pub fn stor<T>(r: Result<T, String>) -> Result<T, DeliveryOpError> {
    r.map_err(DeliveryOpError::Storage)
}

/// Validates and trims a lowercase UUIDv7 string.
pub fn parse_uuid(field: &str, raw: &str) -> Result<String, DeliveryOpError> {
    let trimmed = raw.trim();
    if !org_proto::is_uuid_v7(trimmed) {
        return Err(invalid(field, "must be a lowercase UUIDv7"));
    }
    Ok(trimmed.to_owned())
}

/// Checks optimistic concurrency revision.
pub fn check_revision(current: u64, expected: u64) -> Result<(), DeliveryOpError> {
    if current == expected {
        Ok(())
    } else {
        Err(refused(
            ErrorCode::Conflict,
            format!("revision conflict: current revision is {current}, expected {expected}"),
        ))
    }
}

/// Creates initial [`RecordMeta`] for a newly created record.
pub fn new_meta(ctx: &CallerContext<'_>) -> RecordMeta {
    let ts = Some(ctx.now_rfc3339.to_owned());
    let actor = ActorRef::Employee(ctx.caller_id.to_owned());
    RecordMeta {
        schema_version: 1,
        revision: 1,
        created_at: ts.clone(),
        created_by: actor.clone(),
        updated_at: ts,
        updated_by: actor,
    }
}

/// Advances [`RecordMeta`] on update.
pub fn touch_meta(meta: &mut RecordMeta, ctx: &CallerContext<'_>) {
    meta.revision += 1;
    meta.updated_at = Some(ctx.now_rfc3339.to_owned());
    meta.updated_by = ActorRef::Employee(ctx.caller_id.to_owned());
}

/// Encodes a serializable wire answer to JSON bytes.
pub fn reply<T: Serialize>(value: &T) -> Vec<u8> {
    serde_json::to_vec(value).expect("delivery reply encodes")
}

/// Allocates the next monotonic number in a scoped counter.
pub fn allocate_number<V: DeliveryStoreView>(
    view: &V,
    scope_kind: &str,
    scope_id: &str,
    entity_kind: &str,
    entity_id: &str,
    allocated_at: &str,
) -> Result<(u64, NumberCounterRecord), DeliveryOpError> {
    let allocated_at = canonicalize_timestamp("allocated_at", allocated_at).map_err(rule_err)?;
    let mut counter =
        stor(view.counter(scope_kind, scope_id, entity_kind))?.unwrap_or(NumberCounterRecord {
            next_number: 1,
            allocations: Vec::new(),
        });
    let num = counter.next_number;
    counter.next_number = num
        .checked_add(1)
        .ok_or_else(|| refused(ErrorCode::StorageError, "sequence counter overflow"))?;
    counter.allocations.push(NumberAllocation {
        number: num,
        entity_id: entity_id.to_owned(),
        allocated_at,
    });
    Ok((num, counter))
}

/// Validates an optional owner employee ID (empty clears; non-empty requires an active employee).
pub fn validate_owner<V: DeliveryStoreView>(
    view: &V,
    raw: Option<&str>,
) -> Result<Option<String>, DeliveryOpError> {
    let Some(raw) = raw else {
        return Ok(None);
    };
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }
    let id = parse_uuid("owner_employee_id", trimmed)?;
    if !stor(view.employee_is_active(&id))? {
        return Err(invalid(
            "owner_employee_id",
            "must be an active employee of the organization",
        ));
    }
    Ok(Some(id))
}

/// Resolves changelog owner: clearing `owner_employee_id` (`""`) on a `Current` changelog is
/// refused (matching `docs.update`), while an omitted owner defaults to the caller on `Current`.
pub fn resolve_changelog_owner<V: DeliveryStoreView>(
    view: &V,
    ctx: &CallerContext<'_>,
    status: DocStatus,
    param_owner: Option<&str>,
    existing_owner: Option<&str>,
) -> Result<Option<String>, DeliveryOpError> {
    let mut owner = match param_owner {
        Some(raw) => {
            let parsed = validate_owner(view, Some(raw))?;
            if parsed.is_none() && status == DocStatus::Current {
                return Err(invalid(
                    "owner_employee_id",
                    "a current changelog keeps an owner: name another, or change its status first",
                ));
            }
            parsed
        }
        None => existing_owner.map(str::to_owned),
    };
    if status == DocStatus::Current && owner.is_none() {
        owner = Some(ctx.caller_id.to_owned());
    }
    Ok(owner)
}

/// Validates and deduplicates pinned repository commits.
pub fn validate_commits<V: DeliveryStoreView>(
    view: &V,
    commits: &[RepoCommitWire],
) -> Result<Vec<RepoCommitRecord>, DeliveryOpError> {
    let mut seen = BTreeSet::new();
    let mut out = Vec::with_capacity(commits.len());
    for c in commits {
        let rid = parse_uuid("repository_id", &c.repository_id)?;
        if !stor(view.repository_exists(&rid))? {
            return Err(refused(
                ErrorCode::NotFound,
                format!("repository `{rid}` not found"),
            ));
        }
        if !seen.insert(rid.clone()) {
            return Err(invalid(
                "repository_commits",
                format!("duplicate repository_id `{rid}`"),
            ));
        }
        let sha = c.commit_sha.trim();
        if !is_valid_commit_sha(sha) {
            return Err(invalid("commit_sha", "must be 1..=128 graphic ASCII bytes"));
        }
        out.push(RepoCommitRecord {
            repository_id: rid,
            commit_sha: sha.to_owned(),
        });
    }
    out.sort();
    Ok(out)
}

/// Resolves a feature by UUIDv7, `F-<n>`, `local_slug`, or `<project.slug>/<local_slug>`.
pub fn resolve_feature<V: DeliveryStoreView>(
    view: &V,
    project_id: &str,
    project_slug: &str,
    reference: &str,
) -> Result<FeatureRecord, DeliveryOpError> {
    let trimmed = reference.trim();
    let upper = trimmed.to_ascii_uppercase();
    let num_match = upper.strip_prefix("F-").and_then(|n| n.parse::<u64>().ok());
    let local_from_qualified = trimmed
        .strip_prefix(project_slug)
        .and_then(|s| s.strip_prefix('/'));
    stor(view.project_features(project_id))?
        .into_iter()
        .find(|f| {
            f.id == trimmed
                || num_match == Some(f.number)
                || f.local_slug == trimmed
                || local_from_qualified == Some(f.local_slug.as_str())
        })
        .ok_or_else(|| refused(ErrorCode::NotFound, "feature not found"))
}

/// Resolves a release by UUIDv7 or `R-<n>`.
pub fn resolve_release<V: DeliveryStoreView>(
    view: &V,
    project_id: &str,
    reference: &str,
) -> Result<ReleaseRecord, DeliveryOpError> {
    let trimmed = reference.trim();
    let upper = trimmed.to_ascii_uppercase();
    let num_match = upper.strip_prefix("R-").and_then(|n| n.parse::<u64>().ok());
    stor(view.project_releases(project_id))?
        .into_iter()
        .find(|r| r.id == trimmed || num_match == Some(r.number))
        .ok_or_else(|| refused(ErrorCode::NotFound, "release not found"))
}

/// Resolves a feature flag by UUIDv7 or key.
pub fn resolve_flag<V: DeliveryStoreView>(
    view: &V,
    project_id: &str,
    reference: &str,
) -> Result<FeatureFlagRecord, DeliveryOpError> {
    let trimmed = reference.trim();
    stor(view.project_flags(project_id))?
        .into_iter()
        .find(|f| f.id == trimmed || f.key == trimmed)
        .ok_or_else(|| refused(ErrorCode::NotFound, "feature flag not found"))
}

/// Computes authoritative milestone progress for one milestone.
pub fn milestone_progress_for<V: DeliveryStoreView>(
    view: &V,
    project_id: &str,
    milestone_id: &str,
) -> Result<MilestoneProgress, DeliveryOpError> {
    let tasks = stor(view.project_tasks(project_id))?;
    Ok(compute_milestone_progress(tasks.iter().filter_map(|t| {
        (t.milestone_id.as_deref() == Some(milestone_id)).then_some(t.is_done)
    })))
}

/// Computes authoritative release progress and per-milestone progress.
pub fn release_progress_for<V: DeliveryStoreView>(
    view: &V,
    release: &ReleaseRecord,
) -> Result<(ReleaseProgress, Vec<(MilestoneRecord, MilestoneProgress)>), DeliveryOpError> {
    let milestones = stor(view.release_milestones(&release.id))?;
    let tasks = stor(view.project_tasks(&release.project_id))?;
    let ms_ids: BTreeSet<String> = milestones.iter().map(|m| m.id.clone()).collect();
    let rel_task_ids: BTreeSet<&str> = release.task_ids.iter().map(String::as_str).collect();
    let mut ms_out = Vec::with_capacity(milestones.len());
    let mut ms_progs = Vec::with_capacity(milestones.len());
    for m in milestones {
        let mp = compute_milestone_progress(tasks.iter().filter_map(|t| {
            (t.milestone_id.as_deref() == Some(m.id.as_str())).then_some(t.is_done)
        }));
        ms_progs.push(mp);
        ms_out.push((m, mp));
    }
    let member_tasks: Vec<(String, bool)> = tasks
        .iter()
        .filter(|t| {
            rel_task_ids.contains(t.id.as_str())
                || t.milestone_id
                    .as_deref()
                    .is_some_and(|mid| ms_ids.contains(mid))
        })
        .map(|t| (t.id.clone(), t.is_done))
        .collect();
    Ok((compute_release_progress(&ms_progs, &member_tasks), ms_out))
}

/// Lists all release & deployment snapshot usages of `flag_id`.
pub fn flag_usages<V: DeliveryStoreView>(
    view: &V,
    project_id: &str,
    flag_id: &str,
) -> Result<Vec<FlagUsageRefWire>, DeliveryOpError> {
    let mut usages = Vec::new();
    for r in stor(view.project_releases(project_id))? {
        for s in r.flag_settings {
            if s.flag_id == flag_id {
                usages.push(FlagUsageRefWire {
                    scope_kind: "release".into(),
                    scope_id: r.id.clone(),
                    environment: Some(s.environment),
                    target: Some(s.target),
                });
            }
        }
    }
    for d in stor(view.project_deployments(project_id))? {
        for s in d.flag_settings {
            if s.flag_id == flag_id {
                usages.push(FlagUsageRefWire {
                    scope_kind: "deployment".into(),
                    scope_id: d.id.clone(),
                    environment: Some(d.environment.clone()),
                    target: Some(d.target.clone()),
                });
            }
        }
    }
    Ok(usages)
}

/// Checks that a rollback or baseline deployment exists in the same `(project, env, target)`.
pub fn check_deployment_parent(
    all: &[DeploymentRecord],
    parent_id: Option<&str>,
    field: &str,
    env: &str,
    target: &str,
) -> Result<(), DeliveryOpError> {
    let Some(pid) = parent_id else {
        return Ok(());
    };
    let Some(parent) = all.iter().find(|d| d.id == pid) else {
        return Err(refused(ErrorCode::NotFound, format!("{field} not found")));
    };
    if parent.environment != env || parent.target != target {
        return Err(invalid(
            field,
            "referenced deployment must share project, environment, and target",
        ));
    }
    Ok(())
}
