//! Engine-independent `ReadOp` and `StoreOp` wrappers for the delivery family (`TASK-55`).

use org_docs::delivery::{ops_deployments, ops_features, ops_read, ops_releases};
use org_proto::delivery::*;
use org_proto::{ApiError, ErrorCode};
use org_store::{Store, StoreError};

use crate::delivery::features::{run_read, run_write};
use crate::delivery::ports::DeliveryRead;
use crate::project_docs::access::Denied;

use super::{build_operation, submit_api_op, OfficeEngine, ReadOp, StoreOp};

type Answer<T> = Result<Result<T, ApiError>, StoreError>;

fn decode_err(bytes: &[u8]) -> ApiError {
    Denied::decode_rejection(
        bytes,
        ErrorCode::StorageError,
        "delivery write was rejected without detail",
    )
}

fn settle<T>(result: Result<T, Denied>) -> Answer<T> {
    match result {
        Ok(value) => Ok(Ok(value)),
        Err(Denied::Refused(error)) => Ok(Err(error)),
        Err(Denied::Storage(error)) => Err(error.into()),
    }
}

macro_rules! delivery_read_ops {
    ($($name:ident($params:ty) -> $res:ty => $func:path;)*) => {$(
        pub struct $name {
            pub caller: String,
            pub params: $params,
        }
        impl ReadOp for $name {
            type Out = Result<$res, ApiError>;
            fn read<S: DeliveryRead>(self, snapshot: &S) -> Answer<$res> {
                let p = self.params;
                settle(run_read(snapshot, &self.caller, &p.project, |v, c| $func(v, c, &p)))
            }
        }
    )*};
}

macro_rules! delivery_write_ops {
    ($($name:ident($method:literal, $params:ty) -> $res:ty => $func:path;)*) => {$(
        pub struct $name {
            pub caller: String,
            pub params: $params,
        }
        impl StoreOp for $name {
            type Out = Answer<$res>;
            fn run<E: OfficeEngine>(self, store: &Store<E>) -> Self::Out {
                let op = match build_operation($method, &self.params.command_id, &self.caller, &self.params) {
                    Ok(op) => op,
                    Err(error) => return Ok(Err(error)),
                };
                let Self { caller, params } = self;
                submit_api_op(store, op, "delivery", decode_err, move |uow| {
                    run_write(uow.view(), uow.now(), &caller, &params.project, |v, c| $func(v, c, &params))
                        .map_err(Denied::into_refusal)
                })
            }
        }
    )*};
}

delivery_read_ops! {
    FeaturesListRead(FeaturesListParams) -> FeaturesListResult => ops_read::list_features;
    FeaturesInspectRead(FeaturesInspectParams) -> FeatureInspectResult => ops_read::inspect_feature;
    LinksListRead(LinksListParams) -> LinksListResult => ops_read::list_links;
    ReleasesListRead(ReleasesListParams) -> ReleasesListResult => ops_read::list_releases;
    ReleasesInspectRead(ReleasesInspectParams) -> ReleaseInspectResult => ops_read::inspect_release;
    DeploymentsListRead(DeploymentsListParams) -> DeploymentsListResult => ops_read::list_deployments;
    DeploymentsInspectRead(DeploymentsInspectParams) -> DeploymentInspectResult => ops_read::inspect_deployment;
    FlagsListRead(FlagsListParams) -> FlagsListResult => ops_read::list_flags;
    FlagsInspectRead(FlagsInspectParams) -> FlagInspectResult => ops_read::inspect_flag;
}

delivery_write_ops! {
    FeaturesCreateOp("features.create", FeaturesCreateParams) -> FeatureWriteResult => ops_features::decide_create_feature;
    FeaturesUpdateOp("features.update", FeaturesUpdateParams) -> FeatureWriteResult => ops_features::decide_update_feature;
    FeaturesRetireOp("features.retire", FeaturesRetireParams) -> FeatureWriteResult => ops_features::decide_retire_feature;
    LinksAddOp("links.add", LinksAddParams) -> LinkWriteResult => ops_features::decide_add_link;
    LinksRemoveOp("links.remove", LinksRemoveParams) -> LinkRemoveResult => ops_features::decide_remove_link;
    ReleasesCreateOp("releases.create", ReleasesCreateParams) -> ReleaseWriteResult => ops_releases::decide_create_release;
    ReleasesUpdateOp("releases.update", ReleasesUpdateParams) -> ReleaseWriteResult => ops_releases::decide_update_release;
    ReleasesSetChangelogOp("releases.set_changelog", ReleasesSetChangelogParams) -> ChangelogWriteResult => ops_releases::decide_set_release_changelog;
    MilestonesCreateOp("milestones.create", MilestonesCreateParams) -> MilestoneWriteResult => ops_releases::decide_create_milestone;
    MilestonesUpdateOp("milestones.update", MilestonesUpdateParams) -> MilestoneWriteResult => ops_releases::decide_update_milestone;
    ReleaseFlagSettingsSetOp("releases.set_flag_settings", ReleaseFlagSettingsSetParams) -> ReleaseFlagSettingsResult => ops_releases::decide_set_release_flag_settings;
    DeploymentsCreateOp("deployments.create", DeploymentsCreateParams) -> DeploymentWriteResult => ops_deployments::decide_create_deployment;
    DeploymentsUpdateOp("deployments.update", DeploymentsUpdateParams) -> DeploymentWriteResult => ops_deployments::decide_update_deployment;
    DeploymentsSetChangelogOp("deployments.set_changelog", DeploymentsSetChangelogParams) -> ChangelogWriteResult => ops_deployments::decide_set_deployment_changelog;
    FlagsCreateOp("flags.create", FlagsCreateParams) -> FlagWriteResult => ops_deployments::decide_create_flag;
    FlagsUpdateOp("flags.update", FlagsUpdateParams) -> FlagWriteResult => ops_deployments::decide_update_flag;
    DeploymentFlagSettingsSetOp("deployments.set_flag_settings", DeploymentFlagSettingsSetParams) -> DeploymentFlagSettingsResult => ops_deployments::decide_set_deployment_flag_settings;
}
