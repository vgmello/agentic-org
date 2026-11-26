//! Joining a satellite office: its first join step, the result it answers with and the
//! watcher that adopts or stops the joining worker.

use std::path::PathBuf;
use std::sync::Arc;
use std::thread::{self};
use std::time::{Duration, Instant};

use org_proto::{ApiError, ErrorCode};

use crate::host_log;
use crate::supervisor::JoinStep;

use super::Offices;

/// Launch specification for [`Offices::join_satellite`] (`TASK-52` H-1).
pub(crate) struct SatelliteJoinSpec {
    pub instance_id: String,
    pub operation_id: String,
    pub invitation_id: String,
    pub org_id: String,
    pub org_name: String,
    pub office_id: String,
    pub office_name: String,
    pub hq_office_id: String,
    pub hq_addresses: Vec<String>,
    pub invitation: org_host_control::InvitationTokenSecret,
    pub re_enroll: bool,
    pub actor_account: String,
    pub pending_file: PathBuf,
}

impl Offices {
    /// Launches `agentic-org-office --join` through the supervisor and verified rendezvous for
    /// satellite enrollment or D25 re-enrollment (`TASK-52` H-1). Waits for `JoinPending { check_code }`
    /// or `Joined { office }`; when pending, adopts the office into supervision + catalog in the
    /// background once HQ approval and initial snapshot finish.
    pub(crate) fn join_satellite(
        self: &Arc<Self>,
        spec: SatelliteJoinSpec,
    ) -> Result<org_host_control::ops::OfficeJoinResult, ApiError> {
        join_satellite_impl(self, spec)
    }
}

struct JoinMeta {
    office_id: String,
    org_id: String,
    org_name: String,
    office_name: String,
    hq_office_id: String,
    pending_file: PathBuf,
}

fn join_satellite_impl(
    offices: &Arc<Offices>,
    spec: SatelliteJoinSpec,
) -> Result<org_host_control::ops::OfficeJoinResult, ApiError> {
    let deadline = Instant::now() + offices.create_wait;
    offices.wait_for_startup(deadline)?;
    if spec.re_enroll {
        offices
            .supervisor
            .stop_office_for_re_enroll(&spec.office_id);
    }
    let meta = JoinMeta {
        office_id: spec.office_id.clone(),
        org_id: spec.org_id.clone(),
        org_name: spec.org_name.clone(),
        office_name: spec.office_name.clone(),
        hq_office_id: spec.hq_office_id.clone(),
        pending_file: spec.pending_file,
    };
    let first = org_office_launch::JoinLine {
        office: org_office_launch::OfficeLine {
            instance_id: spec.instance_id,
            office_id: meta.office_id.clone(),
            rendezvous: String::new(),
        },
        invitation_id: spec.invitation_id,
        org_id: spec.org_id,
        org_name: spec.org_name,
        office_name: spec.office_name,
        hq_office_id: spec.hq_office_id,
        hq_addresses: spec.hq_addresses,
        re_enroll: spec.re_enroll,
    };
    let mut run = offices
        .supervisor
        .begin_satellite_join(
            first,
            &spec.operation_id,
            spec.invitation,
            &spec.actor_account,
        )
        .map_err(|err| {
            host_log::log(&format!("satellite join launch refused: {err}"));
            ApiError::new(
                ErrorCode::Unavailable,
                "cannot launch satellite enrollment worker",
            )
        })?;
    let step = offices
        .supervisor
        .wait_join_step(&mut run, offices.create_timeout);
    handle_initial_join_step(offices, meta, run, step)
}

/// Returns whether `office_id` currently has an active, uncancelled `--join` worker running.
pub(crate) fn has_active_pending_join(offices: &Offices, office_id: &str) -> bool {
    crate::supervisor::is_pending_join_active(&offices.supervisor, office_id)
}

/// Cancels, stops, and reaps any active `--join` worker for `office_id` so pending removal can
/// never race with background adoption (`TASK-52` #3 / P4-#11). Returns `false` if reaping timed out.
pub(crate) fn stop_pending_join_for_office(offices: &Offices, office_id: &str) -> bool {
    crate::supervisor::stop_pending_join(&offices.supervisor, office_id)
}

fn handle_initial_join_step(
    offices: &Arc<Offices>,
    meta: JoinMeta,
    run: crate::supervisor::OwnedJoinRun,
    step: JoinStep,
) -> Result<org_host_control::ops::OfficeJoinResult, ApiError> {
    match step {
        JoinStep::Joined {
            office,
            backlog,
            ready,
        } => {
            if office.office_id != meta.office_id || office.org_id != meta.org_id {
                offices.supervisor.finish_join_run(run);
                return Err(ApiError::new(
                    ErrorCode::StorageError,
                    "satellite worker returned mismatched office metadata",
                ));
            }
            if !crate::supervisor::claim_join_adoption(
                &offices.supervisor,
                &meta.office_id,
                run.run_key,
            ) {
                offices.supervisor.finish_join_run(run);
                return Err(ApiError::new(
                    ErrorCode::Conflict,
                    "satellite join was cancelled before completion",
                ));
            }
            let _ = std::fs::remove_file(&meta.pending_file);
            offices
                .supervisor
                .adopt_owned_join(run, office.clone(), backlog, ready);
            offices.add_created(&office, |_| {});
            Ok(build_join_result(
                &meta,
                org_host_control::ops::OfficeJoinState::Joined,
                None,
            ))
        }
        JoinStep::Pending { check_code } => {
            spawn_satellite_join_watcher(
                Arc::clone(offices),
                run,
                meta.office_id.clone(),
                meta.org_id.clone(),
                meta.pending_file.clone(),
            );
            Ok(build_join_result(
                &meta,
                org_host_control::ops::OfficeJoinState::JoinPending,
                Some(check_code),
            ))
        }
        JoinStep::Failed(msg) => {
            offices.supervisor.finish_join_run(run);
            Err(ApiError::new(ErrorCode::InvalidRequest, msg))
        }
        JoinStep::Crashed | JoinStep::TimedOut | JoinStep::Stopped => {
            offices.supervisor.finish_join_run(run);
            Err(ApiError::new(
                ErrorCode::Unavailable,
                "satellite enrollment worker did not reach check-code rendezvous",
            ))
        }
    }
}

fn build_join_result(
    meta: &JoinMeta,
    state: org_host_control::ops::OfficeJoinState,
    check_code: Option<String>,
) -> org_host_control::ops::OfficeJoinResult {
    org_host_control::ops::OfficeJoinResult {
        office_id: meta.office_id.clone(),
        org_id: meta.org_id.clone(),
        org_name: meta.org_name.clone(),
        office_name: meta.office_name.clone(),
        hq_office_id: meta.hq_office_id.clone(),
        state,
        check_code,
    }
}

fn spawn_satellite_join_watcher(
    offices: Arc<Offices>,
    mut run: crate::supervisor::OwnedJoinRun,
    expected_office: String,
    expected_org: String,
    pending_file: PathBuf,
) {
    let _ = thread::Builder::new()
        .name("satellite-join-watch".into())
        .spawn(move || {
            const ENROLL_BG_TIMEOUT: Duration = Duration::from_secs(180);
            match offices
                .supervisor
                .wait_join_step(&mut run, ENROLL_BG_TIMEOUT)
            {
                JoinStep::Joined {
                    office,
                    backlog,
                    ready,
                } if office.office_id == expected_office
                    && office.org_id == expected_org
                    && crate::supervisor::claim_join_adoption(
                        &offices.supervisor,
                        &expected_office,
                        run.run_key,
                    ) =>
                {
                    let _ = std::fs::remove_file(&pending_file);
                    offices
                        .supervisor
                        .adopt_owned_join(run, office.clone(), backlog, ready);
                    offices.add_created(&office, |_| {});
                }
                _ => {
                    offices.supervisor.finish_join_run(run);
                }
            }
        });
}
