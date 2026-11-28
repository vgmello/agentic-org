//! `agentic-org doctor`: read-only host, runtime, security, platform, and organization-state
//! integrity diagnostics with actionable remediation hints and non-zero exit on failure.

use std::path::Path;

use org_cli_shell::context::ExecutionContext;
use org_cli_shell::output::{Cell, ListView, Row, Table};
use org_cli_shell::terminal::sanitize;
use serde::Serialize;

use super::doctor_apparmor::check_linux_apparmor_userns;
use super::doctor_host::{
    check_credentials_dir, check_instance_layout, check_permissions, check_registry_consistency,
    check_release_pin, check_sockets,
};
use super::doctor_runtimes::check_cli_runtimes;
pub(crate) use super::doctor_types::{CheckStatus, DoctorCheck};
use super::org_integrity_checks::check_org_state;
use super::paths::{discover_offices, resolve_instance, DiscoveredOffice};
use super::redact::{redact_json, redact_text};
use super::DoctorArgs;
use crate::error::CliError;
use crate::selection;

#[derive(Debug, Clone, Serialize)]
pub(crate) struct DoctorSummary {
    pub(crate) pass: usize,
    pub(crate) warn: usize,
    pub(crate) fail: usize,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct DoctorReport {
    pub(crate) instance: String,
    pub(crate) ok: bool,
    pub(crate) summary: DoctorSummary,
    pub(crate) checks: Vec<DoctorCheck>,
}

pub(crate) fn run(args: DoctorArgs, ctx: &mut ExecutionContext) -> Result<(), CliError> {
    let explicit_instance = args.instance.name.is_some();
    let target = selection::select(args.instance.name.as_deref(), args.office.as_deref())?;
    let run_instance = args.org_dir.is_none() || explicit_instance;

    let report = collect_doctor(
        target.instance(),
        args.office.as_deref(),
        args.org_dir.as_deref(),
        run_instance,
    )?;
    render_doctor(&report, ctx)?;
    if !report.ok {
        return Err(CliError::Exited(1));
    }
    Ok(())
}

/// Runs diagnostic checks and returns a structured report (also consumed by `support`).
pub(crate) fn collect_doctor(
    instance_name: Option<&str>,
    office_filter: Option<&str>,
    org_dir: Option<&Path>,
    include_instance_checks: bool,
) -> Result<DoctorReport, CliError> {
    let mut checks = Vec::new();
    let instance_label = redact_text(instance_name.unwrap_or("default")).text;

    if include_instance_checks {
        let paths = resolve_instance(instance_name)?;
        check_instance_layout(&paths, &mut checks);
        check_permissions(&paths, &mut checks);
        check_registry_consistency(&paths, &mut checks);
        check_sockets(&paths, &mut checks);
        check_release_pin(&mut checks);
        check_credentials_dir(&paths, &mut checks);
        let offices = discover_offices(&paths, office_filter);
        check_discovered_offices(&offices, &mut checks);
        maybe_append_live_metrics_summary(instance_name, office_filter, &mut checks);
    }

    if let Some(dir) = org_dir {
        checks.extend(check_org_state(dir, None, "org.integrity"));
    }

    if let Some(check) = check_linux_apparmor_userns() {
        checks.push(check);
    }
    check_cli_runtimes(&mut checks);

    let summary = DoctorSummary {
        pass: checks
            .iter()
            .filter(|c| c.status == CheckStatus::Pass)
            .count(),
        warn: checks
            .iter()
            .filter(|c| c.status == CheckStatus::Warn)
            .count(),
        fail: checks
            .iter()
            .filter(|c| c.status == CheckStatus::Fail)
            .count(),
    };
    Ok(DoctorReport {
        instance: instance_label,
        ok: summary.fail == 0,
        summary,
        checks,
    })
}

fn check_discovered_offices(offices: &[DiscoveredOffice], checks: &mut Vec<DoctorCheck>) {
    for office in offices {
        let Some(state_dir) = &office.durable_state else {
            continue;
        };
        let scope = format!("org.integrity.{}", office.office_id);
        checks.extend(check_org_state(
            state_dir,
            office.office_db.as_deref(),
            &scope,
        ));
    }
}

/// Appends a concise live efficiency telemetry summary when the host socket is active and an
/// office answers non-interactively; gracefully omitted when offline or auth-gated (`TASK-136`).
fn maybe_append_live_metrics_summary(
    instance_name: Option<&str>,
    office_filter: Option<&str>,
    checks: &mut Vec<DoctorCheck>,
) {
    let host_live = checks
        .iter()
        .any(|c| c.id == "runtime.host_socket" && c.status == CheckStatus::Pass);
    if !host_live {
        return;
    }
    if let Some(metrics) =
        org_host_setup::try_office_metrics_noninteractive(instance_name, office_filter)
    {
        checks.push(DoctorCheck::pass(
            "office.metrics",
            "telemetry",
            crate::view::format_office_summary_line(&metrics),
            None,
        ));
    }
}

fn render_doctor(report: &DoctorReport, ctx: &mut ExecutionContext) -> Result<(), CliError> {
    let raw_json =
        serde_json::to_value(report).map_err(|e| CliError::failed("cli::encode", e.to_string()))?;
    let (json, _) = redact_json(&raw_json);
    if ctx.modes().json {
        return Ok(ctx.write_json(&json)?);
    }
    if ctx.modes().quiet {
        let mut out = String::new();
        for check in &report.checks {
            if check.status == CheckStatus::Fail {
                out.push_str(&sanitize(&redact_text(&check.id).text));
                out.push('\n');
            }
        }
        return if out.is_empty() {
            Ok(())
        } else {
            Ok(ctx.out(&out)?)
        };
    }
    let rows: Vec<Row> = report
        .checks
        .iter()
        .map(|check| {
            let mut detail = check.message.clone();
            if let Some(hint) = &check.remediation {
                detail.push_str(" — fix: ");
                detail.push_str(hint);
            }
            let detail = redact_text(&detail).text;
            let id = redact_text(&check.id).text;
            Row {
                id: id.clone(),
                cells: vec![
                    Cell::status(check.status.as_str(), check.status.style()),
                    Cell::id(&id),
                    Cell::plain(detail),
                ],
            }
        })
        .collect();
    let table = Table {
        headers: vec!["STATUS".into(), "CHECK".into(), "DETAILS".into()],
        rows,
    };
    let view = ListView {
        empty: "No diagnostic checks ran.",
        total: None,
    };
    ctx.render_table(&table, &json, &view)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn live_metrics_summary_is_omitted_when_host_socket_is_offline() {
        let mut checks = vec![DoctorCheck::warn(
            "runtime.host_socket",
            "runtime",
            "host socket is not listening",
            None,
            None,
        )];
        maybe_append_live_metrics_summary(Some("nonexistent-instance-for-test"), None, &mut checks);
        assert_eq!(checks.len(), 1);
        assert!(checks.iter().all(|c| c.id != "office.metrics"));
        assert!(checks.iter().all(|c| c.status != CheckStatus::Fail));
    }

    #[test]
    fn live_metrics_summary_formats_cleanly_for_doctor_row() {
        let metrics = org_proto::telemetry::OfficeMetricsResult {
            office_id: "off-1".into(),
            org_id: "org-1".into(),
            collected_at: "2026-11-27T00:00:00.000Z".into(),
            aggregate: org_proto::telemetry::OfficeAggregateMetrics {
                active_sessions: 1,
                idle_sessions: 2,
                recent_turn_count: 4,
                total_turn_count: 10,
                p50_wall_latency_ms: Some(1200),
                p95_wall_latency_ms: Some(3400),
                avg_wall_latency_ms: Some(1500),
                input_tokens: 2000,
                output_tokens: 500,
                cached_read_tokens: 6000,
                thought_tokens: 300,
                cache_hit_ratio: Some(0.75),
                current_rss_bytes: 128 * 1024 * 1024,
                peak_rss_bytes: 256 * 1024 * 1024,
                rss_budget_bytes: Some(1024 * 1024 * 1024),
                hibernated_total: 0,
                rss_evicted_total: 0,
                sandbox_denial_count: 1,
            },
            agents: Vec::new(),
        };
        let line = crate::view::format_office_summary_line(&metrics);
        assert!(line.contains("1 active"));
        assert!(line.contains("2 idle"));
        assert!(line.contains("75.0%"));
        assert!(line.contains("128.0 MiB"), "{line}");
        assert!(line.contains("1 sandbox denials"), "{line}");
    }
}
