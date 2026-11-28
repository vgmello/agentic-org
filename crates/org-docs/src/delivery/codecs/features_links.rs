//! Deterministic codecs for canonical features and Why links.

use serde_json::{json, Map};

use super::{
    finish_map, map_opt_text, map_opt_ts, map_opt_uuid, map_text, map_u64, map_uuid,
    parse_doc_bytes, parse_yaml_bytes, read_meta, write_meta,
};
use crate::delivery::records::{
    EntityLinkRecord, FeatureRecord, FeatureStatus, FeatureWorkType, LinkEntityKind, LinkKind,
};
use crate::delivery::rules::{validate_entity_link, validate_feature_record, RuleError};
use crate::project_tree::{yaml_encode, yaml_encode_document};

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
    let delivered_at = map_opt_ts(&mut m, "delivered_at")?;
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
    if kind == LinkKind::DependsOn {
        return Err(RuleError::new(
            "kind",
            "depends_on belongs in task links.yaml, not project links/",
        ));
    }
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
