//! Wire <-> pure domain conversions between `org_proto::delivery` and `org_docs::delivery`.

use org_proto::delivery::*;
use org_proto::docs::DocStatusWire;

use super::records::{
    DeploymentOperation, DeploymentRecord, DeploymentStatus, EntityLinkRecord, FeatureFlagRecord,
    FeatureRecord, FeatureStatus, FeatureWorkType, FlagStatus, FlagValueType, LinkEntityKind,
    LinkKind, MilestoneProgress, MilestoneRecord, ReleaseProgress, ReleaseRecord, RepoCommitRecord,
    ScopedChangelogRecord,
};
use super::rules::{feature_qualified_slug, RuleError};
use crate::project_tree::DocStatus;

/// Converts [`DocStatusWire`] to [`DocStatus`].
pub fn doc_status_from_wire(wire: DocStatusWire) -> Result<DocStatus, RuleError> {
    match wire {
        DocStatusWire::Draft => Ok(DocStatus::Draft),
        DocStatusWire::Current => Ok(DocStatus::Current),
        DocStatusWire::Deprecated => Ok(DocStatus::Deprecated),
        DocStatusWire::Unknown => Err(RuleError::new(
            "status",
            "expected draft, current, or deprecated",
        )),
    }
}

/// Converts [`DocStatus`] to [`DocStatusWire`].
pub const fn doc_status_to_wire(status: DocStatus) -> DocStatusWire {
    match status {
        DocStatus::Draft => DocStatusWire::Draft,
        DocStatus::Current => DocStatusWire::Current,
        DocStatus::Deprecated => DocStatusWire::Deprecated,
    }
}

/// Converts [`FeatureStatusWire`] to [`FeatureStatus`].
pub fn feature_status_from_wire(wire: FeatureStatusWire) -> Result<FeatureStatus, RuleError> {
    match wire {
        FeatureStatusWire::Planned => Ok(FeatureStatus::Planned),
        FeatureStatusWire::InProgress => Ok(FeatureStatus::InProgress),
        FeatureStatusWire::Live => Ok(FeatureStatus::Live),
        FeatureStatusWire::Retired => Ok(FeatureStatus::Retired),
        FeatureStatusWire::Unknown => Err(RuleError::new(
            "status",
            "expected planned, in_progress, live, or retired",
        )),
    }
}

/// Converts [`FeatureStatus`] to [`FeatureStatusWire`].
pub const fn feature_status_to_wire(status: FeatureStatus) -> FeatureStatusWire {
    match status {
        FeatureStatus::Planned => FeatureStatusWire::Planned,
        FeatureStatus::InProgress => FeatureStatusWire::InProgress,
        FeatureStatus::Live => FeatureStatusWire::Live,
        FeatureStatus::Retired => FeatureStatusWire::Retired,
    }
}

/// Converts [`FeatureWorkTypeWire`] to [`FeatureWorkType`].
pub fn feature_work_type_from_wire(
    wire: FeatureWorkTypeWire,
) -> Result<FeatureWorkType, RuleError> {
    match wire {
        FeatureWorkTypeWire::Build => Ok(FeatureWorkType::Build),
        FeatureWorkTypeWire::Run => Ok(FeatureWorkType::Run),
        FeatureWorkTypeWire::Unknown => Err(RuleError::new("work_type", "expected build or run")),
    }
}

/// Converts [`FeatureWorkType`] to [`FeatureWorkTypeWire`].
pub const fn feature_work_type_to_wire(wt: FeatureWorkType) -> FeatureWorkTypeWire {
    match wt {
        FeatureWorkType::Build => FeatureWorkTypeWire::Build,
        FeatureWorkType::Run => FeatureWorkTypeWire::Run,
    }
}

/// Converts [`DeploymentOperationWire`] to [`DeploymentOperation`].
pub fn deployment_op_from_wire(
    wire: DeploymentOperationWire,
) -> Result<DeploymentOperation, RuleError> {
    match wire {
        DeploymentOperationWire::Deploy => Ok(DeploymentOperation::Deploy),
        DeploymentOperationWire::Rollback => Ok(DeploymentOperation::Rollback),
        DeploymentOperationWire::Unknown => {
            Err(RuleError::new("operation", "expected deploy or rollback"))
        }
    }
}

/// Converts [`DeploymentOperation`] to [`DeploymentOperationWire`].
pub const fn deployment_op_to_wire(op: DeploymentOperation) -> DeploymentOperationWire {
    match op {
        DeploymentOperation::Deploy => DeploymentOperationWire::Deploy,
        DeploymentOperation::Rollback => DeploymentOperationWire::Rollback,
    }
}

/// Converts [`DeploymentStatusWire`] to [`DeploymentStatus`].
pub fn deployment_status_from_wire(
    wire: DeploymentStatusWire,
) -> Result<DeploymentStatus, RuleError> {
    match wire {
        DeploymentStatusWire::Queued => Ok(DeploymentStatus::Queued),
        DeploymentStatusWire::Running => Ok(DeploymentStatus::Running),
        DeploymentStatusWire::Succeeded => Ok(DeploymentStatus::Succeeded),
        DeploymentStatusWire::Failed => Ok(DeploymentStatus::Failed),
        DeploymentStatusWire::Cancelled => Ok(DeploymentStatus::Cancelled),
        DeploymentStatusWire::Unknown => Err(RuleError::new(
            "status",
            "expected queued, running, succeeded, failed, or cancelled",
        )),
    }
}

/// Converts [`DeploymentStatus`] to [`DeploymentStatusWire`].
pub const fn deployment_status_to_wire(status: DeploymentStatus) -> DeploymentStatusWire {
    match status {
        DeploymentStatus::Queued => DeploymentStatusWire::Queued,
        DeploymentStatus::Running => DeploymentStatusWire::Running,
        DeploymentStatus::Succeeded => DeploymentStatusWire::Succeeded,
        DeploymentStatus::Failed => DeploymentStatusWire::Failed,
        DeploymentStatus::Cancelled => DeploymentStatusWire::Cancelled,
    }
}

/// Converts [`FlagStatusWire`] to [`FlagStatus`].
pub fn flag_status_from_wire(wire: FlagStatusWire) -> Result<FlagStatus, RuleError> {
    match wire {
        FlagStatusWire::Draft => Ok(FlagStatus::Draft),
        FlagStatusWire::Active => Ok(FlagStatus::Active),
        FlagStatusWire::Retired => Ok(FlagStatus::Retired),
        FlagStatusWire::Unknown => Err(RuleError::new(
            "status",
            "expected draft, active, or retired",
        )),
    }
}

/// Converts [`FlagStatus`] to [`FlagStatusWire`].
pub const fn flag_status_to_wire(status: FlagStatus) -> FlagStatusWire {
    match status {
        FlagStatus::Draft => FlagStatusWire::Draft,
        FlagStatus::Active => FlagStatusWire::Active,
        FlagStatus::Retired => FlagStatusWire::Retired,
    }
}

/// Converts [`FlagValueTypeWire`] to [`FlagValueType`].
pub fn flag_value_type_from_wire(wire: FlagValueTypeWire) -> Result<FlagValueType, RuleError> {
    match wire {
        FlagValueTypeWire::Boolean => Ok(FlagValueType::Boolean),
        FlagValueTypeWire::String => Ok(FlagValueType::String),
        FlagValueTypeWire::Number => Ok(FlagValueType::Number),
        FlagValueTypeWire::Json => Ok(FlagValueType::Json),
        FlagValueTypeWire::Unknown => Err(RuleError::new(
            "value_type",
            "expected boolean, string, number, or json",
        )),
    }
}

/// Converts [`FlagValueType`] to [`FlagValueTypeWire`].
pub const fn flag_value_type_to_wire(vt: FlagValueType) -> FlagValueTypeWire {
    match vt {
        FlagValueType::Boolean => FlagValueTypeWire::Boolean,
        FlagValueType::String => FlagValueTypeWire::String,
        FlagValueType::Number => FlagValueTypeWire::Number,
        FlagValueType::Json => FlagValueTypeWire::Json,
    }
}

/// Converts [`LinkEntityKindWire`] to [`LinkEntityKind`].
pub fn link_entity_kind_from_wire(wire: LinkEntityKindWire) -> Result<LinkEntityKind, RuleError> {
    match wire {
        LinkEntityKindWire::Project => Ok(LinkEntityKind::Project),
        LinkEntityKindWire::Feature => Ok(LinkEntityKind::Feature),
        LinkEntityKindWire::Release => Ok(LinkEntityKind::Release),
        LinkEntityKindWire::Milestone => Ok(LinkEntityKind::Milestone),
        LinkEntityKindWire::Deployment => Ok(LinkEntityKind::Deployment),
        LinkEntityKindWire::Task => Ok(LinkEntityKind::Task),
        LinkEntityKindWire::Document => Ok(LinkEntityKind::Document),
        LinkEntityKindWire::DevelopmentProcess => Ok(LinkEntityKind::DevelopmentProcess),
        LinkEntityKindWire::Media => Ok(LinkEntityKind::Media),
        LinkEntityKindWire::Unknown => Err(RuleError::new("entity_kind", "unknown entity kind")),
    }
}

/// Converts [`LinkEntityKind`] to [`LinkEntityKindWire`].
pub const fn link_entity_kind_to_wire(kind: LinkEntityKind) -> LinkEntityKindWire {
    match kind {
        LinkEntityKind::Project => LinkEntityKindWire::Project,
        LinkEntityKind::Feature => LinkEntityKindWire::Feature,
        LinkEntityKind::Release => LinkEntityKindWire::Release,
        LinkEntityKind::Milestone => LinkEntityKindWire::Milestone,
        LinkEntityKind::Deployment => LinkEntityKindWire::Deployment,
        LinkEntityKind::Task => LinkEntityKindWire::Task,
        LinkEntityKind::Document => LinkEntityKindWire::Document,
        LinkEntityKind::DevelopmentProcess => LinkEntityKindWire::DevelopmentProcess,
        LinkEntityKind::Media => LinkEntityKindWire::Media,
    }
}

/// Converts [`LinkKindWire`] to [`LinkKind`].
pub fn link_kind_from_wire(wire: LinkKindWire) -> Result<LinkKind, RuleError> {
    match wire {
        LinkKindWire::DependsOn => Ok(LinkKind::DependsOn),
        LinkKindWire::RelatesTo => Ok(LinkKind::RelatesTo),
        LinkKindWire::Fixes => Ok(LinkKind::Fixes),
        LinkKindWire::Unknown => Err(RuleError::new(
            "kind",
            "expected depends_on, relates_to, or fixes",
        )),
    }
}

/// Converts [`LinkKind`] to [`LinkKindWire`].
pub const fn link_kind_to_wire(kind: LinkKind) -> LinkKindWire {
    match kind {
        LinkKind::DependsOn => LinkKindWire::DependsOn,
        LinkKind::RelatesTo => LinkKindWire::RelatesTo,
        LinkKind::Fixes => LinkKindWire::Fixes,
    }
}

/// Converts a [`FeatureRecord`] plus its project slug to [`FeatureSummaryWire`].
pub fn feature_summary_wire(f: &FeatureRecord, project_slug: &str) -> FeatureSummaryWire {
    FeatureSummaryWire {
        id: f.id.clone(),
        project_id: f.project_id.clone(),
        number: f.number,
        key: f.key(),
        local_slug: f.local_slug.clone(),
        slug: feature_qualified_slug(project_slug, &f.local_slug),
        title: f.title.clone(),
        status: feature_status_to_wire(f.status),
        work_type: f.work_type.map(feature_work_type_to_wire),
        owner_employee_id: f.owner_employee_id.clone(),
        delivered_at: f.delivered_at.clone(),
        retired_reason: f.retired_reason.clone(),
        replaced_by_feature_id: f.replaced_by_feature_id.clone(),
        revision: f.meta.revision,
        created_at: f.meta.created_at.clone(),
        updated_at: f.meta.updated_at.clone(),
    }
}

/// Converts an [`EntityLinkRecord`] to [`EntityLinkWire`].
pub fn entity_link_wire(l: &EntityLinkRecord) -> EntityLinkWire {
    EntityLinkWire {
        id: l.id.clone(),
        project_id: l.project_id.clone(),
        source_kind: link_entity_kind_to_wire(l.source_kind),
        source_id: l.source_id.clone(),
        kind: link_kind_to_wire(l.kind),
        target_kind: link_entity_kind_to_wire(l.target_kind),
        target_id: l.target_id.clone(),
    }
}

/// Converts a [`ScopedChangelogRecord`] to [`ScopedChangelogWire`].
pub fn changelog_wire(c: &ScopedChangelogRecord) -> ScopedChangelogWire {
    ScopedChangelogWire {
        id: c.id.clone(),
        project_id: c.project_id.clone(),
        release_id: c.release_id.clone(),
        deployment_id: c.deployment_id.clone(),
        title: c.title.clone(),
        status: doc_status_to_wire(c.status),
        owner_employee_id: c.owner_employee_id.clone(),
        body_markdown: c.body_markdown.clone(),
        revision: c.meta.revision,
        created_at: c.meta.created_at.clone(),
        updated_at: c.meta.updated_at.clone(),
    }
}

/// Converts a [`MilestoneRecord`] and its computed progress to [`MilestoneWire`].
pub fn milestone_wire(m: &MilestoneRecord, progress: MilestoneProgress) -> MilestoneWire {
    MilestoneWire {
        id: m.id.clone(),
        project_id: m.project_id.clone(),
        release_id: m.release_id.clone(),
        number: m.number,
        key: m.key(),
        title: m.title.clone(),
        body_markdown: m.body_markdown.clone(),
        progress: MilestoneProgressWire {
            task_count: progress.task_count,
            done_task_count: progress.done_task_count,
            is_done: progress.is_done,
        },
        revision: m.meta.revision,
        created_at: m.meta.created_at.clone(),
        updated_at: m.meta.updated_at.clone(),
    }
}

/// Converts a [`ReleaseRecord`] and its computed progress to [`ReleaseSummaryWire`].
pub fn release_summary_wire(
    r: &ReleaseRecord,
    progress: ReleaseProgress,
    has_changelog: bool,
) -> ReleaseSummaryWire {
    ReleaseSummaryWire {
        id: r.id.clone(),
        project_id: r.project_id.clone(),
        number: r.number,
        key: r.key(),
        title: r.title.clone(),
        version_label: r.version_label.clone(),
        progress: ReleaseProgressWire {
            milestone_count: progress.milestone_count,
            task_count: progress.task_count,
            done_task_count: progress.done_task_count,
            is_done: progress.is_done,
        },
        has_changelog,
        revision: r.meta.revision,
        created_at: r.meta.created_at.clone(),
        updated_at: r.meta.updated_at.clone(),
    }
}

/// Converts a [`DeploymentRecord`] to [`DeploymentSummaryWire`].
pub fn deployment_summary_wire(d: &DeploymentRecord, has_changelog: bool) -> DeploymentSummaryWire {
    DeploymentSummaryWire {
        id: d.id.clone(),
        project_id: d.project_id.clone(),
        release_id: d.release_id.clone(),
        environment: d.environment.clone(),
        target: d.target.clone(),
        operation: deployment_op_to_wire(d.operation),
        rollback_of_id: d.rollback_of_id.clone(),
        baseline_deployment_id: d.baseline_deployment_id.clone(),
        status: deployment_status_to_wire(d.status),
        started_at: d.started_at.clone(),
        finished_at: d.finished_at.clone(),
        external_url: d.external_url.clone(),
        has_changelog,
        revision: d.meta.revision,
        created_at: d.meta.created_at.clone(),
        updated_at: d.meta.updated_at.clone(),
    }
}

/// Converts a [`FeatureFlagRecord`] and its usage flag to [`FeatureFlagWire`].
pub fn feature_flag_wire(f: &FeatureFlagRecord, used: bool) -> FeatureFlagWire {
    FeatureFlagWire {
        id: f.id.clone(),
        project_id: f.project_id.clone(),
        feature_id: f.feature_id.clone(),
        key: f.key.clone(),
        owner_employee_id: f.owner_employee_id.clone(),
        status: flag_status_to_wire(f.status),
        value_type: flag_value_type_to_wire(f.value_type),
        default_value: f.default_value.clone(),
        body_markdown: f.body_markdown.clone(),
        used,
        revision: f.meta.revision,
        created_at: f.meta.created_at.clone(),
        updated_at: f.meta.updated_at.clone(),
    }
}

/// Converts [`RepoCommitRecord`] to [`RepoCommitWire`].
pub fn repo_commit_wire(c: &RepoCommitRecord) -> RepoCommitWire {
    RepoCommitWire {
        repository_id: c.repository_id.clone(),
        commit_sha: c.commit_sha.clone(),
    }
}
