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

use super::{CallerContext, TessService};

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

    Ok(UiPresentResult {
        ui_block: UiBlock {
            title,
            entity_links,
            metrics,
            navigation,
            follow_ups,
            suggestion_id: None,
        },
    })
}

fn check_bounds(params: &UiPresentParams) -> Result<(), ApiError> {
    if params.follow_ups.len() > MAX_UI_FOLLOW_UPS {
        return Err(ApiError::new(
            ErrorCode::InvalidRequest,
            format!("`follow_ups` may contain at most {MAX_UI_FOLLOW_UPS} items"),
        ));
    }
    if params.entity_links.len() > MAX_UI_ENTITY_LINKS {
        return Err(ApiError::new(
            ErrorCode::InvalidRequest,
            format!("`entity_links` may contain at most {MAX_UI_ENTITY_LINKS} items"),
        ));
    }
    if params.metrics.len() > MAX_UI_METRICS {
        return Err(ApiError::new(
            ErrorCode::InvalidRequest,
            format!("`metrics` may contain at most {MAX_UI_METRICS} items"),
        ));
    }
    if params.navigation.len() > MAX_UI_NAVIGATION {
        return Err(ApiError::new(
            ErrorCode::InvalidRequest,
            format!("`navigation` may contain at most {MAX_UI_NAVIGATION} items"),
        ));
    }
    Ok(())
}

fn clean_text(field: &str, raw: &str) -> Result<String, ApiError> {
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
    if trimmed.chars().any(char::is_control) {
        return Err(ApiError::new(
            ErrorCode::InvalidRequest,
            format!("`{field}` must not contain control characters"),
        ));
    }
    Ok(trimmed.to_owned())
}

fn clean_route(raw: &str) -> Result<String, ApiError> {
    let route = clean_text("route", raw)?;
    if !route.starts_with('/')
        || route.starts_with("//")
        || route.contains("..")
        || route.contains("://")
        || route.contains('\\')
    {
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

fn validate_navigation(
    svc: &TessService,
    caller_id: &str,
    authority: &CallerAuthority,
    raw: Vec<UiNavigation>,
) -> Result<Vec<UiNavigation>, ApiError> {
    let mut out = Vec::with_capacity(raw.len());
    for item in raw {
        let label = clean_text("navigation.label", &item.label)?;
        let route = clean_route(&item.route)?;
        let (entity_kind, entity_id) = match (item.entity_kind, item.entity_id) {
            (Some(kind), Some(id)) => {
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
                (Some(verified.kind), Some(verified.id))
            }
            (None, None) => (None, None),
            _ => {
                return Err(ApiError::new(
                    ErrorCode::InvalidRequest,
                    "navigation `entity_kind` and `entity_id` must be provided together",
                ));
            }
        };
        out.push(UiNavigation {
            label,
            route,
            entity_kind,
            entity_id,
        });
    }
    Ok(out)
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
    let inspected = svc.office().task_inspect(raw_id).map_err(|err| {
        crate::logged("tess_calls.ui_task", &err);
        ApiError::new(ErrorCode::Unavailable, "unable to verify task")
    })??;
    Ok(UiEntityLink {
        kind: UiEntityKind::Task,
        id: inspected.task.id,
        route: format!("/tasks/{}", inspected.task.key),
        key: Some(inspected.task.key),
        label: custom_label.unwrap_or(inspected.task.title),
    })
}

fn verify_project(
    svc: &TessService,
    raw_id: &str,
    custom_label: Option<String>,
) -> Result<UiEntityLink, ApiError> {
    let project = svc.office().inspect_project(raw_id).map_err(|err| {
        crate::logged("tess_calls.ui_project", &err);
        ApiError::new(ErrorCode::Unavailable, "unable to verify project")
    })??;
    Ok(UiEntityLink {
        kind: UiEntityKind::Project,
        id: project.id,
        route: format!("/projects/{}", project.key),
        key: Some(project.key),
        label: custom_label.unwrap_or(project.name),
    })
}

fn verify_team(
    svc: &TessService,
    raw_id: &str,
    custom_label: Option<String>,
) -> Result<UiEntityLink, ApiError> {
    let snap = svc.office().org_snapshot().map_err(|err| {
        crate::logged("tess_calls.ui_team", &err);
        ApiError::new(ErrorCode::Unavailable, "unable to verify team")
    })?;
    let team = snap
        .teams
        .into_iter()
        .find(|t| t.id == raw_id || t.name.eq_ignore_ascii_case(raw_id))
        .ok_or_else(|| ApiError::new(ErrorCode::NotFound, format!("team `{raw_id}` not found")))?;
    Ok(UiEntityLink {
        kind: UiEntityKind::Team,
        route: format!("/org/teams/{}", team.id),
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
    let snap = svc.office().org_snapshot().map_err(|err| {
        crate::logged("tess_calls.ui_member", &err);
        ApiError::new(ErrorCode::Unavailable, "unable to verify member")
    })?;
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
        route: format!("/org/members/{}", member.id),
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
        route: format!("/org/roles/{}", role.id),
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
    let shown = svc
        .office()
        .process_show(caller_id, params)
        .map_err(|err| {
            crate::logged("tess_calls.ui_process", &err);
            ApiError::new(ErrorCode::Unavailable, "unable to verify process document")
        })??;
    Ok(UiEntityLink {
        kind: UiEntityKind::Process,
        route: format!("/processes/{}", shown.document.id),
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
        .map_err(|err| {
            crate::logged("tess_calls.ui_channel", &err);
            ApiError::new(ErrorCode::Unavailable, "unable to verify channel")
        })??;
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
        route: format!("/chat/channels/{}", channel.id),
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
    let offices = svc.office().list_org_offices().map_err(|err| {
        crate::logged("tess_calls.ui_office", &err);
        ApiError::new(ErrorCode::Unavailable, "unable to verify office")
    })?;
    let office = offices
        .into_iter()
        .find(|o| o.id.as_str() == raw_id || o.slug.eq_ignore_ascii_case(raw_id))
        .ok_or_else(|| {
            ApiError::new(ErrorCode::NotFound, format!("office `{raw_id}` not found"))
        })?;
    Ok(UiEntityLink {
        kind: UiEntityKind::Office,
        route: format!("/offices/{}", office.id),
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
        .map_err(|err| {
            crate::logged("tess_calls.ui_budget", &err);
            ApiError::new(ErrorCode::Unavailable, "unable to verify budget")
        })??;
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
        route: format!("/budgets/{}", budget.id),
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
        .map_err(|err| {
            crate::logged("tess_calls.ui_hire", &err);
            ApiError::new(ErrorCode::Unavailable, "unable to verify hire request")
        })?
        .ok_or_else(|| {
            ApiError::new(
                ErrorCode::NotFound,
                format!("hire request `{raw_id}` not found"),
            )
        })?;
    if !authority.is_owner {
        let allowed = svc
            .office()
            .caller_may_hire_into(caller_id, &scope.home_team_id)
            .map_err(|err| {
                crate::logged("tess_calls.ui_hire_scope", &err);
                ApiError::new(ErrorCode::Unavailable, "unable to verify hiring scope")
            })?;
        if !allowed {
            return Err(ApiError::new(
                ErrorCode::NotAuthorized,
                "caller may not view hire requests outside their teams",
            ));
        }
    }
    Ok(UiEntityLink {
        kind: UiEntityKind::HireRequest,
        route: format!("/hr/hires/{raw_id}"),
        id: raw_id.to_owned(),
        key: None,
        label: custom_label.unwrap_or_else(|| format!("Hire request {raw_id}")),
    })
}
