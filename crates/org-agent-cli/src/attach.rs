//! Long-running `agent-os session attach` mode (`doc-28` §2.9, `CLI-13`, `ACK-1`).
//!
//! Binds ONCE on startup, reads one JSON command object per line of stdin, executes each
//! through the shared `org-command` table on the same persistent office connection, and
//! writes one compact single-line JSON envelope per line to stdout. A failing line writes an
//! error envelope and never stops subsequent lines.

use std::io::{BufRead, Write};

use org_command::{CommandTable, Office};
use org_proto::ErrorCode;
use serde_json::{Map, Value};

use crate::keys::UuidV7KeySource;
use crate::sources::StdSources;
use crate::table;

/// Runs the `session attach` line loop over arbitrary `reader`, `writer`, and `office`.
pub fn run_attach_loop<R: BufRead, W: Write>(
    program: &str,
    reader: R,
    mut writer: W,
    office: &mut dyn Office,
) -> u8 {
    let table = table::table();
    let mut sources = StdSources;
    let mut keys = UuidV7KeySource;
    for line in reader.lines() {
        let Ok(raw) = line else {
            break;
        };
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            continue;
        }
        let reply = match parse_line_args(trimmed) {
            Ok(args) => run_one(&table, program, &args, &mut sources, &mut keys, office),
            Err(message) => error_envelope(ErrorCode::InvalidRequest, &message),
        };
        if writeln!(writer, "{reply}").is_err() {
            break;
        }
        let _ = writer.flush();
    }
    0
}

fn run_one(
    table: &CommandTable,
    program: &str,
    args: &[String],
    sources: &mut StdSources,
    keys: &mut UuidV7KeySource,
    office: &mut dyn Office,
) -> String {
    let out = org_command::run(table, program, args, sources, keys, office);
    let trimmed = out.stdout.trim_end_matches('\n');
    if !trimmed.contains('\n') && serde_json::from_str::<Value>(trimmed).is_ok() {
        trimmed.to_owned()
    } else {
        error_envelope(
            ErrorCode::InvalidRequest,
            "Command produced non-JSON or multi-line output (run `--help` outside `session attach`).",
        )
    }
}

fn parse_line_args(raw: &str) -> Result<Vec<String>, String> {
    let value: Value = serde_json::from_str(raw).map_err(|_| {
        "Each line in `session attach` must be a JSON object with `command`.".to_owned()
    })?;
    let Some(obj) = value.as_object() else {
        return Err("Each line in `session attach` must be a JSON object.".into());
    };
    let Some(command) = obj.get("command").and_then(Value::as_str) else {
        return Err("`command` must be a string naming a resource and verb.".into());
    };
    let mut parts: Vec<String> = command.split_whitespace().map(str::to_owned).collect();
    if parts.is_empty() {
        return Err("`command` must name a resource and verb (e.g. `msg inbox`).".into());
    }
    if parts.first().map(String::as_str) == Some("who") {
        parts.remove(0);
        parts.insert(0, "list".into());
        parts.insert(0, "directory".into());
    } else if parts.len() >= 2 && parts[0] == "task" && parts[1] == "note" {
        parts.remove(0);
        parts[0] = "task-note".into();
    }
    let mut args = parts;
    if let Some(input) = obj.get("input").filter(|v| !v.is_null()) {
        let json = serde_json::to_string(input)
            .map_err(|_| "Could not serialize `input` as JSON.".to_owned())?;
        args.push("--json".into());
        args.push(json);
    }
    if let Some(opt_val) = obj.get("options").filter(|v| !v.is_null()) {
        let Some(options) = opt_val.as_object() else {
            return Err("`options` in `session attach` must be a JSON object.".into());
        };
        append_options(&mut args, options)?;
    }
    Ok(args)
}

fn append_options(args: &mut Vec<String>, options: &Map<String, Value>) -> Result<(), String> {
    for (key, value) in options {
        let flag = if key.starts_with("--") {
            key.clone()
        } else {
            format!("--{}", key.replace('_', "-"))
        };
        match value {
            Value::Bool(true) => args.push(flag),
            Value::Bool(false) | Value::Null => {}
            Value::String(s) => {
                args.push(flag);
                args.push(s.clone());
            }
            Value::Number(n) => {
                args.push(flag);
                args.push(n.to_string());
            }
            _ => {
                return Err(format!(
                    "Option `{key}` in `session attach` must be a string, number, or boolean."
                ));
            }
        }
    }
    Ok(())
}

/// Formats a single-line `org-command` error envelope when `session attach` fails before dispatch.
pub fn error_envelope(code: ErrorCode, message: &str) -> String {
    let code_str = serde_json::to_value(code)
        .ok()
        .and_then(|v| v.as_str().map(str::to_owned))
        .unwrap_or_else(|| "invalid_request".into());
    serde_json::json!({
        "ok": false,
        "error": {
            "code": code_str,
            "message": message,
            "hint": "Send one JSON object per line with `command`, optional `input`, and optional `options`."
        }
    })
    .to_string()
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use super::*;
    use crate::dispatch::{OfficeTransport, TransportOffice};

    struct StubTransport {
        calls: Vec<(String, Value)>,
    }

    impl OfficeTransport for StubTransport {
        fn call_method(
            &mut self,
            method: &str,
            params: Value,
        ) -> Result<Value, org_command::Refusal> {
            self.calls.push((method.to_owned(), params));
            match method {
                "chat.inbox" => Ok(serde_json::json!({ "items": [] })),
                "status.set" => Ok(serde_json::json!({ "status": "building" })),
                _ => Err(org_command::Refusal::new(
                    ErrorCode::UnknownMethod,
                    "stub unknown",
                    "check",
                )),
            }
        }
    }

    #[test]
    fn attach_answers_every_line_and_continues_after_bad_json() {
        let stdin = concat!(
            r#"{"command":"msg inbox","input":{},"options":{"limit":5}}"#,
            "\n",
            "not json at all\n",
            r#"{"command":"status set","input":{"text":"building"}}"#,
            "\n"
        );
        let mut out = Vec::new();
        let mut office = TransportOffice(StubTransport { calls: Vec::new() });
        let exit = run_attach_loop("agent-os", Cursor::new(stdin), &mut out, &mut office);
        assert_eq!(exit, 0);
        let lines: Vec<&str> = std::str::from_utf8(&out).unwrap().lines().collect();
        assert_eq!(lines.len(), 3, "{lines:?}");
        let l0: Value = serde_json::from_str(lines[0]).unwrap();
        assert_eq!(l0["ok"], true);
        let l1: Value = serde_json::from_str(lines[1]).unwrap();
        assert_eq!(l1["ok"], false);
        assert_eq!(l1["error"]["code"], "invalid_request");
        let l2: Value = serde_json::from_str(lines[2]).unwrap();
        assert_eq!(l2["ok"], true);
        assert_eq!(l2["data"]["status"], "building");
        assert_eq!(office.0.calls.len(), 2);
    }

    #[test]
    fn attach_rejects_non_object_options_and_wraps_help_as_single_line_json() {
        let stdin = concat!(
            r#"{"command":"msg inbox","options":["not-an-object"]}"#,
            "\n",
            r#"{"command":"msg inbox","options":{"help":true}}"#,
            "\n",
            r#"{"command":"status set","input":{"text":"still alive"}}"#,
            "\n"
        );
        let mut out = Vec::new();
        let mut office = TransportOffice(StubTransport { calls: Vec::new() });
        let exit = run_attach_loop("agent-os", Cursor::new(stdin), &mut out, &mut office);
        assert_eq!(exit, 0);
        let lines: Vec<&str> = std::str::from_utf8(&out).unwrap().lines().collect();
        assert_eq!(lines.len(), 3, "{lines:?}");
        let l0: Value = serde_json::from_str(lines[0]).unwrap();
        assert_eq!(l0["ok"], false);
        assert_eq!(l0["error"]["code"], "invalid_request");
        let l1: Value = serde_json::from_str(lines[1]).unwrap();
        assert_eq!(l1["ok"], false);
        assert_eq!(l1["error"]["code"], "invalid_request");
        let l2: Value = serde_json::from_str(lines[2]).unwrap();
        assert_eq!(l2["ok"], true);
        assert_eq!(l2["data"]["status"], "building");

        let parsed =
            parse_line_args(r#"{"command":"task note add","input":{"task":"T-A1","text":"plan"}}"#)
                .unwrap();
        assert_eq!(parsed[0], "task-note");
        assert_eq!(parsed[1], "add");
    }
}
