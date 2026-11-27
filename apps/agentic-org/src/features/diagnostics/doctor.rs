//! `agentic-org doctor`: read-only host, runtime, security, platform, and organization-state
//! integrity diagnostics with actionable remediation hints and non-zero exit on failure.

use std::fs;
use std::path::Path;

use org_cli_shell::context::ExecutionContext;
use org_cli_shell::output::{Cell, ListView, Row, Table};
use org_cli_shell::terminal::sanitize;
use serde::Serialize;

use super::doctor_host::{
    check_credentials_dir, check_instance_layout, check_permissions, check_registry_consistency,
    check_release_pin, check_sockets,
};
pub(crate) use super::doctor_types::{CheckStatus, DoctorCheck};
use super::org_integrity_checks::check_org_state;
use super::paths::{discover_offices, resolve_instance, DiscoveredOffice};
use super::DoctorArgs;
use crate::error::CliError;
use crate::selection;

const APPARMOR_USERNS_SYSCTL: &str = "/proc/sys/kernel/apparmor_restrict_unprivileged_userns";

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
    let instance_label = instance_name.unwrap_or("default").to_owned();

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
    }

    if let Some(dir) = org_dir {
        checks.extend(check_org_state(dir, None, "org.integrity"));
    }

    if let Some(check) = check_linux_apparmor_userns() {
        checks.push(check);
    }

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

/// Safe read-only Linux AppArmor/userns diagnostic with zero platform `#[cfg]`.
fn check_linux_apparmor_userns() -> Option<DoctorCheck> {
    match fs::read_to_string(APPARMOR_USERNS_SYSCTL) {
        Ok(raw) => {
            let value = raw.trim();
            if value == "1" {
                Some(DoctorCheck::warn(
                    "platform.apparmor_userns",
                    "platform",
                    "AppArmor restricts unprivileged user namespaces (apparmor_restrict_unprivileged_userns=1)",
                    Some("Install an AppArmor profile permitting user namespaces for the agent sandbox, or relax kernel.apparmor_restrict_unprivileged_userns.".to_owned()),
                    Some(APPARMOR_USERNS_SYSCTL.to_owned()),
                ))
            } else {
                Some(DoctorCheck::pass(
                    "platform.apparmor_userns",
                    "platform",
                    format!(
                        "unprivileged user namespaces permitted (apparmor_restrict_unprivileged_userns={value})"
                    ),
                    Some(APPARMOR_USERNS_SYSCTL.to_owned()),
                ))
            }
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => Some(DoctorCheck::warn(
            "platform.apparmor_userns",
            "platform",
            format!("could not read AppArmor userns sysctl: {e}"),
            None,
            Some(APPARMOR_USERNS_SYSCTL.to_owned()),
        )),
    }
}

fn render_doctor(report: &DoctorReport, ctx: &mut ExecutionContext) -> Result<(), CliError> {
    if ctx.modes().json {
        return Ok(ctx.write_json(report)?);
    }
    if ctx.modes().quiet {
        let mut out = String::new();
        for check in &report.checks {
            if check.status == CheckStatus::Fail {
                out.push_str(&sanitize(&check.id));
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
            Row {
                id: check.id.clone(),
                cells: vec![
                    Cell::status(check.status.as_str(), check.status.style()),
                    Cell::id(&check.id),
                    Cell::plain(detail),
                ],
            }
        })
        .collect();
    let table = Table {
        headers: vec!["STATUS".into(), "CHECK".into(), "DETAILS".into()],
        rows,
    };
    let json =
        serde_json::to_value(report).map_err(|e| CliError::failed("cli::encode", e.to_string()))?;
    let view = ListView {
        empty: "No diagnostic checks ran.",
        total: None,
    };
    ctx.render_table(&table, &json, &view)?;
    Ok(())
}
