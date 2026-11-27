//! Session launch: admission, route planning, warm reuse checks, and spawn + persist.

use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};

use org_office::organization_family::records::{Employee, PermissionPreset};
use org_proto::sessions::{MemberSessionStartParams, SessionState, SessionView};
use org_proto::{ApiError, ErrorCode};
use org_supervise::SessionMark;

use crate::credentials::rfc3339_now;
use crate::partition::{PartitionKey, PartitionTurn};
use crate::pump::{PumpCommand, SessionRuntimeState};
use crate::recovery::{ActiveTurnRecord, PendingStartRecord};
use crate::subagents::{build_general_subagent_definition, write_subagent_definition};

use super::chief_of_staff::partition_switch_reason;
use super::environment::{
    apply_build_concurrency_env, assemble_launch_env, resolve_workspace,
    write_owner_only_binding_file, SessionEnvInput,
};
use super::operator_home::ensure_operator_home;
use super::registry_guards::{
    check_registry_startable, ensure_live_handle_not_recovery_required, ensure_not_stopped_since,
};
use super::sandbox_runner::{SandboxPlan, SandboxPrepareContext, SandboxVerifyContext};
use super::spec::{
    build_launch_context, build_session_spec, canonical_or_spec, compute_launch_spec_hash,
    ModelChoice,
};
use super::warm::{
    dispatch_warm_handle_turn, warm_handle_stop_reason, LaunchRouteOpts, StartReservation,
    WarmFreshness, WarmHandleTurn,
};
use super::{
    next_entropy_hex, BindingGuard, LiveSessionHandle, PermitGuard, PlannedSessionLaunch,
    PreparedSessionLaunch, SessionManager,
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
        finished: Option<LiveSessionHandle>,
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
    pub(crate) fn reserve_start_or_send(
        &self,
        employee: &Employee,
        objective: Option<String>,
        task_ref: Option<&str>,
        requested_workspace: Option<&str>,
        partition: Option<&PartitionKey>,
    ) -> Result<StartReservation, ApiError> {
        self.reserve_start_or_send_with_attachments(
            employee,
            objective,
            Vec::new(),
            task_ref,
            requested_workspace,
            partition,
        )
    }

    pub(crate) fn reserve_start_or_send_with_attachments(
        &self,
        employee: &Employee,
        objective: Option<String>,
        attachment_ids: Vec<String>,
        task_ref: Option<&str>,
        requested_workspace: Option<&str>,
        partition: Option<&PartitionKey>,
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
                        finished_handle: finished,
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
    fn decide_start_or_send(
        &self,
        emp_id: &str,
        snapshot: Option<&WarmHandleSnapshot>,
        freshness: Option<WarmFreshness>,
        wants_turn: bool,
        requested_workspace: Option<&str>,
        partition: Option<&PartitionKey>,
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
        let stop_reason = switch.or_else(|| {
            warm_handle_stop_reason(handle, requested_workspace, state.state, freshness)
        });
        if stop_reason.is_none() && is_warm_state_reusable(state.state) {
            if !wants_turn {
                return Ok(Some(StartDecision::LiveView(Box::new(state.to_view()))));
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
        let finished = reg.live.remove(emp_id);
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
        self.issue_and_spawn_launch(objective, attachment_ids, planned)
    }

    pub(super) fn plan_session_launch(
        &self,
        employee: &Employee,
        params: &MemberSessionStartParams,
        opts: LaunchRouteOpts<'_>,
    ) -> Result<PlannedSessionLaunch, ApiError> {
        let emp_id = employee.id.as_str().to_owned();
        // A stop that overtook this start while its watch notices were read skips admission,
        // disk setup and the spawn (registration re-checks atomically, `TASK-190`).
        ensure_not_stopped_since(&self.registry, &emp_id, opts.start_stop_epoch)?;
        // Reclaim overdue idle warm sessions and enforce any office RSS budget before launching
        // a new supervised runtime process (`TASK-133`).
        self.sweep_resource_limits();
        // Before creating this agent's runtime directory or session-binding token on disk,
        // ensure no already-running sandboxed sibling lacks isolation for `emp_id` (`TASK-166`).
        self.sandbox_runner
            .enforce_sibling_isolation(self, &emp_id)?;
        let home = self
            .office
            .ensure_private_home(&emp_id)
            .map_err(|_| ApiError::new(ErrorCode::StorageError, "could not prepare agent home"))?;
        let workspace_real = resolve_workspace(&home, params.workspace.as_deref())?;
        ensure_operator_home(employee, &home, &workspace_real, &self.config.runtimes)?;
        let workspace_spec = canonical_or_spec(&workspace_real)?;

        // Auto-pre-prepare any repository-linked task worktrees BEFORE route admission (which
        // hashes `worktree_sandbox_digest`) and BEFORE sandbox planning so Turn 1 always launches
        // with exact least-privilege bare-cache grants (`TASK-135` AC #4).
        if let Some(task_ref) = opts.task_ref {
            self.preprepare_task_worktrees(&emp_id, &[task_ref]);
        }

        let admitted = crate::router::select_and_admit_route(crate::router::AdmitRouteRequest {
            office: &self.office,
            vault: self.enrollment.vault(),
            runtimes: &self.config.runtimes,
            hq_permits: self.config.hq_permits.as_deref(),
            oauth_refresh: Some(&self.vault_oauth_refresh),
            employee_id: &emp_id,
            task_id: opts.task_ref,
            is_failover_switch: opts.is_failover_switch,
            excluded_connections: &opts.excluded_connections,
        })?;
        let mut permit_guard = PermitGuard::new(
            &self.office,
            self.config.hq_permits.as_deref(),
            admitted.permit,
        );
        let binary = self
            .config
            .runtimes
            .resolve_binary(admitted.adapter.as_ref(), None)
            .map_err(|_| ApiError::new(ErrorCode::Unavailable, "agent runtime is unavailable"))?;

        let host_profile = admitted.adapter.host_profile();
        let sandbox_plan = self.sandbox_runner.prepare_launch(&SandboxPrepareContext {
            office: &self.office,
            office_socket: &self.config.office_socket,
            agent_os_state_roots: &self.config.agent_os_state_roots,
            employee_id: &emp_id,
            sandbox_enabled: admitted.sandbox_enabled,
            host_profile,
        })?;

        let mut preset_cfg = self.build_employee_preset_cfg(
            &emp_id,
            PermissionPreset::effective_for(employee.builtin_key, admitted.preset),
            admitted.sandbox_enabled,
            &workspace_spec,
            true,
        )?;
        let (runtime_dir, tmpdir) = self.prepare_run_dirs(&emp_id)?;
        let attachments_root = crate::turn_attachments::attachments_root(&runtime_dir);
        Self::append_attachments_write_denies(&mut preset_cfg, &attachments_root);
        let subagent_def =
            build_general_subagent_definition(preset_cfg.allowed_tools.as_deref(), None);
        let _ = write_subagent_definition(&runtime_dir, &subagent_def);

        let instructions =
            self.build_instructions(employee, &home, &workspace_real, &preset_cfg)?;
        let mut read_only_dirs = self.collect_session_read_only_dirs(
            admitted.adapter.as_ref(),
            &home,
            &workspace_real,
            preset_cfg.sandbox.enabled,
        );
        Self::append_attachments_read_only_dir(&mut read_only_dirs, &attachments_root);
        let mut spec = build_session_spec(
            admitted.adapter.id(),
            employee,
            ModelChoice {
                model: admitted.model_token,
                effort: admitted.effort,
            },
            workspace_spec,
            read_only_dirs,
            instructions,
            &preset_cfg,
        )?;
        self.sandbox_runner
            .adapt_session_spec(&sandbox_plan, &mut spec);
        let spec_hash = compute_launch_spec_hash(&spec, &admitted.route_digest_suffix);
        let run_id = format!("run-{}", next_entropy_hex(10)?);
        let session_id = next_uuid_v4()?;
        let mark_str = format!("m-{}", next_entropy_hex(12)?);
        let allow_host_auth = crate::host_auth::allows_host_subscription_auth(
            &admitted.auth_env,
            admitted.auth_mode,
            host_profile.uses_host_oauth_token_injection,
        );
        let hibernated_resume = self.resolve_hibernated_resume_id(
            &emp_id,
            admitted.adapter.id().as_str(),
            &spec_hash,
            &opts,
        );
        let ctx = build_launch_context(
            binary,
            &runtime_dir,
            &tmpdir,
            &session_id,
            opts.resume_runtime_session_id
                .or(hibernated_resume.as_deref()),
            host_profile.host_config_subdir,
            allow_host_auth,
        )?;
        let (mut plan, session) = admitted
            .adapter
            .plan_session(&spec, &ctx)
            .map_err(|_| ApiError::new(ErrorCode::Unavailable, "runtime rejected session spec"))?;

        let mut launch_env = assemble_launch_env(admitted.auth_env, admitted.endpoint_env);
        let oauth_expires_at_ms = admitted.oauth_expires_at_ms;
        let oauth_token_fp = admitted.oauth_token_fp;
        apply_build_concurrency_env(
            &mut plan,
            &mut launch_env,
            self.config.resource_limits.build_concurrency_jobs,
        );

        Ok(PlannedSessionLaunch {
            emp_id,
            employee_name: employee.name.clone(),
            employee_handle: employee.handle.clone(),
            run_id,
            session_id,
            mark_str,
            runtime_id: admitted.adapter.id().as_str().to_owned(),
            runtime: Arc::clone(&admitted.adapter),
            supports_mid_turn: admitted.adapter.capabilities().supports_mid_turn_delivery(),
            model_supports_vision: admitted.model_supports_vision,
            allow_host_auth,
            sandbox_plan,
            connection_id: admitted.connection_id,
            route_digest: admitted.route_digest_suffix,
            oauth_expires_at_ms,
            oauth_token_fp,
            oauth_refresh_attempted: opts.oauth_refresh_attempted,
            workspace_arg: params.workspace.clone(),
            permit: permit_guard.take(),
            task_ref: opts.task_ref.map(str::to_owned),
            tried_connections: opts.tried_connections,
            start_stop_epoch: opts.start_stop_epoch,
            partition: opts.partition,
            home,
            workspace_real,
            runtime_dir,
            attachments_root,
            tmpdir,
            preset_cfg,
            spec_hash,
            auth_env: launch_env,
            ctx,
            plan,
            session,
        })
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
            home,
            workspace_real,
            runtime_dir,
            attachments_root,
            tmpdir,
            preset_cfg,
            spec_hash,
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
        let spawned = self.spawn_and_persist(
            launch.prepared,
            &emp_id,
            &run_id,
            &session_id,
            &mark_str,
            &objective,
            &attachment_ids,
            persisted_stalls,
            launch
                .lifecycle
                .runtime_session_id
                .as_ref()
                .map(|s| s.as_str()),
        )?;
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
            home,
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

    fn resolve_hibernated_resume_id(
        &self,
        emp_id: &str,
        runtime_id: &str,
        spec_hash: &str,
        opts: &LaunchRouteOpts<'_>,
    ) -> Option<String> {
        if opts.resume_runtime_session_id.is_some()
            || opts.partition.is_some()
            || opts.is_failover_switch
        {
            return None;
        }
        self.hibernation
            .matching_resume_id(emp_id, runtime_id, spec_hash, opts.start_stop_epoch)
    }
}

fn next_uuid_v4() -> Result<String, ApiError> {
    use ring::rand::{SecureRandom, SystemRandom};
    let mut b = [0_u8; 16];
    SystemRandom::new()
        .fill(&mut b)
        .map_err(|_| ApiError::new(ErrorCode::Unavailable, "system entropy unavailable"))?;
    b[6] = (b[6] & 0x0f) | 0x40;
    b[8] = (b[8] & 0x3f) | 0x80;
    Ok(format!(
        "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7], b[8], b[9], b[10], b[11], b[12], b[13], b[14], b[15]
    ))
}

pub(super) fn ensure_warm_sandbox_not_stale(sandbox_topology_stale: bool) -> Result<(), ApiError> {
    if sandbox_topology_stale {
        return Err(ApiError::new(
            ErrorCode::Conflict,
            "sandboxed session topology is stale while busy; wait for current turn to finish or stop session first",
        ));
    }
    Ok(())
}

pub(super) fn is_warm_state_reusable(state: SessionState) -> bool {
    !matches!(
        state,
        SessionState::Stopped
            | SessionState::Stopping
            | SessionState::Suspended
            | SessionState::StallLocked
            | SessionState::RecoveryRequired
    )
}
