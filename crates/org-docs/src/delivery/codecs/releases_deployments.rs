//! Deterministic codecs for releases, milestones, and deployments.

use std::collections::BTreeSet;

use serde_json::{json, Map, Value};

use super::{
    encode_commits, finish_map, map_opt_text, map_opt_ts, map_opt_uuid, map_text, map_u64,
    map_uuid, parse_doc_bytes, read_commits, read_meta, read_uuid_list, write_meta,
};
use crate::delivery::records::{
    DeploymentFlagSettingRecord, DeploymentOperation, DeploymentRecord, DeploymentStatus,
    MilestoneRecord, ReleaseFlagSettingRecord, ReleaseRecord,
};
use crate::delivery::rules::{
    is_valid_env_or_target, is_valid_line, validate_deployment_record, validate_rollout_rules,
    RuleError,
};
use crate::project_tree::{is_body, yaml_encode_document};

fn read_release_flag_settings(
    map: &mut Map<String, Value>,
) -> Result<Vec<ReleaseFlagSettingRecord>, RuleError> {
    let Some(val) = map.remove("flag_settings") else {
        return Ok(Vec::new());
    };
    let Value::Array(items) = val else {
        return Err(RuleError::new("flag_settings", "expected an array"));
    };
    let mut seen = BTreeSet::new();
    let mut out = Vec::with_capacity(items.len());
    for item in items {
        let Value::Object(mut obj) = item else {
            return Err(RuleError::new("flag_settings", "expected object entries"));
        };
        let flag_id = map_uuid(&mut obj, "flag_id")?;
        let environment = map_text(&mut obj, "environment")?;
        let target = map_text(&mut obj, "target")?;
        if !is_valid_env_or_target(&environment) || !is_valid_env_or_target(&target) {
            return Err(RuleError::new(
                "environment",
                "invalid environment or target",
            ));
        }
        if !seen.insert((environment.clone(), target.clone(), flag_id.clone())) {
            return Err(RuleError::new(
                "flag_settings",
                "duplicate (environment, target, flag_id)",
            ));
        }
        let value = obj
            .remove("value")
            .ok_or_else(|| RuleError::new("value", "missing required field"))?;
        let rollout_rules = match obj.remove("rollout_rules") {
            None | Some(Value::Null) => None,
            Some(v) => Some(v),
        };
        validate_rollout_rules(rollout_rules.as_ref())?;
        finish_map(&obj)?;
        out.push(ReleaseFlagSettingRecord {
            flag_id,
            environment,
            target,
            value,
            rollout_rules,
        });
    }
    Ok(out)
}

fn read_deployment_flag_settings(
    map: &mut Map<String, Value>,
) -> Result<Vec<DeploymentFlagSettingRecord>, RuleError> {
    let Some(val) = map.remove("flag_settings") else {
        return Ok(Vec::new());
    };
    let Value::Array(items) = val else {
        return Err(RuleError::new("flag_settings", "expected an array"));
    };
    let mut seen = BTreeSet::new();
    let mut out = Vec::with_capacity(items.len());
    for item in items {
        let Value::Object(mut obj) = item else {
            return Err(RuleError::new("flag_settings", "expected object entries"));
        };
        let flag_id = map_uuid(&mut obj, "flag_id")?;
        if !seen.insert(flag_id.clone()) {
            return Err(RuleError::new("flag_settings", "duplicate flag_id"));
        }
        let value = obj
            .remove("value")
            .ok_or_else(|| RuleError::new("value", "missing required field"))?;
        let rollout_rules = match obj.remove("rollout_rules") {
            None | Some(Value::Null) => None,
            Some(v) => Some(v),
        };
        validate_rollout_rules(rollout_rules.as_ref())?;
        let applied_at = map_opt_ts(&mut obj, "applied_at")?;
        finish_map(&obj)?;
        out.push(DeploymentFlagSettingRecord {
            flag_id,
            value,
            rollout_rules,
            applied_at,
        });
    }
    Ok(out)
}

/// Encodes a [`ReleaseRecord`] to Markdown with YAML front matter.
pub fn encode_release(rel: &ReleaseRecord) -> Vec<u8> {
    let mut f = Map::new();
    write_meta(&mut f, &rel.meta);
    f.insert("id".into(), json!(rel.id));
    f.insert("project_id".into(), json!(rel.project_id));
    f.insert("number".into(), json!(rel.number));
    f.insert("title".into(), json!(rel.title));
    f.insert("version_label".into(), json!(rel.version_label));
    f.insert(
        "repository_commits".into(),
        encode_commits(&rel.repository_commits),
    );
    let mut features = rel.feature_ids.clone();
    features.sort();
    features.dedup();
    f.insert("feature_ids".into(), json!(features));
    let mut tasks = rel.task_ids.clone();
    tasks.sort();
    tasks.dedup();
    f.insert("task_ids".into(), json!(tasks));
    let mut settings = rel.flag_settings.clone();
    settings.sort_by(|a, b| {
        (&a.environment, &a.target, &a.flag_id).cmp(&(&b.environment, &b.target, &b.flag_id))
    });
    f.insert(
        "flag_settings".into(),
        Value::Array(
            settings
                .into_iter()
                .map(|s| {
                    json!({
                        "flag_id": s.flag_id,
                        "environment": s.environment,
                        "target": s.target,
                        "value": s.value,
                        "rollout_rules": s.rollout_rules,
                    })
                })
                .collect(),
        ),
    );
    yaml_encode_document(&f, &rel.body_markdown)
}

/// Decodes and validates a [`ReleaseRecord`] at `(project_id, release_id)`.
pub fn decode_release(
    bytes: &[u8],
    expected_project_id: &str,
    expected_release_id: &str,
) -> Result<ReleaseRecord, RuleError> {
    let (mut m, body_markdown) = parse_doc_bytes(bytes)?;
    let meta = read_meta(&mut m)?;
    let id = map_uuid(&mut m, "id")?;
    let project_id = map_uuid(&mut m, "project_id")?;
    if id != expected_release_id || project_id != expected_project_id {
        return Err(RuleError::new("id", "does not match durable file path"));
    }
    let number = map_u64(&mut m, "number")?;
    if number == 0 {
        return Err(RuleError::new("number", "must be > 0"));
    }
    let title = map_text(&mut m, "title")?;
    if !is_valid_line(&title) {
        return Err(RuleError::new("title", "invalid title"));
    }
    let version_label = map_opt_text(&mut m, "version_label")?;
    if version_label.as_deref().is_some_and(|v| !is_valid_line(v)) {
        return Err(RuleError::new("version_label", "invalid version_label"));
    }
    if !is_body(&body_markdown) {
        return Err(RuleError::new("body_markdown", "invalid markdown body"));
    }
    let repository_commits = read_commits(&mut m)?;
    let feature_ids = read_uuid_list(&mut m, "feature_ids")?;
    let task_ids = read_uuid_list(&mut m, "task_ids")?;
    let flag_settings = read_release_flag_settings(&mut m)?;
    finish_map(&m)?;
    Ok(ReleaseRecord {
        id,
        project_id,
        number,
        title,
        version_label,
        body_markdown,
        repository_commits,
        feature_ids,
        task_ids,
        flag_settings,
        meta,
    })
}

/// Encodes a [`MilestoneRecord`] to Markdown with YAML front matter.
pub fn encode_milestone(ms: &MilestoneRecord) -> Vec<u8> {
    let mut f = Map::new();
    write_meta(&mut f, &ms.meta);
    f.insert("id".into(), json!(ms.id));
    f.insert("project_id".into(), json!(ms.project_id));
    f.insert("release_id".into(), json!(ms.release_id));
    f.insert("number".into(), json!(ms.number));
    f.insert("title".into(), json!(ms.title));
    yaml_encode_document(&f, &ms.body_markdown)
}

/// Decodes and validates a [`MilestoneRecord`].
pub fn decode_milestone(
    bytes: &[u8],
    expected_project_id: &str,
    expected_release_id: &str,
    expected_milestone_id: &str,
) -> Result<MilestoneRecord, RuleError> {
    let (mut m, body_markdown) = parse_doc_bytes(bytes)?;
    let meta = read_meta(&mut m)?;
    let id = map_uuid(&mut m, "id")?;
    let project_id = map_uuid(&mut m, "project_id")?;
    let release_id = map_uuid(&mut m, "release_id")?;
    if id != expected_milestone_id
        || project_id != expected_project_id
        || release_id != expected_release_id
    {
        return Err(RuleError::new("id", "does not match durable file path"));
    }
    let number = map_u64(&mut m, "number")?;
    if number == 0 {
        return Err(RuleError::new("number", "must be > 0"));
    }
    let title = map_text(&mut m, "title")?;
    if !is_valid_line(&title) {
        return Err(RuleError::new("title", "invalid title"));
    }
    if !is_body(&body_markdown) {
        return Err(RuleError::new("body_markdown", "invalid markdown body"));
    }
    finish_map(&m)?;
    Ok(MilestoneRecord {
        id,
        project_id,
        release_id,
        number,
        title,
        body_markdown,
        meta,
    })
}

/// Encodes a [`DeploymentRecord`] to Markdown with YAML front matter.
pub fn encode_deployment(dep: &DeploymentRecord) -> Vec<u8> {
    let mut f = Map::new();
    write_meta(&mut f, &dep.meta);
    f.insert("id".into(), json!(dep.id));
    f.insert("project_id".into(), json!(dep.project_id));
    f.insert("release_id".into(), json!(dep.release_id));
    f.insert("environment".into(), json!(dep.environment));
    f.insert("target".into(), json!(dep.target));
    f.insert("operation".into(), json!(dep.operation.as_str()));
    f.insert("rollback_of_id".into(), json!(dep.rollback_of_id));
    f.insert(
        "baseline_deployment_id".into(),
        json!(dep.baseline_deployment_id),
    );
    f.insert("status".into(), json!(dep.status.as_str()));
    f.insert("started_at".into(), json!(dep.started_at));
    f.insert("finished_at".into(), json!(dep.finished_at));
    f.insert("external_url".into(), json!(dep.external_url));
    f.insert(
        "repository_commits".into(),
        encode_commits(&dep.repository_commits),
    );
    let mut settings = dep.flag_settings.clone();
    settings.sort_by(|a, b| a.flag_id.cmp(&b.flag_id));
    f.insert(
        "flag_settings".into(),
        Value::Array(
            settings
                .into_iter()
                .map(|s| {
                    json!({
                        "flag_id": s.flag_id,
                        "value": s.value,
                        "rollout_rules": s.rollout_rules,
                        "applied_at": s.applied_at,
                    })
                })
                .collect(),
        ),
    );
    yaml_encode_document(&f, &dep.body_markdown)
}

/// Decodes and validates a [`DeploymentRecord`].
pub fn decode_deployment(
    bytes: &[u8],
    expected_project_id: &str,
    expected_deployment_id: &str,
) -> Result<DeploymentRecord, RuleError> {
    let (mut m, body_markdown) = parse_doc_bytes(bytes)?;
    let meta = read_meta(&mut m)?;
    let id = map_uuid(&mut m, "id")?;
    let project_id = map_uuid(&mut m, "project_id")?;
    if id != expected_deployment_id || project_id != expected_project_id {
        return Err(RuleError::new("id", "does not match durable file path"));
    }
    let release_id = map_uuid(&mut m, "release_id")?;
    let environment = map_text(&mut m, "environment")?;
    let target = map_text(&mut m, "target")?;
    let operation = DeploymentOperation::parse(&map_text(&mut m, "operation")?)
        .ok_or_else(|| RuleError::new("operation", "expected deploy or rollback"))?;
    let rollback_of_id = map_opt_uuid(&mut m, "rollback_of_id")?;
    let baseline_deployment_id = map_opt_uuid(&mut m, "baseline_deployment_id")?;
    let status = DeploymentStatus::parse(&map_text(&mut m, "status")?)
        .ok_or_else(|| RuleError::new("status", "invalid deployment status"))?;
    let started_at = map_opt_ts(&mut m, "started_at")?;
    let finished_at = map_opt_ts(&mut m, "finished_at")?;
    let external_url = map_opt_text(&mut m, "external_url")?;
    let repository_commits = read_commits(&mut m)?;
    let flag_settings = read_deployment_flag_settings(&mut m)?;
    finish_map(&m)?;
    let rec = DeploymentRecord {
        id,
        project_id,
        release_id,
        environment,
        target,
        operation,
        rollback_of_id,
        baseline_deployment_id,
        status,
        started_at,
        finished_at,
        external_url,
        body_markdown,
        repository_commits,
        flag_settings,
        meta,
    };
    validate_deployment_record(&rec)?;
    Ok(rec)
}
