//! Office RPC dispatcher for the delivery & knowledge-graph family (`TASK-55`, `L7`/`L11`,
//! `FC-058..070`): `features.*`, `links.*`, `releases.*`, `milestones.*`, `deployments.*`, and
//! `flags.*`. Every call uses an explicit `request.is::<Call>()` check so the host operation
//! inventory AST scan matches every served method 1:1 with `operations.json`.

use org_office::{Office, OfficeError};
use org_proto::delivery::{
    DeploymentFlagSettingsSet, DeploymentsCreate, DeploymentsInspect, DeploymentsList,
    DeploymentsSetChangelog, DeploymentsUpdate, FeaturesCreate, FeaturesInspect, FeaturesList,
    FeaturesRetire, FeaturesUpdate, FlagsCreate, FlagsInspect, FlagsList, FlagsUpdate, LinksAdd,
    LinksList, LinksRemove, MilestonesCreate, MilestonesUpdate, ReleaseFlagSettingsSet,
    ReleasesCreate, ReleasesInspect, ReleasesList, ReleasesSetChangelog, ReleasesUpdate,
};
use org_proto::{ApiError, Call, ErrorCode, Request, Response};

const DELIVERY_METHODS: &[&str] = &[
    FeaturesList::METHOD,
    FeaturesInspect::METHOD,
    FeaturesCreate::METHOD,
    FeaturesUpdate::METHOD,
    FeaturesRetire::METHOD,
    LinksList::METHOD,
    LinksAdd::METHOD,
    LinksRemove::METHOD,
    ReleasesList::METHOD,
    ReleasesInspect::METHOD,
    ReleasesCreate::METHOD,
    ReleasesUpdate::METHOD,
    ReleasesSetChangelog::METHOD,
    MilestonesCreate::METHOD,
    MilestonesUpdate::METHOD,
    ReleaseFlagSettingsSet::METHOD,
    DeploymentsList::METHOD,
    DeploymentsInspect::METHOD,
    DeploymentsCreate::METHOD,
    DeploymentsUpdate::METHOD,
    DeploymentsSetChangelog::METHOD,
    DeploymentFlagSettingsSet::METHOD,
    FlagsList::METHOD,
    FlagsInspect::METHOD,
    FlagsCreate::METHOD,
    FlagsUpdate::METHOD,
];

/// Whether `request` belongs to the delivery family.
pub(crate) fn is_delivery_method(request: &Request) -> bool {
    DELIVERY_METHODS.contains(&request.method.as_str())
}

fn dispatch_features_or_links(office: &Office, caller: &str, req: &Request) -> Option<Response> {
    Some(if req.is::<FeaturesList>() {
        serve::<FeaturesList>(req, |p| office.features_list(caller, p))
    } else if req.is::<FeaturesInspect>() {
        serve::<FeaturesInspect>(req, |p| office.features_inspect(caller, p))
    } else if req.is::<FeaturesCreate>() {
        serve::<FeaturesCreate>(req, |p| office.features_create(caller, p))
    } else if req.is::<FeaturesUpdate>() {
        serve::<FeaturesUpdate>(req, |p| office.features_update(caller, p))
    } else if req.is::<FeaturesRetire>() {
        serve::<FeaturesRetire>(req, |p| office.features_retire(caller, p))
    } else if req.is::<LinksList>() {
        serve::<LinksList>(req, |p| office.links_list(caller, p))
    } else if req.is::<LinksAdd>() {
        serve::<LinksAdd>(req, |p| office.links_add(caller, p))
    } else if req.is::<LinksRemove>() {
        serve::<LinksRemove>(req, |p| office.links_remove(caller, p))
    } else {
        return None;
    })
}

fn dispatch_releases_or_milestones(
    office: &Office,
    caller: &str,
    req: &Request,
) -> Option<Response> {
    Some(if req.is::<ReleasesList>() {
        serve::<ReleasesList>(req, |p| office.releases_list(caller, p))
    } else if req.is::<ReleasesInspect>() {
        serve::<ReleasesInspect>(req, |p| office.releases_inspect(caller, p))
    } else if req.is::<ReleasesCreate>() {
        serve::<ReleasesCreate>(req, |p| office.releases_create(caller, p))
    } else if req.is::<ReleasesUpdate>() {
        serve::<ReleasesUpdate>(req, |p| office.releases_update(caller, p))
    } else if req.is::<ReleasesSetChangelog>() {
        serve::<ReleasesSetChangelog>(req, |p| office.releases_set_changelog(caller, p))
    } else if req.is::<MilestonesCreate>() {
        serve::<MilestonesCreate>(req, |p| office.milestones_create(caller, p))
    } else if req.is::<MilestonesUpdate>() {
        serve::<MilestonesUpdate>(req, |p| office.milestones_update(caller, p))
    } else if req.is::<ReleaseFlagSettingsSet>() {
        serve::<ReleaseFlagSettingsSet>(req, |p| office.releases_set_flag_settings(caller, p))
    } else {
        return None;
    })
}

fn dispatch_deployments(office: &Office, caller: &str, req: &Request) -> Option<Response> {
    Some(if req.is::<DeploymentsList>() {
        serve::<DeploymentsList>(req, |p| office.deployments_list(caller, p))
    } else if req.is::<DeploymentsInspect>() {
        serve::<DeploymentsInspect>(req, |p| office.deployments_inspect(caller, p))
    } else if req.is::<DeploymentsCreate>() {
        serve::<DeploymentsCreate>(req, |p| office.deployments_create(caller, p))
    } else if req.is::<DeploymentsUpdate>() {
        serve::<DeploymentsUpdate>(req, |p| office.deployments_update(caller, p))
    } else if req.is::<DeploymentsSetChangelog>() {
        serve::<DeploymentsSetChangelog>(req, |p| office.deployments_set_changelog(caller, p))
    } else if req.is::<DeploymentFlagSettingsSet>() {
        serve::<DeploymentFlagSettingsSet>(req, |p| office.deployments_set_flag_settings(caller, p))
    } else {
        return None;
    })
}

fn dispatch_flags(office: &Office, caller: &str, req: &Request) -> Option<Response> {
    Some(if req.is::<FlagsList>() {
        serve::<FlagsList>(req, |p| office.flags_list(caller, p))
    } else if req.is::<FlagsInspect>() {
        serve::<FlagsInspect>(req, |p| office.flags_inspect(caller, p))
    } else if req.is::<FlagsCreate>() {
        serve::<FlagsCreate>(req, |p| office.flags_create(caller, p))
    } else if req.is::<FlagsUpdate>() {
        serve::<FlagsUpdate>(req, |p| office.flags_update(caller, p))
    } else {
        return None;
    })
}

/// Dispatches one delivery RPC for `member` against `office`; `None` for any other method.
pub(crate) fn dispatch_delivery_rpc(
    office: &Office,
    member: Option<&str>,
    request: &Request,
) -> Option<Response> {
    if !is_delivery_method(request) {
        return None;
    }
    let Some(caller) = member else {
        return Some(Response::error(
            request.id,
            ApiError::new(ErrorCode::NotBound, "connection is not bound"),
        ));
    };
    dispatch_features_or_links(office, caller, request)
        .or_else(|| dispatch_releases_or_milestones(office, caller, request))
        .or_else(|| dispatch_deployments(office, caller, request))
        .or_else(|| dispatch_flags(office, caller, request))
}

fn serve<C: Call>(
    request: &Request,
    handler: impl FnOnce(C::Params) -> Result<Result<C::Result, ApiError>, OfficeError>,
) -> Response {
    let Ok(params) = request.params::<C>() else {
        return Response::error(
            request.id,
            ApiError::new(
                ErrorCode::InvalidRequest,
                format!("invalid parameters for `{}`", C::METHOD),
            ),
        );
    };
    let answer = handler(params)
        .map_err(|e| ApiError::new(ErrorCode::StorageError, e.to_string()))
        .and_then(|r| r);
    match answer {
        Ok(result) => Response::ok::<C>(request.id, &result).unwrap_or_else(|_| {
            Response::error(
                request.id,
                ApiError::new(ErrorCode::StorageError, "failed encoding response"),
            )
        }),
        Err(error) => Response::error(request.id, error),
    }
}
