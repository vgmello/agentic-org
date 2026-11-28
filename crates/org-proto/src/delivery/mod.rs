//! Delivery & knowledge-graph wire contracts (`TASK-55`, `L7`/`L11`, `FC-058..070`):
//! canonical project features, Why links, releases, milestones, changelogs, deployments,
//! feature flags, and release/deployment flag snapshots.

pub mod deployments;
pub mod features;
pub mod flags;
pub mod links;
pub mod releases;

pub use deployments::{
    DeploymentInspectResult, DeploymentOperationWire, DeploymentStatusWire, DeploymentSummaryWire,
    DeploymentWriteResult, DeploymentsCreate, DeploymentsCreateParams, DeploymentsInspect,
    DeploymentsInspectParams, DeploymentsList, DeploymentsListParams, DeploymentsListResult,
    DeploymentsSetChangelog, DeploymentsSetChangelogParams, DeploymentsUpdate,
    DeploymentsUpdateParams,
};
pub use features::{
    FeatureInspectResult, FeatureStatusWire, FeatureSummaryWire, FeatureWorkTypeWire,
    FeatureWriteResult, FeaturesCreate, FeaturesCreateParams, FeaturesInspect,
    FeaturesInspectParams, FeaturesList, FeaturesListParams, FeaturesListResult, FeaturesRetire,
    FeaturesRetireParams, FeaturesUpdate, FeaturesUpdateParams,
};
pub use flags::{
    DeploymentFlagSettingInput, DeploymentFlagSettingWire, DeploymentFlagSettingsResult,
    DeploymentFlagSettingsSet, DeploymentFlagSettingsSetParams, FeatureFlagWire, FlagInspectResult,
    FlagStatusWire, FlagUsageRefWire, FlagValueTypeWire, FlagWriteResult, FlagsCreate,
    FlagsCreateParams, FlagsInspect, FlagsInspectParams, FlagsList, FlagsListParams,
    FlagsListResult, FlagsUpdate, FlagsUpdateParams, ReleaseFlagSettingInput,
    ReleaseFlagSettingWire, ReleaseFlagSettingsResult, ReleaseFlagSettingsSet,
    ReleaseFlagSettingsSetParams,
};
pub use links::{
    EntityLinkWire, LinkEntityKindWire, LinkKindWire, LinkRemoveResult, LinkWriteResult, LinksAdd,
    LinksAddParams, LinksList, LinksListParams, LinksListResult, LinksRemove, LinksRemoveParams,
};
pub use releases::{
    ChangelogWriteResult, MilestoneProgressWire, MilestoneWire, MilestoneWriteResult,
    MilestonesCreate, MilestonesCreateParams, MilestonesUpdate, MilestonesUpdateParams,
    ReleaseInspectResult, ReleaseProgressWire, ReleaseSummaryWire, ReleaseWriteResult,
    ReleasesCreate, ReleasesCreateParams, ReleasesInspect, ReleasesInspectParams, ReleasesList,
    ReleasesListParams, ReleasesListResult, ReleasesSetChangelog, ReleasesSetChangelogParams,
    ReleasesUpdate, ReleasesUpdateParams, RepoCommitWire, ScopedChangelogWire,
};

#[cfg(test)]
mod tests;
