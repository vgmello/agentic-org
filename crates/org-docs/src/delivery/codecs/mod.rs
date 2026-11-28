//! Strict deterministic YAML/Markdown codecs for all delivery records.

mod features_links;
mod flags_counters;
mod releases_deployments;

use std::collections::BTreeSet;

use org_proto::{is_uuid_v7, parse_rfc3339_ms};
use serde_json::{json, Map, Value};

use super::records::{ActorRef, RecordMeta, RepoCommitRecord};
use super::rules::{canonicalize_opt_timestamp, is_valid_commit_sha, RuleError};
use crate::{parse, parse_mapping};

pub use features_links::{decode_feature, decode_link, encode_feature, encode_link};
pub use flags_counters::{
    decode_changelog, decode_counter, decode_flag, encode_changelog, encode_counter, encode_flag,
};
pub use releases_deployments::{
    decode_deployment, decode_milestone, decode_release, encode_deployment, encode_milestone,
    encode_release,
};

pub(super) fn map_text(map: &mut Map<String, Value>, key: &str) -> Result<String, RuleError> {
    match map.remove(key) {
        Some(Value::String(s)) => Ok(s),
        Some(_) => Err(RuleError::new(key, "expected a string")),
        None => Err(RuleError::new(key, "missing required field")),
    }
}

pub(super) fn map_opt_text(
    map: &mut Map<String, Value>,
    key: &str,
) -> Result<Option<String>, RuleError> {
    match map.remove(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(s)) => Ok(Some(s)),
        Some(_) => Err(RuleError::new(key, "expected a string or null")),
    }
}

pub(super) fn map_opt_ts(
    map: &mut Map<String, Value>,
    key: &str,
) -> Result<Option<String>, RuleError> {
    let raw = map_opt_text(map, key)?;
    canonicalize_opt_timestamp(key, raw.as_deref())
}

pub(super) fn map_u64(map: &mut Map<String, Value>, key: &str) -> Result<u64, RuleError> {
    match map.remove(key) {
        Some(Value::Number(n)) => n
            .as_u64()
            .ok_or_else(|| RuleError::new(key, "expected an unsigned integer")),
        Some(_) => Err(RuleError::new(key, "expected an integer")),
        None => Err(RuleError::new(key, "missing required field")),
    }
}

pub(super) fn map_uuid(map: &mut Map<String, Value>, key: &str) -> Result<String, RuleError> {
    let val = map_text(map, key)?;
    if !is_uuid_v7(&val) {
        return Err(RuleError::new(key, "must be a lowercase UUIDv7"));
    }
    Ok(val)
}

pub(super) fn map_opt_uuid(
    map: &mut Map<String, Value>,
    key: &str,
) -> Result<Option<String>, RuleError> {
    match map_opt_text(map, key)? {
        None => Ok(None),
        Some(val) if is_uuid_v7(&val) => Ok(Some(val)),
        Some(_) => Err(RuleError::new(key, "must be a lowercase UUIDv7")),
    }
}

pub(super) fn finish_map(map: &Map<String, Value>) -> Result<(), RuleError> {
    if let Some(extra) = map.keys().next() {
        return Err(RuleError::new(extra, "unknown field"));
    }
    Ok(())
}

pub(super) fn write_meta(fields: &mut Map<String, Value>, meta: &RecordMeta) {
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

pub(super) fn read_meta(map: &mut Map<String, Value>) -> Result<RecordMeta, RuleError> {
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

pub(super) fn parse_doc_bytes(bytes: &[u8]) -> Result<(Map<String, Value>, String), RuleError> {
    let text = std::str::from_utf8(bytes).map_err(|_| RuleError::new("", "not UTF-8"))?;
    let doc = parse(text).map_err(|e| RuleError::new(e.field, format!("{:?}", e.reason)))?;
    let fields = doc
        .front_matter
        .ok_or_else(|| RuleError::new("", "missing YAML front matter"))?;
    Ok((fields, doc.body.to_owned()))
}

pub(super) fn parse_yaml_bytes(bytes: &[u8]) -> Result<Map<String, Value>, RuleError> {
    let text = std::str::from_utf8(bytes).map_err(|_| RuleError::new("", "not UTF-8"))?;
    parse_mapping(text).map_err(|e| RuleError::new(e.field, format!("{:?}", e.reason)))
}

pub(super) fn encode_commits(commits: &[RepoCommitRecord]) -> Value {
    let mut sorted = commits.to_vec();
    sorted.sort();
    Value::Array(
        sorted
            .into_iter()
            .map(|c| json!({"repository_id": c.repository_id, "commit_sha": c.commit_sha}))
            .collect(),
    )
}

pub(super) fn read_commits(
    map: &mut Map<String, Value>,
) -> Result<Vec<RepoCommitRecord>, RuleError> {
    let Some(val) = map.remove("repository_commits") else {
        return Ok(Vec::new());
    };
    let Value::Array(items) = val else {
        return Err(RuleError::new("repository_commits", "expected an array"));
    };
    let mut seen_repos = BTreeSet::new();
    let mut out = Vec::with_capacity(items.len());
    for item in items {
        let Value::Object(mut obj) = item else {
            return Err(RuleError::new(
                "repository_commits",
                "expected object entries",
            ));
        };
        let repository_id = map_uuid(&mut obj, "repository_id")?;
        if !seen_repos.insert(repository_id.clone()) {
            return Err(RuleError::new(
                "repository_commits",
                "duplicate repository_id",
            ));
        }
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

pub(super) fn read_uuid_list(
    map: &mut Map<String, Value>,
    key: &str,
) -> Result<Vec<String>, RuleError> {
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
