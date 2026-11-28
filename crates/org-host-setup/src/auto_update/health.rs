//! Post-restart host health probing (`hello` + `instance.status` + readiness) via a consumer-owned
//! [`HealthProbe`] port (ADR-0009), plus strict fail-closed evaluation of service refresh reports.

use std::path::PathBuf;
use std::time::Duration;

use serde::Serialize;

use crate::service_refresh::{ManagedServiceInfo, RefreshOutcome, ServiceRefreshReport};

/// One running instance to probe after a release restart or rollback.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HealthTarget {
    pub instance: String,
    pub instance_id: String,
    pub root: PathBuf,
}

/// Structured result of probing one restarted instance.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct InstanceHealthReport {
    pub instance: String,
    pub healthy: bool,
    pub detail: String,
}

/// Consumer-owned port verifying that a restarted host instance is healthy and serving.
pub trait HealthProbe: Send + Sync {
    /// Probes `target` within `timeout`. Returns `Ok(summary)` when healthy (`hello` +
    /// `instance.status` + gateway/readiness check succeed), or `Err(reason)` when unhealthy.
    fn check_healthy(&self, target: &HealthTarget, timeout: Duration) -> Result<String, String>;
}

/// Production [`HealthProbe`] querying `host.sock` (`hello` + `instance.status` + `WebState::Running`)
/// through `crate::wire::ready`.
#[derive(Debug, Default, Clone, Copy)]
pub struct HostSocketHealthProbe;

impl HealthProbe for HostSocketHealthProbe {
    fn check_healthy(&self, target: &HealthTarget, timeout: Duration) -> Result<String, String> {
        let name = (target.instance != "default").then_some(target.instance.as_str());
        let paths = org_host::human::paths(name)?;
        let (_record, id, socket) = crate::wire::registered(&paths, name)?;
        if id != target.instance_id {
            return Err(format!(
                "instance \"{}\" locator ID mismatch during post-restart health check",
                target.instance
            ));
        }
        let status = crate::wire::ready(&socket, &id, timeout, || None)?;
        Ok(format!(
            "hello + instance.status healthy (offices: total={}, active={}, failed={})",
            status.offices.total, status.offices.active, status.offices.failed
        ))
    }
}

/// Probes all restarted instances, returning each report and an overall health flag.
pub fn probe_instances(
    probe: &dyn HealthProbe,
    targets: &[HealthTarget],
    timeout: Duration,
) -> (bool, Vec<InstanceHealthReport>) {
    let mut reports = Vec::with_capacity(targets.len());
    let mut all_healthy = true;
    for target in targets {
        match probe.check_healthy(target, timeout) {
            Ok(detail) => reports.push(InstanceHealthReport {
                instance: target.instance.clone(),
                healthy: true,
                detail,
            }),
            Err(err) => {
                all_healthy = false;
                reports.push(InstanceHealthReport {
                    instance: target.instance.clone(),
                    healthy: false,
                    detail: err,
                });
            }
        }
    }
    (all_healthy, reports)
}

/// Evaluates post-restart service reports and probes every restarted managed instance fail-closed.
///
/// Every instance in `managed_before` MUST appear in `refresh_reports` with outcome `Refreshed` or
/// `Unchanged`, and when `restart` is enabled every instance with `was_running == true` MUST
/// report `running == true` and pass [`HealthProbe::check_healthy`]. Any missing service, failed
/// redefine/start, or stopped previously-running service fails closed immediately.
pub(super) fn evaluate_post_restart_health(
    health: &dyn HealthProbe,
    managed_before: &[ManagedServiceInfo],
    refresh_reports: &[ServiceRefreshReport],
    restart: bool,
    health_timeout: Duration,
) -> (bool, Vec<InstanceHealthReport>) {
    let failures = collect_refresh_failures(managed_before, refresh_reports, restart);
    if !failures.is_empty() {
        return (false, failures);
    }
    let targets = health_targets_for(managed_before, refresh_reports, restart);
    let expected_running = restart && managed_before.iter().any(|m| m.was_running);
    if targets.is_empty() {
        if expected_running {
            return (
                false,
                vec![InstanceHealthReport {
                    instance: "service-restart".into(),
                    healthy: false,
                    detail: "no running managed instances remained to verify after restart".into(),
                }],
            );
        }
        return (true, Vec::new());
    }
    probe_instances(health, &targets, health_timeout)
}

fn collect_refresh_failures(
    managed_before: &[ManagedServiceInfo],
    refresh_reports: &[ServiceRefreshReport],
    restart: bool,
) -> Vec<InstanceHealthReport> {
    let mut failures = Vec::new();
    for managed in managed_before {
        let Some(report) = refresh_reports
            .iter()
            .find(|r| r.instance == managed.instance)
        else {
            failures.push(InstanceHealthReport {
                instance: managed.instance.clone(),
                healthy: false,
                detail: "managed service vanished before refresh or rollback completed".into(),
            });
            continue;
        };
        if !matches!(
            report.outcome,
            RefreshOutcome::Refreshed | RefreshOutcome::Unchanged
        ) {
            failures.push(InstanceHealthReport {
                instance: managed.instance.clone(),
                healthy: false,
                detail: format!(
                    "service redefine/refresh did not succeed ({}): {}",
                    report.outcome.wire(),
                    report.note
                ),
            });
            continue;
        }
        if restart && managed.was_running && !report.running {
            failures.push(InstanceHealthReport {
                instance: managed.instance.clone(),
                healthy: false,
                detail: format!(
                    "previously running service failed to restart: {}",
                    report.note
                ),
            });
        }
    }
    failures
}

pub(super) fn health_targets_for(
    managed_before: &[ManagedServiceInfo],
    reports: &[ServiceRefreshReport],
    restart: bool,
) -> Vec<HealthTarget> {
    if !restart {
        return Vec::new();
    }
    managed_before
        .iter()
        .filter(|m| {
            m.was_running
                && reports.iter().any(|r| {
                    r.instance == m.instance
                        && r.running
                        && matches!(
                            r.outcome,
                            RefreshOutcome::Refreshed | RefreshOutcome::Unchanged
                        )
                })
        })
        .map(|m| HealthTarget {
            instance: m.instance.clone(),
            instance_id: m.instance_id.clone(),
            root: m.root.clone(),
        })
        .collect()
}

pub(super) fn summarize_health_failures(reports: &[InstanceHealthReport]) -> String {
    let failed: Vec<String> = reports
        .iter()
        .filter(|r| !r.healthy)
        .map(|r| format!("{}: {}", r.instance, r.detail))
        .collect();
    if failed.is_empty() {
        "post-restart health check failed".into()
    } else {
        failed.join("; ")
    }
}
