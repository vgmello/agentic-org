//! Chief of Staff `ui present` command (`TASK-80.4`): server-validated structured UI blocks.

use org_command::{Answer, Command, Example, Field, Kind, Refusal, Request, Type};
use org_proto::tess::{
    UiEntityLinkInput, UiFollowUp, UiMetric, UiNavigation, UiPresent, UiPresentParams,
    UiPresentResult,
};
use org_proto::ErrorCode;
use serde::de::DeserializeOwned;

use super::TESS_ERRORS;
use crate::runtime::domain::CommandCue;
use crate::runtime::input::{call_rpc, encode, opt_string};
use crate::runtime::transport::OfficeTransport;

const ENTITY_KIND_ENUM: &[&str] = &[
    "task",
    "project",
    "team",
    "member",
    "role",
    "process",
    "channel",
    "office",
    "budget",
    "hire_request",
];

const UI_ENTITY_LINK_INPUT_FIELDS: &[Field] = &[
    Field::required("kind", Type::Enum(ENTITY_KIND_ENUM), "Entity kind"),
    Field::required("id", Type::String, "Entity UUIDv7, key, handle, or name"),
    Field::optional("label", Type::String, "Optional display label override"),
];
const UI_ENTITY_LINK_INPUT_TYPE: Type = Type::Object(UI_ENTITY_LINK_INPUT_FIELDS);

const UI_ENTITY_LINK_FIELDS: &[Field] = &[
    Field::required("kind", Type::String, "Verified entity kind"),
    Field::required("id", Type::String, "Canonical entity UUIDv7"),
    Field::optional("key", Type::String, "Human-readable key or handle"),
    Field::required("label", Type::String, "Verified display label"),
    Field::required("route", Type::String, "Verified in-app route"),
];
const UI_ENTITY_LINK_TYPE: Type = Type::Object(UI_ENTITY_LINK_FIELDS);

const UI_METRIC_FIELDS: &[Field] = &[
    Field::required("label", Type::String, "Metric label"),
    Field::required("value", Type::String, "Formatted metric value"),
    Field::optional("tone", Type::String, "Optional visual tone"),
    Field::optional("unit", Type::String, "Optional metric unit"),
];
const UI_METRIC_TYPE: Type = Type::Object(UI_METRIC_FIELDS);

const UI_NAVIGATION_FIELDS: &[Field] = &[
    Field::required("label", Type::String, "Navigation action label"),
    Field::required("route", Type::String, "Safe relative in-app route"),
    Field::optional(
        "entity_kind",
        Type::Enum(ENTITY_KIND_ENUM),
        "Optional entity kind to verify",
    ),
    Field::optional(
        "entity_id",
        Type::String,
        "Optional entity ID/key to verify",
    ),
];
const UI_NAVIGATION_TYPE: Type = Type::Object(UI_NAVIGATION_FIELDS);

const UI_FOLLOW_UP_FIELDS: &[Field] = &[
    Field::required("label", Type::String, "Short follow-up chip label"),
    Field::required("prompt", Type::String, "Follow-up prompt text"),
];
const UI_FOLLOW_UP_TYPE: Type = Type::Object(UI_FOLLOW_UP_FIELDS);

pub(super) const UI_BLOCK_FIELDS: &[Field] = &[
    Field::optional("title", Type::String, "Optional block heading"),
    Field::optional(
        "entity_links",
        Type::List(&UI_ENTITY_LINK_TYPE),
        "Server-verified entity links",
    ),
    Field::optional(
        "metrics",
        Type::List(&UI_METRIC_TYPE),
        "Compact metric cards",
    ),
    Field::optional(
        "navigation",
        Type::List(&UI_NAVIGATION_TYPE),
        "Validated in-app navigation links",
    ),
    Field::optional(
        "follow_ups",
        Type::List(&UI_FOLLOW_UP_TYPE),
        "Suggested follow-up prompts (max 3)",
    ),
    Field::optional(
        "suggestion_id",
        Type::String,
        "Attached pending suggestion ID, when present",
    ),
];

const UI_PRESENT_INPUT: &[Field] = &[
    Field::optional("title", Type::String, "Optional block title"),
    Field::optional(
        "entity_links",
        Type::List(&UI_ENTITY_LINK_INPUT_TYPE),
        "Entities to verify and link (structured JSON)",
    ),
    Field::optional(
        "metrics",
        Type::List(&UI_METRIC_TYPE),
        "Metric items (structured JSON)",
    ),
    Field::optional(
        "navigation",
        Type::List(&UI_NAVIGATION_TYPE),
        "In-app navigation actions (structured JSON)",
    ),
    Field::optional(
        "follow_ups",
        Type::List(&UI_FOLLOW_UP_TYPE),
        "Up to 3 follow-up prompts (structured JSON)",
    ),
];

const UI_PRESENT_OUTPUT: &[Field] = &[Field::required(
    "ui_block",
    Type::Object(UI_BLOCK_FIELDS),
    "Server-verified structured UI block",
)];

pub(super) const COMMANDS: &[Command] = &[Command {
    resource: "ui",
    verb: "present",
    summary: "Validate and emit a structured UI block",
    call: "ui.present",
    kind: Kind::Read,
    input: UI_PRESENT_INPUT,
    content_field: None,
    output: UI_PRESENT_OUTPUT,
    errors: TESS_ERRORS,
    example: Example {
        input: r#"{"title":"Brief summary","metrics":[{"label":"Open tasks","value":"4"}],"follow_ups":[{"label":"Check blockers","prompt":"Show blocked tasks"}]}"#,
        output: r#"{"ui_block":{"title":"Brief summary","metrics":[{"label":"Open tasks","value":"4"}],"follow_ups":[{"label":"Check blockers","prompt":"Show blocked tasks"}]}}"#,
    },
}];

pub(super) const CUES: &[CommandCue] = &[(
    "ui",
    "present",
    "attaching server-verified entity links, compact metrics, navigation shortcuts, and up to 3 follow-ups to a Chief of Staff answer.",
)];

fn decode_vec<T: DeserializeOwned>(request: &Request, field: &str) -> Result<Vec<T>, Refusal> {
    let Some(raw) = request.input.get(field) else {
        return Ok(Vec::new());
    };
    serde_json::from_value(raw.clone()).map_err(|e| {
        Refusal::new(
            ErrorCode::InvalidRequest,
            format!("invalid `{field}` payload: {e}"),
            "Pass structured fields via `--json` matching `agent-os ui present --schema`.",
        )
    })
}

pub(super) fn present_ui<T: OfficeTransport + ?Sized>(
    transport: &mut T,
    request: &Request,
) -> Result<Answer, Refusal> {
    let params = UiPresentParams {
        title: opt_string(request, "title"),
        entity_links: decode_vec::<UiEntityLinkInput>(request, "entity_links")?,
        metrics: decode_vec::<UiMetric>(request, "metrics")?,
        navigation: decode_vec::<UiNavigation>(request, "navigation")?,
        follow_ups: decode_vec::<UiFollowUp>(request, "follow_ups")?,
    };
    let res: UiPresentResult = call_rpc::<UiPresent, _>(transport, &params)?;
    Ok(Answer::completed(encode(&res, "ui.present")?))
}
