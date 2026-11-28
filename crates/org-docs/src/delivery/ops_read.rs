//! Pure read and inspect handlers for features, Why links, releases, deployments, and flags.

use std::collections::BTreeMap;

use org_proto::delivery::*;
use org_proto::ErrorCode;

use super::ops::{
    flag_usages, parse_uuid, refused, release_progress_for, resolve_feature, resolve_flag,
    resolve_release, rule_err, stor, CallerContext, DeliveryOpError, DeliveryStoreView,
};
use super::records::LinkEntityKind;
use super::wire::{
    changelog_wire, deployment_status_from_wire, deployment_summary_wire, entity_link_wire,
    feature_flag_wire, feature_status_from_wire, feature_summary_wire, flag_status_from_wire,
    link_entity_kind_from_wire, link_kind_from_wire, milestone_wire, release_summary_wire,
    repo_commit_wire,
};

/// Lists canonical features for an authorized project.
pub fn list_features<V: DeliveryStoreView>(
    view: &V,
    ctx: &CallerContext<'_>,
    params: &FeaturesListParams,
) -> Result<FeaturesListResult, DeliveryOpError> {
    let want_status = params
        .status
        .map(feature_status_from_wire)
        .transpose()
        .map_err(rule_err)?;
    let features = stor(view.project_features(ctx.project_id))?
        .into_iter()
        .filter(|f| want_status.is_none_or(|s| f.status == s))
        .map(|f| feature_summary_wire(&f, ctx.project_slug))
        .collect();
    Ok(FeaturesListResult {
        project_id: ctx.project_id.to_owned(),
        features,
    })
}

/// Inspects one canonical feature with implementing tasks, affecting bugs, releases, flags, and links.
pub fn inspect_feature<V: DeliveryStoreView>(
    view: &V,
    ctx: &CallerContext<'_>,
    params: &FeaturesInspectParams,
) -> Result<FeatureInspectResult, DeliveryOpError> {
    let f = resolve_feature(view, ctx.project_id, ctx.project_slug, &params.feature)?;
    let tasks = stor(view.project_tasks(ctx.project_id))?;
    let implementing_task_ids = tasks
        .iter()
        .filter(|t| t.feature_id.as_deref() == Some(f.id.as_str()))
        .map(|t| t.id.clone())
        .collect();
    let affecting_bug_ids = tasks
        .iter()
        .filter(|t| t.affects_feature_id.as_deref() == Some(f.id.as_str()))
        .map(|t| t.id.clone())
        .collect();
    let release_ids = stor(view.project_releases(ctx.project_id))?
        .into_iter()
        .filter(|r| r.feature_ids.contains(&f.id))
        .map(|r| r.id)
        .collect();
    let flag_ids = stor(view.project_flags(ctx.project_id))?
        .into_iter()
        .filter(|fl| fl.feature_id.as_deref() == Some(f.id.as_str()))
        .map(|fl| fl.id)
        .collect();
    let links = stor(view.project_links(ctx.project_id))?
        .into_iter()
        .filter(|l| {
            (l.source_kind == LinkEntityKind::Feature && l.source_id == f.id)
                || (l.target_kind == LinkEntityKind::Feature && l.target_id == f.id)
        })
        .map(|l| entity_link_wire(&l))
        .collect();
    Ok(FeatureInspectResult {
        feature: feature_summary_wire(&f, ctx.project_slug),
        body_markdown: f.body_markdown,
        implementing_task_ids,
        affecting_bug_ids,
        release_ids,
        flag_ids,
        links,
    })
}

/// Lists Why links scoped to an authorized project.
pub fn list_links<V: DeliveryStoreView>(
    view: &V,
    ctx: &CallerContext<'_>,
    params: &LinksListParams,
) -> Result<LinksListResult, DeliveryOpError> {
    let want_entity_kind = params
        .entity_kind
        .map(link_entity_kind_from_wire)
        .transpose()
        .map_err(rule_err)?;
    let want_entity_id = params
        .entity_id
        .as_deref()
        .map(|s| parse_uuid("entity_id", s))
        .transpose()?;
    let want_kind = params
        .kind
        .map(link_kind_from_wire)
        .transpose()
        .map_err(rule_err)?;
    let links = stor(view.project_links(ctx.project_id))?
        .into_iter()
        .filter(|l| want_kind.is_none_or(|k| l.kind == k))
        .filter(|l| match (want_entity_kind, want_entity_id.as_deref()) {
            (Some(k), Some(id)) => {
                (l.source_kind == k && l.source_id == id)
                    || (l.target_kind == k && l.target_id == id)
            }
            (Some(k), None) => l.source_kind == k || l.target_kind == k,
            (None, Some(id)) => l.source_id == id || l.target_id == id,
            (None, None) => true,
        })
        .map(|l| entity_link_wire(&l))
        .collect();
    Ok(LinksListResult {
        project_id: ctx.project_id.to_owned(),
        links,
    })
}

/// Lists releases with progress and changelog presence for an authorized project.
pub fn list_releases<V: DeliveryStoreView>(
    view: &V,
    ctx: &CallerContext<'_>,
    _params: &ReleasesListParams,
) -> Result<ReleasesListResult, DeliveryOpError> {
    let mut releases = Vec::new();
    for r in stor(view.project_releases(ctx.project_id))? {
        let (progress, _) = release_progress_for(view, &r)?;
        let has_changelog = stor(view.release_changelog(&r.id))?.is_some();
        releases.push(release_summary_wire(&r, progress, has_changelog));
    }
    Ok(ReleasesListResult {
        project_id: ctx.project_id.to_owned(),
        releases,
    })
}

/// Inspects one release with commits, features, tasks, milestones, changelog, and flag snapshot.
pub fn inspect_release<V: DeliveryStoreView>(
    view: &V,
    ctx: &CallerContext<'_>,
    params: &ReleasesInspectParams,
) -> Result<ReleaseInspectResult, DeliveryOpError> {
    let r = resolve_release(view, ctx.project_id, &params.release)?;
    let (progress, milestones) = release_progress_for(view, &r)?;
    let changelog = stor(view.release_changelog(&r.id))?;
    let flag_keys: BTreeMap<String, String> = stor(view.project_flags(ctx.project_id))?
        .into_iter()
        .map(|f| (f.id, f.key))
        .collect();
    let flag_settings = r
        .flag_settings
        .iter()
        .map(|s| ReleaseFlagSettingWire {
            flag_id: s.flag_id.clone(),
            flag_key: flag_keys.get(&s.flag_id).cloned().unwrap_or_default(),
            environment: s.environment.clone(),
            target: s.target.clone(),
            value: s.value.clone(),
            rollout_rules: s.rollout_rules.clone(),
        })
        .collect();
    Ok(ReleaseInspectResult {
        release: release_summary_wire(&r, progress, changelog.is_some()),
        body_markdown: r.body_markdown,
        repository_commits: r.repository_commits.iter().map(repo_commit_wire).collect(),
        feature_ids: r.feature_ids,
        task_ids: r.task_ids,
        milestones: milestones
            .into_iter()
            .map(|(m, mp)| milestone_wire(&m, mp))
            .collect(),
        changelog: changelog.as_ref().map(changelog_wire),
        flag_settings,
    })
}

/// Lists deployments for an authorized project.
pub fn list_deployments<V: DeliveryStoreView>(
    view: &V,
    ctx: &CallerContext<'_>,
    params: &DeploymentsListParams,
) -> Result<DeploymentsListResult, DeliveryOpError> {
    let want_status = params
        .status
        .map(deployment_status_from_wire)
        .transpose()
        .map_err(rule_err)?;
    let mut deployments = Vec::new();
    for d in stor(view.project_deployments(ctx.project_id))? {
        if params
            .release_id
            .as_deref()
            .is_some_and(|r| d.release_id != r.trim())
        {
            continue;
        }
        if params
            .environment
            .as_deref()
            .is_some_and(|e| d.environment != e.trim())
        {
            continue;
        }
        if want_status.is_some_and(|s| d.status != s) {
            continue;
        }
        let has_changelog = stor(view.deployment_changelog(&d.id))?.is_some();
        deployments.push(deployment_summary_wire(&d, has_changelog));
    }
    Ok(DeploymentsListResult {
        project_id: ctx.project_id.to_owned(),
        deployments,
    })
}

/// Inspects one deployment with commits, changelog, and flag snapshot.
pub fn inspect_deployment<V: DeliveryStoreView>(
    view: &V,
    ctx: &CallerContext<'_>,
    params: &DeploymentsInspectParams,
) -> Result<DeploymentInspectResult, DeliveryOpError> {
    let id = parse_uuid("deployment_id", &params.deployment_id)?;
    let Some(d) = stor(view.deployment(&id))? else {
        return Err(refused(ErrorCode::NotFound, "deployment not found"));
    };
    if d.project_id != ctx.project_id {
        return Err(refused(ErrorCode::NotFound, "deployment not found"));
    }
    let changelog = stor(view.deployment_changelog(&id))?;
    let flag_keys: BTreeMap<String, String> = stor(view.project_flags(ctx.project_id))?
        .into_iter()
        .map(|f| (f.id, f.key))
        .collect();
    let flag_settings = d
        .flag_settings
        .iter()
        .map(|s| DeploymentFlagSettingWire {
            flag_id: s.flag_id.clone(),
            flag_key: flag_keys.get(&s.flag_id).cloned().unwrap_or_default(),
            value: s.value.clone(),
            rollout_rules: s.rollout_rules.clone(),
            applied_at: s.applied_at.clone(),
        })
        .collect();
    Ok(DeploymentInspectResult {
        deployment: deployment_summary_wire(&d, changelog.is_some()),
        body_markdown: d.body_markdown,
        repository_commits: d.repository_commits.iter().map(repo_commit_wire).collect(),
        changelog: changelog.as_ref().map(changelog_wire),
        flag_settings,
    })
}

/// Lists feature flags and their `used` snapshot indicator for an authorized project.
pub fn list_flags<V: DeliveryStoreView>(
    view: &V,
    ctx: &CallerContext<'_>,
    params: &FlagsListParams,
) -> Result<FlagsListResult, DeliveryOpError> {
    let want_status = params
        .status
        .map(flag_status_from_wire)
        .transpose()
        .map_err(rule_err)?;
    let used = stor(view.used_flag_ids(ctx.project_id))?;
    let flags = stor(view.project_flags(ctx.project_id))?
        .into_iter()
        .filter(|f| want_status.is_none_or(|s| f.status == s))
        .filter(|f| !params.used_only || used.contains(&f.id))
        .map(|f| {
            let is_used = used.contains(&f.id);
            feature_flag_wire(&f, is_used)
        })
        .collect();
    Ok(FlagsListResult {
        project_id: ctx.project_id.to_owned(),
        flags,
    })
}

/// Inspects one feature flag and all release/deployment snapshots referencing it.
pub fn inspect_flag<V: DeliveryStoreView>(
    view: &V,
    ctx: &CallerContext<'_>,
    params: &FlagsInspectParams,
) -> Result<FlagInspectResult, DeliveryOpError> {
    let f = resolve_flag(view, ctx.project_id, &params.flag)?;
    let usages = flag_usages(view, ctx.project_id, &f.id)?;
    let is_used = !usages.is_empty();
    Ok(FlagInspectResult {
        flag: feature_flag_wire(&f, is_used),
        usages,
    })
}
