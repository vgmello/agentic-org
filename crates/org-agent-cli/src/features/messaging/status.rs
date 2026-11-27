//! `status set|clear`: the short live status line teammates see in the directory.

use org_command::{Answer, Refusal, Request};
use org_proto::directory::StatusSetResult;
use serde_json::{Map, Value};

use super::with_ack;
use crate::runtime::domain::CommandCue;
use crate::runtime::input::{decode_err, insert_opt, opt_u64, reject_dry_run, require_str};
use crate::runtime::transport::OfficeTransport;

/// `Use when:` guidance of each `status` command.
pub(super) const CUES: &[CommandCue] = &[
    ("status", "clear", "clearing your live status line when you finish an activity or go idle."),
    ("status", "set", "publishing a short live status line (with optional `--ttl-ms`) so teammates see what you are doing."),
];

pub(super) fn write_status<T: OfficeTransport + ?Sized>(
    transport: &mut T,
    request: &Request,
    clear: bool,
) -> Result<Answer, Refusal> {
    reject_dry_run(request)?;
    let mut params = Map::new();
    if clear {
        params.insert("text".into(), Value::Null);
    } else {
        let text = require_str(request, "text")?;
        params.insert("text".into(), Value::String(text.to_owned()));
        if let Some(ttl_ms) = opt_u64(request, "ttl_ms")? {
            params.insert("ttl_ms".into(), Value::from(ttl_ms));
        }
    }
    let raw = transport.call_method("status.set", Value::Object(params))?;
    let res: StatusSetResult = serde_json::from_value(raw).map_err(|_| decode_err("status.set"))?;
    let mut obj = Map::new();
    insert_opt(&mut obj, "status", res.status.as_deref());
    insert_opt(&mut obj, "status_expires_at", res.expires_at.as_deref());
    Ok(with_ack(Answer::completed(Value::Object(obj)), request))
}
