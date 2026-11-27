//! Pure domain records for canonical features, Why links, releases, milestones,
//! deployments, changelogs, feature flags, flag snapshots, and monotonic reference counters.

use serde_json::Value;

use crate::project_tree::DocStatus;

/// Actor provenance for a record creation or update.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ActorRef {
    /// Employee UUIDv7.
    Employee(String),
    /// System actor.
    System,
    /// Unknown actor.
    Unknown,
}

impl ActorRef {
    /// Wire/SQL actor kind string (`employee`, `system`, `unknown`).
    pub const fn kind(&self) -> &'static str {
        match self {
            Self::Employee(_) => "employee",
            Self::System => "system",
            Self::Unknown => "unknown",
        }
    }

    /// Employee UUIDv7 if `Self::Employee`, else `None`.
    pub fn employee_id(&self) -> Option<&str> {
        match self {
            Self::Employee(id) => Some(id.as_str()),
            Self::System | Self::Unknown => None,
        }
    }
}

/// Shared schema metadata persisted on every delivery record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecordMeta {
    /// Schema version (always `1`).
    pub schema_version: u64,
    /// Monotonic revision (`>= 1`).
    pub revision: u64,
    /// RFC 3339 creation timestamp.
    pub created_at: Option<String>,
    /// Creator actor.
    pub created_by: ActorRef,
    /// RFC 3339 update timestamp.
    pub updated_at: Option<String>,
    /// Last updater actor.
    pub updated_by: ActorRef,
}

/// Canonical feature status (`features.status`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum FeatureStatus {
    /// Planned feature.
    Planned,
    /// In-progress feature (requires owner).
    InProgress,
    /// Live feature (requires owner).
    Live,
    /// Retired feature.
    Retired,
}

impl FeatureStatus {
    /// All feature statuses.
    pub const ALL: [Self; 4] = [Self::Planned, Self::InProgress, Self::Live, Self::Retired];

    /// SQL/wire spelling.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Planned => "planned",
            Self::InProgress => "in_progress",
            Self::Live => "live",
            Self::Retired => "retired",
        }
    }

    /// Parses SQL/wire spelling.
    pub fn parse(raw: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|s| s.as_str() == raw)
    }
}

/// Feature work category (`features.work_type`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum FeatureWorkType {
    /// Build work.
    Build,
    /// Run work.
    Run,
}

impl FeatureWorkType {
    /// SQL/wire spelling.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Build => "build",
            Self::Run => "run",
        }
    }

    /// Parses SQL/wire spelling.
    pub fn parse(raw: &str) -> Option<Self> {
        match raw {
            "build" => Some(Self::Build),
            "run" => Some(Self::Run),
            _ => None,
        }
    }
}

/// Canonical project feature record (`features`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FeatureRecord {
    /// Feature UUIDv7.
    pub id: String,
    /// Owning project UUIDv7.
    pub project_id: String,
    /// Project-scoped positive sequence number (`F-<number>`).
    pub number: u64,
    /// Project-unique local slug.
    pub local_slug: String,
    /// Feature owner employee UUIDv7 (required for `in_progress` and `live`).
    pub owner_employee_id: Option<String>,
    /// Optional work category (`build` or `run`).
    pub work_type: Option<FeatureWorkType>,
    /// Feature title.
    pub title: String,
    /// Lifecycle status.
    pub status: FeatureStatus,
    /// Markdown description.
    pub body_markdown: String,
    /// Optional RFC 3339 delivery timestamp.
    pub delivered_at: Option<String>,
    /// Optional retirement reason.
    pub retired_reason: Option<String>,
    /// Optional replacement feature UUIDv7 (same project, acyclic).
    pub replaced_by_feature_id: Option<String>,
    /// Schema/revision/audit metadata.
    pub meta: RecordMeta,
}

impl FeatureRecord {
    /// Human-readable project key (`F-<number>`).
    pub fn key(&self) -> String {
        format!("F-{}", self.number)
    }
}

/// One pinned repository commit (`release_repository_commits` / `deployment_repository_commits`).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct RepoCommitRecord {
    /// Repository UUIDv7.
    pub repository_id: String,
    /// Commit SHA string.
    pub commit_sha: String,
}

/// One release flag snapshot entry (`release_flag_settings`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReleaseFlagSettingRecord {
    /// Feature flag UUIDv7.
    pub flag_id: String,
    /// Target environment name.
    pub environment: String,
    /// Target name (`primary` by default).
    pub target: String,
    /// Typed JSON value matching the flag's `value_type`.
    pub value: Value,
    /// Optional rollout rules JSON object.
    pub rollout_rules: Option<Value>,
}

/// Canonical project release record (`releases` + embedded commits, scope, and flag snapshot).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReleaseRecord {
    /// Release UUIDv7.
    pub id: String,
    /// Owning project UUIDv7.
    pub project_id: String,
    /// Project-scoped positive sequence number (`R-<number>`).
    pub number: u64,
    /// Release title.
    pub title: String,
    /// Optional semver or display version label.
    pub version_label: Option<String>,
    /// Markdown release notes / overview.
    pub body_markdown: String,
    /// Pinned repository commits (unique per `repository_id`).
    pub repository_commits: Vec<RepoCommitRecord>,
    /// Canonical feature UUIDv7s included in this release.
    pub feature_ids: Vec<String>,
    /// Task UUIDv7s directly attached to this release (`release_tasks`).
    pub task_ids: Vec<String>,
    /// Flag snapshot settings attached to this release (`release_flag_settings`).
    pub flag_settings: Vec<ReleaseFlagSettingRecord>,
    /// Schema/revision/audit metadata.
    pub meta: RecordMeta,
}

impl ReleaseRecord {
    /// Human-readable project key (`R-<number>`).
    pub fn key(&self) -> String {
        format!("R-{}", self.number)
    }
}

/// Release milestone record (`milestones`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MilestoneRecord {
    /// Milestone UUIDv7.
    pub id: String,
    /// Owning project UUIDv7.
    pub project_id: String,
    /// Owning release UUIDv7.
    pub release_id: String,
    /// Release-scoped positive sequence number (`M-<number>`).
    pub number: u64,
    /// Milestone title.
    pub title: String,
    /// Markdown body.
    pub body_markdown: String,
    /// Schema/revision/audit metadata.
    pub meta: RecordMeta,
}

impl MilestoneRecord {
    /// Human-readable release key (`M-<number>`).
    pub fn key(&self) -> String {
        format!("M-{}", self.number)
    }
}

/// Deployment operation (`deployments.operation`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum DeploymentOperation {
    /// Forward deploy.
    Deploy,
    /// Rollback of an earlier deployment in the same `(project, environment, target)`.
    Rollback,
}

impl DeploymentOperation {
    /// SQL/wire spelling.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Deploy => "deploy",
            Self::Rollback => "rollback",
        }
    }

    /// Parses SQL/wire spelling.
    pub fn parse(raw: &str) -> Option<Self> {
        match raw {
            "deploy" => Some(Self::Deploy),
            "rollback" => Some(Self::Rollback),
            _ => None,
        }
    }
}

/// Deployment status (`deployments.status`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum DeploymentStatus {
    /// Queued (neither `started_at` nor `finished_at`).
    Queued,
    /// Running (`started_at` set, `finished_at` absent).
    Running,
    /// Succeeded (`started_at` and `finished_at` set).
    Succeeded,
    /// Failed (`started_at` and `finished_at` set).
    Failed,
    /// Cancelled (`finished_at` set, `started_at` optional).
    Cancelled,
}

impl DeploymentStatus {
    /// All deployment statuses.
    pub const ALL: [Self; 5] = [
        Self::Queued,
        Self::Running,
        Self::Succeeded,
        Self::Failed,
        Self::Cancelled,
    ];

    /// SQL/wire spelling.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Running => "running",
            Self::Succeeded => "succeeded",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
        }
    }

    /// Parses SQL/wire spelling.
    pub fn parse(raw: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|s| s.as_str() == raw)
    }
}

/// One deployment flag snapshot entry (`deployment_flag_settings`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeploymentFlagSettingRecord {
    /// Feature flag UUIDv7.
    pub flag_id: String,
    /// Typed JSON value matching the flag's `value_type`.
    pub value: Value,
    /// Optional rollout rules JSON object.
    pub rollout_rules: Option<Value>,
    /// Optional RFC 3339 timestamp when applied.
    pub applied_at: Option<String>,
}

/// Canonical project deployment record (`deployments` + embedded commits & flag snapshot).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeploymentRecord {
    /// Deployment UUIDv7.
    pub id: String,
    /// Owning project UUIDv7.
    pub project_id: String,
    /// Deployed release UUIDv7.
    pub release_id: String,
    /// Environment name (`staging`, `production`, ...).
    pub environment: String,
    /// Deployment target (`primary` by default).
    pub target: String,
    /// `deploy` or `rollback`.
    pub operation: DeploymentOperation,
    /// Required iff `operation == Rollback` (same project/env/target, acyclic).
    pub rollback_of_id: Option<String>,
    /// Optional prior baseline deployment (same project/env/target).
    pub baseline_deployment_id: Option<String>,
    /// Execution status.
    pub status: DeploymentStatus,
    /// Optional RFC 3339 start timestamp.
    pub started_at: Option<String>,
    /// Optional RFC 3339 finish timestamp.
    pub finished_at: Option<String>,
    /// Optional external deployment URL.
    pub external_url: Option<String>,
    /// Markdown deployment notes.
    pub body_markdown: String,
    /// Pinned repository commits.
    pub repository_commits: Vec<RepoCommitRecord>,
    /// Flag snapshot settings applied by this deployment (`deployment_flag_settings`).
    pub flag_settings: Vec<DeploymentFlagSettingRecord>,
    /// Schema/revision/audit metadata.
    pub meta: RecordMeta,
}

/// Feature flag lifecycle status (`feature_flags.status`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum FlagStatus {
    /// Draft flag.
    Draft,
    /// Active flag (requires owner).
    Active,
    /// Retired flag.
    Retired,
}

impl FlagStatus {
    /// All flag statuses.
    pub const ALL: [Self; 3] = [Self::Draft, Self::Active, Self::Retired];

    /// SQL/wire spelling.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Draft => "draft",
            Self::Active => "active",
            Self::Retired => "retired",
        }
    }

    /// Parses SQL/wire spelling.
    pub fn parse(raw: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|s| s.as_str() == raw)
    }
}

/// Feature flag value type (`feature_flags.value_type`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum FlagValueType {
    /// JSON boolean (`true` / `false`).
    Boolean,
    /// JSON string.
    String,
    /// JSON number.
    Number,
    /// Arbitrary JSON value.
    Json,
}

impl FlagValueType {
    /// All flag value types.
    pub const ALL: [Self; 4] = [Self::Boolean, Self::String, Self::Number, Self::Json];

    /// SQL/wire spelling.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Boolean => "boolean",
            Self::String => "string",
            Self::Number => "number",
            Self::Json => "json",
        }
    }

    /// Parses SQL/wire spelling.
    pub fn parse(raw: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|s| s.as_str() == raw)
    }
}

/// Canonical feature flag record (`feature_flags`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FeatureFlagRecord {
    /// Flag UUIDv7.
    pub id: String,
    /// Owning project UUIDv7.
    pub project_id: String,
    /// Optional owning feature UUIDv7 (same project).
    pub feature_id: Option<String>,
    /// Project-unique flag key slug (immutable once used in a snapshot).
    pub key: String,
    /// Owner employee UUIDv7 (required when `status == Active`).
    pub owner_employee_id: Option<String>,
    /// Lifecycle status.
    pub status: FlagStatus,
    /// Typed value kind (immutable once used in a snapshot).
    pub value_type: FlagValueType,
    /// Default JSON value matching `value_type`.
    pub default_value: Value,
    /// Markdown flag notes.
    pub body_markdown: String,
    /// Schema/revision/audit metadata.
    pub meta: RecordMeta,
}

/// Scoped release or deployment changelog document (`documents` with `kind = 'changelog'`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScopedChangelogRecord {
    /// Document UUIDv7.
    pub id: String,
    /// Owning project UUIDv7.
    pub project_id: String,
    /// Release UUIDv7 (mutually exclusive with `deployment_id`).
    pub release_id: Option<String>,
    /// Deployment UUIDv7 (mutually exclusive with `release_id`).
    pub deployment_id: Option<String>,
    /// Changelog title.
    pub title: String,
    /// Document status (`current` requires owner).
    pub status: DocStatus,
    /// Optional owner employee UUIDv7.
    pub owner_employee_id: Option<String>,
    /// Markdown changelog body.
    pub body_markdown: String,
    /// Schema/revision/audit metadata.
    pub meta: RecordMeta,
}

/// Record kind participating in a Why link (`links.source_kind` / `links.target_kind`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum LinkEntityKind {
    /// Project record.
    Project,
    /// Feature record.
    Feature,
    /// Release record.
    Release,
    /// Milestone record.
    Milestone,
    /// Deployment record.
    Deployment,
    /// Task record.
    Task,
    /// Document record.
    Document,
    /// Development process record.
    DevelopmentProcess,
    /// Media record.
    Media,
}

impl LinkEntityKind {
    /// Every valid target kind in `links`.
    pub const ALL: [Self; 9] = [
        Self::Project,
        Self::Feature,
        Self::Release,
        Self::Milestone,
        Self::Deployment,
        Self::Task,
        Self::Document,
        Self::DevelopmentProcess,
        Self::Media,
    ];

    /// SQL/wire spelling.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Project => "project",
            Self::Feature => "feature",
            Self::Release => "release",
            Self::Milestone => "milestone",
            Self::Deployment => "deployment",
            Self::Task => "task",
            Self::Document => "document",
            Self::DevelopmentProcess => "development_process",
            Self::Media => "media",
        }
    }

    /// Parses SQL/wire spelling.
    pub fn parse(raw: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|k| k.as_str() == raw)
    }

    /// Whether this kind is allowed as `links.source_kind` (`schema.sql` line 585).
    pub const fn is_valid_source(self) -> bool {
        matches!(
            self,
            Self::Project | Self::Feature | Self::Task | Self::Document
        )
    }
}

/// Directed Why link kind (`links.kind`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum LinkKind {
    /// Task dependency (`task -> task` only).
    DependsOn,
    /// General Why / traceability link across entities.
    RelatesTo,
    /// Fix relationship (`task -> task` only per `schema.sql`).
    Fixes,
}

impl LinkKind {
    /// Every link kind.
    pub const ALL: [Self; 3] = [Self::DependsOn, Self::RelatesTo, Self::Fixes];

    /// SQL/wire spelling.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::DependsOn => "depends_on",
            Self::RelatesTo => "relates_to",
            Self::Fixes => "fixes",
        }
    }

    /// Parses SQL/wire spelling.
    pub fn parse(raw: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|k| k.as_str() == raw)
    }
}

/// One directed Why link persisted under a project (`links`).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct EntityLinkRecord {
    /// Link UUIDv7.
    pub id: String,
    /// Owning project UUIDv7.
    pub project_id: String,
    /// Source entity kind (`project`, `feature`, `task`, or `document`).
    pub source_kind: LinkEntityKind,
    /// Source entity UUIDv7.
    pub source_id: String,
    /// Relationship kind.
    pub kind: LinkKind,
    /// Target entity kind.
    pub target_kind: LinkEntityKind,
    /// Target entity UUIDv7.
    pub target_id: String,
}

/// One allocated sequence number in a scoped counter file.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct NumberAllocation {
    /// Allocated positive sequence number.
    pub number: u64,
    /// Entity UUIDv7 holding `number`.
    pub entity_id: String,
}

/// Durable counter + allocation history for `F-<n>`, `R-<n>`, or `M-<n>` numbering.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NumberCounterRecord {
    /// Next sequence number to allocate (`>= 1`, strictly greater than every allocation).
    pub next_number: u64,
    /// Every number allocated so far, sorted by `number`.
    pub allocations: Vec<NumberAllocation>,
}

/// Authoritative milestone progress (`milestone_progress` view).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct MilestoneProgress {
    /// Total tasks assigned to the milestone.
    pub task_count: u64,
    /// Tasks in `done` or `cancelled` status.
    pub done_task_count: u64,
    /// True iff `task_count > 0 && task_count == done_task_count`.
    pub is_done: bool,
}

/// Authoritative release progress (`release_progress` view).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ReleaseProgress {
    /// Number of milestones belonging to the release.
    pub milestone_count: u64,
    /// Distinct tasks in the release (via `release_tasks` union milestone tasks).
    pub task_count: u64,
    /// Distinct release tasks in `done` or `cancelled` status.
    pub done_task_count: u64,
    /// True iff `task_count > 0`, every task is done/cancelled, and every milestone (if any) is done.
    pub is_done: bool,
}
