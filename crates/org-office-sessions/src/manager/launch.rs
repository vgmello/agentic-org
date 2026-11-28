//! Session launch: admission, route planning, warm reuse checks, and spawn + persist.

use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};

use org_office::organization_family::records::Employee;
use org_proto::sessions::{MemberSessionStartParams, SessionView};
use org_proto::{ApiError, ErrorCode};
use org_supervise::SessionMark;

use crate::credentials::rfc3339_now;
use crate::partition::{PartitionKey, PartitionTurn};
use crate::pump::{PumpCommand, SessionRuntimeState};
use crate::recovery::{ActiveTurnRecord, PendingStartRecord};

use super::affinity::{
    affinity_entry, control_stop_reason, ensure_warm_sandbox_not_stale, is_warm_state_reusable,
    prepend_fallback_notice, WarmReuseControl,
};
use super::chief_of_staff::partition_switch_reason;
use super::environment::{write_owner_only_binding_file, SessionEnvInput};
use super::registry_guards::{
    check_registry_startable, ensure_live_handle_not_recovery_required, ensure_not_stopped_since,
};
use super::sandbox_runner::{SandboxPlan, SandboxPrepareContext, SandboxVerifyContext};
use super::spec::assemble_planned_session_launch;
use super::warm::{
    dispatch_warm_handle_turn, warm_handle_stop_reason, LaunchRouteOpts, StartReservation,
    WarmFreshness, WarmHandleTurn,
};
use super::{
    BindingGuard, LiveSessionHandle, PermitGuard, PlannedSessionLaunch, PreparedSessionLaunch,
    SessionManager,
};

/// Times a start re-plans when its warm handle is replaced while freshness checks run unlocked.
const MAX_WARM_REVALIDATIONS: usize = 3;

/// A warm handle as seen under a brief lock: its identity (the runtime-state `Arc`) and the
/// immutable fields the unlocked disk and SQLite freshness checks read.
struct WarmHandleSnapshot {
    state: Arc<Mutex<SessionRuntimeState>>,
    has_connection: bool,
    sandbox_plan: SandboxPlan,
}

/// What a start does once its warm handle (if any) is re-validated under the lock.
enum StartDecision {
    LiveView(Box<SessionView>),
    WarmSend(WarmSendTarget),
    Cold {
        finished: Option<Box<LiveSessionHandle>>,
        start_stop_epoch: u64,
    },
}

struct WarmSendTarget {
    tx: Sender<PumpCommand>,
    state: Arc<Mutex<SessionRuntimeState>>,
    conn_id: Option<String>,
    route_digest: String,
    active_permit: Arc<Mutex<Option<(String, u64)>>>,
    start_stop_epoch: u64,
}

impl SessionManager {
    /// Reserves the start slot or sends a turn to the warm handle. No disk, SQLite or
    /// watch-notice I/O runs under `registry` or a handle's state lock (`TASK-190`, rules §3.4):
    /// the handle is snapshotted under a brief lock, checked unlocked, then re-validated by
    /// identity under the lock before anything is decided.
    ///
    /// A Chief of Staff turn names its `partition` (`TASK-80.1`): whether the live process
    /// serves it is decided in that same critical section, and the turn is counted in the
    /// partition only once the slot (or the warm send) is this start's.
    #[cfg(test)]
    pub(crate) fn reserve_start_or_send(
        &self,
        employee: &Employee,
        objective: Option<String>,
        task_ref: Option<&str>,
        requested_workspace: Option<&str>,
        partition: Option<&PartitionKey>,
    ) -> Result<StartReservation, ApiError> {
        self.reserve_start_or_send_with_control(
            employee,
            objective,
            Vec::new(),
            task_ref,
            requested_workspace,
            partition,
            WarmReuseControl::Default,
            &crate::router::TurnModelOverride::default(),
        )
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "start reservation coordinates employee, turn payload, workspace, partition, affinity control, and model/effort override"
    )]
    pub(super) fn reserve_start_or_send_with_control(
        &self,
        employee: &Employee,
        objective: Option<String>,
        attachment_ids: Vec<String>,
        task_ref: Option<&str>,
        requested_workspace: Option<&str>,
        partition: Option<&PartitionKey>,
        control: WarmReuseControl<'_>,
        turn_override: &crate::router::TurnModelOverride,
    ) -> Result<StartReservation, ApiError> {
        let emp_id = employee.id.as_str();
        // Load the lease cache now, so the check under the registry lock below never misses it.
        let _ = self.external_lease(emp_id);
        for _ in 0..MAX_WARM_REVALIDATIONS {
            let snapshot = self.snapshot_warm_handle(emp_id)?;
            let freshness = snapshot
                .as_ref()
                .map(|snap| self.warm_handle_freshness(emp_id, snap));
            let decision = self.decide_start_or_send(
                emp_id,
                snapshot.as_ref(),
                freshness,
                objective.is_some() || !attachment_ids.is_empty(),
                requested_workspace,
                partition,
                control,
                turn_override,
            )?;
            match decision {
                None => continue,
                Some(StartDecision::LiveView(view)) => {
                    return Ok(StartReservation {
                        live_view: Some(*view),
                        finished_handle: None,
                        start_stop_epoch: 0,
                        partition_turn: None,
                    });
                }
                Some(StartDecision::Cold {
                    finished,
                    start_stop_epoch,
                }) => {
                    return Ok(StartReservation {
                        live_view: None,
                        finished_handle: finished.map(|h| *h),
                        start_stop_epoch,
                        partition_turn: self.begin_partition_turn(emp_id, partition),
                    });
                }
                Some(StartDecision::WarmSend(target)) => {
                    let partition_turn = self.begin_partition_turn(emp_id, partition);
                    let mut reservation = dispatch_warm_handle_turn(
                        self,
                        WarmHandleTurn {
                            emp_id,
                            task_ref,
                            tx: target.tx,
                            state: target.state,
                            conn_id: target.conn_id,
                            route_digest: target.route_digest,
                            active_permit: target.active_permit,
                            text: objective.unwrap_or_default(),
                            attachment_ids,
                            start_stop_epoch: target.start_stop_epoch,
                            turn_override,
                        },
                    )?;
                    reservation.partition_turn = partition_turn;
                    return Ok(reservation);
                }
            }
        }
        Err(ApiError::new(
            ErrorCode::Conflict,
            "agent session changed while the start was being planned; retry",
        ))
    }

    /// Counts a Chief of Staff turn in `partition`, rotating it first when due; called only
    /// while this start holds the employee's start slot.
    fn begin_partition_turn(
        &self,
        emp_id: &str,
        partition: Option<&PartitionKey>,
    ) -> Option<PartitionTurn> {
        partition.map(|key| self.partitions.begin_turn(emp_id, key))
    }

    fn snapshot_warm_handle(&self, emp_id: &str) -> Result<Option<WarmHandleSnapshot>, ApiError> {
        let reg = self.registry.lock().unwrap_or_else(|p| p.into_inner());
        check_registry_startable(&reg, emp_id)?;
        let Some(handle) = reg.live.get(emp_id) else {
            return Ok(None);
        };
        ensure_live_handle_not_recovery_required(
            &handle.state.lock().unwrap_or_else(|p| p.into_inner()),
        )?;
        Ok(Some(WarmHandleSnapshot {
            state: Arc::clone(&handle.state),
            has_connection: handle.connection_id.is_some(),
            sandbox_plan: handle.sandbox_plan.clone(),
        }))
    }

    /// Disk (`read_dir`) and SQLite (`list_employees`, `load_agent_execution`) checks, run
    /// with no lock held.
    fn warm_handle_freshness(&self, emp_id: &str, snap: &WarmHandleSnapshot) -> WarmFreshness {
        WarmFreshness {
            sandbox_topology_stale: self.sandbox_runner.is_plan_stale(
                &self.office,
                emp_id,
                &snap.sandbox_plan,
            ),
            route_stale: !snap.has_connection
                && self
                    .office
                    .load_agent_execution(emp_id)
                    .ok()
                    .flatten()
                    .is_some(),
        }
    }

    /// Re-validates the snapshotted handle by identity under the lock and decides. `None` means
    /// the handle was replaced or removed while unlocked, so the caller plans again.
    #[expect(
        clippy::too_many_arguments,
        reason = "re-validates snapshotted warm handle under lock with freshness, workspace, partition, affinity control, and model/effort override"
    )]
    fn decide_start_or_send(
        &self,
        emp_id: &str,
        snapshot: Option<&WarmHandleSnapshot>,
        freshness: Option<WarmFreshness>,
        wants_turn: bool,
        requested_workspace: Option<&str>,
        partition: Option<&PartitionKey>,
        control: WarmReuseControl<'_>,
        turn_override: &crate::router::TurnModelOverride,
    ) -> Result<Option<StartDecision>, ApiError> {
        let mut reg = self.registry.lock().unwrap_or_else(|p| p.into_inner());
        check_registry_startable(&reg, emp_id)?;
        if self
            .external
            .cached_active(emp_id, crate::credentials::now_ms())
        {
            return Err(ApiError::new(
                ErrorCode::Conflict,
                "this agent is signed in from an external workspace; revoke that session first",
            ));
        }
        let handle = reg.live.get(emp_id);
        let unchanged = match (snapshot, handle) {
            (None, None) => true,
            (Some(snap), Some(handle)) => Arc::ptr_eq(&snap.state, &handle.state),
            _ => false,
        };
        if !unchanged {
            return Ok(None);
        }
        let (Some(handle), Some(freshness)) = (handle, freshness) else {
            reg.starting.insert(emp_id.to_owned());
            return Ok(Some(StartDecision::Cold {
                finished: None,
                start_stop_epoch: reg.stop_epoch(emp_id),
            }));
        };
        let state = handle.state.lock().unwrap_or_else(|p| p.into_inner());
        ensure_live_handle_not_recovery_required(&state)?;
        // A Chief of Staff process serving another partition (or due to rotate) is stopped
        // when idle and refused when busy, here, with the slot not yet taken (`TASK-80.1`).
        let switch = match partition {
            Some(key) => partition_switch_reason(&self.partitions, emp_id, handle, &state, key)?,
            None => None,
        };
        let control_stop = control_stop_reason(control, state.state)?;
        let stop_reason = switch.or(control_stop).or_else(|| {
            warm_handle_stop_reason(
                handle,
                requested_workspace,
                turn_override,
                state.state,
                freshness,
            )
        });
        if stop_reason.is_none() && is_warm_state_reusable(state.state) {
            if !wants_turn {
                return Ok(Some(StartDecision::LiveView(Box::new(state.to_view()))));
            }
            if handle.pinned_override != *turn_override {
                return Err(ApiError::new(
                    ErrorCode::Conflict,
                    "agent session is busy with a different model or reasoning effort; wait for the current turn to finish or stop the session first",
                ));
            }
            ensure_warm_sandbox_not_stale(freshness.sandbox_topology_stale)?;
            let target = WarmSendTarget {
                tx: handle.tx.clone(),
                state: Arc::clone(&handle.state),
                conn_id: handle.connection_id.clone(),
                route_digest: handle.route_digest.clone(),
                active_permit: Arc::clone(&handle.active_permit),
                start_stop_epoch: reg.stop_epoch(emp_id),
            };
            drop(state);
            // Reserve `starting` BEFORE releasing the `registry` lock so no concurrent caller
            // can race into a second spawn (`Finding #3`).
            reg.starting.insert(emp_id.to_owned());
            return Ok(Some(StartDecision::WarmSend(target)));
        }
        if let Some(reason) = stop_reason {
            let _ = handle.tx.send(PumpCommand::Stop {
                reason: reason.into(),
            });
        }
        drop(state);
        let finished = reg.live.remove(emp_id).map(Box::new);
        reg.starting.insert(emp_id.to_owned());
        Ok(Some(StartDecision::Cold {
            finished,
            start_stop_epoch: reg.stop_epoch(emp_id),
        }))
    }

    pub(super) fn prepare_and_spawn_raw(
        &self,
        employee: &Employee,
        params: &MemberSessionStartParams,
        objective: String,
        attachment_ids: Vec<String>,
        opts: LaunchRouteOpts<'_>,
    ) -> Result<PreparedSessionLaunch, ApiError> {
        let objective = self.prepend_watch_notices(employee.id.as_str(), objective);
        let planned = self.plan_session_launch(employee, params, opts)?;
        let objective = prepend_fallback_notice(planned.fallback_notice.as_deref(), objective);
        self.issue_and_spawn_launch(objective, attachment_ids, planned)
    }

    pub(super) fn plan_session_launch(
        &self,
        employee: &Employee,
        params: &MemberSessionStartParams,
        opts: LaunchRouteOpts<'_>,
    ) -> Result<PlannedSessionLaunch, ApiError> {
        let emp_id = employee.id.as_str();
        let workspace = self.prepare_launch_workspace(
            employee,
            params.workspace.as_deref(),
            opts.task_ref,
            opts.start_stop_epoch,
        )?;
        let mut admitted =
            crate::router::select_and_admit_route(crate::router::AdmitRouteRequest {
                office: &self.office,
                vault: self.enrollment.vault(),
                runtimes: &self.config.runtimes,
                hq_permits: self.config.hq_permits.as_deref(),
                oauth_refresh: Some(&self.vault_oauth_refresh),
                employee_id: emp_id,
                task_id: opts.task_ref,
                is_failover_switch: opts.is_failover_switch,
                excluded_connections: &opts.excluded_connections,
                turn_override: Some(&opts.turn_override),
            })?;
        let mut permit_guard = PermitGuard::new(
            &self.office,
            self.config.hq_permits.as_deref(),
            admitted.permit.take(),
        );
        let binary = self
            .config
            .runtimes
            .resolve_binary(admitted.adapter.as_ref(), None)
            .map_err(|_| ApiError::new(ErrorCode::Unavailable, "agent runtime is unavailable"))?;
        let sandbox_plan = self.sandbox_runner.prepare_launch(&SandboxPrepareContext {
            office: &self.office,
            office_socket: &self.config.office_socket,
            agent_os_state_roots: &self.config.agent_os_state_roots,
            employee_id: emp_id,
            sandbox_enabled: admitted.sandbox_enabled,
            host_profile: admitted.adapter.host_profile(),
        })?;
        let invocation = self.plan_runtime_invocation(
            employee,
            &mut admitted,
            binary,
            &workspace,
            sandbox_plan,
            &opts,
        )?;
        Ok(assemble_planned_session_launch(
            employee,
            admitted,
            permit_guard.take(),
            workspace,
            params.workspace.clone(),
            opts,
            invocation,
        ))
    }

    pub(super) fn issue_and_spawn_launch(
        &self,
        objective: String,
        attachment_ids: Vec<String>,
        planned: PlannedSessionLaunch,
    ) -> Result<PreparedSessionLaunch, ApiError> {
        let PlannedSessionLaunch {
            emp_id,
            employee_name,
            employee_handle,
            run_id,
            session_id,
            mark_str,
            runtime_id,
            runtime,
            supports_mid_turn,
            model_supports_vision,
            allow_host_auth,
            sandbox_plan,
            connection_id,
            route_digest,
            oauth_expires_at_ms,
            oauth_token_fp,
            oauth_refresh_attempted,
            workspace_arg,
            permit,
            task_ref,
            tried_connections,
            start_stop_epoch,
            partition,
            active_model,
            active_effort,
            turn_override,
            home,
            workspace_real,
            workspace_spec,
            runtime_dir,
            attachments_root,
            tmpdir,
            preset_cfg,
            spec_hash,
            fallback_notice: _,
            auth_env,
            ctx,
            plan,
            session,
        } = planned;
        let mut permit_guard =
            PermitGuard::new(&self.office, self.config.hq_permits.as_deref(), permit);
        // The run acts for the caller of the partition it was launched for, named on this very
        // launch, and for nobody when it has none (`TASK-80.1`).
        let acting_for = partition.as_ref().and_then(PartitionKey::caller_id);
        let binding = self
            .bindings
            .issue(&emp_id, &run_id, &session_id, 0, acting_for)
            .map_err(|_| ApiError::new(ErrorCode::Unavailable, "could not mint session token"))?;
        let binding_file = runtime_dir.join(format!("session-binding-{run_id}.v1"));
        let mut binding_guard = BindingGuard::new(&self.bindings, &emp_id, &run_id, &binding_file);
        write_owner_only_binding_file(&binding_file, binding.expose_token())?;
        let agent_base = self.agent_base_dir(&emp_id);
        let env = self.build_environment(SessionEnvInput {
            home: &home,
            agent_base: Some(&agent_base),
            workspace: &workspace_real,
            tmpdir: &tmpdir,
            employee_id: &emp_id,
            employee_name: &employee_name,
            employee_handle: &employee_handle,
            session_id: &session_id,
            token: binding.expose_token(),
            binding_file: &binding_file,
            sandbox_enabled: preset_cfg.sandbox.enabled,
            operator: !preset_cfg.operator_shell_prefixes.is_empty(),
        })?;
        let mark = SessionMark::new(mark_str.clone())
            .map_err(|_| ApiError::new(ErrorCode::StorageError, "invalid session mark"))?;
        let launch = org_supervise::runtime::prepare_with_auth(plan, &ctx, env, &auth_env, mark)
            .map_err(|_| {
                ApiError::new(
                    ErrorCode::Unavailable,
                    "failed to prepare supervised session",
                )
            })?;
        let persisted_stalls = self
            .store
            .read_active_turn(&emp_id)
            .ok()
            .flatten()
            .map_or(0, |r| r.consecutive_stalls);

        let active_employees = self.active_or_starting_employee_ids();
        self.sandbox_runner.verify_before_spawn(
            &SandboxVerifyContext {
                office: &self.office,
                employee_id: &emp_id,
                active_employees: &active_employees,
            },
            &sandbox_plan,
        )?;
        ensure_not_stopped_since(&self.registry, &emp_id, start_stop_epoch)?;
        let initial_rsid = launch
            .lifecycle
            .runtime_session_id
            .as_ref()
            .map(|s| s.as_str());
        let spawned = self.spawn_and_persist(
            launch.prepared,
            &emp_id,
            &run_id,
            &session_id,
            &mark_str,
            &objective,
            &attachment_ids,
            persisted_stalls,
            initial_rsid,
        )?;
        if let Some(rsid) = initial_rsid {
            let _ = self.store.record_session_affinity(
                &emp_id,
                affinity_entry(rsid, &runtime_id, &workspace_spec, &spec_hash),
            );
        }
        binding_guard.disarm();
        drop(binding_guard);

        Ok(PreparedSessionLaunch {
            emp_id,
            run_id,
            session_id,
            runtime_id,
            runtime,
            supports_mid_turn,
            model_supports_vision,
            allow_host_auth,
            sandbox_plan,
            connection_id,
            route_digest,
            oauth_expires_at_ms,
            oauth_token_fp,
            oauth_refresh_attempted,
            workspace_arg,
            permit: permit_guard.take(),
            task_ref,
            tried_connections,
            start_stop_epoch,
            partition,
            active_model,
            active_effort,
            turn_override,
            home,
            workspace_spec,
            attachments_root,
            tmpdir,
            binding_file,
            preset_cfg,
            spec_hash,
            objective,
            attachment_ids,
            persisted_stalls,
            spawned,
            session,
            lifecycle: launch.lifecycle,
        })
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "spawn persistence records prepared process, identifiers, mark, objective, attachment ids, stalls, and runtime session"
    )]
    pub(super) fn spawn_and_persist(
        &self,
        prepared: org_supervise::PreparedSpawn,
        emp_id: &str,
        run_id: &str,
        session_id: &str,
        mark_str: &str,
        objective: &str,
        attachment_ids: &[String],
        persisted_stalls: u32,
        runtime_session_id: Option<&str>,
    ) -> Result<org_supervise::Spawned, ApiError> {
        let pending = PendingStartRecord {
            version: 1,
            employee_id: emp_id.to_owned(),
            run_id: run_id.to_owned(),
            session_id: session_id.to_owned(),
            mark: mark_str.to_owned(),
            started_at: rfc3339_now(),
        };
        let recorded = prepared
            .record_mark(|_| self.store.write_pending_start(&pending))
            .map_err(|_| {
                self.bindings.revoke(emp_id, Some(run_id));
                ApiError::new(ErrorCode::StorageError, "could not record pending start")
            })?;
        let pending_child = recorded.spawn().map_err(|_| {
            self.bindings.revoke(emp_id, Some(run_id));
            let _ = self.store.clear_pending_start(emp_id);
            ApiError::new(ErrorCode::Unavailable, "failed to spawn agent process")
        })?;
        pending_child
            .record_identity(|ident| {
                self.bindings
                    .set_root_pid(emp_id, run_id, ident.pid as libc::pid_t);
                self.hibernation.record_spawned_pid(
                    emp_id,
                    run_id,
                    ident.pid,
                    std::time::Instant::now(),
                );
                let active_turn = ActiveTurnRecord {
                    version: 1,
                    employee_id: emp_id.to_owned(),
                    run_id: run_id.to_owned(),
                    session_id: session_id.to_owned(),
                    runtime_session_id: runtime_session_id.map(str::to_owned),
                    mark: ident.mark.clone(),
                    pid: Some(ident.pid),
                    pgid: Some(ident.pgid),
                    started_identity: Some(ident.started.clone()),
                    boot_identity: Some(ident.boot.clone()),
                    objective: objective.to_owned(),
                    attachment_ids: attachment_ids.to_vec(),
                    accepted_commands: Vec::new(),
                    resume_attempts: 0,
                    consecutive_stalls: persisted_stalls,
                    stall_locked: false,
                    recovery_required: false,
                    active: true,
                    updated_at: rfc3339_now(),
                };
                self.store.write_active_turn(&active_turn)?;
                self.store.clear_pending_start(emp_id)
            })
            .map_err(|_| {
                self.bindings.revoke(emp_id, Some(run_id));
                ApiError::new(ErrorCode::StorageError, "could not persist child identity")
            })
    }

    /// Brief unlocked snapshot of currently live or starting employee IDs for confinement checks.
    pub(super) fn active_or_starting_employee_ids(&self) -> Vec<String> {
        let reg = self.registry.lock().unwrap_or_else(|p| p.into_inner());
        reg.live
            .keys()
            .chain(reg.starting.iter())
            .cloned()
            .collect()
    }
}
