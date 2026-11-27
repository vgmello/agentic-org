//! `process inspect`: one process or standard, read whole or in chunks (`--offset`, pinned by
//! `--revision`), or the `--why` inheritance chain of an override key.

use org_command::{Answer, Refusal, Request};
use org_proto::processes::{
    ProcessForWire, ProcessShow, ProcessShowParams, ProcessShowResult, ProcessWhy,
    ProcessWhyParams, ProcessWhyResult,
};
use org_proto::{Call, ErrorCode};
use serde_json::Value;

use super::process::parse_for;
use super::unreadable;
use crate::runtime::domain::CommandCue;
use crate::runtime::input::{encode, opt_string, opt_u64, opt_usize};
use crate::runtime::transport::OfficeTransport;

/// `Use when:` guidance of `process inspect`.
pub(super) const CUES: &[CommandCue] = &[
    ("process", "inspect", "reading a process/standard's full Markdown body (chunked with `--offset`) or tracing inheritance with `--why`."),
];

pub(super) fn read_process_inspect<T: OfficeTransport + ?Sized>(
    transport: &mut T,
    request: &Request,
) -> Result<Answer, Refusal> {
    let why = request
        .input
        .get("why")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let key = opt_string(&request.input, "key");
    let document_id = opt_string(&request.input, "id");
    let target_for = parse_for(&request.input)?;
    let offset = opt_usize(&request.input, "offset")?;
    let expected_revision = opt_u64(&request.input, "revision")?;

    if why {
        return read_process_why(
            transport,
            key,
            document_id,
            offset,
            expected_revision,
            target_for,
        );
    }

    if key.is_some() && document_id.is_some() {
        return Err(Refusal::new(
            ErrorCode::InvalidRequest,
            "`--key` and `--id` are mutually exclusive",
            "Pass either `--key <key>` or `--id <document_id>`, not both.",
        ));
    }
    if key.is_none() && document_id.is_none() {
        return Err(Refusal::new(
            ErrorCode::InvalidRequest,
            "provide `--key` or `--id`",
            "Run `agent-os process list` to see available keys.",
        ));
    }
    if expected_revision.is_some() && document_id.is_none() {
        return Err(Refusal::new(
            ErrorCode::InvalidRequest,
            "`--revision` requires `--id <document_id>`",
            "Pass `--id <document.id> --revision <document.revision> --offset <next_offset>` from the first chunk.",
        ));
    }
    let params = ProcessShowParams {
        key,
        document_id,
        employee_id: None,
        r#for: target_for,
        offset,
        revision: expected_revision,
    };
    let raw = transport
        .call_method(ProcessShow::METHOD, encode(&params, ProcessShow::METHOD)?)
        .map_err(|mut refusal| {
            if refusal.code == ErrorCode::Conflict {
                refusal.hint = "The document changed while paginating. Discard accumulated chunks and re-run `process inspect` from offset 0.".to_owned();
            }
            refusal
        })?;
    let res: ProcessShowResult =
        serde_json::from_value(raw).map_err(|_| unreadable("process.show", "process inspect"))?;
    Ok(Answer::completed(encode(&res, ProcessShow::METHOD)?))
}

fn read_process_why<T: OfficeTransport + ?Sized>(
    transport: &mut T,
    key: Option<String>,
    document_id: Option<String>,
    offset: Option<usize>,
    expected_revision: Option<u64>,
    target_for: Option<ProcessForWire>,
) -> Result<Answer, Refusal> {
    if offset.is_some() || expected_revision.is_some() {
        return Err(Refusal::new(
            ErrorCode::InvalidRequest,
            "`--offset` and `--revision` cannot be combined with `--why`",
            "Omit `--why` to paginate body chunks, or omit `--offset`/`--revision` to inspect the inheritance chain.",
        ));
    }
    if document_id.is_some() {
        return Err(Refusal::new(
            ErrorCode::InvalidRequest,
            "`--id` cannot be combined with `--why`",
            "Pass `--key <key> --why` to inspect the inheritance chain for an override key.",
        ));
    }
    let Some(k) = key else {
        return Err(Refusal::new(
            ErrorCode::InvalidRequest,
            "`--why` requires `--key <key>`",
            "Pass `--key <key> --why`.",
        ));
    };
    let params = ProcessWhyParams {
        key: k,
        employee_id: None,
        r#for: target_for,
    };
    let raw = transport.call_method(ProcessWhy::METHOD, encode(&params, ProcessWhy::METHOD)?)?;
    let res: ProcessWhyResult =
        serde_json::from_value(raw).map_err(|_| unreadable("process.why", "process why"))?;
    Ok(Answer::completed(encode(&res, ProcessWhy::METHOD)?))
}
