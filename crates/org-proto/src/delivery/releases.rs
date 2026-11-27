//! Releases, milestones, repository commits, changelogs, and progress wire contract
//! (`TASK-55`, `FC-062..066`).

use serde::{Deserialize, Serialize};

use super::flags::ReleaseFlagSettingWire;
use crate::docs::DocStatusWire;
use crate::Call;

/// One repository commit pinned to a release or deployment.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RepoCommitWire {
    pub repository_id: String,
    pub commit_sha: String,
}

/// One release or deployment changelog document (`documents` row with `kind = 'changelog'`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScopedChangelogWire {
    pub id: String,
    pub project_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub release_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deployment_id: Option<String>,
    pub title: String,
    pub status: DocStatusWire,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner_employee_id: Option<String>,
    pub body_markdown: String,
    pub revision: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<String>,
}

/// Write response for changelog mutations.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChangelogWriteResult {
    pub changelog: ScopedChangelogWire,
}

/// Computed progress of one milestone (`milestone_progress` view).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct MilestoneProgressWire {
    pub task_count: u64,
    pub done_task_count: u64,
    pub is_done: bool,
}

/// One release milestone (`M-<n>`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MilestoneWire {
    pub id: String,
    pub project_id: String,
    pub release_id: String,
    pub number: u64,
    pub key: String,
    pub title: String,
    #[serde(default)]
    pub body_markdown: String,
    pub progress: MilestoneProgressWire,
    pub revision: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<String>,
}

/// Write response for milestone mutations.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MilestoneWriteResult {
    pub milestone: MilestoneWire,
}

/// Computed progress of one release (`release_progress` view).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReleaseProgressWire {
    pub milestone_count: u64,
    pub task_count: u64,
    pub done_task_count: u64,
    pub is_done: bool,
}

/// Summary view of one release (`R-<n>`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReleaseSummaryWire {
    pub id: String,
    pub project_id: String,
    pub number: u64,
    pub key: String,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version_label: Option<String>,
    pub progress: ReleaseProgressWire,
    pub has_changelog: bool,
    pub revision: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<String>,
}

/// Write response for release mutations.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReleaseWriteResult {
    pub release: ReleaseSummaryWire,
}

/// Full inspected release.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReleaseInspectResult {
    pub release: ReleaseSummaryWire,
    pub body_markdown: String,
    #[serde(default)]
    pub repository_commits: Vec<RepoCommitWire>,
    #[serde(default)]
    pub feature_ids: Vec<String>,
    #[serde(default)]
    pub task_ids: Vec<String>,
    #[serde(default)]
    pub milestones: Vec<MilestoneWire>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub changelog: Option<ScopedChangelogWire>,
    #[serde(default)]
    pub flag_settings: Vec<ReleaseFlagSettingWire>,
}

/// `releases.list`: list releases of one project.
pub struct ReleasesList;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReleasesListParams {
    pub project: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReleasesListResult {
    pub project_id: String,
    pub releases: Vec<ReleaseSummaryWire>,
}

impl Call for ReleasesList {
    const METHOD: &'static str = "releases.list";
    type Params = ReleasesListParams;
    type Result = ReleasesListResult;
}

/// `releases.inspect`: inspect one release by UUIDv7 or `R-<n>`.
pub struct ReleasesInspect;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReleasesInspectParams {
    pub project: String,
    pub release: String,
}

impl Call for ReleasesInspect {
    const METHOD: &'static str = "releases.inspect";
    type Params = ReleasesInspectParams;
    type Result = ReleaseInspectResult;
}

/// `releases.create`: create one project release with commits, features, and tasks.
pub struct ReleasesCreate;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReleasesCreateParams {
    pub command_id: String,
    #[serde(default)]
    pub project: String,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version_label: Option<String>,
    #[serde(default)]
    pub body_markdown: String,
    #[serde(default)]
    pub repository_commits: Vec<RepoCommitWire>,
    #[serde(default)]
    pub feature_ids: Vec<String>,
    #[serde(default)]
    pub task_ids: Vec<String>,
}

impl Call for ReleasesCreate {
    const METHOD: &'static str = "releases.create";
    type Params = ReleasesCreateParams;
    type Result = ReleaseWriteResult;
}

/// `releases.update`: update a release at `expected_revision`.
pub struct ReleasesUpdate;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReleasesUpdateParams {
    pub command_id: String,
    #[serde(default)]
    pub project: String,
    #[serde(default)]
    pub release_id: String,
    pub expected_revision: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version_label: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body_markdown: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repository_commits: Option<Vec<RepoCommitWire>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub feature_ids: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task_ids: Option<Vec<String>>,
}

impl Call for ReleasesUpdate {
    const METHOD: &'static str = "releases.update";
    type Params = ReleasesUpdateParams;
    type Result = ReleaseWriteResult;
}

/// `releases.set_changelog`: create or update the single changelog for a release.
pub struct ReleasesSetChangelog;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReleasesSetChangelogParams {
    pub command_id: String,
    #[serde(default)]
    pub project: String,
    #[serde(default)]
    pub release_id: String,
    pub title: String,
    #[serde(default)]
    pub body_markdown: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<DocStatusWire>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner_employee_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expected_revision: Option<u64>,
}

impl Call for ReleasesSetChangelog {
    const METHOD: &'static str = "releases.set_changelog";
    type Params = ReleasesSetChangelogParams;
    type Result = ChangelogWriteResult;
}

/// `milestones.create`: create one milestone (`M-<n>`) inside a release.
pub struct MilestonesCreate;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MilestonesCreateParams {
    pub command_id: String,
    #[serde(default)]
    pub project: String,
    #[serde(default)]
    pub release_id: String,
    pub title: String,
    #[serde(default)]
    pub body_markdown: String,
}

impl Call for MilestonesCreate {
    const METHOD: &'static str = "milestones.create";
    type Params = MilestonesCreateParams;
    type Result = MilestoneWriteResult;
}

/// `milestones.update`: update a release milestone at `expected_revision`.
pub struct MilestonesUpdate;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MilestonesUpdateParams {
    pub command_id: String,
    #[serde(default)]
    pub project: String,
    #[serde(default)]
    pub milestone_id: String,
    pub expected_revision: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body_markdown: Option<String>,
}

impl Call for MilestonesUpdate {
    const METHOD: &'static str = "milestones.update";
    type Params = MilestonesUpdateParams;
    type Result = MilestoneWriteResult;
}
