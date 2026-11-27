//! Query-string and path-segment parsers for delivery BFF routes (`TASK-55`, `FC-058..070`).

use org_proto::delivery::{
    DeploymentStatusWire, DeploymentsListParams, FeatureStatusWire, FeaturesListParams,
    FlagStatusWire, FlagsListParams, LinkEntityKindWire, LinkKindWire, LinksListParams,
};

use super::super::doc_routes::decode_query_value;
use super::super::invalid;
use crate::service::ApiReply;

pub(super) fn valid_uuid(value: &str) -> Result<String, ApiReply> {
    org_proto::is_uuid_v7(value)
        .then(|| value.to_owned())
        .ok_or_else(invalid)
}

/// Safe project-scoped reference segment (UUIDv7, `F-<n>`, `R-<n>`, slug, or flag key).
pub(super) fn valid_ref(value: &str) -> Result<String, ApiReply> {
    let ok = !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'));
    ok.then(|| value.to_owned()).ok_or_else(invalid)
}

fn parse_feature_status(raw: &str) -> Result<FeatureStatusWire, ApiReply> {
    match raw {
        "planned" => Ok(FeatureStatusWire::Planned),
        "in_progress" => Ok(FeatureStatusWire::InProgress),
        "live" => Ok(FeatureStatusWire::Live),
        "retired" => Ok(FeatureStatusWire::Retired),
        _ => Err(invalid()),
    }
}

fn parse_flag_status(raw: &str) -> Result<FlagStatusWire, ApiReply> {
    match raw {
        "draft" => Ok(FlagStatusWire::Draft),
        "active" => Ok(FlagStatusWire::Active),
        "retired" => Ok(FlagStatusWire::Retired),
        _ => Err(invalid()),
    }
}

fn parse_deployment_status(raw: &str) -> Result<DeploymentStatusWire, ApiReply> {
    match raw {
        "queued" => Ok(DeploymentStatusWire::Queued),
        "running" => Ok(DeploymentStatusWire::Running),
        "succeeded" => Ok(DeploymentStatusWire::Succeeded),
        "failed" => Ok(DeploymentStatusWire::Failed),
        "cancelled" => Ok(DeploymentStatusWire::Cancelled),
        _ => Err(invalid()),
    }
}

fn parse_link_entity_kind(raw: &str) -> Result<LinkEntityKindWire, ApiReply> {
    match raw {
        "project" => Ok(LinkEntityKindWire::Project),
        "feature" => Ok(LinkEntityKindWire::Feature),
        "release" => Ok(LinkEntityKindWire::Release),
        "milestone" => Ok(LinkEntityKindWire::Milestone),
        "deployment" => Ok(LinkEntityKindWire::Deployment),
        "task" => Ok(LinkEntityKindWire::Task),
        "document" => Ok(LinkEntityKindWire::Document),
        "development_process" => Ok(LinkEntityKindWire::DevelopmentProcess),
        "media" => Ok(LinkEntityKindWire::Media),
        _ => Err(invalid()),
    }
}

fn parse_link_kind(raw: &str) -> Result<LinkKindWire, ApiReply> {
    match raw {
        "depends_on" => Ok(LinkKindWire::DependsOn),
        "relates_to" => Ok(LinkKindWire::RelatesTo),
        "fixes" => Ok(LinkKindWire::Fixes),
        _ => Err(invalid()),
    }
}

fn parse_bool(raw: &str) -> Result<bool, ApiReply> {
    match raw {
        "true" => Ok(true),
        "false" => Ok(false),
        _ => Err(invalid()),
    }
}

fn walk_query(
    query: Option<&str>,
    mut handle: impl FnMut(&str, String) -> Result<(), ApiReply>,
) -> Result<(), ApiReply> {
    let Some(query) = query.filter(|q| !q.is_empty()) else {
        return Ok(());
    };
    let mut seen: Vec<&str> = Vec::new();
    for pair in query.split('&') {
        let (key, raw) = pair.split_once('=').ok_or_else(invalid)?;
        if seen.contains(&key) {
            return Err(invalid());
        }
        seen.push(key);
        let value = decode_query_value(raw)?;
        if value.len() > 256 || value.contains('\0') {
            return Err(invalid());
        }
        handle(key, value)?;
    }
    Ok(())
}

pub(super) fn parse_features_query(
    project: String,
    query: Option<&str>,
) -> Result<FeaturesListParams, ApiReply> {
    let mut params = FeaturesListParams {
        project,
        status: None,
    };
    walk_query(query, |key, value| match key {
        "status" => {
            params.status = Some(parse_feature_status(&value)?);
            Ok(())
        }
        _ => Err(invalid()),
    })?;
    Ok(params)
}

pub(super) fn parse_links_query(
    project: String,
    query: Option<&str>,
) -> Result<LinksListParams, ApiReply> {
    let mut params = LinksListParams {
        project,
        ..Default::default()
    };
    walk_query(query, |key, value| match key {
        "entity_kind" => {
            params.entity_kind = Some(parse_link_entity_kind(&value)?);
            Ok(())
        }
        "entity_id" => {
            params.entity_id = Some(valid_uuid(&value)?);
            Ok(())
        }
        "kind" => {
            params.kind = Some(parse_link_kind(&value)?);
            Ok(())
        }
        _ => Err(invalid()),
    })?;
    if params.entity_kind.is_some() != params.entity_id.is_some() {
        return Err(invalid());
    }
    Ok(params)
}

pub(super) fn parse_deployments_query(
    project: String,
    query: Option<&str>,
) -> Result<DeploymentsListParams, ApiReply> {
    let mut params = DeploymentsListParams {
        project,
        ..Default::default()
    };
    walk_query(query, |key, value| match key {
        "release_id" => {
            params.release_id = Some(valid_uuid(&value)?);
            Ok(())
        }
        "environment" => {
            params.environment = Some(valid_ref(&value)?);
            Ok(())
        }
        "status" => {
            params.status = Some(parse_deployment_status(&value)?);
            Ok(())
        }
        _ => Err(invalid()),
    })?;
    Ok(params)
}

pub(super) fn parse_flags_query(
    project: String,
    query: Option<&str>,
) -> Result<FlagsListParams, ApiReply> {
    let mut params = FlagsListParams {
        project,
        status: None,
        used_only: false,
    };
    walk_query(query, |key, value| match key {
        "status" => {
            params.status = Some(parse_flag_status(&value)?);
            Ok(())
        }
        "used_only" => {
            params.used_only = parse_bool(&value)?;
            Ok(())
        }
        _ => Err(invalid()),
    })?;
    Ok(params)
}
