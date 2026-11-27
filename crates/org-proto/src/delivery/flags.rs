//! Feature flags & release/deployment flag snapshots wire contract (`TASK-55`, `FC-069..070`).

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::Call;

/// Lifecycle state of a feature flag (`feature_flags.status`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FlagStatusWire {
    Draft,
    Active,
    Retired,
    #[serde(other)]
    Unknown,
}

/// Value type of a feature flag (`feature_flags.value_type`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FlagValueTypeWire {
    Boolean,
    String,
    Number,
    Json,
    #[serde(other)]
    Unknown,
}

/// One flag snapshot reference proving a flag is used (`used_flags`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FlagUsageRefWire {
    /// `"release"` or `"deployment"`.
    pub scope_kind: String,
    pub scope_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub environment: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<String>,
}

/// One project feature flag definition.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FeatureFlagWire {
    pub id: String,
    pub project_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub feature_id: Option<String>,
    pub key: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner_employee_id: Option<String>,
    pub status: FlagStatusWire,
    pub value_type: FlagValueTypeWire,
    pub default_value: Value,
    #[serde(default)]
    pub body_markdown: String,
    /// True once referenced by any release or deployment flag snapshot (locks identity).
    pub used: bool,
    pub revision: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<String>,
}

/// Write response for flag mutations.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FlagWriteResult {
    pub flag: FeatureFlagWire,
}

/// One release flag snapshot entry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReleaseFlagSettingWire {
    pub flag_id: String,
    pub flag_key: String,
    pub environment: String,
    pub target: String,
    pub value: Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rollout_rules: Option<Value>,
}

/// One deployment flag snapshot entry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeploymentFlagSettingWire {
    pub flag_id: String,
    pub flag_key: String,
    pub value: Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rollout_rules: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub applied_at: Option<String>,
}

/// `flags.list`: list feature flags of one project.
pub struct FlagsList;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FlagsListParams {
    pub project: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<FlagStatusWire>,
    #[serde(default)]
    pub used_only: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FlagsListResult {
    pub project_id: String,
    pub flags: Vec<FeatureFlagWire>,
}

impl Call for FlagsList {
    const METHOD: &'static str = "flags.list";
    type Params = FlagsListParams;
    type Result = FlagsListResult;
}

/// `flags.inspect`: inspect one feature flag and its release/deployment usages.
pub struct FlagsInspect;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FlagsInspectParams {
    pub project: String,
    pub flag: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FlagInspectResult {
    pub flag: FeatureFlagWire,
    #[serde(default)]
    pub usages: Vec<FlagUsageRefWire>,
}

impl Call for FlagsInspect {
    const METHOD: &'static str = "flags.inspect";
    type Params = FlagsInspectParams;
    type Result = FlagInspectResult;
}

/// `flags.create`: create one feature flag in a project.
pub struct FlagsCreate;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FlagsCreateParams {
    pub command_id: String,
    #[serde(default)]
    pub project: String,
    pub key: String,
    pub value_type: FlagValueTypeWire,
    pub default_value: Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub feature_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner_employee_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<FlagStatusWire>,
    #[serde(default)]
    pub body_markdown: String,
}

impl Call for FlagsCreate {
    const METHOD: &'static str = "flags.create";
    type Params = FlagsCreateParams;
    type Result = FlagWriteResult;
}

/// `flags.update`: update a feature flag at `expected_revision` (used-flag identity immutable).
pub struct FlagsUpdate;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FlagsUpdateParams {
    pub command_id: String,
    #[serde(default)]
    pub project: String,
    #[serde(default)]
    pub flag_id: String,
    pub expected_revision: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value_type: Option<FlagValueTypeWire>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_value: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub feature_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner_employee_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<FlagStatusWire>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body_markdown: Option<String>,
}

impl Call for FlagsUpdate {
    const METHOD: &'static str = "flags.update";
    type Params = FlagsUpdateParams;
    type Result = FlagWriteResult;
}

/// Input row for `releases.set_flag_settings`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReleaseFlagSettingInput {
    pub flag_id: String,
    pub environment: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<String>,
    pub value: Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rollout_rules: Option<Value>,
}

/// `releases.set_flag_settings`: replace a release's flag snapshot atomically at `expected_revision`.
pub struct ReleaseFlagSettingsSet;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReleaseFlagSettingsSetParams {
    pub command_id: String,
    #[serde(default)]
    pub project: String,
    #[serde(default)]
    pub release_id: String,
    pub expected_revision: u64,
    #[serde(default)]
    pub settings: Vec<ReleaseFlagSettingInput>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReleaseFlagSettingsResult {
    pub release_id: String,
    pub revision: u64,
    pub flag_settings: Vec<ReleaseFlagSettingWire>,
}

impl Call for ReleaseFlagSettingsSet {
    const METHOD: &'static str = "releases.set_flag_settings";
    type Params = ReleaseFlagSettingsSetParams;
    type Result = ReleaseFlagSettingsResult;
}

/// Input row for `deployments.set_flag_settings`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeploymentFlagSettingInput {
    pub flag_id: String,
    pub value: Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rollout_rules: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub applied_at: Option<String>,
}

/// `deployments.set_flag_settings`: replace a deployment's flag snapshot atomically.
pub struct DeploymentFlagSettingsSet;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeploymentFlagSettingsSetParams {
    pub command_id: String,
    #[serde(default)]
    pub project: String,
    #[serde(default)]
    pub deployment_id: String,
    pub expected_revision: u64,
    #[serde(default)]
    pub settings: Vec<DeploymentFlagSettingInput>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeploymentFlagSettingsResult {
    pub deployment_id: String,
    pub revision: u64,
    pub flag_settings: Vec<DeploymentFlagSettingWire>,
}

impl Call for DeploymentFlagSettingsSet {
    const METHOD: &'static str = "deployments.set_flag_settings";
    type Params = DeploymentFlagSettingsSetParams;
    type Result = DeploymentFlagSettingsResult;
}
