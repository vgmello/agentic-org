//! Pure cross-file rebuild validation for a project's delivery subtree.

use std::collections::{BTreeMap, BTreeSet};

use super::ops::DeliveryStoreView;
use super::paths::{
    deployment_changelog_path, deployment_path, feature_path, flag_path, link_path, milestone_path,
    release_changelog_path, release_path,
};
use super::records::{
    DeploymentRecord, EntityLinkRecord, FeatureFlagRecord, FeatureRecord, FlagValueType,
    RecordMeta, ReleaseRecord, RepoCommitRecord,
};
use super::rules::{
    check_deployment_rollback_acyclic, check_feature_replacement_acyclic, validate_flag_value,
};

/// A rebuild validation error naming the offending durable path and reason.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeliveryValidationError {
    /// Durable path of the offending file.
    pub path: String,
    /// Human-readable reason.
    pub reason: String,
}

fn err(path: String, reason: impl Into<String>) -> DeliveryValidationError {
    DeliveryValidationError {
        path,
        reason: reason.into(),
    }
}

fn check_meta<V: DeliveryStoreView>(
    view: &V,
    path: &str,
    meta: &RecordMeta,
    owner: Option<&str>,
) -> Result<(), DeliveryValidationError> {
    for emp in [
        meta.created_by.employee_id(),
        meta.updated_by.employee_id(),
        owner,
    ]
    .into_iter()
    .flatten()
    {
        if !view
            .employee_exists(emp)
            .map_err(|e| err(path.to_owned(), e))?
        {
            return Err(err(path.to_owned(), "referenced employee does not exist"));
        }
    }
    Ok(())
}

fn check_commits<V: DeliveryStoreView>(
    view: &V,
    path: &str,
    commits: &[RepoCommitRecord],
    label: &str,
) -> Result<(), DeliveryValidationError> {
    for c in commits {
        if !view
            .repository_exists(&c.repository_id)
            .map_err(|e| err(path.to_owned(), e))?
        {
            return Err(err(
                path.to_owned(),
                format!("{label} commit references unknown repository"),
            ));
        }
    }
    Ok(())
}

fn validate_features_pass<V: DeliveryStoreView>(
    view: &V,
    project_id: &str,
    features: &[FeatureRecord],
) -> Result<BTreeSet<String>, DeliveryValidationError> {
    let mut ids = BTreeSet::new();
    let mut slugs = BTreeSet::new();
    let mut nums = BTreeSet::new();
    for f in features {
        let p = feature_path(&f.project_id, &f.id);
        check_meta(view, &p, &f.meta, f.owner_employee_id.as_deref())?;
        if !ids.insert(f.id.clone())
            || !slugs.insert(f.local_slug.clone())
            || !nums.insert(f.number)
        {
            return Err(err(p, "duplicate feature id, number, or local_slug"));
        }
    }
    for f in features {
        if let Some(ref rep) = f.replaced_by_feature_id {
            if !ids.contains(rep) {
                return Err(err(
                    feature_path(&f.project_id, &f.id),
                    "replaced_by_feature_id does not exist in this project",
                ));
            }
        }
    }
    check_feature_replacement_acyclic(features).map_err(|e| {
        err(
            format!("state/projects/{project_id}/features"),
            e.to_string(),
        )
    })?;
    Ok(ids)
}

fn validate_flags_pass<V: DeliveryStoreView>(
    view: &V,
    flags: &[FeatureFlagRecord],
    feature_ids: &BTreeSet<String>,
) -> Result<BTreeMap<String, FlagValueType>, DeliveryValidationError> {
    let mut flag_map = BTreeMap::new();
    let mut flag_keys = BTreeSet::new();
    for fl in flags {
        let p = flag_path(&fl.project_id, &fl.id);
        check_meta(view, &p, &fl.meta, fl.owner_employee_id.as_deref())?;
        if !flag_keys.insert(fl.key.clone()) {
            return Err(err(p, "duplicate feature flag key in project"));
        }
        if let Some(ref fid) = fl.feature_id {
            if !feature_ids.contains(fid) {
                return Err(err(p, "flag feature_id does not exist in this project"));
            }
        }
        flag_map.insert(fl.id.clone(), fl.value_type);
    }
    Ok(flag_map)
}

fn validate_one_release<V: DeliveryStoreView>(
    view: &V,
    r: &ReleaseRecord,
    feature_ids: &BTreeSet<String>,
    project_tasks: &BTreeSet<String>,
    flag_map: &BTreeMap<String, FlagValueType>,
    milestone_ids: &mut BTreeSet<String>,
) -> Result<(), DeliveryValidationError> {
    let p = release_path(&r.project_id, &r.id);
    check_meta(view, &p, &r.meta, None)?;
    check_commits(view, &p, &r.repository_commits, "release")?;
    if r.feature_ids.iter().any(|fid| !feature_ids.contains(fid)) {
        return Err(err(p, "release references unknown feature"));
    }
    if r.task_ids.iter().any(|tid| !project_tasks.contains(tid)) {
        return Err(err(p, "release references task outside project"));
    }
    for s in &r.flag_settings {
        let Some(&vt) = flag_map.get(&s.flag_id) else {
            return Err(err(p, "release flag setting references unknown flag"));
        };
        validate_flag_value(vt, &s.value).map_err(|e| err(p.clone(), e.to_string()))?;
    }
    if let Some(cl) = view
        .release_changelog(&r.id)
        .map_err(|e| err(p.clone(), e))?
    {
        let cp = release_changelog_path(&r.project_id, &r.id);
        check_meta(view, &cp, &cl.meta, cl.owner_employee_id.as_deref())?;
    }
    let mut ms_nums = BTreeSet::new();
    for m in view.release_milestones(&r.id).map_err(|e| err(p, e))? {
        let mp = milestone_path(&m.project_id, &m.release_id, &m.id);
        check_meta(view, &mp, &m.meta, None)?;
        if !ms_nums.insert(m.number) || !milestone_ids.insert(m.id.clone()) {
            return Err(err(mp, "duplicate milestone id or number"));
        }
    }
    Ok(())
}

fn validate_releases_pass<V: DeliveryStoreView>(
    view: &V,
    project_id: &str,
    releases: &[ReleaseRecord],
    feature_ids: &BTreeSet<String>,
    flag_map: &BTreeMap<String, FlagValueType>,
) -> Result<BTreeSet<String>, DeliveryValidationError> {
    let project_tasks: BTreeSet<String> = view
        .project_tasks(project_id)
        .map_err(|e| err(format!("state/projects/{project_id}/releases"), e))?
        .into_iter()
        .map(|t| t.id)
        .collect();
    let mut release_ids = BTreeSet::new();
    let mut release_nums = BTreeSet::new();
    let mut milestone_ids = BTreeSet::new();
    for r in releases {
        let p = release_path(&r.project_id, &r.id);
        if !release_ids.insert(r.id.clone()) || !release_nums.insert(r.number) {
            return Err(err(p, "duplicate release id or number"));
        }
        validate_one_release(
            view,
            r,
            feature_ids,
            &project_tasks,
            flag_map,
            &mut milestone_ids,
        )?;
    }
    Ok(release_ids)
}

fn validate_deployments_pass<V: DeliveryStoreView>(
    view: &V,
    project_id: &str,
    deployments: &[DeploymentRecord],
    release_ids: &BTreeSet<String>,
    flag_map: &BTreeMap<String, FlagValueType>,
) -> Result<(), DeliveryValidationError> {
    let dep_map: BTreeMap<&str, (&str, &str)> = deployments
        .iter()
        .map(|d| (d.id.as_str(), (d.environment.as_str(), d.target.as_str())))
        .collect();
    for d in deployments {
        let p = deployment_path(&d.project_id, &d.id);
        check_meta(view, &p, &d.meta, None)?;
        if !release_ids.contains(&d.release_id) {
            return Err(err(p, "deployment references unknown release"));
        }
        for target_ref in [&d.rollback_of_id, &d.baseline_deployment_id]
            .into_iter()
            .flatten()
        {
            if dep_map.get(target_ref.as_str())
                != Some(&(d.environment.as_str(), d.target.as_str()))
            {
                return Err(err(
                    p,
                    "rollback_of_id/baseline_deployment_id must match project, environment, and target",
                ));
            }
        }
        check_commits(view, &p, &d.repository_commits, "deployment")?;
        for s in &d.flag_settings {
            let Some(&vt) = flag_map.get(&s.flag_id) else {
                return Err(err(p, "deployment flag setting references unknown flag"));
            };
            validate_flag_value(vt, &s.value).map_err(|e| err(p.clone(), e.to_string()))?;
        }
        if let Some(cl) = view.deployment_changelog(&d.id).map_err(|e| err(p, e))? {
            let cp = deployment_changelog_path(&d.project_id, &d.id);
            check_meta(view, &cp, &cl.meta, cl.owner_employee_id.as_deref())?;
        }
    }
    check_deployment_rollback_acyclic(deployments).map_err(|e| {
        err(
            format!("state/projects/{project_id}/deployments"),
            e.to_string(),
        )
    })
}

fn validate_links_pass<V: DeliveryStoreView>(
    view: &V,
    project_id: &str,
    links: &[EntityLinkRecord],
) -> Result<(), DeliveryValidationError> {
    for l in links {
        let p = link_path(&l.project_id, l.source_kind, &l.source_id, &l.id);
        let src_ok = view
            .entity_exists_in_project(project_id, l.source_kind, &l.source_id)
            .map_err(|e| err(p.clone(), e))?;
        let dst_ok = view
            .entity_exists_in_project(project_id, l.target_kind, &l.target_id)
            .map_err(|e| err(p.clone(), e))?;
        if !src_ok || !dst_ok {
            return Err(err(p, "link references missing or out-of-project entity"));
        }
    }
    Ok(())
}

/// Validates all cross-file delivery invariants for one project after rebuild or import.
pub fn validate_project_delivery<V: DeliveryStoreView>(
    view: &V,
    project_id: &str,
    project_exists: bool,
) -> Result<(), DeliveryValidationError> {
    let base = || format!("state/projects/{project_id}/features");
    let features = view
        .project_features(project_id)
        .map_err(|e| err(base(), e))?;
    let releases = view
        .project_releases(project_id)
        .map_err(|e| err(base(), e))?;
    let deployments = view
        .project_deployments(project_id)
        .map_err(|e| err(base(), e))?;
    let flags = view.project_flags(project_id).map_err(|e| err(base(), e))?;
    let links = view.project_links(project_id).map_err(|e| err(base(), e))?;
    let empty = features.is_empty()
        && releases.is_empty()
        && deployments.is_empty()
        && flags.is_empty()
        && links.is_empty();
    if empty {
        return Ok(());
    }
    if !project_exists {
        return Err(err(
            base(),
            "delivery records belong to a project that does not exist",
        ));
    }
    let feature_ids = validate_features_pass(view, project_id, &features)?;
    let flag_map = validate_flags_pass(view, &flags, &feature_ids)?;
    let release_ids = validate_releases_pass(view, project_id, &releases, &feature_ids, &flag_map)?;
    validate_deployments_pass(view, project_id, &deployments, &release_ids, &flag_map)?;
    validate_links_pass(view, project_id, &links)
}
