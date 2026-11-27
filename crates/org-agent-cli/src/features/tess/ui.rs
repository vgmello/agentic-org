//! Chief of Staff `ui present` command (`TASK-80.4`): server-validated structured UI blocks.

use org_command::{Answer, Refusal, Request};
use org_proto::tess::{
    UiEntityLinkInput, UiFollowUp, UiMetric, UiNavigation, UiPresent, UiPresentParams,
    UiPresentResult,
};
use org_proto::ErrorCode;
use serde::de::DeserializeOwned;

use crate::runtime::domain::CommandCue;
use crate::runtime::input::{call_rpc, encode, opt_string};
use crate::runtime::transport::OfficeTransport;

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
            "Pass structured fields via `--input-json` matching `agent-os ui present --schema`.",
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
