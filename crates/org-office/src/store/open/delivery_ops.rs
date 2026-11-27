//! Engine-independent `ReadOp` and `StoreOp` wrappers for the delivery family (`TASK-55`).

use org_docs::delivery::{ops_deployments, ops_features, ops_read, ops_releases};
use org_proto::delivery::*;
use org_proto::{ApiError, ErrorCode};
use org_store::{
    Actor, Change, Id, Operation, Outcome, Refusal, Sha256, Store, StoreError, UnitOfWork,
};
use serde::{de::DeserializeOwned, Serialize};

use crate::delivery::features::{decode_rejection, refusal, run_read, run_write};
use crate::delivery::ports::DeliveryRead;
use crate::project_docs::access::Denied;

use super::{OfficeEngine, ReadOp, StoreOp};

type Answer<T> = Result<Result<T, ApiError>, StoreError>;

fn settle<T>(result: Result<T, Denied>) -> Answer<T> {
    match result {
        Ok(value) => Ok(Ok(value)),
        Err(Denied::Refused(error)) => Ok(Err(error)),
        Err(Denied::Storage(error)) => Err(error.into()),
    }
}

fn operation<P: Serialize>(
    kind: &'static str,
    command_id: &str,
    caller: &str,
    params: &P,
) -> Result<Operation, ApiError> {
    let id = org_proto::is_uuid_v7(command_id)
        .then(|| Id::parse(command_id).ok())
        .flatten()
        .ok_or_else(|| {
            ApiError::new(ErrorCode::InvalidRequest, "command_id: a lowercase UUIDv7")
        })?;
    let fields = serde_json::to_vec(params).expect("delivery params encode");
    Ok(Operation {
        id,
        kind,
        actor: Id::parse(caller).map_or(Actor::System, Actor::Employee),
        fingerprint: Sha256::of_parts(&[kind.as_bytes(), b"\0", &fields]),
    })
}

fn submit<E, T, F>(store: &Store<E>, op: Operation, decide: F) -> Answer<T>
where
    E: OfficeEngine,
    T: DeserializeOwned,
    F: FnMut(&mut UnitOfWork<'_, E>) -> Result<Change, Refusal> + Send + 'static,
{
    let reply = match store.submit(Some(op), false, decide) {
        Ok(reply) => reply,
        Err(StoreError::Conflict) => {
            return Ok(Err(ApiError::new(
                ErrorCode::OperationReused,
                "command_id reused with a different request",
            )))
        }
        Err(error) => return Err(error),
    };
    if reply.outcome == Outcome::Rejected {
        return Ok(Err(decode_rejection(&reply.bytes)));
    }
    serde_json::from_slice(&reply.bytes)
        .map(Ok)
        .map_err(|e| StoreError::Invalid(format!("corrupt delivery reply: {e}")))
}

macro_rules! delivery_read_op {
    ($name:ident, $params:ty, $res:ty, $func:path) => {
        pub struct $name {
            pub caller: String,
            pub params: $params,
        }
        impl ReadOp for $name {
            type Out = Result<$res, ApiError>;
            fn read<S: DeliveryRead>(self, snapshot: &S) -> Answer<$res> {
                let p = self.params;
                settle(run_read(snapshot, &self.caller, &p.project, |v, c| {
                    $func(v, c, &p)
                }))
            }
        }
    };
}

macro_rules! delivery_write_op {
    ($name:ident, $method:literal, $params:ty, $res:ty, $func:path) => {
        pub struct $name {
            pub caller: String,
            pub params: $params,
        }
        impl StoreOp for $name {
            type Out = Answer<$res>;
            fn run<E: OfficeEngine>(self, store: &Store<E>) -> Self::Out {
                let op =
                    match operation($method, &self.params.command_id, &self.caller, &self.params) {
                        Ok(op) => op,
                        Err(error) => return Ok(Err(error)),
                    };
                let Self { caller, params } = self;
                submit(store, op, move |uow| {
                    run_write(uow.view(), uow.now(), &caller, &params.project, |v, c| {
                        $func(v, c, &params)
                    })
                    .map_err(refusal)
                })
            }
        }
    };
}

delivery_read_op!(
    FeaturesListRead,
    FeaturesListParams,
    FeaturesListResult,
    ops_read::list_features
);
delivery_read_op!(
    FeaturesInspectRead,
    FeaturesInspectParams,
    FeatureInspectResult,
    ops_read::inspect_feature
);
delivery_read_op!(
    LinksListRead,
    LinksListParams,
    LinksListResult,
    ops_read::list_links
);
delivery_read_op!(
    ReleasesListRead,
    ReleasesListParams,
    ReleasesListResult,
    ops_read::list_releases
);
delivery_read_op!(
    ReleasesInspectRead,
    ReleasesInspectParams,
    ReleaseInspectResult,
    ops_read::inspect_release
);
delivery_read_op!(
    DeploymentsListRead,
    DeploymentsListParams,
    DeploymentsListResult,
    ops_read::list_deployments
);
delivery_read_op!(
    DeploymentsInspectRead,
    DeploymentsInspectParams,
    DeploymentInspectResult,
    ops_read::inspect_deployment
);
delivery_read_op!(
    FlagsListRead,
    FlagsListParams,
    FlagsListResult,
    ops_read::list_flags
);
delivery_read_op!(
    FlagsInspectRead,
    FlagsInspectParams,
    FlagInspectResult,
    ops_read::inspect_flag
);

delivery_write_op!(
    FeaturesCreateOp,
    "features.create",
    FeaturesCreateParams,
    FeatureWriteResult,
    ops_features::decide_create_feature
);
delivery_write_op!(
    FeaturesUpdateOp,
    "features.update",
    FeaturesUpdateParams,
    FeatureWriteResult,
    ops_features::decide_update_feature
);
delivery_write_op!(
    FeaturesRetireOp,
    "features.retire",
    FeaturesRetireParams,
    FeatureWriteResult,
    ops_features::decide_retire_feature
);
delivery_write_op!(
    LinksAddOp,
    "links.add",
    LinksAddParams,
    LinkWriteResult,
    ops_features::decide_add_link
);
delivery_write_op!(
    LinksRemoveOp,
    "links.remove",
    LinksRemoveParams,
    LinkRemoveResult,
    ops_features::decide_remove_link
);
delivery_write_op!(
    ReleasesCreateOp,
    "releases.create",
    ReleasesCreateParams,
    ReleaseWriteResult,
    ops_releases::decide_create_release
);
delivery_write_op!(
    ReleasesUpdateOp,
    "releases.update",
    ReleasesUpdateParams,
    ReleaseWriteResult,
    ops_releases::decide_update_release
);
delivery_write_op!(
    ReleasesSetChangelogOp,
    "releases.set_changelog",
    ReleasesSetChangelogParams,
    ChangelogWriteResult,
    ops_releases::decide_set_release_changelog
);
delivery_write_op!(
    MilestonesCreateOp,
    "milestones.create",
    MilestonesCreateParams,
    MilestoneWriteResult,
    ops_releases::decide_create_milestone
);
delivery_write_op!(
    MilestonesUpdateOp,
    "milestones.update",
    MilestonesUpdateParams,
    MilestoneWriteResult,
    ops_releases::decide_update_milestone
);
delivery_write_op!(
    ReleaseFlagSettingsSetOp,
    "releases.set_flag_settings",
    ReleaseFlagSettingsSetParams,
    ReleaseFlagSettingsResult,
    ops_releases::decide_set_release_flag_settings
);
delivery_write_op!(
    DeploymentsCreateOp,
    "deployments.create",
    DeploymentsCreateParams,
    DeploymentWriteResult,
    ops_deployments::decide_create_deployment
);
delivery_write_op!(
    DeploymentsUpdateOp,
    "deployments.update",
    DeploymentsUpdateParams,
    DeploymentWriteResult,
    ops_deployments::decide_update_deployment
);
delivery_write_op!(
    DeploymentsSetChangelogOp,
    "deployments.set_changelog",
    DeploymentsSetChangelogParams,
    ChangelogWriteResult,
    ops_deployments::decide_set_deployment_changelog
);
delivery_write_op!(
    FlagsCreateOp,
    "flags.create",
    FlagsCreateParams,
    FlagWriteResult,
    ops_deployments::decide_create_flag
);
delivery_write_op!(
    FlagsUpdateOp,
    "flags.update",
    FlagsUpdateParams,
    FlagWriteResult,
    ops_deployments::decide_update_flag
);
delivery_write_op!(
    DeploymentFlagSettingsSetOp,
    "deployments.set_flag_settings",
    DeploymentFlagSettingsSetParams,
    DeploymentFlagSettingsResult,
    ops_deployments::decide_set_deployment_flag_settings
);
