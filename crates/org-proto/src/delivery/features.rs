//! Canonical project features wire contract (`TASK-55`, `FC-058..060`, `schema.sql` `features`).

use serde::{Deserialize, Serialize};

use super::links::EntityLinkWire;
use crate::Call;

/// Lifecycle state of a canonical feature (`features.status`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FeatureStatusWire {
    Planned,
    InProgress,
    Live,
    Retired,
    #[serde(other)]
    Unknown,
}

/// Work category (`features.work_type`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FeatureWorkTypeWire {
    Build,
    Run,
    #[serde(other)]
    Unknown,
}

/// Summary view of one canonical feature.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FeatureSummaryWire {
    pub id: String,
    pub project_id: String,
    pub number: u64,
    /// Human-readable project-scoped reference (`F-1`, `F-2`, ...).
    pub key: String,
    pub local_slug: String,
    /// Canonical qualified slug (`<project.slug>/<local_slug>`, `feature_slugs` view).
    pub slug: String,
    pub title: String,
    pub status: FeatureStatusWire,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub work_type: Option<FeatureWorkTypeWire>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner_employee_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delivered_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retired_reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub replaced_by_feature_id: Option<String>,
    pub revision: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<String>,
}

/// Full inspected feature with body, linked work, releases, flags, and Why links.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FeatureInspectResult {
    pub feature: FeatureSummaryWire,
    pub body_markdown: String,
    #[serde(default)]
    pub implementing_task_ids: Vec<String>,
    #[serde(default)]
    pub affecting_bug_ids: Vec<String>,
    #[serde(default)]
    pub release_ids: Vec<String>,
    #[serde(default)]
    pub flag_ids: Vec<String>,
    #[serde(default)]
    pub links: Vec<EntityLinkWire>,
}

/// Write response for feature mutations.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FeatureWriteResult {
    pub feature: FeatureSummaryWire,
}

/// `features.list`: list canonical features of one project.
pub struct FeaturesList;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FeaturesListParams {
    pub project: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<FeatureStatusWire>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FeaturesListResult {
    pub project_id: String,
    pub features: Vec<FeatureSummaryWire>,
}

impl Call for FeaturesList {
    const METHOD: &'static str = "features.list";
    type Params = FeaturesListParams;
    type Result = FeaturesListResult;
}

/// `features.inspect`: inspect one canonical feature by UUIDv7, `F-<n>`, `local_slug`, or qualified slug.
pub struct FeaturesInspect;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FeaturesInspectParams {
    pub project: String,
    pub feature: String,
}

impl Call for FeaturesInspect {
    const METHOD: &'static str = "features.inspect";
    type Params = FeaturesInspectParams;
    type Result = FeatureInspectResult;
}

/// `features.create`: create one canonical feature in a project.
pub struct FeaturesCreate;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FeaturesCreateParams {
    pub command_id: String,
    #[serde(default)]
    pub project: String,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub local_slug: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<FeatureStatusWire>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub work_type: Option<FeatureWorkTypeWire>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner_employee_id: Option<String>,
    #[serde(default)]
    pub body_markdown: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delivered_at: Option<String>,
}

impl Call for FeaturesCreate {
    const METHOD: &'static str = "features.create";
    type Params = FeaturesCreateParams;
    type Result = FeatureWriteResult;
}

/// `features.update`: update a canonical feature at `expected_revision`.
pub struct FeaturesUpdate;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FeaturesUpdateParams {
    pub command_id: String,
    #[serde(default)]
    pub project: String,
    #[serde(default)]
    pub feature_id: String,
    pub expected_revision: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub local_slug: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<FeatureStatusWire>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub work_type: Option<FeatureWorkTypeWire>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner_employee_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body_markdown: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delivered_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retired_reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub replaced_by_feature_id: Option<String>,
}

impl Call for FeaturesUpdate {
    const METHOD: &'static str = "features.update";
    type Params = FeaturesUpdateParams;
    type Result = FeatureWriteResult;
}

/// `features.retire`: retire a feature with optional replacement (checked acyclic).
pub struct FeaturesRetire;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FeaturesRetireParams {
    pub command_id: String,
    #[serde(default)]
    pub project: String,
    #[serde(default)]
    pub feature_id: String,
    pub expected_revision: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retired_reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub replaced_by_feature_id: Option<String>,
}

impl Call for FeaturesRetire {
    const METHOD: &'static str = "features.retire";
    type Params = FeaturesRetireParams;
    type Result = FeatureWriteResult;
}
