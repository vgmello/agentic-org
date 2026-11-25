//! Periodic supervisor readiness probe for locally bound provider connections (`TASK-120`, `doc-88` §4).
//!
//! `router.rs` remains strictly fail-closed on stale readiness observations (`MAX_OBSERVATION_TTL_SECS = 300s`).
//! This office-local background task periodically re-verifies locally bound `Ready` runtimes (via
//! `<binary> --version`) and renews their readiness and discovered-model observations before the
//! 300s TTL expires, without ever promoting `AuthExpired`, `Unavailable`, or unbound connections.

use std::io::Read as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use org_office::providers::MAX_OBSERVATION_TTL_SECS;
use org_office::{Office, OfficeRole};
use org_office_sessions::RuntimeRegistry;
use org_proto::providers::{
    BoundConnectionSummary, ConnectionReadinessStatus, DiscoveredModelInput,
    ProviderBindingListParams,
};

/// Renew ready observations when less than this many seconds remain before `valid_until`.
const RENEW_BEFORE_EXPIRY_MS: u64 = 150_000;
/// Polling cadence between readiness checks.
const REFRESH_TICK: Duration = Duration::from_secs(60);
/// Stop-check granularity while sleeping between ticks.
const SLEEP_STEP: Duration = Duration::from_millis(500);
/// Hard cap on each `<runtime> --version` probe so a stalled binary never blocks shutdown.
const PROBE_TIMEOUT: Duration = Duration::from_secs(5);
const PROBE_POLL_STEP: Duration = Duration::from_millis(50);
const MAX_VERSION_OUTPUT_BYTES: u64 = 4096;

/// Spawns the background readiness refresher thread for `office`.
pub(crate) fn spawn_readiness_refresh_loop(
    office: Arc<Office>,
    stop: Arc<AtomicBool>,
) -> Option<JoinHandle<()>> {
    thread::Builder::new()
        .name("provider-readiness".into())
        .spawn(move || {
            let runtimes = RuntimeRegistry::production();
            while !stop.load(Ordering::SeqCst) {
                refresh_ready_bindings_once(&office, &runtimes, &stop);
                sleep_interruptible(REFRESH_TICK, &stop);
            }
        })
        .ok()
}

fn sleep_interruptible(total: Duration, stop: &AtomicBool) {
    let mut waited = Duration::ZERO;
    while waited < total && !stop.load(Ordering::SeqCst) {
        let step = (total - waited).min(SLEEP_STEP);
        thread::sleep(step);
        waited += step;
    }
}

fn office_operator_actor(office: &Office) -> &str {
    if office.info().role == OfficeRole::Satellite {
        office.info().office_id.as_str()
    } else {
        office.owner_member_id()
    }
}

fn needs_renewal(entry: &BoundConnectionSummary, now_ms: u64) -> bool {
    let Some(ref state) = entry.state else {
        return false;
    };
    if state.status != ConnectionReadinessStatus::Ready || entry.models.is_empty() {
        return false;
    }
    if state.stale || entry.models.iter().any(|m| m.stale) {
        return true;
    }
    let Some(ref valid_until) = state.valid_until else {
        return true;
    };
    let Some(exp_ms) = org_proto::parse_rfc3339_ms(valid_until) else {
        return true;
    };
    exp_ms.saturating_sub(now_ms) <= RENEW_BEFORE_EXPIRY_MS
}

fn parse_version_output(stdout: &[u8], stderr: &[u8]) -> Option<String> {
    let text = if !stdout.is_empty() {
        String::from_utf8_lossy(stdout)
    } else {
        String::from_utf8_lossy(stderr)
    };
    let token = text
        .lines()
        .find_map(|line| line.split_whitespace().next())
        .map(str::trim)
        .filter(|s| !s.is_empty())?;
    let bounded: String = token.chars().take(64).collect();
    (!bounded.is_empty()).then_some(bounded)
}

fn probe_runtime_version(
    binary_hint: Option<&str>,
    driver: &str,
    runtimes: &RuntimeRegistry,
    stop: &AtomicBool,
) -> Option<String> {
    if stop.load(Ordering::SeqCst) {
        return None;
    }
    let program: PathBuf = match binary_hint.map(str::trim).filter(|s| !s.is_empty()) {
        Some(path) if Path::new(path).is_absolute() && Path::new(path).is_file() => {
            PathBuf::from(path)
        }
        Some(_) => return None,
        None => {
            let adapter = runtimes.resolve(driver).ok()?;
            runtimes.resolve_binary(adapter.as_ref(), None).ok()?
        }
    };
    let mut child = Command::new(&program)
        .arg("--version")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .ok()?;
    let deadline = Instant::now() + PROBE_TIMEOUT;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                if !status.success() {
                    return None;
                }
                let mut stdout_buf = Vec::new();
                if let Some(out) = child.stdout.take() {
                    let _ = out
                        .take(MAX_VERSION_OUTPUT_BYTES)
                        .read_to_end(&mut stdout_buf);
                }
                let mut stderr_buf = Vec::new();
                if let Some(err) = child.stderr.take() {
                    let _ = err
                        .take(MAX_VERSION_OUTPUT_BYTES)
                        .read_to_end(&mut stderr_buf);
                }
                return Some(
                    parse_version_output(&stdout_buf, &stderr_buf)
                        .unwrap_or_else(|| "unknown".to_owned()),
                );
            }
            Ok(None) => {
                if stop.load(Ordering::SeqCst) || Instant::now() >= deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    return None;
                }
                thread::sleep(PROBE_POLL_STEP);
            }
            Err(_) => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
        }
    }
}

/// Refreshes all locally bound `Ready` provider connections whose observation is stale or nearing
/// expiry by re-verifying the runtime binary on this host.
pub(crate) fn refresh_ready_bindings_once(
    office: &Office,
    runtimes: &RuntimeRegistry,
    stop: &AtomicBool,
) {
    let actor = office_operator_actor(office);
    let Ok(Ok(listed)) = office.provider_binding_list(
        actor,
        ProviderBindingListParams {
            connection_id: None,
        },
    ) else {
        return;
    };
    let now_ms = office.current_time_ms();
    for entry in listed.bindings {
        if stop.load(Ordering::SeqCst) {
            return;
        }
        if !needs_renewal(&entry, now_ms) {
            continue;
        }
        let Ok(Some(conn)) = office.load_provider_connection(&entry.binding.connection_id) else {
            continue;
        };
        if conn.archived || !conn.enabled {
            continue;
        }
        let prev_version = entry
            .state
            .as_ref()
            .and_then(|s| s.driver_version.clone())
            .unwrap_or_else(|| "unknown".to_owned());
        let probed = probe_runtime_version(
            entry.binding.runtime_binary_path.as_deref(),
            &conn.runtime_driver,
            runtimes,
            stop,
        );
        let Some(driver_version) = probed else {
            continue;
        };
        let effective_version = if driver_version == "unknown" {
            prev_version
        } else {
            driver_version
        };
        let models: Vec<DiscoveredModelInput> = entry
            .models
            .into_iter()
            .map(|m| DiscoveredModelInput {
                model_id: m.model_id,
                driver_model_id: m.driver_model_id,
                entitlement: m.entitlement,
                capabilities_json: m.capabilities_json,
            })
            .collect();
        let _ = office.renew_ready_provider_observation_if_unchanged(
            &entry.binding.connection_id,
            entry.binding.revision,
            Some(effective_version),
            models,
            MAX_OBSERVATION_TTL_SECS,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use org_proto::providers::{
        DiscoveredModelEntitlement, OfficeConnectionBindingView, OfficeConnectionModelView,
        OfficeConnectionStateView,
    };

    fn sample_summary(
        status: ConnectionReadinessStatus,
        valid_until: Option<&str>,
        stale: bool,
    ) -> BoundConnectionSummary {
        BoundConnectionSummary {
            binding: OfficeConnectionBindingView {
                office_id: "off-1".into(),
                connection_id: "conn-1".into(),
                revision: 1,
                auth_context_ref: "local-host-oauth".into(),
                runtime_binary_path: None,
                account_identity_ref: Some("sub:test".into()),
                updated_at: "2026-11-25T12:00:00.000Z".into(),
            },
            state: Some(OfficeConnectionStateView {
                office_id: "off-1".into(),
                connection_id: "conn-1".into(),
                status,
                binding_revision: 1,
                driver_version: Some("2.1.332".into()),
                observed_at: Some("2026-11-25T12:00:00.000Z".into()),
                valid_until: valid_until.map(str::to_owned),
                issue_code: None,
                stale,
            }),
            models: vec![OfficeConnectionModelView {
                office_id: "off-1".into(),
                connection_id: "conn-1".into(),
                model_id: "sonnet".into(),
                driver_model_id: "sonnet".into(),
                entitlement: DiscoveredModelEntitlement::Confirmed,
                capabilities_json: "{}".into(),
                observed_at: "2026-11-25T12:00:00.000Z".into(),
                valid_until: valid_until.unwrap_or("2026-11-25T12:05:00.000Z").into(),
                stale,
            }],
        }
    }

    #[test]
    fn needs_renewal_respects_status_and_expiry_window() {
        let now_ms = org_proto::parse_rfc3339_ms("2026-11-25T12:03:00.000Z").unwrap();
        // 120s remaining <= 150s renew window -> renew
        let soon = sample_summary(
            ConnectionReadinessStatus::Ready,
            Some("2026-11-25T12:05:00.000Z"),
            false,
        );
        assert!(needs_renewal(&soon, now_ms));

        // 240s remaining > 150s -> skip
        let fresh = sample_summary(
            ConnectionReadinessStatus::Ready,
            Some("2026-11-25T12:07:00.000Z"),
            false,
        );
        assert!(!needs_renewal(&fresh, now_ms));

        // Already stale -> renew
        let stale = sample_summary(
            ConnectionReadinessStatus::Ready,
            Some("2026-11-25T12:01:00.000Z"),
            true,
        );
        assert!(needs_renewal(&stale, now_ms));

        // AuthExpired / Unavailable must NEVER be auto-renewed as Ready
        let expired = sample_summary(ConnectionReadinessStatus::AuthExpired, None, true);
        assert!(!needs_renewal(&expired, now_ms));
        let unavailable = sample_summary(ConnectionReadinessStatus::Unavailable, None, true);
        assert!(!needs_renewal(&unavailable, now_ms));
    }

    #[test]
    fn parse_version_output_extracts_first_token() {
        assert_eq!(
            parse_version_output(b"2.1.332 (Claude Code)\n", b"").as_deref(),
            Some("2.1.332")
        );
        assert_eq!(
            parse_version_output(b"", b"codex-cli 0.118.0\n").as_deref(),
            Some("codex-cli")
        );
        assert_eq!(parse_version_output(b"   \n", b""), None);
    }

    #[test]
    fn probe_runtime_version_aborts_immediately_when_stop_is_set() {
        let stop = AtomicBool::new(true);
        let runtimes = RuntimeRegistry::production();
        assert!(probe_runtime_version(Some("/bin/sh"), "claude", &runtimes, &stop).is_none());

        let running = AtomicBool::new(false);
        assert!(
            probe_runtime_version(Some("./relative-bin"), "claude", &runtimes, &running).is_none()
        );
    }
}
