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

macro_rules! office_delivery_read {
    ($fn_name:ident, $params:ty, $res:ty, $op:ident) => {
        pub fn $fn_name(&self, caller: &str, params: $params) -> Answer<$res> {
            Ok(self
                .store
                .read(delivery_ops::$op {
                    caller: caller.to_owned(),
                    params,
                })
                .map_err(storage)?
                .0)
        }
    };
}

macro_rules! office_delivery_write {
    ($fn_name:ident, $params:ty, $res:ty, $op:ident) => {
        pub fn $fn_name(&self, caller: &str, params: $params) -> Answer<$res> {
            if self.info.role == OfficeRole::Satellite {
                return Ok(Err(hq_only()));
            }
            self.store
                .run(delivery_ops::$op {
                    caller: caller.to_owned(),
                    params,
                })
                .map_err(storage)
        }
    };
}

impl Office {
    office_delivery_read!(
        features_list,
        FeaturesListParams,
        FeaturesListResult,
        FeaturesListRead
    );
    office_delivery_read!(
        features_inspect,
        FeaturesInspectParams,
        FeatureInspectResult,
        FeaturesInspectRead
    );
    office_delivery_write!(
        features_create,
        FeaturesCreateParams,
        FeatureWriteResult,
        FeaturesCreateOp
    );
    office_delivery_write!(
        features_update,
        FeaturesUpdateParams,
        FeatureWriteResult,
        FeaturesUpdateOp
    );
    office_delivery_write!(
        features_retire,
        FeaturesRetireParams,
        FeatureWriteResult,
        FeaturesRetireOp
    );

    office_delivery_read!(links_list, LinksListParams, LinksListResult, LinksListRead);
    office_delivery_write!(links_add, LinksAddParams, LinkWriteResult, LinksAddOp);
    office_delivery_write!(
        links_remove,
        LinksRemoveParams,
        LinkRemoveResult,
        LinksRemoveOp
    );

    office_delivery_read!(
        releases_list,
        ReleasesListParams,
        ReleasesListResult,
        ReleasesListRead
    );
    office_delivery_read!(
        releases_inspect,
        ReleasesInspectParams,
        ReleaseInspectResult,
        ReleasesInspectRead
    );
    office_delivery_write!(
        releases_create,
        ReleasesCreateParams,
        ReleaseWriteResult,
        ReleasesCreateOp
    );
    office_delivery_write!(
        releases_update,
        ReleasesUpdateParams,
        ReleaseWriteResult,
        ReleasesUpdateOp
    );
    office_delivery_write!(
        releases_set_changelog,
        ReleasesSetChangelogParams,
        ChangelogWriteResult,
        ReleasesSetChangelogOp
    );
    office_delivery_write!(
        milestones_create,
        MilestonesCreateParams,
        MilestoneWriteResult,
        MilestonesCreateOp
    );
    office_delivery_write!(
        milestones_update,
        MilestonesUpdateParams,
        MilestoneWriteResult,
        MilestonesUpdateOp
    );
    office_delivery_write!(
        releases_set_flag_settings,
        ReleaseFlagSettingsSetParams,
        ReleaseFlagSettingsResult,
        ReleaseFlagSettingsSetOp
    );

    office_delivery_read!(
        deployments_list,
        DeploymentsListParams,
        DeploymentsListResult,
        DeploymentsListRead
    );
    office_delivery_read!(
        deployments_inspect,
        DeploymentsInspectParams,
        DeploymentInspectResult,
        DeploymentsInspectRead
    );
    office_delivery_write!(
        deployments_create,
        DeploymentsCreateParams,
        DeploymentWriteResult,
        DeploymentsCreateOp
    );
    office_delivery_write!(
        deployments_update,
        DeploymentsUpdateParams,
        DeploymentWriteResult,
        DeploymentsUpdateOp
    );
    office_delivery_write!(
        deployments_set_changelog,
        DeploymentsSetChangelogParams,
        ChangelogWriteResult,
        DeploymentsSetChangelogOp
    );

    office_delivery_read!(flags_list, FlagsListParams, FlagsListResult, FlagsListRead);
    office_delivery_read!(
        flags_inspect,
        FlagsInspectParams,
        FlagInspectResult,
        FlagsInspectRead
    );
    office_delivery_write!(
        flags_create,
        FlagsCreateParams,
        FlagWriteResult,
        FlagsCreateOp
    );
    office_delivery_write!(
        flags_update,
        FlagsUpdateParams,
        FlagWriteResult,
        FlagsUpdateOp
    );
    office_delivery_write!(
        deployments_set_flag_settings,
        DeploymentFlagSettingsSetParams,
        DeploymentFlagSettingsResult,
        DeploymentFlagSettingsSetOp
    );
}
