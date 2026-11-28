//! Pure write decisions for deployments, deployment changelogs, feature flags, and
//! deployment flag snapshots.

use std::collections::{BTreeMap, BTreeSet};

use org_proto::delivery::*;
use org_proto::ErrorCode;

use super::codecs::{encode_changelog, encode_deployment, encode_flag};
use super::ops::{
    check_deployment_parent, check_revision, invalid, new_meta, parse_uuid, refused, reply,
    resolve_changelog_owner, rule_err, stor, touch_meta, validate_commits, validate_owner,
    CallerContext, DeliveryMutation, DeliveryOpError, DeliveryStoreView,
};
use super::paths::{deployment_changelog_path, deployment_path, flag_path};
use super::records::{
    DeploymentFlagSettingRecord, DeploymentOperation, DeploymentRecord, DeploymentStatus,
    FeatureFlagRecord, FlagStatus, ScopedChangelogRecord,
};
use super::rules::{
    canonicalize_opt_timestamp, check_deployment_rollback_acyclic,
    check_used_flag_identity_unchanged, validate_changelog_invariants, validate_deployment_record,
    validate_flag_record, validate_flag_value, validate_rollout_rules,
};
use super::wire::{
    changelog_wire, deployment_op_from_wire, deployment_status_from_wire, deployment_summary_wire,
    doc_status_from_wire, feature_flag_wire, flag_status_from_wire, flag_value_type_from_wire,
};
use crate::project_tree::DocStatus;

/// Decides `deployments.create`.
pub fn decide_create_deployment<V: DeliveryStoreView>(
    view: &V,
    ctx: &CallerContext<'_>,
    params: &DeploymentsCreateParams,
) -> Result<DeliveryMutation, DeliveryOpError> {
    let rid = parse_uuid("release_id", &params.release_id)?;
    let Some(rel) = stor(view.release(&rid))? else {
        return Err(refused(ErrorCode::NotFound, "release not found"));
    };
    if rel.project_id != ctx.project_id {
        return Err(refused(ErrorCode::NotFound, "release not found"));
    }
    let operation = match params.operation {
        Some(o) => deployment_op_from_wire(o).map_err(rule_err)?,
        None => DeploymentOperation::Deploy,
    };
    let status = match params.status {
        Some(s) => deployment_status_from_wire(s).map_err(rule_err)?,
        None => DeploymentStatus::Queued,
    };
    let env = params.environment.trim().to_owned();
    let target = params
        .target
        .as_deref()
        .map_or_else(|| "primary".to_owned(), |t| t.trim().to_owned());
    let rollback_of_id = params
        .rollback_of_id
        .as_deref()
        .filter(|s| !s.trim().is_empty())
        .map(|s| parse_uuid("rollback_of_id", s))
        .transpose()?;
    let baseline_deployment_id = params
        .baseline_deployment_id
        .as_deref()
        .filter(|s| !s.trim().is_empty())
        .map(|s| parse_uuid("baseline_deployment_id", s))
        .transpose()?;
    let mut all = stor(view.project_deployments(ctx.project_id))?;
    check_deployment_parent(
        &all,
        rollback_of_id.as_deref(),
        "rollback_of_id",
        &env,
        &target,
    )?;
    check_deployment_parent(
        &all,
        baseline_deployment_id.as_deref(),
        "baseline_deployment_id",
        &env,
        &target,
    )?;
    let repository_commits = validate_commits(view, &params.repository_commits)?;
    let started_at =
        canonicalize_opt_timestamp("started_at", params.started_at.as_deref()).map_err(rule_err)?;
    let finished_at = canonicalize_opt_timestamp("finished_at", params.finished_at.as_deref())
        .map_err(rule_err)?;
    let rec = DeploymentRecord {
        id: view.derive_uuid_v7("delivery-deployment", &params.command_id, ctx.now_ms),
        project_id: ctx.project_id.to_owned(),
        release_id: rid,
        environment: env,
        target,
        operation,
        rollback_of_id,
        baseline_deployment_id,
        status,
        started_at,
        finished_at,
        external_url: params
            .external_url
            .as_deref()
            .filter(|u| !u.trim().is_empty())
            .map(|u| u.trim().to_owned()),
        body_markdown: params.body_markdown.clone(),
        repository_commits,
        flag_settings: Vec::new(),
        meta: new_meta(ctx),
    };
    validate_deployment_record(&rec).map_err(rule_err)?;
    all.push(rec.clone());
    check_deployment_rollback_acyclic(&all).map_err(rule_err)?;
    let answer = DeploymentWriteResult {
        deployment: deployment_summary_wire(&rec, false),
    };
    Ok(DeliveryMutation {
        puts: vec![(
            deployment_path(&rec.project_id, &rec.id),
            encode_deployment(&rec),
        )],
        removes: Vec::new(),
        reply: reply(&answer),
    })
}

/// Decides `deployments.update`.
pub fn decide_update_deployment<V: DeliveryStoreView>(
    view: &V,
    ctx: &CallerContext<'_>,
    params: &DeploymentsUpdateParams,
) -> Result<DeliveryMutation, DeliveryOpError> {
    let did = parse_uuid("deployment_id", &params.deployment_id)?;
    let mut all = stor(view.project_deployments(ctx.project_id))?;
    let Some(pos) = all.iter().position(|d| d.id == did) else {
        return Err(refused(ErrorCode::NotFound, "deployment not found"));
    };
    let before = all[pos].clone();
    check_revision(before.meta.revision, params.expected_revision)?;
    let mut rec = before.clone();
    if let Some(s) = params.status {
        rec.status = deployment_status_from_wire(s).map_err(rule_err)?;
    }
    if let Some(ref ts) = params.started_at {
        rec.started_at = canonicalize_opt_timestamp("started_at", Some(ts)).map_err(rule_err)?;
    }
    if let Some(ref ts) = params.finished_at {
        rec.finished_at = canonicalize_opt_timestamp("finished_at", Some(ts)).map_err(rule_err)?;
    }
    if let Some(ref u) = params.external_url {
        rec.external_url = (!u.trim().is_empty()).then(|| u.trim().to_owned());
    }
    if let Some(ref body) = params.body_markdown {
        rec.body_markdown = body.clone();
    }
    if let Some(ref r) = params.rollback_of_id {
        rec.rollback_of_id = (!r.trim().is_empty())
            .then(|| parse_uuid("rollback_of_id", r))
            .transpose()?;
        check_deployment_parent(
            &all,
            rec.rollback_of_id.as_deref(),
            "rollback_of_id",
            &rec.environment,
            &rec.target,
        )?;
    }
    if let Some(ref b) = params.baseline_deployment_id {
        rec.baseline_deployment_id = (!b.trim().is_empty())
            .then(|| parse_uuid("baseline_deployment_id", b))
            .transpose()?;
        check_deployment_parent(
            &all,
            rec.baseline_deployment_id.as_deref(),
            "baseline_deployment_id",
            &rec.environment,
            &rec.target,
        )?;
    }
    if let Some(ref commits) = params.repository_commits {
        rec.repository_commits = validate_commits(view, commits)?;
    }
    if rec == before {
        return Err(invalid("deployment", "nothing to change"));
    }
    validate_deployment_record(&rec).map_err(rule_err)?;
    all[pos] = rec.clone();
    check_deployment_rollback_acyclic(&all).map_err(rule_err)?;
    touch_meta(&mut rec.meta, ctx);
    let has_changelog = stor(view.deployment_changelog(&did))?.is_some();
    let answer = DeploymentWriteResult {
        deployment: deployment_summary_wire(&rec, has_changelog),
    };
    Ok(DeliveryMutation {
        puts: vec![(
            deployment_path(&rec.project_id, &rec.id),
            encode_deployment(&rec),
        )],
        removes: Vec::new(),
        reply: reply(&answer),
    })
}

/// Decides `deployments.set_changelog`.
pub fn decide_set_deployment_changelog<V: DeliveryStoreView>(
    view: &V,
    ctx: &CallerContext<'_>,
    params: &DeploymentsSetChangelogParams,
) -> Result<DeliveryMutation, DeliveryOpError> {
    let did = parse_uuid("deployment_id", &params.deployment_id)?;
    let Some(dep) = stor(view.deployment(&did))? else {
        return Err(refused(ErrorCode::NotFound, "deployment not found"));
    };
    if dep.project_id != ctx.project_id {
        return Err(refused(ErrorCode::NotFound, "deployment not found"));
    }
    let existing = stor(view.deployment_changelog(&did))?;
    let status = match params.status {
        Some(s) => doc_status_from_wire(s).map_err(rule_err)?,
        None => existing.as_ref().map_or(DocStatus::Draft, |c| c.status),
    };
    let owner_employee_id = resolve_changelog_owner(
        view,
        ctx,
        status,
        params.owner_employee_id.as_deref(),
        existing
            .as_ref()
            .and_then(|c| c.owner_employee_id.as_deref()),
    )?;
    let title = params.title.trim().to_owned();
    validate_changelog_invariants(
        &title,
        status,
        owner_employee_id.as_deref(),
        &params.body_markdown,
        false,
        true,
    )
    .map_err(rule_err)?;
    let rec = match existing {
        Some(mut prev) => {
            if let Some(exp) = params.expected_revision {
                check_revision(prev.meta.revision, exp)?;
            }
            prev.title = title;
            prev.status = status;
            prev.owner_employee_id = owner_employee_id;
            prev.body_markdown = params.body_markdown.clone();
            touch_meta(&mut prev.meta, ctx);
            prev
        }
        None => {
            if let Some(exp) = params.expected_revision {
                return Err(refused(
                    ErrorCode::Conflict,
                    format!("changelog does not exist yet (expected_revision={exp})"),
                ));
            }
            ScopedChangelogRecord {
                id: view.derive_uuid_v7(
                    "delivery-deployment-changelog",
                    &params.command_id,
                    ctx.now_ms,
                ),
                project_id: ctx.project_id.to_owned(),
                release_id: None,
                deployment_id: Some(did.clone()),
                title,
                status,
                owner_employee_id,
                body_markdown: params.body_markdown.clone(),
                meta: new_meta(ctx),
            }
        }
    };
    let answer = ChangelogWriteResult {
        changelog: changelog_wire(&rec),
    };
    Ok(DeliveryMutation {
        puts: vec![(
            deployment_changelog_path(&rec.project_id, &did),
            encode_changelog(&rec),
        )],
        removes: Vec::new(),
        reply: reply(&answer),
    })
}

/// Decides `flags.create`.
pub fn decide_create_flag<V: DeliveryStoreView>(
    view: &V,
    ctx: &CallerContext<'_>,
    params: &FlagsCreateParams,
) -> Result<DeliveryMutation, DeliveryOpError> {
    let value_type = flag_value_type_from_wire(params.value_type).map_err(rule_err)?;
    let status = match params.status {
        Some(s) => flag_status_from_wire(s).map_err(rule_err)?,
        None => FlagStatus::Draft,
    };
    let key = params.key.trim().to_owned();
    if stor(view.project_flags(ctx.project_id))?
        .iter()
        .any(|f| f.key == key)
    {
        return Err(refused(
            ErrorCode::AlreadyExists,
            "key: another feature flag in this project uses this key",
        ));
    }
    let feature_id = match params.feature_id.as_deref() {
        Some(raw) if !raw.trim().is_empty() => {
            let fid = parse_uuid("feature_id", raw)?;
            if stor(view.feature(&fid))?.is_none_or(|f| f.project_id != ctx.project_id) {
                return Err(refused(ErrorCode::NotFound, "feature not found in project"));
            }
            Some(fid)
        }
        _ => None,
    };
    let mut owner_employee_id = validate_owner(view, params.owner_employee_id.as_deref())?;
    if status == FlagStatus::Active && owner_employee_id.is_none() {
        owner_employee_id = Some(ctx.caller_id.to_owned());
    }
    let rec = FeatureFlagRecord {
        id: view.derive_uuid_v7("delivery-flag", &params.command_id, ctx.now_ms),
        project_id: ctx.project_id.to_owned(),
        feature_id,
        key,
        owner_employee_id,
        status,
        value_type,
        default_value: params.default_value.clone(),
        body_markdown: params.body_markdown.clone(),
        meta: new_meta(ctx),
    };
    validate_flag_record(&rec).map_err(rule_err)?;
    let answer = FlagWriteResult {
        flag: feature_flag_wire(&rec, false),
    };
    Ok(DeliveryMutation {
        puts: vec![(flag_path(&rec.project_id, &rec.id), encode_flag(&rec))],
        removes: Vec::new(),
        reply: reply(&answer),
    })
}

/// Decides `flags.update` (enforcing `used_flag_identity_immutable`).
pub fn decide_update_flag<V: DeliveryStoreView>(
    view: &V,
    ctx: &CallerContext<'_>,
    params: &FlagsUpdateParams,
) -> Result<DeliveryMutation, DeliveryOpError> {
    let flid = parse_uuid("flag_id", &params.flag_id)?;
    let all = stor(view.project_flags(ctx.project_id))?;
    let Some(before) = all.iter().find(|f| f.id == flid).cloned() else {
        return Err(refused(ErrorCode::NotFound, "feature flag not found"));
    };
    check_revision(before.meta.revision, params.expected_revision)?;
    let is_used = stor(view.used_flag_ids(ctx.project_id))?.contains(&before.id);
    let mut rec = before.clone();
    if let Some(ref key) = params.key {
        let k = key.trim().to_owned();
        if all.iter().any(|f| f.id != rec.id && f.key == k) {
            return Err(refused(
                ErrorCode::AlreadyExists,
                "key: another feature flag in this project uses this key",
            ));
        }
        rec.key = k;
    }
    if let Some(vt) = params.value_type {
        rec.value_type = flag_value_type_from_wire(vt).map_err(rule_err)?;
    }
    if let Some(ref dv) = params.default_value {
        rec.default_value = dv.clone();
    }
    if let Some(ref fid_raw) = params.feature_id {
        let trimmed = fid_raw.trim();
        if trimmed.is_empty() {
            rec.feature_id = None;
        } else {
            let fid = parse_uuid("feature_id", trimmed)?;
            if stor(view.feature(&fid))?.is_none_or(|f| f.project_id != ctx.project_id) {
                return Err(refused(ErrorCode::NotFound, "feature not found in project"));
            }
            rec.feature_id = Some(fid);
        }
    }
    if let Some(ref owner) = params.owner_employee_id {
        rec.owner_employee_id = validate_owner(view, Some(owner))?;
    }
    if let Some(s) = params.status {
        rec.status = flag_status_from_wire(s).map_err(rule_err)?;
    }
    if let Some(ref body) = params.body_markdown {
        rec.body_markdown = body.clone();
    }
    if rec == before {
        return Err(invalid("flag", "nothing to change"));
    }
    check_used_flag_identity_unchanged(&before, &rec, is_used)
        .map_err(|e| refused(ErrorCode::Conflict, e.to_string()))?;
    validate_flag_record(&rec).map_err(rule_err)?;
    touch_meta(&mut rec.meta, ctx);
    let answer = FlagWriteResult {
        flag: feature_flag_wire(&rec, is_used),
    };
    Ok(DeliveryMutation {
        puts: vec![(flag_path(&rec.project_id, &rec.id), encode_flag(&rec))],
        removes: Vec::new(),
        reply: reply(&answer),
    })
}

/// Decides `deployments.set_flag_settings`.
pub fn decide_set_deployment_flag_settings<V: DeliveryStoreView>(
    view: &V,
    ctx: &CallerContext<'_>,
    params: &DeploymentFlagSettingsSetParams,
) -> Result<DeliveryMutation, DeliveryOpError> {
    let did = parse_uuid("deployment_id", &params.deployment_id)?;
    let Some(mut dep) = stor(view.deployment(&did))? else {
        return Err(refused(ErrorCode::NotFound, "deployment not found"));
    };
    if dep.project_id != ctx.project_id {
        return Err(refused(ErrorCode::NotFound, "deployment not found"));
    }
    check_revision(dep.meta.revision, params.expected_revision)?;
    let flags: BTreeMap<String, _> = stor(view.project_flags(ctx.project_id))?
        .into_iter()
        .map(|f| (f.id.clone(), f))
        .collect();
    let mut seen = BTreeSet::new();
    let mut next_settings = Vec::with_capacity(params.settings.len());
    let mut wire_settings = Vec::with_capacity(params.settings.len());
    for item in &params.settings {
        let fid = parse_uuid("flag_id", &item.flag_id)?;
        let Some(flag) = flags.get(&fid) else {
            return Err(refused(
                ErrorCode::NotFound,
                format!("feature flag `{fid}` not found in project"),
            ));
        };
        if !seen.insert(fid.clone()) {
            return Err(invalid(
                "settings",
                "duplicate flag_id in deployment snapshot",
            ));
        }
        validate_flag_value(flag.value_type, &item.value).map_err(rule_err)?;
        validate_rollout_rules(item.rollout_rules.as_ref()).map_err(rule_err)?;
        let applied_at = canonicalize_opt_timestamp("applied_at", item.applied_at.as_deref())
            .map_err(rule_err)?;
        wire_settings.push(DeploymentFlagSettingWire {
            flag_id: fid.clone(),
            flag_key: flag.key.clone(),
            value: item.value.clone(),
            rollout_rules: item.rollout_rules.clone(),
            applied_at: applied_at.clone(),
        });
        next_settings.push(DeploymentFlagSettingRecord {
            flag_id: fid,
            value: item.value.clone(),
            rollout_rules: item.rollout_rules.clone(),
            applied_at,
        });
    }
    dep.flag_settings = next_settings;
    touch_meta(&mut dep.meta, ctx);
    let answer = DeploymentFlagSettingsResult {
        deployment_id: dep.id.clone(),
        revision: dep.meta.revision,
        flag_settings: wire_settings,
    };
    Ok(DeliveryMutation {
        puts: vec![(
            deployment_path(&dep.project_id, &dep.id),
            encode_deployment(&dep),
        )],
        removes: Vec::new(),
        reply: reply(&answer),
    })
}
