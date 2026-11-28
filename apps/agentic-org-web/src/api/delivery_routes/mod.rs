//! Browser HTTP routes for canonical features, Why links, releases, milestones, changelogs,
//! deployments, and feature flags (`TASK-55`, `L7`/`L11`, `FC-058..070`), forwarded to the
//! organization's HQ office as the signed-in member:
//!
//! - `GET/POST .../projects/{project}/features`
//! - `GET/PATCH .../projects/{project}/features/{feature}`, `POST .../features/{id}/retire`
//! - `GET/POST .../projects/{project}/links`, `POST .../links/remove`
//! - `GET/POST .../projects/{project}/releases`, `GET/PATCH .../releases/{release}`
//! - `POST .../releases/{id}/changelog`, `POST .../releases/{id}/flag-settings`
//! - `POST .../releases/{id}/milestones`, `PATCH .../releases/{id}/milestones/{mid}`
//! - `GET/POST .../projects/{project}/deployments`, `GET/PATCH .../deployments/{id}`
//! - `POST .../deployments/{id}/changelog`, `POST .../deployments/{id}/flag-settings`
//! - `GET/POST .../projects/{project}/flags`, `GET/PATCH .../flags/{flag}`
//!
//! Path identifiers always win over any duplicate fields in the JSON body.

mod parse;

use hyper::{http::request::Parts, Method};
use org_proto::delivery::{
    DeploymentFlagSettingsSet, DeploymentFlagSettingsSetParams, DeploymentsCreate,
    DeploymentsCreateParams, DeploymentsInspect, DeploymentsInspectParams, DeploymentsList,
    DeploymentsListParams, DeploymentsSetChangelog, DeploymentsSetChangelogParams,
    DeploymentsUpdate, DeploymentsUpdateParams, FeaturesCreate, FeaturesCreateParams,
    FeaturesInspect, FeaturesInspectParams, FeaturesList, FeaturesListParams, FeaturesRetire,
    FeaturesRetireParams, FeaturesUpdate, FeaturesUpdateParams, FlagsCreate, FlagsCreateParams,
    FlagsInspect, FlagsInspectParams, FlagsList, FlagsListParams, FlagsUpdate, FlagsUpdateParams,
    LinksAdd, LinksAddParams, LinksList, LinksListParams, LinksRemove, LinksRemoveParams,
    MilestonesCreate, MilestonesCreateParams, MilestonesUpdate, MilestonesUpdateParams,
    ReleaseFlagSettingsSet, ReleaseFlagSettingsSetParams, ReleasesCreate, ReleasesCreateParams,
    ReleasesInspect, ReleasesInspectParams, ReleasesList, ReleasesListParams, ReleasesSetChangelog,
    ReleasesSetChangelogParams, ReleasesUpdate, ReleasesUpdateParams,
};

use super::doc_routes::{decode_body, run_office, valid_command_id, valid_org_id, valid_project};
use super::{invalid, not_found, Call, CallerAuth, Route, SessionApi};
use crate::service::{ApiReply, PreparedCall};
use parse::{
    parse_deployments_query, parse_features_query, parse_flags_query, parse_links_query, valid_ref,
    valid_uuid,
};

pub enum DeliveryRoute {
    FeaturesList(CallerAuth, String, FeaturesListParams),
    FeaturesInspect(CallerAuth, String, String, String),
    FeaturesCreate(CallerAuth, String, String),
    FeaturesUpdate(CallerAuth, String, String, String),
    FeaturesRetire(CallerAuth, String, String, String),
    LinksList(CallerAuth, String, LinksListParams),
    LinksAdd(CallerAuth, String, String),
    LinksRemove(CallerAuth, String, String),
    ReleasesList(CallerAuth, String, String),
    ReleasesInspect(CallerAuth, String, String, String),
    ReleasesCreate(CallerAuth, String, String),
    ReleasesUpdate(CallerAuth, String, String, String),
    ReleasesSetChangelog(CallerAuth, String, String, String),
    ReleasesSetFlagSettings(CallerAuth, String, String, String),
    MilestonesCreate(CallerAuth, String, String, String),
    MilestonesUpdate(CallerAuth, String, String, String, String),
    DeploymentsList(CallerAuth, String, DeploymentsListParams),
    DeploymentsInspect(CallerAuth, String, String, String),
    DeploymentsCreate(CallerAuth, String, String),
    DeploymentsUpdate(CallerAuth, String, String, String),
    DeploymentsSetChangelog(CallerAuth, String, String, String),
    DeploymentsSetFlagSettings(CallerAuth, String, String, String),
    FlagsList(CallerAuth, String, FlagsListParams),
    FlagsInspect(CallerAuth, String, String, String),
    FlagsCreate(CallerAuth, String, String),
    FlagsUpdate(CallerAuth, String, String, String),
}

pub enum DeliveryCall {
    FeaturesList(CallerAuth, String, FeaturesListParams),
    FeaturesInspect(CallerAuth, String, FeaturesInspectParams),
    FeaturesCreate(CallerAuth, String, FeaturesCreateParams),
    FeaturesUpdate(CallerAuth, String, FeaturesUpdateParams),
    FeaturesRetire(CallerAuth, String, FeaturesRetireParams),
    LinksList(CallerAuth, String, LinksListParams),
    LinksAdd(CallerAuth, String, LinksAddParams),
    LinksRemove(CallerAuth, String, LinksRemoveParams),
    ReleasesList(CallerAuth, String, ReleasesListParams),
    ReleasesInspect(CallerAuth, String, ReleasesInspectParams),
    ReleasesCreate(CallerAuth, String, ReleasesCreateParams),
    ReleasesUpdate(CallerAuth, String, ReleasesUpdateParams),
    ReleasesSetChangelog(CallerAuth, String, ReleasesSetChangelogParams),
    ReleasesSetFlagSettings(CallerAuth, String, ReleaseFlagSettingsSetParams),
    MilestonesCreate(CallerAuth, String, MilestonesCreateParams),
    MilestonesUpdate(CallerAuth, String, MilestonesUpdateParams),
    DeploymentsList(CallerAuth, String, DeploymentsListParams),
    DeploymentsInspect(CallerAuth, String, DeploymentsInspectParams),
    DeploymentsCreate(CallerAuth, String, DeploymentsCreateParams),
    DeploymentsUpdate(CallerAuth, String, DeploymentsUpdateParams),
    DeploymentsSetChangelog(CallerAuth, String, DeploymentsSetChangelogParams),
    DeploymentsSetFlagSettings(CallerAuth, String, DeploymentFlagSettingsSetParams),
    FlagsList(CallerAuth, String, FlagsListParams),
    FlagsInspect(CallerAuth, String, FlagsInspectParams),
    FlagsCreate(CallerAuth, String, FlagsCreateParams),
    FlagsUpdate(CallerAuth, String, FlagsUpdateParams),
}

type RouteBuilder<'a> = &'a dyn Fn(CallerAuth, String, String) -> Result<DeliveryRoute, ApiReply>;

fn route_features(
    rest: &[&str],
    get: bool,
    post: bool,
    patch: bool,
    query: Option<&str>,
    route: &dyn Fn(RouteBuilder<'_>) -> Result<Route, ApiReply>,
) -> Result<Route, ApiReply> {
    match rest {
        [] if get => route(&|pair, org, project| {
            Ok(DeliveryRoute::FeaturesList(
                pair,
                org,
                parse_features_query(project, query)?,
            ))
        }),
        [] if post => route(&|p, o, pr| Ok(DeliveryRoute::FeaturesCreate(p, o, pr))),
        [f] if get && query.is_none() => {
            route(&|p, o, pr| Ok(DeliveryRoute::FeaturesInspect(p, o, pr, valid_ref(f)?)))
        }
        [id] if patch => {
            route(&|p, o, pr| Ok(DeliveryRoute::FeaturesUpdate(p, o, pr, valid_uuid(id)?)))
        }
        [id, "retire"] if post => {
            route(&|p, o, pr| Ok(DeliveryRoute::FeaturesRetire(p, o, pr, valid_uuid(id)?)))
        }
        [] | [_] | [_, "retire"] => Err(invalid()),
        _ => Err(not_found()),
    }
}

fn route_links(
    rest: &[&str],
    get: bool,
    post: bool,
    query: Option<&str>,
    route: &dyn Fn(RouteBuilder<'_>) -> Result<Route, ApiReply>,
) -> Result<Route, ApiReply> {
    match rest {
        [] if get => route(&|pair, org, project| {
            Ok(DeliveryRoute::LinksList(
                pair,
                org,
                parse_links_query(project, query)?,
            ))
        }),
        [] if post => route(&|p, o, pr| Ok(DeliveryRoute::LinksAdd(p, o, pr))),
        ["remove"] if post => route(&|p, o, pr| Ok(DeliveryRoute::LinksRemove(p, o, pr))),
        [] | ["remove"] => Err(invalid()),
        _ => Err(not_found()),
    }
}

fn route_releases(
    rest: &[&str],
    get: bool,
    post: bool,
    patch: bool,
    has_q: bool,
    route: &dyn Fn(RouteBuilder<'_>) -> Result<Route, ApiReply>,
) -> Result<Route, ApiReply> {
    match rest {
        [] if get && !has_q => route(&|p, o, pr| Ok(DeliveryRoute::ReleasesList(p, o, pr))),
        [] if post => route(&|p, o, pr| Ok(DeliveryRoute::ReleasesCreate(p, o, pr))),
        [r] if get && !has_q => {
            route(&|p, o, pr| Ok(DeliveryRoute::ReleasesInspect(p, o, pr, valid_ref(r)?)))
        }
        [id] if patch => {
            route(&|p, o, pr| Ok(DeliveryRoute::ReleasesUpdate(p, o, pr, valid_uuid(id)?)))
        }
        [id, "changelog"] if post => route(&|p, o, pr| {
            Ok(DeliveryRoute::ReleasesSetChangelog(
                p,
                o,
                pr,
                valid_uuid(id)?,
            ))
        }),
        [id, "flag-settings"] if post => route(&|p, o, pr| {
            Ok(DeliveryRoute::ReleasesSetFlagSettings(
                p,
                o,
                pr,
                valid_uuid(id)?,
            ))
        }),
        [id, "milestones"] if post => {
            route(&|p, o, pr| Ok(DeliveryRoute::MilestonesCreate(p, o, pr, valid_uuid(id)?)))
        }
        [rid, "milestones", mid] if patch => route(&|p, o, pr| {
            Ok(DeliveryRoute::MilestonesUpdate(
                p,
                o,
                pr,
                valid_uuid(rid)?,
                valid_uuid(mid)?,
            ))
        }),
        [] | [_] | [_, "changelog" | "flag-settings" | "milestones"] | [_, "milestones", _] => {
            Err(invalid())
        }
        _ => Err(not_found()),
    }
}

fn route_deployments(
    rest: &[&str],
    get: bool,
    post: bool,
    patch: bool,
    query: Option<&str>,
    route: &dyn Fn(RouteBuilder<'_>) -> Result<Route, ApiReply>,
) -> Result<Route, ApiReply> {
    match rest {
        [] if get => route(&|pair, org, project| {
            Ok(DeliveryRoute::DeploymentsList(
                pair,
                org,
                parse_deployments_query(project, query)?,
            ))
        }),
        [] if post => route(&|p, o, pr| Ok(DeliveryRoute::DeploymentsCreate(p, o, pr))),
        [id] if get && query.is_none() => {
            route(&|p, o, pr| Ok(DeliveryRoute::DeploymentsInspect(p, o, pr, valid_uuid(id)?)))
        }
        [id] if patch => {
            route(&|p, o, pr| Ok(DeliveryRoute::DeploymentsUpdate(p, o, pr, valid_uuid(id)?)))
        }
        [id, "changelog"] if post => route(&|p, o, pr| {
            Ok(DeliveryRoute::DeploymentsSetChangelog(
                p,
                o,
                pr,
                valid_uuid(id)?,
            ))
        }),
        [id, "flag-settings"] if post => route(&|p, o, pr| {
            Ok(DeliveryRoute::DeploymentsSetFlagSettings(
                p,
                o,
                pr,
                valid_uuid(id)?,
            ))
        }),
        [] | [_] | [_, "changelog" | "flag-settings"] => Err(invalid()),
        _ => Err(not_found()),
    }
}

fn route_flags(
    rest: &[&str],
    get: bool,
    post: bool,
    patch: bool,
    query: Option<&str>,
    route: &dyn Fn(RouteBuilder<'_>) -> Result<Route, ApiReply>,
) -> Result<Route, ApiReply> {
    match rest {
        [] if get => route(&|pair, org, project| {
            Ok(DeliveryRoute::FlagsList(
                pair,
                org,
                parse_flags_query(project, query)?,
            ))
        }),
        [] if post => route(&|p, o, pr| Ok(DeliveryRoute::FlagsCreate(p, o, pr))),
        [f] if get && query.is_none() => {
            route(&|p, o, pr| Ok(DeliveryRoute::FlagsInspect(p, o, pr, valid_ref(f)?)))
        }
        [id] if patch => {
            route(&|p, o, pr| Ok(DeliveryRoute::FlagsUpdate(p, o, pr, valid_uuid(id)?)))
        }
        [] | [_] => Err(invalid()),
        _ => Err(not_found()),
    }
}

/// Routes `projects/{project}/{features|links|releases|deployments|flags}/...`.
pub(super) fn try_route(
    api: &SessionApi,
    request: &Parts,
    org: &str,
    tail: &[&str],
) -> Option<Result<Route, ApiReply>> {
    let ["projects", project, resource, rest @ ..] = tail else {
        return None;
    };
    let query = request.uri.query();
    let has_q = query.is_some();
    let get = matches!(request.method, Method::GET | Method::HEAD);
    let post = !has_q && request.method == Method::POST;
    let patch = !has_q && request.method == Method::PATCH;
    let route = |make: RouteBuilder<'_>| {
        let pair = api.caller_auth(request)?;
        valid_org_id(org)?;
        let project = valid_project(project)?;
        Ok(Route::Delivery(make(pair, org.to_owned(), project)?))
    };
    Some(match *resource {
        "features" => route_features(rest, get, post, patch, query, &route),
        "links" => route_links(rest, get, post, query, &route),
        "releases" => route_releases(rest, get, post, patch, has_q, &route),
        "deployments" => route_deployments(rest, get, post, patch, query, &route),
        "flags" => route_flags(rest, get, post, patch, query, &route),
        _ => return None,
    })
}

pub(super) fn prepare(route: DeliveryRoute, bytes: &[u8]) -> Result<PreparedCall<Call>, ApiReply> {
    let call = to_call(route, bytes)?;
    Ok(PreparedCall::read(Call::Delivery(call), bytes.len()))
}

fn to_call(route: DeliveryRoute, bytes: &[u8]) -> Result<DeliveryCall, ApiReply> {
    let empty = || bytes.is_empty().then_some(()).ok_or_else(invalid);
    Ok(match route {
        DeliveryRoute::FeaturesList(p, o, params) => {
            empty()?;
            DeliveryCall::FeaturesList(p, o, params)
        }
        DeliveryRoute::FeaturesInspect(p, o, project, feature) => {
            empty()?;
            DeliveryCall::FeaturesInspect(p, o, FeaturesInspectParams { project, feature })
        }
        DeliveryRoute::FeaturesCreate(p, o, project) => {
            let mut params: FeaturesCreateParams = decode_body(bytes)?;
            valid_command_id(&params.command_id)?;
            params.project = project;
            DeliveryCall::FeaturesCreate(p, o, params)
        }
        DeliveryRoute::FeaturesUpdate(p, o, project, feature_id) => {
            let mut params: FeaturesUpdateParams = decode_body(bytes)?;
            valid_command_id(&params.command_id)?;
            (params.project, params.feature_id) = (project, feature_id);
            DeliveryCall::FeaturesUpdate(p, o, params)
        }
        DeliveryRoute::FeaturesRetire(p, o, project, feature_id) => {
            let mut params: FeaturesRetireParams = decode_body(bytes)?;
            valid_command_id(&params.command_id)?;
            (params.project, params.feature_id) = (project, feature_id);
            DeliveryCall::FeaturesRetire(p, o, params)
        }
        DeliveryRoute::LinksList(p, o, params) => {
            empty()?;
            DeliveryCall::LinksList(p, o, params)
        }
        DeliveryRoute::LinksAdd(p, o, project) => {
            let mut params: LinksAddParams = decode_body(bytes)?;
            valid_command_id(&params.command_id)?;
            params.project = project;
            DeliveryCall::LinksAdd(p, o, params)
        }
        DeliveryRoute::LinksRemove(p, o, project) => {
            let mut params: LinksRemoveParams = decode_body(bytes)?;
            valid_command_id(&params.command_id)?;
            params.project = project;
            DeliveryCall::LinksRemove(p, o, params)
        }
        DeliveryRoute::ReleasesList(p, o, project) => {
            empty()?;
            DeliveryCall::ReleasesList(p, o, ReleasesListParams { project })
        }
        DeliveryRoute::ReleasesInspect(p, o, project, release) => {
            empty()?;
            DeliveryCall::ReleasesInspect(p, o, ReleasesInspectParams { project, release })
        }
        DeliveryRoute::ReleasesCreate(p, o, project) => {
            let mut params: ReleasesCreateParams = decode_body(bytes)?;
            valid_command_id(&params.command_id)?;
            params.project = project;
            DeliveryCall::ReleasesCreate(p, o, params)
        }
        DeliveryRoute::ReleasesUpdate(p, o, project, release_id) => {
            let mut params: ReleasesUpdateParams = decode_body(bytes)?;
            valid_command_id(&params.command_id)?;
            (params.project, params.release_id) = (project, release_id);
            DeliveryCall::ReleasesUpdate(p, o, params)
        }
        DeliveryRoute::ReleasesSetChangelog(p, o, project, release_id) => {
            let mut params: ReleasesSetChangelogParams = decode_body(bytes)?;
            valid_command_id(&params.command_id)?;
            (params.project, params.release_id) = (project, release_id);
            DeliveryCall::ReleasesSetChangelog(p, o, params)
        }
        DeliveryRoute::ReleasesSetFlagSettings(p, o, project, release_id) => {
            let mut params: ReleaseFlagSettingsSetParams = decode_body(bytes)?;
            valid_command_id(&params.command_id)?;
            (params.project, params.release_id) = (project, release_id);
            DeliveryCall::ReleasesSetFlagSettings(p, o, params)
        }
        DeliveryRoute::MilestonesCreate(p, o, project, release_id) => {
            let mut params: MilestonesCreateParams = decode_body(bytes)?;
            valid_command_id(&params.command_id)?;
            (params.project, params.release_id) = (project, release_id);
            DeliveryCall::MilestonesCreate(p, o, params)
        }
        DeliveryRoute::MilestonesUpdate(p, o, project, release_id, milestone_id) => {
            let mut params: MilestonesUpdateParams = decode_body(bytes)?;
            valid_command_id(&params.command_id)?;
            params.project = project;
            params.release_id = Some(release_id);
            params.milestone_id = milestone_id;
            DeliveryCall::MilestonesUpdate(p, o, params)
        }
        DeliveryRoute::DeploymentsList(p, o, params) => {
            empty()?;
            DeliveryCall::DeploymentsList(p, o, params)
        }
        DeliveryRoute::DeploymentsInspect(p, o, project, deployment_id) => {
            empty()?;
            DeliveryCall::DeploymentsInspect(
                p,
                o,
                DeploymentsInspectParams {
                    project,
                    deployment_id,
                },
            )
        }
        DeliveryRoute::DeploymentsCreate(p, o, project) => {
            let mut params: DeploymentsCreateParams = decode_body(bytes)?;
            valid_command_id(&params.command_id)?;
            params.project = project;
            DeliveryCall::DeploymentsCreate(p, o, params)
        }
        DeliveryRoute::DeploymentsUpdate(p, o, project, deployment_id) => {
            let mut params: DeploymentsUpdateParams = decode_body(bytes)?;
            valid_command_id(&params.command_id)?;
            (params.project, params.deployment_id) = (project, deployment_id);
            DeliveryCall::DeploymentsUpdate(p, o, params)
        }
        DeliveryRoute::DeploymentsSetChangelog(p, o, project, deployment_id) => {
            let mut params: DeploymentsSetChangelogParams = decode_body(bytes)?;
            valid_command_id(&params.command_id)?;
            (params.project, params.deployment_id) = (project, deployment_id);
            DeliveryCall::DeploymentsSetChangelog(p, o, params)
        }
        DeliveryRoute::DeploymentsSetFlagSettings(p, o, project, deployment_id) => {
            let mut params: DeploymentFlagSettingsSetParams = decode_body(bytes)?;
            valid_command_id(&params.command_id)?;
            (params.project, params.deployment_id) = (project, deployment_id);
            DeliveryCall::DeploymentsSetFlagSettings(p, o, params)
        }
        DeliveryRoute::FlagsList(p, o, params) => {
            empty()?;
            DeliveryCall::FlagsList(p, o, params)
        }
        DeliveryRoute::FlagsInspect(p, o, project, flag) => {
            empty()?;
            DeliveryCall::FlagsInspect(p, o, FlagsInspectParams { project, flag })
        }
        DeliveryRoute::FlagsCreate(p, o, project) => {
            let mut params: FlagsCreateParams = decode_body(bytes)?;
            valid_command_id(&params.command_id)?;
            params.project = project;
            DeliveryCall::FlagsCreate(p, o, params)
        }
        DeliveryRoute::FlagsUpdate(p, o, project, flag_id) => {
            let mut params: FlagsUpdateParams = decode_body(bytes)?;
            valid_command_id(&params.command_id)?;
            (params.project, params.flag_id) = (project, flag_id);
            DeliveryCall::FlagsUpdate(p, o, params)
        }
    })
}

pub(super) async fn execute(api: &SessionApi, call: DeliveryCall) -> ApiReply {
    match call {
        DeliveryCall::FeaturesList(p, o, params) => {
            run_office::<FeaturesList>(api, p, &o, &params, true).await
        }
        DeliveryCall::FeaturesInspect(p, o, params) => {
            run_office::<FeaturesInspect>(api, p, &o, &params, true).await
        }
        DeliveryCall::FeaturesCreate(p, o, params) => {
            run_office::<FeaturesCreate>(api, p, &o, &params, false).await
        }
        DeliveryCall::FeaturesUpdate(p, o, params) => {
            run_office::<FeaturesUpdate>(api, p, &o, &params, false).await
        }
        DeliveryCall::FeaturesRetire(p, o, params) => {
            run_office::<FeaturesRetire>(api, p, &o, &params, false).await
        }
        DeliveryCall::LinksList(p, o, params) => {
            run_office::<LinksList>(api, p, &o, &params, true).await
        }
        DeliveryCall::LinksAdd(p, o, params) => {
            run_office::<LinksAdd>(api, p, &o, &params, false).await
        }
        DeliveryCall::LinksRemove(p, o, params) => {
            run_office::<LinksRemove>(api, p, &o, &params, false).await
        }
        DeliveryCall::ReleasesList(p, o, params) => {
            run_office::<ReleasesList>(api, p, &o, &params, true).await
        }
        DeliveryCall::ReleasesInspect(p, o, params) => {
            run_office::<ReleasesInspect>(api, p, &o, &params, true).await
        }
        DeliveryCall::ReleasesCreate(p, o, params) => {
            run_office::<ReleasesCreate>(api, p, &o, &params, false).await
        }
        DeliveryCall::ReleasesUpdate(p, o, params) => {
            run_office::<ReleasesUpdate>(api, p, &o, &params, false).await
        }
        DeliveryCall::ReleasesSetChangelog(p, o, params) => {
            run_office::<ReleasesSetChangelog>(api, p, &o, &params, false).await
        }
        DeliveryCall::ReleasesSetFlagSettings(p, o, params) => {
            run_office::<ReleaseFlagSettingsSet>(api, p, &o, &params, false).await
        }
        DeliveryCall::MilestonesCreate(p, o, params) => {
            run_office::<MilestonesCreate>(api, p, &o, &params, false).await
        }
        DeliveryCall::MilestonesUpdate(p, o, params) => {
            run_office::<MilestonesUpdate>(api, p, &o, &params, false).await
        }
        DeliveryCall::DeploymentsList(p, o, params) => {
            run_office::<DeploymentsList>(api, p, &o, &params, true).await
        }
        DeliveryCall::DeploymentsInspect(p, o, params) => {
            run_office::<DeploymentsInspect>(api, p, &o, &params, true).await
        }
        DeliveryCall::DeploymentsCreate(p, o, params) => {
            run_office::<DeploymentsCreate>(api, p, &o, &params, false).await
        }
        DeliveryCall::DeploymentsUpdate(p, o, params) => {
            run_office::<DeploymentsUpdate>(api, p, &o, &params, false).await
        }
        DeliveryCall::DeploymentsSetChangelog(p, o, params) => {
            run_office::<DeploymentsSetChangelog>(api, p, &o, &params, false).await
        }
        DeliveryCall::DeploymentsSetFlagSettings(p, o, params) => {
            run_office::<DeploymentFlagSettingsSet>(api, p, &o, &params, false).await
        }
        DeliveryCall::FlagsList(p, o, params) => {
            run_office::<FlagsList>(api, p, &o, &params, true).await
        }
        DeliveryCall::FlagsInspect(p, o, params) => {
            run_office::<FlagsInspect>(api, p, &o, &params, true).await
        }
        DeliveryCall::FlagsCreate(p, o, params) => {
            run_office::<FlagsCreate>(api, p, &o, &params, false).await
        }
        DeliveryCall::FlagsUpdate(p, o, params) => {
            run_office::<FlagsUpdate>(api, p, &o, &params, false).await
        }
    }
}

#[cfg(test)]
mod tests;
