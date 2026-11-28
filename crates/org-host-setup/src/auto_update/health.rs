//! Post-restart host health probing (`hello` + `instance.status` + readiness) via a consumer-owned
//! [`HealthProbe`] port (ADR-0009).

use std::path::PathBuf;
use std::time::Duration;

use serde::Serialize;

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
