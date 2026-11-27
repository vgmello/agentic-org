//! Server-validated `ui.present` handler: verifies every entity reference against live office
//! state and caller visibility, enforces strict item bounds, and returns a verified [`UiBlock`].

use org_office::CallerAuthority;
use org_proto::budgets::BudgetGetParams;
use org_proto::chat::{ChatConversationsParams, ConversationKind};
use org_proto::hiring::RoleList;
use org_proto::processes::ProcessShowParams;
use org_proto::tess::{
    UiBlock, UiEntityKind, UiEntityLink, UiEntityLinkInput, UiFollowUp, UiMetric, UiNavigation,
    UiPresentParams, UiPresentResult, MAX_UI_ENTITY_LINKS, MAX_UI_FOLLOW_UPS, MAX_UI_METRICS,
    MAX_UI_NAVIGATION, MAX_UI_TEXT_BYTES,
};
use org_proto::{ApiError, ErrorCode, Outcome, Request};

use super::preflight::HIRE_NOT_VISIBLE;
use super::{unavailable, CallerContext, TessService};

/// Explicit static allowlist of top-level in-app views permitted without a verified entity target.
pub(crate) const STATIC_TOP_LEVEL_ROUTES: &[&str] = &[
    "/",
    "/home",
    "/inbox",
    "/work",
    "/projects",
    "/chat",
    "/activity",
    "/insights",
    "/search",
    "/preferences",
    "/teams",
    "/teams/members",
    "/teams/org-chart",
    "/teams/hire",
    "/teams/hires",
    "/settings",
    "/settings/offices",
    "/settings/agents",
    "/settings/roles",
    "/settings/processes",
    "/settings/processes/development",
    "/settings/access",
    "/settings/security",
    "/settings/costs",
    "/settings/updates",
];

pub(super) fn handle_present(
    svc: &TessService,
    context: &CallerContext,
    params: UiPresentParams,
) -> Result<UiPresentResult, ApiError> {
    let (caller_id, authority) = match context {
        CallerContext::TessActingFor {
            caller_id,
            authority,
            ..
        }
        | CallerContext::HumanCaller {
            caller_id,
            authority,
        } => (caller_id.as_str(), authority),
        CallerContext::TessWake => {
            return Err(ApiError::new(
                ErrorCode::NotAuthorized,
                "ui.present requires an active human caller",
            ));
        }
    };

    check_bounds(&params)?;
    let title = params
        .title
        .as_deref()
        .map(|t| clean_text("title", t))
        .transpose()?;

    let mut entity_links = Vec::with_capacity(params.entity_links.len());
    for input in &params.entity_links {
        entity_links.push(verify_entity(svc, caller_id, authority, input)?);
    }

    let metrics = validate_metrics(params.metrics)?;
    let navigation = validate_navigation(svc, caller_id, authority, params.navigation)?;
    let follow_ups = validate_follow_ups(params.follow_ups)?;

    let ui_block = UiBlock {
        title,
        entity_links,
        metrics,
        navigation,
        follow_ups,
        suggestion_id: None,
    };
    if let CallerContext::TessActingFor { caller_id, .. } = context {
        svc.publish_turn_ui_block(caller_id, ui_block.clone());
    }
    Ok(UiPresentResult { ui_block })
}

fn check_bounds(params: &UiPresentParams) -> Result<(), ApiError> {
    for (field, len, max) in [
        ("follow_ups", params.follow_ups.len(), MAX_UI_FOLLOW_UPS),
        (
            "entity_links",
            params.entity_links.len(),
            MAX_UI_ENTITY_LINKS,
        ),
        ("metrics", params.metrics.len(), MAX_UI_METRICS),
        ("navigation", params.navigation.len(), MAX_UI_NAVIGATION),
    ] {
        if len > max {
            return Err(ApiError::new(
                ErrorCode::InvalidRequest,
                format!("`{field}` may contain at most {max} items"),
            ));
        }
    }
    Ok(())
}

fn clean_text(field: &str, raw: &str) -> Result<String, ApiError> {
    if raw.chars().any(char::is_control) {
        return Err(ApiError::new(
            ErrorCode::InvalidRequest,
            format!("`{field}` must not contain control characters"),
        ));
    }
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(ApiError::new(
            ErrorCode::InvalidRequest,
            format!("`{field}` must not be empty"),
        ));
    }
    if trimmed.len() > MAX_UI_TEXT_BYTES {
        return Err(ApiError::new(
            ErrorCode::InvalidRequest,
            format!("`{field}` exceeds {MAX_UI_TEXT_BYTES} bytes"),
        ));
    }
    Ok(trimmed.to_owned())
}

fn is_valid_channel_query_route(route: &str) -> bool {
    let Some(channel_id) = route.strip_prefix("/chat?c=") else {
        return false;
    };
    !channel_id.is_empty() && !channel_id.contains(['?', '&', '=', '/'])
}

pub(crate) fn clean_route(raw: &str, allow_channel_query: bool) -> Result<String, ApiError> {
    let route = clean_text("route", raw)?;
    let has_disallowed_query =
        route.contains('?') && !(allow_channel_query && is_valid_channel_query_route(&route));
    let unsafe_shape = !route.starts_with('/')
        || route.contains("//")
        || route.contains("..")
        || route.contains("://")
        || route.contains('\\')
        || has_disallowed_query
        || route.contains('#')
        || route.contains('%')
        || route.chars().any(char::is_whitespace);
    if unsafe_shape {
        return Err(ApiError::new(
            ErrorCode::InvalidRequest,
            "navigation `route` must be a safe relative in-app path starting with `/`",
        ));
    }
    Ok(route)
}

fn validate_metrics(raw: Vec<UiMetric>) -> Result<Vec<UiMetric>, ApiError> {
    let mut out = Vec::with_capacity(raw.len());
    for m in raw {
        out.push(UiMetric {
            label: clean_text("metrics.label", &m.label)?,
            value: clean_text("metrics.value", &m.value)?,
            tone: m
                .tone
                .as_deref()
                .map(|t| clean_text("metrics.tone", t))
                .transpose()?,
            unit: m
                .unit
                .as_deref()
                .map(|u| clean_text("metrics.unit", u))
                .transpose()?,
        });
    }
    Ok(out)
}

fn validate_navigation_item(
    svc: &TessService,
    caller_id: &str,
    authority: &CallerAuthority,
    item: UiNavigation,
) -> Result<UiNavigation, ApiError> {
    let label = clean_text("navigation.label", &item.label)?;
    let (verified_route, entity_kind, entity_id) = match (item.entity_kind, item.entity_id) {
        (Some(kind), Some(id)) => {
            let route = clean_route(&item.route, kind == UiEntityKind::Channel)?;
            let verified = verify_entity(
                svc,
                caller_id,
                authority,
                &UiEntityLinkInput {
                    kind,
                    id,
                    label: Some(label.clone()),
                },
            )?;
            if route != verified.route {
                return Err(ApiError::new(
                    ErrorCode::InvalidRequest,
                    format!(
                        "navigation `route` `{route}` does not match verified entity route `{}`",
                        verified.route
                    ),
                ));
            }
            (verified.route, Some(verified.kind), Some(verified.id))
        }
        (None, None) => {
            let route = clean_route(&item.route, false)?;
            if !STATIC_TOP_LEVEL_ROUTES.contains(&route.as_str()) {
                return Err(ApiError::new(
                    ErrorCode::InvalidRequest,
                    format!(
                        "unverified navigation route `{route}` requires a verified `entity_kind` and `entity_id`"
                    ),
                ));
            }
            (route, None, None)
        }
        _ => {
            return Err(ApiError::new(
                ErrorCode::InvalidRequest,
                "navigation `entity_kind` and `entity_id` must be provided together",
            ));
        }
    };
    Ok(UiNavigation {
        label,
        route: verified_route,
        entity_kind,
        entity_id,
    })
}

fn validate_navigation(
    svc: &TessService,
    caller_id: &str,
    authority: &CallerAuthority,
    raw: Vec<UiNavigation>,
) -> Result<Vec<UiNavigation>, ApiError> {
    raw.into_iter()
        .map(|item| validate_navigation_item(svc, caller_id, authority, item))
        .collect()
}

fn validate_follow_ups(raw: Vec<UiFollowUp>) -> Result<Vec<UiFollowUp>, ApiError> {
    let mut out = Vec::with_capacity(raw.len());
    for f in raw {
        out.push(UiFollowUp {
            label: clean_text("follow_ups.label", &f.label)?,
            prompt: clean_text("follow_ups.prompt", &f.prompt)?,
        });
    }
    Ok(out)
}

fn verify_entity(
    svc: &TessService,
    caller_id: &str,
    authority: &CallerAuthority,
    input: &UiEntityLinkInput,
) -> Result<UiEntityLink, ApiError> {
    let raw_id = clean_text("entity_links.id", &input.id)?;
    let custom_label = input
        .label
        .as_deref()
        .map(|l| clean_text("entity_links.label", l))
        .transpose()?;
    match input.kind {
        UiEntityKind::Task => verify_task(svc, &raw_id, custom_label),
        UiEntityKind::Project => verify_project(svc, &raw_id, custom_label),
        UiEntityKind::Team => verify_team(svc, &raw_id, custom_label),
        UiEntityKind::Member => verify_member(svc, &raw_id, custom_label),
        UiEntityKind::Role => verify_role(svc, caller_id, &raw_id, custom_label),
        UiEntityKind::Process => verify_process(svc, caller_id, &raw_id, custom_label),
        UiEntityKind::Channel => verify_channel(svc, caller_id, &raw_id, custom_label),
        UiEntityKind::Office => verify_office(svc, &raw_id, custom_label),
        UiEntityKind::Budget => verify_budget(svc, caller_id, &raw_id, custom_label),
        UiEntityKind::HireRequest => verify_hire(svc, caller_id, authority, &raw_id, custom_label),
        _ => Err(ApiError::new(
            ErrorCode::InvalidRequest,
            "unknown entity kind in `ui.present`",
        )),
    }
}

fn verify_task(
    svc: &TessService,
    raw_id: &str,
    custom_label: Option<String>,
) -> Result<UiEntityLink, ApiError> {
    let inspected = svc
        .office()
        .task_inspect(raw_id)
        .map_err(|e| unavailable("tess_calls.ui_task", &e, "unable to verify task"))??;
    Ok(UiEntityLink {
        kind: UiEntityKind::Task,
        id: inspected.task.id,
        route: format!("/work/{}", inspected.task.key),
        key: Some(inspected.task.key),
        label: custom_label.unwrap_or(inspected.task.title),
    })
}

fn verify_project(
    svc: &TessService,
    raw_id: &str,
    custom_label: Option<String>,
) -> Result<UiEntityLink, ApiError> {
    let project = svc
        .office()
        .inspect_project(raw_id)
        .map_err(|e| unavailable("tess_calls.ui_project", &e, "unable to verify project"))??;
    Ok(UiEntityLink {
        kind: UiEntityKind::Project,
        route: format!("/projects/{}", project.id),
        id: project.id,
        key: Some(project.key),
        label: custom_label.unwrap_or(project.name),
    })
}

fn verify_team(
    svc: &TessService,
    raw_id: &str,
    custom_label: Option<String>,
) -> Result<UiEntityLink, ApiError> {
    let snap = svc
        .office()
        .org_snapshot()
        .map_err(|e| unavailable("tess_calls.ui_team", &e, "unable to verify team"))?;
    let team = snap
        .teams
        .into_iter()
        .find(|t| t.id == raw_id || t.name.eq_ignore_ascii_case(raw_id))
        .ok_or_else(|| ApiError::new(ErrorCode::NotFound, format!("team `{raw_id}` not found")))?;
    Ok(UiEntityLink {
        kind: UiEntityKind::Team,
        route: format!("/teams/{}", team.id),
        id: team.id,
        key: None,
        label: custom_label.unwrap_or(team.name),
    })
}

fn verify_member(
    svc: &TessService,
    raw_id: &str,
    custom_label: Option<String>,
) -> Result<UiEntityLink, ApiError> {
    let snap = svc
        .office()
        .org_snapshot()
        .map_err(|e| unavailable("tess_calls.ui_member", &e, "unable to verify member"))?;
    let needle = raw_id.strip_prefix('@').unwrap_or(raw_id);
    let member = snap
        .members
        .into_iter()
        .find(|m| {
            m.id == needle
                || m.handle
                    .as_deref()
                    .is_some_and(|h| h.eq_ignore_ascii_case(needle))
                || (needle.eq_ignore_ascii_case("owner") && m.id == svc.office().owner_member_id())
        })
        .ok_or_else(|| {
            ApiError::new(ErrorCode::NotFound, format!("member `{raw_id}` not found"))
        })?;
    Ok(UiEntityLink {
        kind: UiEntityKind::Member,
        route: format!("/members/{}", member.id),
        id: member.id,
        key: member.handle,
        label: custom_label.unwrap_or(member.name),
    })
}

fn verify_role(
    svc: &TessService,
    caller_id: &str,
    raw_id: &str,
    custom_label: Option<String>,
) -> Result<UiEntityLink, ApiError> {
    let req = Request::new::<RoleList>(0, &org_proto::hiring::RoleListParams {})
        .map_err(|_| ApiError::new(ErrorCode::Unavailable, "unable to construct role lookup"))?;
    let Some(resp) = svc.office().dispatch_hiring_rpc(Some(caller_id), &req) else {
        return Err(ApiError::new(
            ErrorCode::Unavailable,
            "role lookup is unavailable",
        ));
    };
    let roles = match resp.outcome {
        Outcome::Result(v) => {
            serde_json::from_value::<org_proto::hiring::RoleListResult>(v)
                .map_err(|_| ApiError::new(ErrorCode::Unavailable, "invalid role list response"))?
                .roles
        }
        Outcome::Error(err) => return Err(err),
    };
    let role = roles
        .into_iter()
        .find(|r| {
            r.id == raw_id
                || r.key.as_deref() == Some(raw_id)
                || r.name.eq_ignore_ascii_case(raw_id)
        })
        .ok_or_else(|| ApiError::new(ErrorCode::NotFound, format!("role `{raw_id}` not found")))?;
    Ok(UiEntityLink {
        kind: UiEntityKind::Role,
        route: format!("/settings/roles/{}", role.id),
        id: role.id,
        key: role.key,
        label: custom_label.unwrap_or(role.name),
    })
}

fn verify_process(
    svc: &TessService,
    caller_id: &str,
    raw_id: &str,
    custom_label: Option<String>,
) -> Result<UiEntityLink, ApiError> {
    let (document_id, key) = if org_proto::is_uuid_v7(raw_id) {
        (Some(raw_id.to_owned()), None)
    } else {
        (None, Some(raw_id.to_owned()))
    };
    let params = ProcessShowParams {
        document_id,
        key,
        employee_id: None,
        r#for: None,
        offset: Some(0),
        revision: None,
    };
    let shown = svc.office().process_show(caller_id, params).map_err(|e| {
        unavailable(
            "tess_calls.ui_process",
            &e,
            "unable to verify process document",
        )
    })??;
    Ok(UiEntityLink {
        kind: UiEntityKind::Process,
        route: format!("/settings/processes/documents/{}", shown.document.id),
        id: shown.document.id,
        key: shown.document.key,
        label: custom_label.unwrap_or(shown.document.title),
    })
}

fn verify_channel(
    svc: &TessService,
    caller_id: &str,
    raw_id: &str,
    custom_label: Option<String>,
) -> Result<UiEntityLink, ApiError> {
    let listed = svc
        .office()
        .list_conversations(
            caller_id,
            ChatConversationsParams {
                include_archived: true,
            },
        )
        .map_err(|e| unavailable("tess_calls.ui_channel", &e, "unable to verify channel"))??;
    let needle = raw_id.strip_prefix('#').unwrap_or(raw_id);
    // Only visible channels — never link private DMs through `ui.present`.
    let channel = listed
        .conversations
        .into_iter()
        .filter(|c| c.kind == ConversationKind::Channel)
        .find(|c| {
            c.id == needle
                || c.name
                    .as_deref()
                    .is_some_and(|n| n.eq_ignore_ascii_case(needle))
        })
        .ok_or_else(|| {
            ApiError::new(
                ErrorCode::NotFound,
                format!("channel `{raw_id}` not found or not visible to caller"),
            )
        })?;
    let name = channel.name.clone().unwrap_or_else(|| channel.id.clone());
    Ok(UiEntityLink {
        kind: UiEntityKind::Channel,
        route: format!("/chat?c={}", channel.id),
        id: channel.id,
        key: channel.name.map(|n| format!("#{n}")),
        label: custom_label.unwrap_or(name),
    })
}

fn verify_office(
    svc: &TessService,
    raw_id: &str,
    custom_label: Option<String>,
) -> Result<UiEntityLink, ApiError> {
    let offices = svc
        .office()
        .list_org_offices()
        .map_err(|e| unavailable("tess_calls.ui_office", &e, "unable to verify office"))?;
    let office = offices
        .into_iter()
        .find(|o| o.id.as_str() == raw_id || o.slug.eq_ignore_ascii_case(raw_id))
        .ok_or_else(|| {
            ApiError::new(ErrorCode::NotFound, format!("office `{raw_id}` not found"))
        })?;
    Ok(UiEntityLink {
        kind: UiEntityKind::Office,
        route: "/settings/offices".to_owned(),
        id: office.id.to_string(),
        key: Some(office.slug),
        label: custom_label.unwrap_or(office.name),
    })
}

fn verify_budget(
    svc: &TessService,
    caller_id: &str,
    raw_id: &str,
    custom_label: Option<String>,
) -> Result<UiEntityLink, ApiError> {
    let result = svc
        .office()
        .budget_get(
            caller_id,
            BudgetGetParams {
                budget_id: Some(raw_id.to_owned()),
                ..BudgetGetParams::default()
            },
        )
        .map_err(|e| unavailable("tess_calls.ui_budget", &e, "unable to verify budget"))??;
    let budget = result.budget.ok_or_else(|| {
        ApiError::new(
            ErrorCode::NotFound,
            format!("budget `{raw_id}` not found or not visible to caller"),
        )
    })?;
    let default_label = format!(
        "{} {} budget",
        budget.scope_kind.as_str(),
        budget.period_kind.as_str()
    );
    Ok(UiEntityLink {
        kind: UiEntityKind::Budget,
        route: "/settings/costs".to_owned(),
        id: budget.id,
        key: None,
        label: custom_label.unwrap_or(default_label),
    })
}

fn verify_hire(
    svc: &TessService,
    caller_id: &str,
    authority: &CallerAuthority,
    raw_id: &str,
    custom_label: Option<String>,
) -> Result<UiEntityLink, ApiError> {
    if !(authority.is_owner || authority.may_hire) {
        return Err(ApiError::new(
            ErrorCode::NotAuthorized,
            "caller is not authorized to view hire requests",
        ));
    }
    let scope = svc
        .office()
        .delegated_hire_scope(raw_id)
        .map_err(|e| unavailable("tess_calls.ui_hire", &e, "unable to verify hire request"))?
        .ok_or_else(|| ApiError::new(ErrorCode::NotFound, HIRE_NOT_VISIBLE))?;
    if !authority.is_owner {
        let allowed = svc
            .office()
            .caller_may_hire_into(caller_id, &scope.home_team_id)
            .map_err(|e| {
                unavailable(
                    "tess_calls.ui_hire_scope",
                    &e,
                    "unable to verify hiring scope",
                )
            })?;
        if !allowed {
            return Err(ApiError::new(ErrorCode::NotFound, HIRE_NOT_VISIBLE));
        }
    }
    Ok(UiEntityLink {
        kind: UiEntityKind::HireRequest,
        route: "/teams/hires".to_owned(),
        id: raw_id.to_owned(),
        key: None,
        label: custom_label.unwrap_or_else(|| format!("Hire request {raw_id}")),
    })
}
