//! Pure write decisions for canonical features and Why links.

use org_proto::delivery::*;
use org_proto::ErrorCode;

use super::codecs::{encode_counter, encode_feature, encode_link};
use super::ops::{
    allocate_number, check_revision, invalid, new_meta, parse_uuid, refused, reply, rule_err, stor,
    touch_meta, validate_owner, CallerContext, DeliveryMutation, DeliveryOpError,
    DeliveryStoreView,
};
use super::paths::{feature_counter_path, feature_path, link_path};
use super::records::{EntityLinkRecord, FeatureRecord, FeatureStatus, LinkKind};
use super::rules::{
    canonicalize_opt_timestamp, check_feature_replacement_acyclic, slug_from_title,
    validate_entity_link, validate_feature_record,
};
use super::wire::{
    entity_link_wire, feature_status_from_wire, feature_summary_wire, feature_work_type_from_wire,
    link_entity_kind_from_wire, link_kind_from_wire,
};

/// Decides `features.create`.
pub fn decide_create_feature<V: DeliveryStoreView>(
    view: &V,
    ctx: &CallerContext<'_>,
    params: &FeaturesCreateParams,
) -> Result<DeliveryMutation, DeliveryOpError> {
    let id = view.derive_uuid_v7("delivery-feature", &params.command_id, ctx.now_ms);
    let status = match params.status {
        Some(s) => feature_status_from_wire(s).map_err(rule_err)?,
        None => FeatureStatus::Planned,
    };
    let work_type = params
        .work_type
        .map(feature_work_type_from_wire)
        .transpose()
        .map_err(rule_err)?;
    let local_slug = match params.local_slug.as_deref() {
        Some(raw) => raw.trim().to_owned(),
        None => slug_from_title(&params.title),
    };
    let mut owner_employee_id = validate_owner(view, params.owner_employee_id.as_deref())?;
    if matches!(status, FeatureStatus::InProgress | FeatureStatus::Live)
        && owner_employee_id.is_none()
    {
        owner_employee_id = Some(ctx.caller_id.to_owned());
    }
    if stor(view.project_features(ctx.project_id))?
        .iter()
        .any(|f| f.local_slug == local_slug)
    {
        return Err(refused(
            ErrorCode::AlreadyExists,
            "local_slug: another feature in this project uses this slug",
        ));
    }
    let delivered_at = canonicalize_opt_timestamp("delivered_at", params.delivered_at.as_deref())
        .map_err(rule_err)?;
    let (number, counter) = allocate_number(
        view,
        "project",
        ctx.project_id,
        "feature",
        &id,
        ctx.now_rfc3339,
    )?;
    let rec = FeatureRecord {
        id,
        project_id: ctx.project_id.to_owned(),
        number,
        local_slug,
        owner_employee_id,
        work_type,
        title: params.title.trim().to_owned(),
        status,
        body_markdown: params.body_markdown.clone(),
        delivered_at,
        retired_reason: None,
        replaced_by_feature_id: None,
        meta: new_meta(ctx),
    };
    validate_feature_record(&rec).map_err(rule_err)?;
    let answer = FeatureWriteResult {
        feature: feature_summary_wire(&rec, ctx.project_slug),
    };
    Ok(DeliveryMutation {
        puts: vec![
            (
                feature_counter_path(&rec.project_id),
                encode_counter(&counter),
            ),
            (feature_path(&rec.project_id, &rec.id), encode_feature(&rec)),
        ],
        removes: Vec::new(),
        reply: reply(&answer),
    })
}

/// Decides `features.update`.
pub fn decide_update_feature<V: DeliveryStoreView>(
    view: &V,
    ctx: &CallerContext<'_>,
    params: &FeaturesUpdateParams,
) -> Result<DeliveryMutation, DeliveryOpError> {
    let fid = parse_uuid("feature_id", &params.feature_id)?;
    let mut all = stor(view.project_features(ctx.project_id))?;
    let Some(pos) = all.iter().position(|f| f.id == fid) else {
        return Err(refused(ErrorCode::NotFound, "feature not found"));
    };
    let before = all[pos].clone();
    check_revision(before.meta.revision, params.expected_revision)?;
    let mut rec = before.clone();
    if let Some(ref t) = params.title {
        rec.title = t.trim().to_owned();
    }
    if let Some(ref slug) = params.local_slug {
        let s = slug.trim().to_owned();
        if all.iter().any(|f| f.id != rec.id && f.local_slug == s) {
            return Err(refused(
                ErrorCode::AlreadyExists,
                "local_slug: another feature in this project uses this slug",
            ));
        }
        rec.local_slug = s;
    }
    if let Some(s) = params.status {
        rec.status = feature_status_from_wire(s).map_err(rule_err)?;
    }
    if let Some(wt) = params.work_type {
        rec.work_type = Some(feature_work_type_from_wire(wt).map_err(rule_err)?);
    }
    if let Some(ref owner) = params.owner_employee_id {
        rec.owner_employee_id = validate_owner(view, Some(owner))?;
    }
    if let Some(ref body) = params.body_markdown {
        rec.body_markdown = body.clone();
    }
    if let Some(ref ts) = params.delivered_at {
        rec.delivered_at =
            canonicalize_opt_timestamp("delivered_at", Some(ts)).map_err(rule_err)?;
    }
    if let Some(ref reason) = params.retired_reason {
        rec.retired_reason = (!reason.trim().is_empty()).then(|| reason.trim().to_owned());
    }
    if let Some(ref rep) = params.replaced_by_feature_id {
        let trimmed = rep.trim();
        if trimmed.is_empty() {
            rec.replaced_by_feature_id = None;
        } else {
            let rep_id = parse_uuid("replaced_by_feature_id", trimmed)?;
            if !all.iter().any(|f| f.id == rep_id) {
                return Err(refused(
                    ErrorCode::NotFound,
                    "replacement feature not found",
                ));
            }
            rec.replaced_by_feature_id = Some(rep_id);
        }
    }
    if rec == before {
        return Err(invalid("feature", "nothing to change"));
    }
    validate_feature_record(&rec).map_err(rule_err)?;
    all[pos] = rec.clone();
    check_feature_replacement_acyclic(&all).map_err(rule_err)?;
    touch_meta(&mut rec.meta, ctx);
    let answer = FeatureWriteResult {
        feature: feature_summary_wire(&rec, ctx.project_slug),
    };
    Ok(DeliveryMutation {
        puts: vec![(feature_path(&rec.project_id, &rec.id), encode_feature(&rec))],
        removes: Vec::new(),
        reply: reply(&answer),
    })
}

/// Decides `features.retire`.
pub fn decide_retire_feature<V: DeliveryStoreView>(
    view: &V,
    ctx: &CallerContext<'_>,
    params: &FeaturesRetireParams,
) -> Result<DeliveryMutation, DeliveryOpError> {
    let fid = parse_uuid("feature_id", &params.feature_id)?;
    let mut all = stor(view.project_features(ctx.project_id))?;
    let Some(pos) = all.iter().position(|f| f.id == fid) else {
        return Err(refused(ErrorCode::NotFound, "feature not found"));
    };
    let mut rec = all[pos].clone();
    check_revision(rec.meta.revision, params.expected_revision)?;
    rec.status = FeatureStatus::Retired;
    if let Some(ref reason) = params.retired_reason {
        rec.retired_reason = (!reason.trim().is_empty()).then(|| reason.trim().to_owned());
    }
    if let Some(ref rep) = params.replaced_by_feature_id {
        let trimmed = rep.trim();
        if trimmed.is_empty() {
            rec.replaced_by_feature_id = None;
        } else {
            let rep_id = parse_uuid("replaced_by_feature_id", trimmed)?;
            if !all.iter().any(|f| f.id == rep_id) {
                return Err(refused(
                    ErrorCode::NotFound,
                    "replacement feature not found",
                ));
            }
            rec.replaced_by_feature_id = Some(rep_id);
        }
    }
    validate_feature_record(&rec).map_err(rule_err)?;
    all[pos] = rec.clone();
    check_feature_replacement_acyclic(&all).map_err(rule_err)?;
    touch_meta(&mut rec.meta, ctx);
    let answer = FeatureWriteResult {
        feature: feature_summary_wire(&rec, ctx.project_slug),
    };
    Ok(DeliveryMutation {
        puts: vec![(feature_path(&rec.project_id, &rec.id), encode_feature(&rec))],
        removes: Vec::new(),
        reply: reply(&answer),
    })
}

/// Decides `links.add`.
pub fn decide_add_link<V: DeliveryStoreView>(
    view: &V,
    ctx: &CallerContext<'_>,
    params: &LinksAddParams,
) -> Result<DeliveryMutation, DeliveryOpError> {
    let id = view.derive_uuid_v7("delivery-link", &params.command_id, ctx.now_ms);
    let source_kind = link_entity_kind_from_wire(params.source_kind).map_err(rule_err)?;
    let target_kind = link_entity_kind_from_wire(params.target_kind).map_err(rule_err)?;
    let kind = link_kind_from_wire(params.kind).map_err(rule_err)?;
    if kind == LinkKind::DependsOn {
        return Err(invalid(
            "kind",
            "use tasks.dependency_add for depends_on task dependencies; links.add handles relates_to and fixes",
        ));
    }
    let source_id = parse_uuid("source_id", &params.source_id)?;
    let target_id = parse_uuid("target_id", &params.target_id)?;
    let rec = EntityLinkRecord {
        id,
        project_id: ctx.project_id.to_owned(),
        source_kind,
        source_id: source_id.clone(),
        kind,
        target_kind,
        target_id: target_id.clone(),
    };
    validate_entity_link(&rec).map_err(rule_err)?;
    if !stor(view.entity_exists_in_project(ctx.project_id, source_kind, &source_id))? {
        return Err(refused(
            ErrorCode::NotFound,
            "source entity not found in project",
        ));
    }
    if !stor(view.entity_exists_in_project(ctx.project_id, target_kind, &target_id))? {
        return Err(refused(
            ErrorCode::NotFound,
            "target entity not found in project",
        ));
    }
    if stor(view.project_links(ctx.project_id))?.iter().any(|l| {
        l.source_kind == rec.source_kind
            && l.source_id == rec.source_id
            && l.kind == rec.kind
            && l.target_kind == rec.target_kind
            && l.target_id == rec.target_id
    }) {
        return Err(refused(ErrorCode::AlreadyExists, "link already exists"));
    }
    let answer = LinkWriteResult {
        link: entity_link_wire(&rec),
    };
    Ok(DeliveryMutation {
        puts: vec![(
            link_path(&rec.project_id, rec.source_kind, &rec.source_id, &rec.id),
            encode_link(&rec),
        )],
        removes: Vec::new(),
        reply: reply(&answer),
    })
}

/// Decides `links.remove`.
pub fn decide_remove_link<V: DeliveryStoreView>(
    view: &V,
    ctx: &CallerContext<'_>,
    params: &LinksRemoveParams,
) -> Result<DeliveryMutation, DeliveryOpError> {
    let source_kind = link_entity_kind_from_wire(params.source_kind).map_err(rule_err)?;
    let source_id = parse_uuid("source_id", &params.source_id)?;
    let link_id = parse_uuid("link_id", &params.link_id)?;
    let found = stor(view.project_links(ctx.project_id))?
        .into_iter()
        .find(|l| l.source_kind == source_kind && l.source_id == source_id && l.id == link_id);
    let Some(link) = found else {
        return Err(refused(ErrorCode::NotFound, "link not found"));
    };
    let answer = LinkRemoveResult {
        removed_id: link.id.clone(),
    };
    Ok(DeliveryMutation {
        puts: Vec::new(),
        removes: vec![link_path(
            &link.project_id,
            link.source_kind,
            &link.source_id,
            &link.id,
        )],
        reply: reply(&answer),
    })
}
