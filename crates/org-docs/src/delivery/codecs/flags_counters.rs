//! Deterministic codecs for feature flags, scoped changelogs, and reference counters.

use std::collections::BTreeSet;

use serde_json::{json, Map, Value};

use super::{
    finish_map, map_opt_uuid, map_text, map_u64, map_uuid, parse_doc_bytes, parse_yaml_bytes,
    read_meta, write_meta,
};
use crate::delivery::records::{
    FeatureFlagRecord, FlagStatus, FlagValueType, NumberAllocation, NumberCounterRecord,
    ScopedChangelogRecord,
};
use crate::delivery::rules::{
    canonicalize_timestamp, validate_changelog_invariants, validate_flag_record, RuleError,
};
use crate::project_tree::{yaml_encode, yaml_encode_document, DocStatus};

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
    let status = FlagStatus::parse(&map_text(&mut m, "status")?)
        .ok_or_else(|| RuleError::new("status", "expected draft, active, or retired"))?;
    let value_type = FlagValueType::parse(&map_text(&mut m, "value_type")?)
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
    let status = DocStatus::parse(&map_text(&mut m, "status")?)
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
                .map(|a| {
                    json!({
                        "number": a.number,
                        "entity_id": a.entity_id,
                        "allocated_at": a.allocated_at,
                    })
                })
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
    let mut seen_numbers = BTreeSet::new();
    let mut seen_entities = BTreeSet::new();
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
            if !seen_numbers.insert(number) {
                return Err(RuleError::new("allocations", "duplicate allocated number"));
            }
            let entity_id = map_uuid(&mut obj, "entity_id")?;
            if !seen_entities.insert(entity_id.clone()) {
                return Err(RuleError::new(
                    "allocations",
                    "duplicate allocated entity_id",
                ));
            }
            let allocated_at =
                canonicalize_timestamp("allocated_at", &map_text(&mut obj, "allocated_at")?)?;
            finish_map(&obj)?;
            allocations.push(NumberAllocation {
                number,
                entity_id,
                allocated_at,
            });
        }
    }
    finish_map(&m)?;
    allocations.sort();
    Ok(NumberCounterRecord {
        next_number,
        allocations,
    })
}
