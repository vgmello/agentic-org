//! Office facade for delivery calls (`features.*`, `links.*`, `releases.*`, `milestones.*`,
//! `deployments.*`, `flags.*`). HQ owns writes; satellites serve reads from their replicated copy.

use org_proto::delivery::*;
use org_proto::{ApiError, ErrorCode};

use crate::error::OfficeError;
use crate::model::OfficeRole;
use crate::store::open::delivery_ops;

use super::{storage, Office};

type Answer<T> = Result<Result<T, ApiError>, OfficeError>;

fn hq_only() -> ApiError {
    ApiError::new(
        ErrorCode::Unavailable,
        "project delivery records are edited at HQ; this satellite reads its replicated copy",
    )
}

macro_rules! office_delivery_reads {
    ($($fn_name:ident($params:ty) -> $res:ty => $op:ident;)*) => {$(
        pub fn $fn_name(&self, caller: &str, params: $params) -> Answer<$res> {
            Ok(self.store.read(delivery_ops::$op { caller: caller.to_owned(), params }).map_err(storage)?.0)
        }
    )*};
}

macro_rules! office_delivery_writes {
    ($($fn_name:ident($params:ty) -> $res:ty => $op:ident;)*) => {$(
        pub fn $fn_name(&self, caller: &str, params: $params) -> Answer<$res> {
            if self.info.role == OfficeRole::Satellite {
                return Ok(Err(hq_only()));
            }
            self.store.run(delivery_ops::$op { caller: caller.to_owned(), params }).map_err(storage)
        }
    )*};
}

impl Office {
    office_delivery_reads! {
        features_list(FeaturesListParams) -> FeaturesListResult => FeaturesListRead;
        features_inspect(FeaturesInspectParams) -> FeatureInspectResult => FeaturesInspectRead;
        links_list(LinksListParams) -> LinksListResult => LinksListRead;
        releases_list(ReleasesListParams) -> ReleasesListResult => ReleasesListRead;
        releases_inspect(ReleasesInspectParams) -> ReleaseInspectResult => ReleasesInspectRead;
        deployments_list(DeploymentsListParams) -> DeploymentsListResult => DeploymentsListRead;
        deployments_inspect(DeploymentsInspectParams) -> DeploymentInspectResult => DeploymentsInspectRead;
        flags_list(FlagsListParams) -> FlagsListResult => FlagsListRead;
        flags_inspect(FlagsInspectParams) -> FlagInspectResult => FlagsInspectRead;
    }

    office_delivery_writes! {
        features_create(FeaturesCreateParams) -> FeatureWriteResult => FeaturesCreateOp;
        features_update(FeaturesUpdateParams) -> FeatureWriteResult => FeaturesUpdateOp;
        features_retire(FeaturesRetireParams) -> FeatureWriteResult => FeaturesRetireOp;
        links_add(LinksAddParams) -> LinkWriteResult => LinksAddOp;
        links_remove(LinksRemoveParams) -> LinkRemoveResult => LinksRemoveOp;
        releases_create(ReleasesCreateParams) -> ReleaseWriteResult => ReleasesCreateOp;
        releases_update(ReleasesUpdateParams) -> ReleaseWriteResult => ReleasesUpdateOp;
        releases_set_changelog(ReleasesSetChangelogParams) -> ChangelogWriteResult => ReleasesSetChangelogOp;
        milestones_create(MilestonesCreateParams) -> MilestoneWriteResult => MilestonesCreateOp;
        milestones_update(MilestonesUpdateParams) -> MilestoneWriteResult => MilestonesUpdateOp;
        releases_set_flag_settings(ReleaseFlagSettingsSetParams) -> ReleaseFlagSettingsResult => ReleaseFlagSettingsSetOp;
        deployments_create(DeploymentsCreateParams) -> DeploymentWriteResult => DeploymentsCreateOp;
        deployments_update(DeploymentsUpdateParams) -> DeploymentWriteResult => DeploymentsUpdateOp;
        deployments_set_changelog(DeploymentsSetChangelogParams) -> ChangelogWriteResult => DeploymentsSetChangelogOp;
        flags_create(FlagsCreateParams) -> FlagWriteResult => FlagsCreateOp;
        flags_update(FlagsUpdateParams) -> FlagWriteResult => FlagsUpdateOp;
        deployments_set_flag_settings(DeploymentFlagSettingsSetParams) -> DeploymentFlagSettingsResult => DeploymentFlagSettingsSetOp;
    }
}
