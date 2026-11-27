//! Strict deterministic YAML/Markdown codecs for all delivery records.

use org_proto::{is_uuid_v7, parse_rfc3339_ms};
use serde_json::{json, Map, Value};

use super::records::{
    ActorRef, DeploymentFlagSettingRecord, DeploymentOperation, DeploymentRecord, DeploymentStatus,
    EntityLinkRecord, FeatureFlagRecord, FeatureRecord, FeatureStatus, FeatureWorkType, FlagStatus,
    FlagValueType, LinkEntityKind, LinkKind, MilestoneRecord, NumberAllocation,
    NumberCounterRecord, RecordMeta, ReleaseFlagSettingRecord, ReleaseRecord, RepoCommitRecord,
    ScopedChangelogRecord,
};
use super::rules::{
    is_valid_commit_sha, is_valid_env_or_target, is_valid_line, validate_changelog_invariants,
    validate_deployment_record, validate_entity_link, validate_feature_record,
    validate_flag_record, validate_rollout_rules, RuleError,
};
use crate::project_tree::{is_body, yaml_encode, yaml_encode_document, DocStatus};
use crate::{parse, parse_mapping};

fn map_text(map: &mut Map<String, Value>, key: &str) -> Result<String, RuleError> {
    match map.remove(key) {
        Some(Value::String(s)) => Ok(s),
        Some(_) => Err(RuleError::new(key, "expected a string")),
        None => Err(RuleError::new(key, "missing required field")),
    }
}

fn map_opt_text(map: &mut Map<String, Value>, key: &str) -> Result<Option<String>, RuleError> {
    match map.remove(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(s)) => Ok(Some(s)),
        Some(_) => Err(RuleError::new(key, "expected a string or null")),
    }
}

fn map_u64(map: &mut Map<String, Value>, key: &str) -> Result<u64, RuleError> {
    match map.remove(key) {
        Some(Value::Number(n)) => n
            .as_u64()
            .ok_or_else(|| RuleError::new(key, "expected an unsigned integer")),
        Some(_) => Err(RuleError::new(key, "expected an integer")),
        None => Err(RuleError::new(key, "missing required field")),
    }
}

fn map_uuid(map: &mut Map<String, Value>, key: &str) -> Result<String, RuleError> {
    let val = map_text(map, key)?;
    if !is_uuid_v7(&val) {
        return Err(RuleError::new(key, "must be a lowercase UUIDv7"));
    }
    Ok(val)
}

fn map_opt_uuid(map: &mut Map<String, Value>, key: &str) -> Result<Option<String>, RuleError> {
    match map_opt_text(map, key)? {
        None => Ok(None),
        Some(val) if is_uuid_v7(&val) => Ok(Some(val)),
        Some(_) => Err(RuleError::new(key, "must be a lowercase UUIDv7")),
    }
}

fn finish_map(map: &Map<String, Value>) -> Result<(), RuleError> {
    if let Some(extra) = map.keys().next() {
        return Err(RuleError::new(extra, "unknown field"));
    }
    Ok(())
}

fn write_meta(fields: &mut Map<String, Value>, meta: &RecordMeta) {
    fields.insert("schema_version".into(), json!(meta.schema_version));
    fields.insert("revision".into(), json!(meta.revision));
    fields.insert("created_at".into(), json!(meta.created_at));
    fields.insert("created_by_kind".into(), json!(meta.created_by.kind()));
    fields.insert("created_by_id".into(), json!(meta.created_by.employee_id()));
    fields.insert("updated_at".into(), json!(meta.updated_at));
    fields.insert("updated_by_kind".into(), json!(meta.updated_by.kind()));
    fields.insert("updated_by_id".into(), json!(meta.updated_by.employee_id()));
}

fn read_actor(
    map: &mut Map<String, Value>,
    kind_key: &str,
    id_key: &str,
) -> Result<ActorRef, RuleError> {
    let kind = map_text(map, kind_key)?;
    let id = map_opt_uuid(map, id_key)?;
    match (kind.as_str(), id) {
        ("employee", Some(emp)) => Ok(ActorRef::Employee(emp)),
        ("system", None) => Ok(ActorRef::System),
        ("unknown", None) => Ok(ActorRef::Unknown),
        _ => Err(RuleError::new(kind_key, "invalid actor kind/id pair")),
    }
}

fn read_meta(map: &mut Map<String, Value>) -> Result<RecordMeta, RuleError> {
    let schema_version = map_u64(map, "schema_version")?;
    if schema_version != 1 {
        return Err(RuleError::new("schema_version", "must be 1"));
    }
    let revision = map_u64(map, "revision")?;
    if revision < 1 {
        return Err(RuleError::new("revision", "must be >= 1"));
    }
    let created_at = map_opt_text(map, "created_at")?;
    if let Some(ref ts) = created_at {
        if parse_rfc3339_ms(ts).is_none() {
            return Err(RuleError::new("created_at", "invalid RFC 3339 timestamp"));
        }
    }
    let created_by = read_actor(map, "created_by_kind", "created_by_id")?;
    let updated_at = map_opt_text(map, "updated_at")?;
    if let Some(ref ts) = updated_at {
        if parse_rfc3339_ms(ts).is_none() {
            return Err(RuleError::new("updated_at", "invalid RFC 3339 timestamp"));
        }
    }
    let updated_by = read_actor(map, "updated_by_kind", "updated_by_id")?;
    Ok(RecordMeta {
        schema_version,
        revision,
        created_at,
        created_by,
        updated_at,
        updated_by,
    })
}

fn parse_doc_bytes(bytes: &[u8]) -> Result<(Map<String, Value>, String), RuleError> {
    let text = std::str::from_utf8(bytes).map_err(|_| RuleError::new("", "not UTF-8"))?;
    let doc = parse(text).map_err(|e| RuleError::new(e.field, format!("{:?}", e.reason)))?;
    let fields = doc
        .front_matter
        .ok_or_else(|| RuleError::new("", "missing YAML front matter"))?;
    Ok((fields, doc.body.to_owned()))
}

fn parse_yaml_bytes(bytes: &[u8]) -> Result<Map<String, Value>, RuleError> {
    let text = std::str::from_utf8(bytes).map_err(|_| RuleError::new("", "not UTF-8"))?;
    parse_mapping(text).map_err(|e| RuleError::new(e.field, format!("{:?}", e.reason)))
}

fn encode_commits(commits: &[RepoCommitRecord]) -> Value {
    let mut sorted = commits.to_vec();
    sorted.sort();
    Value::Array(
        sorted
            .into_iter()
            .map(|c| json!({"repository_id": c.repository_id, "commit_sha": c.commit_sha}))
            .collect(),
    )
}

fn read_commits(map: &mut Map<String, Value>) -> Result<Vec<RepoCommitRecord>, RuleError> {
    let Some(val) = map.remove("repository_commits") else {
        return Ok(Vec::new());
    };
    let Value::Array(items) = val else {
        return Err(RuleError::new("repository_commits", "expected an array"));
    };
    let mut out = Vec::with_capacity(items.len());
    for item in items {
        let Value::Object(mut obj) = item else {
            return Err(RuleError::new(
                "repository_commits",
                "expected object entries",
            ));
        };
        let repository_id = map_uuid(&mut obj, "repository_id")?;
        let commit_sha = map_text(&mut obj, "commit_sha")?;
        if !is_valid_commit_sha(&commit_sha) {
            return Err(RuleError::new("commit_sha", "invalid commit sha"));
        }
        finish_map(&obj)?;
        out.push(RepoCommitRecord {
            repository_id,
            commit_sha,
        });
    }
    out.sort();
    Ok(out)
}

fn read_uuid_list(map: &mut Map<String, Value>, key: &str) -> Result<Vec<String>, RuleError> {
    let Some(val) = map.remove(key) else {
        return Ok(Vec::new());
    };
    let Value::Array(items) = val else {
        return Err(RuleError::new(key, "expected an array"));
    };
    let mut out = Vec::with_capacity(items.len());
    for item in items {
        let Value::String(id) = item else {
            return Err(RuleError::new(key, "expected UUIDv7 strings"));
        };
        if !is_uuid_v7(&id) {
            return Err(RuleError::new(key, "must contain lowercase UUIDv7s"));
        }
        out.push(id);
    }
    out.sort();
    out.dedup();
    Ok(out)
}

/// Encodes a canonical [`FeatureRecord`] to Markdown with YAML front matter.
pub fn encode_feature(feature: &FeatureRecord) -> Vec<u8> {
    let mut f = Map::new();
    write_meta(&mut f, &feature.meta);
    f.insert("id".into(), json!(feature.id));
    f.insert("project_id".into(), json!(feature.project_id));
    f.insert("number".into(), json!(feature.number));
    f.insert("local_slug".into(), json!(feature.local_slug));
    f.insert("owner_employee_id".into(), json!(feature.owner_employee_id));
    f.insert(
        "work_type".into(),
        json!(feature.work_type.map(FeatureWorkType::as_str)),
    );
    f.insert("title".into(), json!(feature.title));
    f.insert("status".into(), json!(feature.status.as_str()));
    f.insert("delivered_at".into(), json!(feature.delivered_at));
    f.insert("retired_reason".into(), json!(feature.retired_reason));
    f.insert(
        "replaced_by_feature_id".into(),
        json!(feature.replaced_by_feature_id),
    );
    yaml_encode_document(&f, &feature.body_markdown)
}

/// Decodes and validates a [`FeatureRecord`] at `(project_id, feature_id)`.
pub fn decode_feature(
    bytes: &[u8],
    expected_project_id: &str,
    expected_feature_id: &str,
) -> Result<FeatureRecord, RuleError> {
    let (mut m, body_markdown) = parse_doc_bytes(bytes)?;
    let meta = read_meta(&mut m)?;
    let id = map_uuid(&mut m, "id")?;
    let project_id = map_uuid(&mut m, "project_id")?;
    if id != expected_feature_id || project_id != expected_project_id {
        return Err(RuleError::new("id", "does not match durable file path"));
    }
    let number = map_u64(&mut m, "number")?;
    let local_slug = map_text(&mut m, "local_slug")?;
    let owner_employee_id = map_opt_uuid(&mut m, "owner_employee_id")?;
    let work_type = match map_opt_text(&mut m, "work_type")? {
        None => None,
        Some(raw) => Some(
            FeatureWorkType::parse(&raw)
                .ok_or_else(|| RuleError::new("work_type", "expected build or run"))?,
        ),
    };
    let title = map_text(&mut m, "title")?;
    let status_raw = map_text(&mut m, "status")?;
    let status = FeatureStatus::parse(&status_raw).ok_or_else(|| {
        RuleError::new("status", "expected planned, in_progress, live, or retired")
    })?;
    let delivered_at = map_opt_text(&mut m, "delivered_at")?;
    let retired_reason = map_opt_text(&mut m, "retired_reason")?;
    let replaced_by_feature_id = map_opt_uuid(&mut m, "replaced_by_feature_id")?;
    finish_map(&m)?;
    let rec = FeatureRecord {
        id,
        project_id,
        number,
        local_slug,
        owner_employee_id,
        work_type,
        title,
        status,
        body_markdown,
        delivered_at,
        retired_reason,
        replaced_by_feature_id,
        meta,
    };
    validate_feature_record(&rec)?;
    Ok(rec)
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
    if let Some(ref vl) = version_label {
        if !is_valid_line(vl) {
            return Err(RuleError::new("version_label", "invalid version_label"));
        }
    }
    if !is_body(&body_markdown) {
        return Err(RuleError::new("body_markdown", "invalid markdown body"));
    }
    let repository_commits = read_commits(&mut m)?;
    let feature_ids = read_uuid_list(&mut m, "feature_ids")?;
    let task_ids = read_uuid_list(&mut m, "task_ids")?;
    let mut flag_settings = Vec::new();
    if let Some(val) = m.remove("flag_settings") {
        let Value::Array(items) = val else {
            return Err(RuleError::new("flag_settings", "expected an array"));
        };
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
            let value = obj
                .remove("value")
                .ok_or_else(|| RuleError::new("value", "missing required field"))?;
            let rollout_rules = match obj.remove("rollout_rules") {
                None | Some(Value::Null) => None,
                Some(v) => Some(v),
            };
            validate_rollout_rules(rollout_rules.as_ref())?;
            finish_map(&obj)?;
            flag_settings.push(ReleaseFlagSettingRecord {
                flag_id,
                environment,
                target,
                value,
                rollout_rules,
            });
        }
    }
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
    let op_raw = map_text(&mut m, "operation")?;
    let operation = DeploymentOperation::parse(&op_raw)
        .ok_or_else(|| RuleError::new("operation", "expected deploy or rollback"))?;
    let rollback_of_id = map_opt_uuid(&mut m, "rollback_of_id")?;
    let baseline_deployment_id = map_opt_uuid(&mut m, "baseline_deployment_id")?;
    let status_raw = map_text(&mut m, "status")?;
    let status = DeploymentStatus::parse(&status_raw)
        .ok_or_else(|| RuleError::new("status", "invalid deployment status"))?;
    let started_at = map_opt_text(&mut m, "started_at")?;
    let finished_at = map_opt_text(&mut m, "finished_at")?;
    let external_url = map_opt_text(&mut m, "external_url")?;
    let repository_commits = read_commits(&mut m)?;
    let mut flag_settings = Vec::new();
    if let Some(val) = m.remove("flag_settings") {
        let Value::Array(items) = val else {
            return Err(RuleError::new("flag_settings", "expected an array"));
        };
        for item in items {
            let Value::Object(mut obj) = item else {
                return Err(RuleError::new("flag_settings", "expected object entries"));
            };
            let flag_id = map_uuid(&mut obj, "flag_id")?;
            let value = obj
                .remove("value")
                .ok_or_else(|| RuleError::new("value", "missing required field"))?;
            let rollout_rules = match obj.remove("rollout_rules") {
                None | Some(Value::Null) => None,
                Some(v) => Some(v),
            };
            validate_rollout_rules(rollout_rules.as_ref())?;
            let applied_at = map_opt_text(&mut obj, "applied_at")?;
            if let Some(ref ts) = applied_at {
                if parse_rfc3339_ms(ts).is_none() {
                    return Err(RuleError::new("applied_at", "invalid RFC 3339 timestamp"));
                }
            }
            finish_map(&obj)?;
            flag_settings.push(DeploymentFlagSettingRecord {
                flag_id,
                value,
                rollout_rules,
                applied_at,
            });
        }
    }
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

/// Encodes a [`FeatureFlagRecord`] to Markdown with YAML front matter.
pub fn encode_flag(flag: &FeatureFlagRecord) -> Vec<u8> {
    let mut f = Map::new();
    write_meta(&mut f, &flag.meta);
    f.insert("id".into(), json!(flag.id));
    f.insert("project_id".into(), json!(flag.project_id));
    f.insert("feature_id".into(), json!(flag.feature_id));
    f.insert("key".into(), json!(flag.key));
    f.insert("owner_employee_id".into(), json!(flag.owner_employee_id));
    f.insert("status".into(), json!(flag.status.as_str()));
    f.insert("value_type".into(), json!(flag.value_type.as_str()));
    f.insert("default_value".into(), flag.default_value.clone());
    yaml_encode_document(&f, &flag.body_markdown)
}

/// Decodes and validates a [`FeatureFlagRecord`].
pub fn decode_flag(
    bytes: &[u8],
    expected_project_id: &str,
    expected_flag_id: &str,
) -> Result<FeatureFlagRecord, RuleError> {
    let (mut m, body_markdown) = parse_doc_bytes(bytes)?;
    let meta = read_meta(&mut m)?;
    let id = map_uuid(&mut m, "id")?;
    let project_id = map_uuid(&mut m, "project_id")?;
    if id != expected_flag_id || project_id != expected_project_id {
        return Err(RuleError::new("id", "does not match durable file path"));
    }
    let feature_id = map_opt_uuid(&mut m, "feature_id")?;
    let key = map_text(&mut m, "key")?;
    let owner_employee_id = map_opt_uuid(&mut m, "owner_employee_id")?;
    let status_raw = map_text(&mut m, "status")?;
    let status = FlagStatus::parse(&status_raw)
        .ok_or_else(|| RuleError::new("status", "expected draft, active, or retired"))?;
    let value_type_raw = map_text(&mut m, "value_type")?;
    let value_type = FlagValueType::parse(&value_type_raw)
        .ok_or_else(|| RuleError::new("value_type", "expected boolean, string, number, or json"))?;
    let default_value = m
        .remove("default_value")
        .ok_or_else(|| RuleError::new("default_value", "missing required field"))?;
    finish_map(&m)?;
    let rec = FeatureFlagRecord {
        id,
        project_id,
        feature_id,
        key,
        owner_employee_id,
        status,
        value_type,
        default_value,
        body_markdown,
        meta,
    };
    validate_flag_record(&rec)?;
    Ok(rec)
}

/// Encodes a [`ScopedChangelogRecord`] to Markdown with YAML front matter.
pub fn encode_changelog(doc: &ScopedChangelogRecord) -> Vec<u8> {
    let mut f = Map::new();
    write_meta(&mut f, &doc.meta);
    f.insert("id".into(), json!(doc.id));
    f.insert("project_id".into(), json!(doc.project_id));
    f.insert("release_id".into(), json!(doc.release_id));
    f.insert("deployment_id".into(), json!(doc.deployment_id));
    f.insert("title".into(), json!(doc.title));
    f.insert("status".into(), json!(doc.status.as_str()));
    f.insert("owner_employee_id".into(), json!(doc.owner_employee_id));
    yaml_encode_document(&f, &doc.body_markdown)
}

/// Decodes and validates a [`ScopedChangelogRecord`].
pub fn decode_changelog(
    bytes: &[u8],
    expected_project_id: &str,
    expected_release_id: Option<&str>,
    expected_deployment_id: Option<&str>,
) -> Result<ScopedChangelogRecord, RuleError> {
    let (mut m, body_markdown) = parse_doc_bytes(bytes)?;
    let meta = read_meta(&mut m)?;
    let id = map_uuid(&mut m, "id")?;
    let project_id = map_uuid(&mut m, "project_id")?;
    let release_id = map_opt_uuid(&mut m, "release_id")?;
    let deployment_id = map_opt_uuid(&mut m, "deployment_id")?;
    if project_id != expected_project_id
        || release_id.as_deref() != expected_release_id
        || deployment_id.as_deref() != expected_deployment_id
    {
        return Err(RuleError::new("scope", "does not match durable file path"));
    }
    let title = map_text(&mut m, "title")?;
    let status_raw = map_text(&mut m, "status")?;
    let status = DocStatus::parse(&status_raw)
        .ok_or_else(|| RuleError::new("status", "expected draft, current, or deprecated"))?;
    let owner_employee_id = map_opt_uuid(&mut m, "owner_employee_id")?;
    finish_map(&m)?;
    validate_changelog_invariants(
        &title,
        status,
        owner_employee_id.as_deref(),
        &body_markdown,
        release_id.is_some(),
        deployment_id.is_some(),
    )?;
    Ok(ScopedChangelogRecord {
        id,
        project_id,
        release_id,
        deployment_id,
        title,
        status,
        owner_employee_id,
        body_markdown,
        meta,
    })
}

/// Encodes an [`EntityLinkRecord`] to YAML.
pub fn encode_link(link: &EntityLinkRecord) -> Vec<u8> {
    let mut f = Map::new();
    f.insert("schema_version".into(), json!(1u64));
    f.insert("id".into(), json!(link.id));
    f.insert("project_id".into(), json!(link.project_id));
    f.insert("source_kind".into(), json!(link.source_kind.as_str()));
    f.insert("source_id".into(), json!(link.source_id));
    f.insert("kind".into(), json!(link.kind.as_str()));
    f.insert("target_kind".into(), json!(link.target_kind.as_str()));
    f.insert("target_id".into(), json!(link.target_id));
    yaml_encode(&f)
}

/// Decodes and validates an [`EntityLinkRecord`].
pub fn decode_link(
    bytes: &[u8],
    expected_project_id: &str,
    expected_source_kind: LinkEntityKind,
    expected_source_id: &str,
    expected_link_id: &str,
) -> Result<EntityLinkRecord, RuleError> {
    let mut m = parse_yaml_bytes(bytes)?;
    if map_u64(&mut m, "schema_version")? != 1 {
        return Err(RuleError::new("schema_version", "must be 1"));
    }
    let id = map_uuid(&mut m, "id")?;
    let project_id = map_uuid(&mut m, "project_id")?;
    let source_kind = LinkEntityKind::parse(&map_text(&mut m, "source_kind")?)
        .ok_or_else(|| RuleError::new("source_kind", "invalid source_kind"))?;
    let source_id = map_uuid(&mut m, "source_id")?;
    let kind = LinkKind::parse(&map_text(&mut m, "kind")?)
        .ok_or_else(|| RuleError::new("kind", "invalid link kind"))?;
    let target_kind = LinkEntityKind::parse(&map_text(&mut m, "target_kind")?)
        .ok_or_else(|| RuleError::new("target_kind", "invalid target_kind"))?;
    let target_id = map_uuid(&mut m, "target_id")?;
    finish_map(&m)?;
    if id != expected_link_id
        || project_id != expected_project_id
        || source_kind != expected_source_kind
        || source_id != expected_source_id
    {
        return Err(RuleError::new("id", "does not match durable file path"));
    }
    let rec = EntityLinkRecord {
        id,
        project_id,
        source_kind,
        source_id,
        kind,
        target_kind,
        target_id,
    };
    validate_entity_link(&rec)?;
    Ok(rec)
}

/// Encodes a [`NumberCounterRecord`] to YAML.
pub fn encode_counter(counter: &NumberCounterRecord) -> Vec<u8> {
    let mut f = Map::new();
    f.insert("schema_version".into(), json!(1u64));
    f.insert("next_number".into(), json!(counter.next_number));
    let mut allocs = counter.allocations.clone();
    allocs.sort();
    f.insert(
        "allocations".into(),
        Value::Array(
            allocs
                .into_iter()
                .map(|a| json!({"number": a.number, "entity_id": a.entity_id}))
                .collect(),
        ),
    );
    yaml_encode(&f)
}

/// Decodes and validates a [`NumberCounterRecord`].
pub fn decode_counter(bytes: &[u8]) -> Result<NumberCounterRecord, RuleError> {
    let mut m = parse_yaml_bytes(bytes)?;
    if map_u64(&mut m, "schema_version")? != 1 {
        return Err(RuleError::new("schema_version", "must be 1"));
    }
    let next_number = map_u64(&mut m, "next_number")?;
    if next_number < 1 {
        return Err(RuleError::new("next_number", "must be >= 1"));
    }
    let mut allocations = Vec::new();
    if let Some(val) = m.remove("allocations") {
        let Value::Array(items) = val else {
            return Err(RuleError::new("allocations", "expected an array"));
        };
        for item in items {
            let Value::Object(mut obj) = item else {
                return Err(RuleError::new("allocations", "expected object entries"));
            };
            let number = map_u64(&mut obj, "number")?;
            if number == 0 || number >= next_number {
                return Err(RuleError::new(
                    "allocations",
                    "number must be in 1..next_number",
                ));
            }
            let entity_id = map_uuid(&mut obj, "entity_id")?;
            finish_map(&obj)?;
            allocations.push(NumberAllocation { number, entity_id });
        }
    }
    finish_map(&m)?;
    allocations.sort();
    Ok(NumberCounterRecord {
        next_number,
        allocations,
    })
}
