//! Deployments, repository commits, rollback guards, and changelogs wire contract
//! (`TASK-55`, `FC-067..068`).

use serde::{Deserialize, Serialize};

use super::flags::DeploymentFlagSettingWire;
use super::releases::{ChangelogWriteResult, RepoCommitWire, ScopedChangelogWire};
use crate::docs::DocStatusWire;
use crate::Call;

/// Deployment operation (`deployments.operation`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeploymentOperationWire {
    #[default]
    Deploy,
    Rollback,
    #[serde(other)]
    Unknown,
}

/// Lifecycle state of a deployment (`deployments.status`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeploymentStatusWire {
    #[default]
    Queued,
    Running,
    Succeeded,
    Failed,
    Cancelled,
    #[serde(other)]
    Unknown,
}

/// Summary view of one deployment.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeploymentSummaryWire {
    pub id: String,
    pub project_id: String,
    pub release_id: String,
    pub environment: String,
    pub target: String,
    pub operation: DeploymentOperationWire,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rollback_of_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub baseline_deployment_id: Option<String>,
    pub status: DeploymentStatusWire,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub started_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finished_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub external_url: Option<String>,
    pub has_changelog: bool,
    pub revision: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<String>,
}

/// Write response for deployment mutations.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeploymentWriteResult {
    pub deployment: DeploymentSummaryWire,
}

/// Full inspected deployment.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeploymentInspectResult {
    pub deployment: DeploymentSummaryWire,
    pub body_markdown: String,
    #[serde(default)]
    pub repository_commits: Vec<RepoCommitWire>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub changelog: Option<ScopedChangelogWire>,
    #[serde(default)]
    pub flag_settings: Vec<DeploymentFlagSettingWire>,
}

/// `deployments.list`: list deployments of one project.
pub struct DeploymentsList;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeploymentsListParams {
    pub project: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub release_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub environment: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<DeploymentStatusWire>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeploymentsListResult {
    pub project_id: String,
    pub deployments: Vec<DeploymentSummaryWire>,
}

impl Call for DeploymentsList {
    const METHOD: &'static str = "deployments.list";
    type Params = DeploymentsListParams;
    type Result = DeploymentsListResult;
}

/// `deployments.inspect`: inspect one deployment by UUIDv7.
pub struct DeploymentsInspect;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeploymentsInspectParams {
    pub project: String,
    pub deployment_id: String,
}

impl Call for DeploymentsInspect {
    const METHOD: &'static str = "deployments.inspect";
    type Params = DeploymentsInspectParams;
    type Result = DeploymentInspectResult;
}

/// `deployments.create`: record one deployment or rollback.
pub struct DeploymentsCreate;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeploymentsCreateParams {
    pub command_id: String,
    #[serde(default)]
    pub project: String,
    pub release_id: String,
    pub environment: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub operation: Option<DeploymentOperationWire>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rollback_of_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub baseline_deployment_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<DeploymentStatusWire>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub started_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finished_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub external_url: Option<String>,
    #[serde(default)]
    pub body_markdown: String,
    #[serde(default)]
    pub repository_commits: Vec<RepoCommitWire>,
}

impl Call for DeploymentsCreate {
    const METHOD: &'static str = "deployments.create";
    type Params = DeploymentsCreateParams;
    type Result = DeploymentWriteResult;
}

/// `deployments.update`: update a deployment at `expected_revision`.
pub struct DeploymentsUpdate;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeploymentsUpdateParams {
    pub command_id: String,
    #[serde(default)]
    pub project: String,
    #[serde(default)]
    pub deployment_id: String,
    pub expected_revision: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<DeploymentStatusWire>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub started_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finished_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub external_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body_markdown: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rollback_of_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub baseline_deployment_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repository_commits: Option<Vec<RepoCommitWire>>,
}

impl Call for DeploymentsUpdate {
    const METHOD: &'static str = "deployments.update";
    type Params = DeploymentsUpdateParams;
    type Result = DeploymentWriteResult;
}

/// `deployments.set_changelog`: create or update the single changelog for a deployment.
pub struct DeploymentsSetChangelog;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeploymentsSetChangelogParams {
    pub command_id: String,
    #[serde(default)]
    pub project: String,
    #[serde(default)]
    pub deployment_id: String,
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

impl Call for DeploymentsSetChangelog {
    const METHOD: &'static str = "deployments.set_changelog";
    type Params = DeploymentsSetChangelogParams;
    type Result = ChangelogWriteResult;
}
