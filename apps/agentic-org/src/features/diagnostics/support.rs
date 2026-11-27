//! `agentic-org support`: generates a deterministic, 100% secret-redacted JSON diagnostic bundle.
//!
//! Never reads or copies private keys, `host-registry.db`, `protected/`, `private/`, tokens,
//! cookies, or credential blobs. Writes atomically with mode `0600` when `--output <path>` is
//! supplied (refusing symlinks and non-regular destinations), or prints to stdout.

use org_cli_shell::context::ExecutionContext;
use org_cli_shell::output::Cell;
use serde::Serialize;
use serde_json::{json, Value};

use super::doctor::{collect_doctor, DoctorReport};
use super::logs::{collect_logs, validate_lines, LogReport};
use super::paths::{discover_offices, resolve_instance, write_atomic_owner_only, DiscoveredOffice};
use super::redact::redact_json;
use super::SupportArgs;
use crate::error::CliError;
use crate::selection;

const BUNDLE_FORMAT: &str = "agentic-org-support-v1";

#[derive(Serialize)]
struct SupportVersion {
    binary: &'static str,
    version: &'static str,
}

#[derive(Serialize)]
struct InstanceSummary {
    name: String,
    instance_id: Option<String>,
    service_mode: Option<String>,
    offices: Vec<DiscoveredOffice>,
}

#[derive(Serialize)]
struct SupportBundle {
    format: &'static str,
    version: SupportVersion,
    instance: InstanceSummary,
    doctor: DoctorReport,
    logs: LogReport,
    redacted_count: usize,
}

#[derive(Serialize)]
struct BundleWriteReceipt {
    output: String,
    bytes: usize,
    redacted_count: usize,
    doctor_ok: bool,
    log_lines: usize,
}

pub(crate) fn run(args: SupportArgs, ctx: &mut ExecutionContext) -> Result<(), CliError> {
    validate_lines(args.lines)?;
    let target = selection::select(args.instance.name.as_deref(), None)?;
    let (bundle, meta) = build_redacted_bundle(target.instance(), args.lines)?;

    if let Some(output_path) = args.output.as_deref() {
        let mut encoded = serde_json::to_vec_pretty(&bundle)
            .map_err(|e| CliError::failed("cli::encode", e.to_string()))?;
        encoded.push(b'\n');
        let written = write_atomic_owner_only(output_path, &encoded)
            .map_err(|e| CliError::failed("support::write", e))?;
        let receipt = BundleWriteReceipt {
            output: written.display().to_string(),
            bytes: encoded.len(),
            redacted_count: meta.redacted_count,
            doctor_ok: meta.doctor_ok,
            log_lines: meta.log_lines,
        };
        let fields = [
            ("output", Cell::plain(&receipt.output)),
            ("bytes", Cell::plain(receipt.bytes.to_string())),
            (
                "redacted_count",
                Cell::plain(receipt.redacted_count.to_string()),
            ),
            ("doctor_ok", Cell::plain(receipt.doctor_ok.to_string())),
            ("log_lines", Cell::plain(receipt.log_lines.to_string())),
        ];
        let json_val = serde_json::to_value(&receipt)
            .map_err(|e| CliError::failed("cli::encode", e.to_string()))?;
        ctx.render_detail(&receipt.output, &fields, &json_val)?;
        ctx.success(&format!(
            "Wrote redacted support bundle to {} (mode 0600, {} redaction(s)).",
            receipt.output, receipt.redacted_count
        ));
        return Ok(());
    }

    if ctx.modes().quiet {
        return Ok(ctx.out(&format!("{}\n", meta.instance_name))?);
    }
    Ok(ctx.write_json(&bundle)?)
}

struct BundleMeta {
    instance_name: String,
    redacted_count: usize,
    doctor_ok: bool,
    log_lines: usize,
}

fn build_redacted_bundle(
    instance_name: Option<&str>,
    max_lines: usize,
) -> Result<(Value, BundleMeta), CliError> {
    let paths = resolve_instance(instance_name)?;
    let offices = discover_offices(&paths, None);
    let doctor = collect_doctor(instance_name, None, None, true)?;
    let logs = collect_logs(&paths, None, max_lines)?;
    let log_redactions = logs.redacted_count;
    let log_lines = logs.lines.len();
    let doctor_ok = doctor.ok;

    let raw_bundle = SupportBundle {
        format: BUNDLE_FORMAT,
        version: SupportVersion {
            binary: "agentic-org",
            version: env!("CARGO_PKG_VERSION"),
        },
        instance: InstanceSummary {
            name: paths.name.clone(),
            instance_id: paths.instance_id.clone(),
            service_mode: paths.service_mode.clone(),
            offices,
        },
        doctor,
        logs,
        redacted_count: log_redactions,
    };

    let raw_val = serde_json::to_value(&raw_bundle)
        .map_err(|e| CliError::failed("cli::encode", e.to_string()))?;
    // Defense-in-depth full-tree redaction pass over every key and string value.
    let (mut redacted_val, extra_redactions) = redact_json(&raw_val);
    let total_redactions = log_redactions + extra_redactions;
    if let Value::Object(map) = &mut redacted_val {
        map.insert("redacted_count".to_owned(), json!(total_redactions));
    }

    Ok((
        redacted_val,
        BundleMeta {
            instance_name: paths.name,
            redacted_count: total_redactions,
            doctor_ok,
            log_lines,
        },
    ))
}
