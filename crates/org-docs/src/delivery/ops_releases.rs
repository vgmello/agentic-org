//! Pure write decisions for releases, milestones, release changelogs, and release flag snapshots.

use std::collections::{BTreeMap, BTreeSet};

use org_proto::delivery::*;
use org_proto::ErrorCode;

use super::codecs::{encode_changelog, encode_counter, encode_milestone, encode_release};
use super::ops::{
    allocate_number, check_revision, invalid, milestone_progress_for, new_meta, parse_uuid,
    refused, release_progress_for, reply, resolve_changelog_owner, rule_err, stor, touch_meta,
    validate_commits, CallerContext, DeliveryMutation, DeliveryOpError, DeliveryStoreView,
};
use super::paths::{
    milestone_counter_path, milestone_path, release_changelog_path, release_counter_path,
    release_path,
};
use super::records::{
    MilestoneRecord, ReleaseFlagSettingRecord, ReleaseRecord, ScopedChangelogRecord,
};
use super::rules::{
    is_valid_env_or_target, is_valid_line, validate_changelog_invariants, validate_flag_value,
    validate_rollout_rules,
};
use super::wire::{changelog_wire, doc_status_from_wire, milestone_wire, release_summary_wire};
use crate::project_tree::{is_body, DocStatus};

fn validate_release_features<V: DeliveryStoreView>(
    view: &V,
    project_id: &str,
    raw: &[String],
) -> Result<Vec<String>, DeliveryOpError> {
    let valid: BTreeSet<String> = stor(view.project_features(project_id))?
        .into_iter()
        .map(|f| f.id)
        .collect();
    let mut out = Vec::with_capacity(raw.len());
    for item in raw {
        let id = parse_uuid("feature_ids", item)?;
        if !valid.contains(&id) {
            return Err(refused(
                ErrorCode::NotFound,
                format!("feature `{id}` not found in project"),
            ));
        }
        out.push(id);
    }
    out.sort();
    out.dedup();
    Ok(out)
}

fn validate_release_tasks<V: DeliveryStoreView>(
    view: &V,
    project_id: &str,
    raw: &[String],
) -> Result<Vec<String>, DeliveryOpError> {
    let project_tasks: BTreeSet<String> = stor(view.project_tasks(project_id))?
        .into_iter()
        .map(|t| t.id)
        .collect();
    let mut out = Vec::with_capacity(raw.len());
    for item in raw {
        let id = parse_uuid("task_ids", item)?;
        if !project_tasks.contains(&id) {
            return Err(invalid(
                "task_ids",
                format!("task `{id}` not found in project"),
            ));
        }
        out.push(id);
    }
    out.sort();
    out.dedup();
    Ok(out)
}

/// Decides `releases.create`.
pub fn decide_create_release<V: DeliveryStoreView>(
    view: &V,
    ctx: &CallerContext<'_>,
    params: &ReleasesCreateParams,
) -> Result<DeliveryMutation, DeliveryOpError> {
    let id = view.derive_uuid_v7("delivery-release", &params.command_id, ctx.now_ms);
    let title = params.title.trim().to_owned();
    if !is_valid_line(&title) {
        return Err(invalid(
            "title",
            "must be 1..=200 visible single-line chars",
        ));
    }
    let version_label = match params.version_label.as_deref() {
        Some(v) if !v.trim().is_empty() => {
            let t = v.trim().to_owned();
            if !is_valid_line(&t) {
                return Err(invalid("version_label", "invalid version_label"));
            }
            Some(t)
        }
        _ => None,
    };
    if !is_body(&params.body_markdown) {
        return Err(invalid(
            "body_markdown",
            "body exceeds 256 KiB or holds NUL",
        ));
    }
    let repository_commits = validate_commits(view, &params.repository_commits)?;
    let feature_ids = validate_release_features(view, ctx.project_id, &params.feature_ids)?;
    let task_ids = validate_release_tasks(view, ctx.project_id, &params.task_ids)?;
    let (number, counter) = allocate_number(
        view,
        "project",
        ctx.project_id,
        "release",
        &id,
        ctx.now_rfc3339,
    )?;
    let rec = ReleaseRecord {
        id,
        project_id: ctx.project_id.to_owned(),
        number,
        title,
        version_label,
        body_markdown: params.body_markdown.clone(),
        repository_commits,
        feature_ids,
        task_ids,
        flag_settings: Vec::new(),
        meta: new_meta(ctx),
    };
    let (progress, _) = release_progress_for(view, &rec)?;
    let answer = ReleaseWriteResult {
        release: release_summary_wire(&rec, progress, false),
    };
    Ok(DeliveryMutation {
        puts: vec![
            (
                release_counter_path(&rec.project_id),
                encode_counter(&counter),
            ),
            (release_path(&rec.project_id, &rec.id), encode_release(&rec)),
        ],
        removes: Vec::new(),
        reply: reply(&answer),
    })
}

/// Decides `releases.update`.
pub fn decide_update_release<V: DeliveryStoreView>(
    view: &V,
    ctx: &CallerContext<'_>,
    params: &ReleasesUpdateParams,
) -> Result<DeliveryMutation, DeliveryOpError> {
    let rid = parse_uuid("release_id", &params.release_id)?;
    let Some(before) = stor(view.release(&rid))? else {
        return Err(refused(ErrorCode::NotFound, "release not found"));
    };
    if before.project_id != ctx.project_id {
        return Err(refused(ErrorCode::NotFound, "release not found"));
    }
    check_revision(before.meta.revision, params.expected_revision)?;
    let mut rec = before.clone();
    if let Some(ref t) = params.title {
        let trimmed = t.trim().to_owned();
        if !is_valid_line(&trimmed) {
            return Err(invalid("title", "invalid title"));
        }
        rec.title = trimmed;
    }
    if let Some(ref vl) = params.version_label {
        let trimmed = vl.trim();
        if trimmed.is_empty() {
            rec.version_label = None;
        } else {
            if !is_valid_line(trimmed) {
                return Err(invalid("version_label", "invalid version_label"));
            }
            rec.version_label = Some(trimmed.to_owned());
        }
    }
    if let Some(ref body) = params.body_markdown {
        if !is_body(body) {
            return Err(invalid("body_markdown", "invalid markdown body"));
        }
        rec.body_markdown = body.clone();
    }
    if let Some(ref commits) = params.repository_commits {
        rec.repository_commits = validate_commits(view, commits)?;
    }
    if let Some(ref fids) = params.feature_ids {
        rec.feature_ids = validate_release_features(view, ctx.project_id, fids)?;
    }
    if let Some(ref tids) = params.task_ids {
        rec.task_ids = validate_release_tasks(view, ctx.project_id, tids)?;
    }
    if rec == before {
        return Err(invalid("release", "nothing to change"));
    }
    touch_meta(&mut rec.meta, ctx);
    let (progress, _) = release_progress_for(view, &rec)?;
    let has_changelog = stor(view.release_changelog(&rid))?.is_some();
    let answer = ReleaseWriteResult {
        release: release_summary_wire(&rec, progress, has_changelog),
    };
    Ok(DeliveryMutation {
        puts: vec![(release_path(&rec.project_id, &rec.id), encode_release(&rec))],
        removes: Vec::new(),
        reply: reply(&answer),
    })
}

/// Decides `releases.set_changelog`.
pub fn decide_set_release_changelog<V: DeliveryStoreView>(
    view: &V,
    ctx: &CallerContext<'_>,
    params: &ReleasesSetChangelogParams,
) -> Result<DeliveryMutation, DeliveryOpError> {
    let rid = parse_uuid("release_id", &params.release_id)?;
    let Some(rel) = stor(view.release(&rid))? else {
        return Err(refused(ErrorCode::NotFound, "release not found"));
    };
    if rel.project_id != ctx.project_id {
        return Err(refused(ErrorCode::NotFound, "release not found"));
    }
    let existing = stor(view.release_changelog(&rid))?;
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
        true,
        false,
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
                    "delivery-release-changelog",
                    &params.command_id,
                    ctx.now_ms,
                ),
                project_id: ctx.project_id.to_owned(),
                release_id: Some(rid.clone()),
                deployment_id: None,
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
            release_changelog_path(&rec.project_id, &rid),
            encode_changelog(&rec),
        )],
        removes: Vec::new(),
        reply: reply(&answer),
    })
}

/// Decides `milestones.create`.
pub fn decide_create_milestone<V: DeliveryStoreView>(
    view: &V,
    ctx: &CallerContext<'_>,
    params: &MilestonesCreateParams,
) -> Result<DeliveryMutation, DeliveryOpError> {
    let rid = parse_uuid("release_id", &params.release_id)?;
    let Some(rel) = stor(view.release(&rid))? else {
        return Err(refused(ErrorCode::NotFound, "release not found"));
    };
    if rel.project_id != ctx.project_id {
        return Err(refused(ErrorCode::NotFound, "release not found"));
    }
    let title = params.title.trim().to_owned();
    if !is_valid_line(&title) {
        return Err(invalid("title", "invalid title"));
    }
    if !is_body(&params.body_markdown) {
        return Err(invalid("body_markdown", "invalid markdown body"));
    }
    let id = view.derive_uuid_v7("delivery-milestone", &params.command_id, ctx.now_ms);
    let (number, counter) =
        allocate_number(view, "release", &rid, "milestone", &id, ctx.now_rfc3339)?;
    let rec = MilestoneRecord {
        id,
        project_id: ctx.project_id.to_owned(),
        release_id: rid,
        number,
        title,
        body_markdown: params.body_markdown.clone(),
        meta: new_meta(ctx),
    };
    let progress = milestone_progress_for(view, ctx.project_id, &rec.id)?;
    let answer = MilestoneWriteResult {
        milestone: milestone_wire(&rec, progress),
    };
    Ok(DeliveryMutation {
        puts: vec![
            (
                milestone_counter_path(&rec.project_id, &rec.release_id),
                encode_counter(&counter),
            ),
            (
                milestone_path(&rec.project_id, &rec.release_id, &rec.id),
                encode_milestone(&rec),
            ),
        ],
        removes: Vec::new(),
        reply: reply(&answer),
    })
}

/// Decides `milestones.update`.
pub fn decide_update_milestone<V: DeliveryStoreView>(
    view: &V,
    ctx: &CallerContext<'_>,
    params: &MilestonesUpdateParams,
) -> Result<DeliveryMutation, DeliveryOpError> {
    let mid = parse_uuid("milestone_id", &params.milestone_id)?;
    let expected_rid = params
        .release_id
        .as_deref()
        .filter(|s| !s.trim().is_empty())
        .map(|s| parse_uuid("release_id", s))
        .transpose()?;
    let Some(before) = stor(view.milestone(&mid))? else {
        return Err(refused(ErrorCode::NotFound, "milestone not found"));
    };
    if before.project_id != ctx.project_id
        || expected_rid
            .as_deref()
            .is_some_and(|rid| before.release_id != rid)
    {
        return Err(refused(ErrorCode::NotFound, "milestone not found"));
    }
    check_revision(before.meta.revision, params.expected_revision)?;
    let mut rec = before.clone();
    if let Some(ref t) = params.title {
        let trimmed = t.trim().to_owned();
        if !is_valid_line(&trimmed) {
            return Err(invalid("title", "invalid title"));
        }
        rec.title = trimmed;
    }
    if let Some(ref body) = params.body_markdown {
        if !is_body(body) {
            return Err(invalid("body_markdown", "invalid markdown body"));
        }
        rec.body_markdown = body.clone();
    }
    if rec == before {
        return Err(invalid("milestone", "nothing to change"));
    }
    touch_meta(&mut rec.meta, ctx);
    let progress = milestone_progress_for(view, ctx.project_id, &rec.id)?;
    let answer = MilestoneWriteResult {
        milestone: milestone_wire(&rec, progress),
    };
    Ok(DeliveryMutation {
        puts: vec![(
            milestone_path(&rec.project_id, &rec.release_id, &rec.id),
            encode_milestone(&rec),
        )],
        removes: Vec::new(),
        reply: reply(&answer),
    })
}

/// Decides `releases.set_flag_settings`.
pub fn decide_set_release_flag_settings<V: DeliveryStoreView>(
    view: &V,
    ctx: &CallerContext<'_>,
    params: &ReleaseFlagSettingsSetParams,
) -> Result<DeliveryMutation, DeliveryOpError> {
    let rid = parse_uuid("release_id", &params.release_id)?;
    let Some(mut rel) = stor(view.release(&rid))? else {
        return Err(refused(ErrorCode::NotFound, "release not found"));
    };
    if rel.project_id != ctx.project_id {
        return Err(refused(ErrorCode::NotFound, "release not found"));
    }
    check_revision(rel.meta.revision, params.expected_revision)?;
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
        let env = item.environment.trim().to_owned();
        let target = item
            .target
            .as_deref()
            .map_or_else(|| "primary".to_owned(), |t| t.trim().to_owned());
        if !is_valid_env_or_target(&env) || !is_valid_env_or_target(&target) {
            return Err(invalid("environment", "invalid environment or target"));
        }
        if !seen.insert((env.clone(), target.clone(), fid.clone())) {
            return Err(invalid(
                "settings",
                "duplicate (environment, target, flag_id) entry",
            ));
        }
        validate_flag_value(flag.value_type, &item.value).map_err(rule_err)?;
        validate_rollout_rules(item.rollout_rules.as_ref()).map_err(rule_err)?;
        wire_settings.push(ReleaseFlagSettingWire {
            flag_id: fid.clone(),
            flag_key: flag.key.clone(),
            environment: env.clone(),
            target: target.clone(),
            value: item.value.clone(),
            rollout_rules: item.rollout_rules.clone(),
        });
        next_settings.push(ReleaseFlagSettingRecord {
            flag_id: fid,
            environment: env,
            target,
            value: item.value.clone(),
            rollout_rules: item.rollout_rules.clone(),
        });
    }
    rel.flag_settings = next_settings;
    touch_meta(&mut rel.meta, ctx);
    let answer = ReleaseFlagSettingsResult {
        release_id: rel.id.clone(),
        revision: rel.meta.revision,
        flag_settings: wire_settings,
    };
    Ok(DeliveryMutation {
        puts: vec![(release_path(&rel.project_id, &rel.id), encode_release(&rel))],
        removes: Vec::new(),
        reply: reply(&answer),
    })
}
