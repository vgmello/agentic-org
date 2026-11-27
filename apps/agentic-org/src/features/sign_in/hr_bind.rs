//! Inline `--bind-workspace <dir>` support for `agentic-org hr hire ...` (`TASK-211`):
//! hires the agent over the operator's signed-in office channel and immediately binds `<dir>`
//! to the newly hired employee ID on the same channel, storing credentials strictly outside Git.

use std::path::{Path, PathBuf};

use org_agent_cli::external_auth::bind::{
    bind_workspace, preflight_workspace, BoundWorkspace, WorkspaceBindRequest,
};
use org_agent_cli::{OfficeTransport, TransportOffice};
use org_cli_shell::context::ExecutionContext;
use org_command::{Output, Refusal};
use serde_json::Value;

use super::office_proxy::Shown;
use super::render::{self, print_answer};
use super::selectors::{command_path_len, flag_takes_value, has_flag, resource_and_verb};
use crate::error::CliError;

const BIND_WORKSPACE_FLAG: &str = "--bind-workspace";

/// Splits `--bind-workspace <dir>` (or `--bind-workspace=<dir>`) out of `hr hire` arguments,
/// ignoring values of other flags and anything after `--`.
pub(super) fn split_bind_workspace(
    forwarded: &[String],
) -> Result<(Option<PathBuf>, Vec<String>), CliError> {
    if resource_and_verb(forwarded) != Some(("hr", "hire"))
        || has_flag(forwarded, &["--help", "-h", "--schema", "--version"])
    {
        return Ok((None, forwarded.to_vec()));
    }
    let mut workspace: Option<PathBuf> = None;
    let mut kept = Vec::with_capacity(forwarded.len());
    let mut iter = forwarded.iter();
    let mut awaiting_other = false;
    let mut after_terminator = false;
    while let Some(arg) = iter.next() {
        if awaiting_other || after_terminator {
            awaiting_other = false;
            kept.push(arg.clone());
            continue;
        }
        if arg == "--" {
            after_terminator = true;
            kept.push(arg.clone());
            continue;
        }
        if let Some(value) = arg.strip_prefix("--bind-workspace=") {
            set_workspace(&mut workspace, value)?;
            continue;
        }
        if arg == BIND_WORKSPACE_FLAG {
            let value = iter
                .next()
                .ok_or_else(|| CliError::usage("--bind-workspace needs a value"))?;
            set_workspace(&mut workspace, value)?;
            continue;
        }
        if let Some(flag_name) = arg.strip_prefix("--") {
            if !flag_name.contains('=') && flag_takes_value(&kept, flag_name) {
                awaiting_other = true;
            }
        }
        kept.push(arg.clone());
    }
    Ok((workspace, kept))
}

fn set_workspace(slot: &mut Option<PathBuf>, value: &str) -> Result<(), CliError> {
    if value.is_empty() {
        return Err(CliError::usage("--bind-workspace needs a directory path"));
    }
    if slot.replace(PathBuf::from(value)).is_some() {
        return Err(CliError::usage("--bind-workspace is given more than once"));
    }
    Ok(())
}

/// Explicit `--format` requested by the operator on `hr hire ... --bind-workspace`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ExplicitFormat {
    Json,
    Yaml,
    Compact,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EffectiveOutput {
    Quiet,
    Rendered,
    Json,
    Yaml,
    Compact,
}

/// Validated preflight outcome before calling `hr.hire`.
pub(super) struct PreflightedHrBind {
    pub(super) canonical_workspace: PathBuf,
    pub(super) explicit_format: Option<ExplicitFormat>,
}

fn resolve_output(explicit: Option<ExplicitFormat>, shown: Shown) -> EffectiveOutput {
    match explicit {
        Some(ExplicitFormat::Json) => EffectiveOutput::Json,
        Some(ExplicitFormat::Yaml) => EffectiveOutput::Yaml,
        Some(ExplicitFormat::Compact) => EffectiveOutput::Compact,
        None => match shown {
            Shown::Quiet => EffectiveOutput::Quiet,
            Shown::Rendered | Shown::Tsv => EffectiveOutput::Rendered,
            Shown::Json | Shown::AsWritten => EffectiveOutput::Json,
            Shown::Yaml => EffectiveOutput::Yaml,
            Shown::Compact => EffectiveOutput::Compact,
        },
    }
}

/// Extracts the value of a single value-taking flag (`--flag value` or `--flag=value`) from
/// `args`, ignoring values of other flags and anything after `--`.
fn extract_flag_value(args: &[String], target_flag: &str) -> Result<Option<String>, CliError> {
    let eq_prefix = format!("{target_flag}=");
    let mut found: Option<String> = None;
    let mut iter = args.iter();
    let mut awaiting_other = false;
    while let Some(arg) = iter.next() {
        if awaiting_other {
            awaiting_other = false;
            continue;
        }
        if arg == "--" {
            break;
        }
        if let Some(value) = arg.strip_prefix(&eq_prefix) {
            if found.replace(value.to_owned()).is_some() {
                return Err(CliError::usage(format!(
                    "{target_flag} is given more than once"
                )));
            }
            continue;
        }
        if arg == target_flag {
            let value = iter
                .next()
                .ok_or_else(|| CliError::usage(format!("{target_flag} needs a value")))?;
            if found.replace(value.clone()).is_some() {
                return Err(CliError::usage(format!(
                    "{target_flag} is given more than once"
                )));
            }
            continue;
        }
        if let Some(flag_name) = arg.strip_prefix("--") {
            if !flag_name.contains('=') && flag_takes_value(args, flag_name) {
                awaiting_other = true;
            }
        }
    }
    Ok(found)
}

fn parse_explicit_format(args: &[String]) -> Result<Option<ExplicitFormat>, CliError> {
    let Some(value) = extract_flag_value(args, "--format")? else {
        return Ok(None);
    };
    match value.as_str() {
        "json" => Ok(Some(ExplicitFormat::Json)),
        "yaml" => Ok(Some(ExplicitFormat::Yaml)),
        "compact" => Ok(Some(ExplicitFormat::Compact)),
        other => Err(CliError::usage(format!(
            "--format `{other}` is not valid; expected json, yaml or compact"
        ))
        .with_help("Pass --format json, --format yaml or --format compact.")),
    }
}

/// Validates `--bind-workspace <dir>` flags, `--format`, `--office-id`, and workspace path
/// BEFORE calling `hr.hire` so invalid combinations never mutate organization state.
pub(super) fn preflight_hr_bind(
    hire_args: &[String],
    workspace: &Path,
    target_office: Option<&str>,
    ctx: &ExecutionContext,
    state_root: Option<&Path>,
) -> Result<PreflightedHrBind, CliError> {
    if has_flag(hire_args, &["--draft"]) {
        return Err(
            CliError::usage("--bind-workspace cannot be combined with --draft")
                .with_help("Omit --draft to hire and bind in one step, or bind after confirming."),
        );
    }
    if has_flag(hire_args, &["--withdraw"]) {
        return Err(CliError::usage(
            "--bind-workspace cannot be combined with --withdraw",
        ));
    }
    if ctx.modes().dry_run || has_flag(hire_args, &["--dry-run"]) {
        return Err(CliError::usage(
            "--bind-workspace cannot be combined with --dry-run",
        ));
    }
    let explicit_format = parse_explicit_format(hire_args)?;
    if let Some(hire_office) = extract_flag_value(hire_args, "--office-id")? {
        if hire_office.is_empty() {
            return Err(CliError::usage("--office-id needs a value"));
        }
        if target_office != Some(hire_office.as_str()) {
            let office_arg = shell_arg(&hire_office);
            let quoted_ws = shell_quote(&workspace.display().to_string());
            return Err(CliError::usage(format!(
                "--bind-workspace cannot hire onto office `{hire_office}` while targeting a different office"
            ))
            .with_help(format!(
                "Run `agentic-org hr hire --office-id {office_arg} ...` first, then bind on that office with: \
                 agentic-org agent bind --member <employee-id> --office {office_arg} --workspace {quoted_ws}"
            )));
        }
    }
    let (canonical_workspace, _) =
        preflight_workspace(workspace, state_root).map_err(refusal_to_cli)?;
    Ok(PreflightedHrBind {
        canonical_workspace,
        explicit_format,
    })
}

/// Options for [`execute_hire_and_bind`].
pub(super) struct HireBindOptions<'a> {
    pub(super) workspace: &'a Path,
    pub(super) target_office: Option<&'a str>,
    pub(super) office_selector: Option<&'a str>,
    pub(super) shown: Shown,
    pub(super) state_root: Option<&'a Path>,
}

/// Runs `hr hire` and, when it succeeds in `hired` state, binds `workspace` to the newly hired
/// agent over the same `transport`.
pub(super) fn execute_hire_and_bind<T: OfficeTransport + ?Sized>(
    hire_args: Vec<String>,
    opts: HireBindOptions<'_>,
    ctx: &mut ExecutionContext,
    transport: &mut T,
) -> Result<(), CliError> {
    let pre = preflight_hr_bind(
        &hire_args,
        opts.workspace,
        opts.target_office,
        ctx,
        opts.state_root,
    )?;
    let effective = resolve_output(pre.explicit_format, opts.shown);
    let json_args = with_json_format(&hire_args);
    let command = org_agent_cli::runtime_table().find("hr", "hire");
    let output = org_agent_cli::execute_with_office(
        "agentic-org",
        &json_args,
        &mut TransportOffice(&mut *transport),
    );
    if output.exit_code != 0 {
        return show_failure(&output, effective, command, ctx);
    }
    let mut envelope: Value = serde_json::from_str(output.stdout.trim_end())
        .map_err(|_| CliError::failed("cli::decode", "hr hire returned an unreadable response"))?;
    if envelope["ok"] != Value::Bool(true) {
        let failed = Output {
            stdout: output.stdout,
            exit_code: if output.exit_code == 0 {
                1
            } else {
                output.exit_code
            },
        };
        return show_failure(&failed, effective, command, ctx);
    }
    let employee_id = envelope["data"]["employee_id"]
        .as_str()
        .filter(|id| !id.is_empty())
        .map(str::to_owned);
    let state = envelope["data"]["state"].as_str();
    let (Some(employee_id), Some("hired")) = (employee_id, state) else {
        return show_success(&envelope, effective, command, None, ctx);
    };
    let bound = bind_workspace(
        transport,
        &WorkspaceBindRequest {
            member: &employee_id,
            workspace_dir: &pre.canonical_workspace,
            label: None,
            ttl_secs: None,
            takeover: false,
            command_id: None,
            state_root: opts.state_root,
        },
    )
    .map_err(|refusal| {
        bind_failure_after_hire(
            &employee_id,
            &pre.canonical_workspace,
            opts.office_selector,
            refusal,
        )
    })?;
    let binding_json =
        serde_json::to_value(&bound).map_err(|e| CliError::failed("cli::encode", e.to_string()))?;
    if let Value::Object(data) = &mut envelope["data"] {
        data.insert("workspace_binding".into(), binding_json);
    }
    show_success(&envelope, effective, command, Some(&bound), ctx)
}

fn show_failure(
    output: &Output,
    effective: EffectiveOutput,
    command: Option<&org_command::Command>,
    ctx: &mut ExecutionContext,
) -> Result<(), CliError> {
    match effective {
        EffectiveOutput::Quiet | EffectiveOutput::Rendered => {
            render::answer(output, command, effective == EffectiveOutput::Quiet, ctx)
        }
        EffectiveOutput::Json | EffectiveOutput::Yaml | EffectiveOutput::Compact => {
            print_answer(ctx, &output.stdout, output.exit_code)
        }
    }
}

fn show_success(
    envelope: &Value,
    effective: EffectiveOutput,
    command: Option<&org_command::Command>,
    bound: Option<&BoundWorkspace>,
    ctx: &mut ExecutionContext,
) -> Result<(), CliError> {
    match effective {
        EffectiveOutput::Quiet | EffectiveOutput::Rendered => {
            let combined = format!("{}\n", escape_json_line(envelope)?);
            render::answer(
                &Output {
                    stdout: combined,
                    exit_code: 0,
                },
                command,
                effective == EffectiveOutput::Quiet,
                ctx,
            )
        }
        EffectiveOutput::Json => {
            let combined = format!("{}\n", escape_json_line(envelope)?);
            print_answer(ctx, &combined, 0)
        }
        EffectiveOutput::Yaml => {
            let yaml = format_envelope_yaml(envelope);
            print_answer(ctx, &yaml, 0)
        }
        EffectiveOutput::Compact => {
            let text = match bound {
                Some(bound) => compact_hire_bind_line(&envelope["data"], bound),
                None => org_command::Node::from_value(&envelope["data"]).to_compact(&[]),
            };
            print_answer(ctx, &text, 0)
        }
    }
}

fn format_envelope_yaml(envelope: &Value) -> String {
    org_command::Node::map([
        ("ok", org_command::Node::from_value(&envelope["ok"])),
        ("data", org_command::Node::from_value(&envelope["data"])),
        ("meta", org_command::Node::from_value(&envelope["meta"])),
        ("error", org_command::Node::from_value(&envelope["error"])),
    ])
    .to_yaml()
}

fn with_json_format(args: &[String]) -> Vec<String> {
    let mut cleaned = Vec::with_capacity(args.len() + 2);
    let mut iter = args.iter();
    let mut awaiting_other = false;
    let mut after_terminator = false;
    while let Some(arg) = iter.next() {
        if awaiting_other || after_terminator {
            awaiting_other = false;
            cleaned.push(arg.clone());
            continue;
        }
        if arg == "--" {
            after_terminator = true;
            cleaned.push(arg.clone());
            continue;
        }
        if arg == "--format" {
            let _ = iter.next();
            continue;
        }
        if arg.starts_with("--format=") {
            continue;
        }
        if let Some(flag_name) = arg.strip_prefix("--") {
            if !flag_name.contains('=') && flag_takes_value(&cleaned, flag_name) {
                awaiting_other = true;
            }
        }
        cleaned.push(arg.clone());
    }
    let at = command_path_len(&cleaned);
    cleaned.splice(at..at, ["--format".to_owned(), "json".to_owned()]);
    cleaned
}

fn compact_hire_bind_line(data: &Value, bound: &BoundWorkspace) -> String {
    let field = |name: &str| match &data[name] {
        Value::String(s) => s.clone(),
        Value::Null => String::new(),
        other => other.to_string(),
    };
    let line = [
        field("request_id"),
        field("state"),
        field("employee_id"),
        field("revision"),
        field("generation"),
        bound.session.session_id.clone(),
        bound.workspace_root.clone(),
    ]
    .into_iter()
    .map(|cell| org_cli_shell::terminal::sanitize(&cell).into_owned())
    .collect::<Vec<_>>()
    .join("\t");
    format!("{line}\n")
}

/// Serializes `value` as a single JSON line with control, line-separator, and bidi characters
/// escaped (`CVE-2021-25743`).
fn escape_json_line(value: &Value) -> Result<String, CliError> {
    let raw =
        serde_json::to_string(value).map_err(|e| CliError::failed("cli::encode", e.to_string()))?;
    let mut out = String::with_capacity(raw.len());
    for ch in raw.chars() {
        let escape = ch == '\u{7f}'
            || ch == '\u{2028}'
            || ch == '\u{2029}'
            || matches!(
                ch,
                '\u{0080}'..='\u{009f}'
                    | '\u{202a}'..='\u{202e}'
                    | '\u{2066}'..='\u{2069}'
            );
        if escape {
            out.push_str(&format!("\\u{:04x}", u32::from(ch)));
        } else {
            out.push(ch);
        }
    }
    Ok(out)
}

fn refusal_to_cli(refusal: Refusal) -> CliError {
    CliError::Office {
        code: format!("office::{}", org_command::code_name(refusal.code)),
        message: refusal.message,
        hint: (!refusal.hint.is_empty()).then_some(refusal.hint),
        exit: 1,
    }
}

fn shell_arg(text: &str) -> String {
    if !text.is_empty()
        && text
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
    {
        text.to_owned()
    } else {
        shell_quote(text)
    }
}

/// POSIX-safe single-quoted argument (`'` escaped as `'\''`).
fn shell_quote(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    out.push('\'');
    for ch in text.chars() {
        if ch == '\'' {
            out.push_str("'\\''");
        } else {
            out.push(ch);
        }
    }
    out.push('\'');
    out
}

fn bind_failure_after_hire(
    employee_id: &str,
    workspace: &Path,
    office: Option<&str>,
    refusal: Refusal,
) -> CliError {
    let office_flag = office
        .map(|id| format!(" --office {}", shell_arg(id)))
        .unwrap_or_default();
    let quoted_ws = shell_quote(&workspace.display().to_string());
    let recovery = format!(
        "Agent {employee_id} was hired, but workspace binding failed; bind it with: \
         agentic-org agent bind --member {employee_id}{office_flag} --workspace {quoted_ws}"
    );
    let hint = if refusal.hint.is_empty() {
        recovery
    } else {
        format!("{} {recovery}", refusal.hint)
    };
    CliError::Office {
        code: format!("office::{}", org_command::code_name(refusal.code)),
        message: refusal.message,
        hint: Some(hint),
        exit: 1,
    }
}

/// Builds a non-secret summary JSON object for testing or callers.
#[cfg(test)]
pub(super) fn test_hire_reply(employee_id: &str) -> Value {
    serde_json::json!({
        "request_id": "0193a5c0-0000-7000-8000-000000000050",
        "state": "hired",
        "employee_id": employee_id,
        "revision": 1,
        "generation": 5
    })
}

#[cfg(test)]
#[path = "hr_bind_tests.rs"]
mod tests;
