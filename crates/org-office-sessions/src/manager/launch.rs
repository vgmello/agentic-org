//! Session launch: admission, route planning, warm reuse checks, and spawn + persist.

use super::*;

impl SessionManager {
    pub(super) fn reserve_start_or_send(
        &self,
        employee: &Employee,
        objective: Option<String>,
        task_ref: Option<&str>,
        requested_workspace: Option<&str>,
    ) -> Result<(Option<SessionView>, Option<LiveSessionHandle>), ApiError> {
        let emp_id = employee.id.as_str();
        let mut reg = self.registry.lock().unwrap_or_else(|p| p.into_inner());
        check_registry_startable(&reg, emp_id)?;
        let finished = if let Some(handle) = reg.live.get(emp_id) {
            let state = handle.state.lock().unwrap_or_else(|p| p.into_inner());
            ensure_live_handle_not_recovery_required(&state)?;
            let bwrap_topology_stale = is_bwrap_topology_stale(&self.office, emp_id, handle);
            let stop_reason = warm_handle_stop_reason(
                &self.office,
                emp_id,
                handle,
                requested_workspace,
                state.state,
                bwrap_topology_stale,
            );
            if stop_reason.is_none() && is_warm_state_reusable(state.state) {
                if let Some(text) = objective {
                    ensure_warm_bwrap_not_stale(bwrap_topology_stale)?;
                    let text = self.prepend_watch_notices(emp_id, text);
                    let tx = handle.tx.clone();
                    let st = Arc::clone(&handle.state);
                    let conn_id = handle.connection_id.clone();
                    let route_digest = handle.route_digest.clone();
                    let active_permit = Arc::clone(&handle.active_permit);
                    drop(state);
                    // Reserve `starting` BEFORE releasing `reg` lock so no concurrent caller
                    // can race into a second spawn (`Finding #3`).
                    reg.starting.insert(emp_id.to_owned());
                    drop(reg);
                    return dispatch_warm_handle_turn(
                        self,
                        WarmHandleTurn {
                            emp_id,
                            task_ref,
                            tx,
                            state: st,
                            conn_id,
                            route_digest,
                            active_permit,
                            text,
                        },
                    );
                }
                return Ok((Some(state.to_view()), None));
            }
            if let Some(reason) = stop_reason {
                let _ = handle.tx.send(PumpCommand::Stop {
                    reason: reason.into(),
                });
            }
            true
        } else {
            false
        };
        let old = if finished {
            reg.live.remove(emp_id)
        } else {
            None
        };
        reg.starting.insert(emp_id.to_owned());
        Ok((None, old))
    }

    pub(super) fn prepare_and_spawn(
        &self,
        actor_member_id: &str,
        employee: &Employee,
        params: &MemberSessionStartParams,
        resume_runtime_session_id: Option<&str>,
    ) -> Result<PreparedSessionLaunch, ApiError> {
        let objective = params
            .instruction
            .as_deref()
            .map(|text| build_objective_text(employee, actor_member_id, text))
            .unwrap_or_default();
        self.prepare_and_spawn_raw(employee, params, objective, resume_runtime_session_id, None)
    }

    pub(super) fn prepare_and_spawn_raw(
        &self,
        employee: &Employee,
        params: &MemberSessionStartParams,
        objective: String,
        resume_runtime_session_id: Option<&str>,
        task_ref: Option<&str>,
    ) -> Result<PreparedSessionLaunch, ApiError> {
        let opts = LaunchRouteOpts {
            resume_runtime_session_id,
            task_ref,
            is_failover_switch: false,
            oauth_refresh_attempted: false,
            excluded_connections: HashSet::new(),
            tried_connections: Vec::new(),
        };
        let objective = self.prepend_watch_notices(employee.id.as_str(), objective);
        let planned = self.plan_session_launch(employee, params, opts)?;
        self.issue_and_spawn_launch(objective, planned)
    }

    pub(super) fn plan_session_launch(
        &self,
        employee: &Employee,
        params: &MemberSessionStartParams,
        opts: LaunchRouteOpts<'_>,
    ) -> Result<PlannedSessionLaunch, ApiError> {
        let emp_id = employee.id.as_str().to_owned();
        // Before creating this agent's runtime directory or session-binding token on disk,
        // ensure no already-running Linux `bwrap` sibling lacks a mount mask for `emp_id`
        // (`TASK-166`).
        crate::bwrap_isolation::enforce_bwrap_sibling_isolation(self, &emp_id)?;
        let home = self
            .office
            .ensure_private_home(&emp_id)
            .map_err(|_| ApiError::new(ErrorCode::StorageError, "could not prepare agent home"))?;
        let workspace_real = resolve_workspace(&home, params.workspace.as_deref())?;
        let workspace_spec = canonical_or_spec(&workspace_real)?;

        let admitted = crate::router::select_and_admit_route(crate::router::AdmitRouteRequest {
            office: &self.office,
            vault: self.enrollment.vault(),
            runtimes: &self.config.runtimes,
            hq_permits: self.config.hq_permits.as_deref(),
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

        let is_linux_claude_bwrap =
            is_linux_claude_bwrap(admitted.sandbox_enabled, admitted.adapter.id().as_str());
        let bwrap_masked_siblings = if is_linux_claude_bwrap {
            Some(crate::bwrap_isolation::ensure_bwrap_deny_dirs(
                self, &emp_id,
            )?)
        } else {
            None
        };

        let preset_cfg = self.build_employee_preset_cfg(
            &emp_id,
            admitted.preset,
            admitted.sandbox_enabled,
            &workspace_spec,
            true,
        )?;
        let (runtime_dir, tmpdir) = self.prepare_run_dirs(&emp_id)?;
        let subagent_def =
            build_general_subagent_definition(preset_cfg.allowed_tools.as_deref(), None);
        let _ = write_subagent_definition(&runtime_dir, &subagent_def);

        let instructions =
            self.build_instructions(employee, &home, &workspace_real, &preset_cfg, opts.task_ref)?;
        let read_only_dirs = self.collect_session_read_only_dirs(
            admitted.adapter.id(),
            &home,
            &workspace_real,
            preset_cfg.sandbox.enabled,
        );
        let spec = build_session_spec_for_launch(
            admitted.adapter.id(),
            employee,
            admitted.model_token,
            workspace_spec,
            read_only_dirs,
            instructions,
            &preset_cfg,
            is_linux_claude_bwrap,
        )?;
        let spec_hash = compute_launch_spec_hash(&spec, &admitted.route_digest_suffix);
        let run_id = format!("run-{}", next_entropy_hex(10)?);
        let session_id = next_uuid_v4()?;
        let mark_str = format!("m-{}", next_entropy_hex(12)?);
        let allow_host_auth =
            crate::host_auth::allows_host_subscription_auth(&admitted.auth_env, admitted.auth_mode);
        let adapter_id = admitted.adapter.id();
        let runtime_name = adapter_id.as_str();
        let ctx = build_launch_context(
            binary,
            &runtime_dir,
            &tmpdir,
            &session_id,
            opts.resume_runtime_session_id,
            runtime_name,
            allow_host_auth,
        )?;
        let (plan, session) = admitted
            .adapter
            .plan_session(&spec, &ctx)
            .map_err(|_| ApiError::new(ErrorCode::Unavailable, "runtime rejected session spec"))?;

        let (launch_env, oauth_expires_at_ms, oauth_token_fp) = assemble_launch_env(
            admitted.auth_env,
            admitted.endpoint_env,
            runtime_name,
            allow_host_auth,
            &ctx.runtime_path,
            &plan,
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
            bwrap_masked_siblings,
            connection_id: admitted.connection_id,
            route_digest: admitted.route_digest_suffix,
            oauth_expires_at_ms,
            oauth_token_fp,
            oauth_refresh_attempted: opts.oauth_refresh_attempted,
            workspace_arg: params.workspace.clone(),
            permit: permit_guard.take(),
            task_ref: opts.task_ref.map(str::to_owned),
            tried_connections: opts.tried_connections,
            home,
            workspace_real,
            runtime_dir,
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
            bwrap_masked_siblings,
            connection_id,
            route_digest,
            oauth_expires_at_ms,
            oauth_token_fp,
            oauth_refresh_attempted,
            workspace_arg,
            permit,
            task_ref,
            tried_connections,
            home,
            workspace_real,
            runtime_dir,
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
        let binding = self
            .bindings
            .issue(&emp_id, &run_id, &session_id, 0)
            .map_err(|_| ApiError::new(ErrorCode::Unavailable, "could not mint session token"))?;
        let binding_file = runtime_dir.join(format!("session-binding-{run_id}.v1"));
        let mut binding_guard = BindingGuard::new(&self.bindings, &emp_id, &run_id, &binding_file);
        write_owner_only_binding_file(&binding_file, binding.expose_token())?;
        let env = self.build_environment(SessionEnvInput {
            home: &home,
            workspace: &workspace_real,
            tmpdir: &tmpdir,
            employee_id: &emp_id,
            employee_name: &employee_name,
            employee_handle: &employee_handle,
            session_id: &session_id,
            token: binding.expose_token(),
            binding_file: &binding_file,
            sandbox_enabled: preset_cfg.sandbox.enabled,
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

        crate::bwrap_isolation::verify_bwrap_masked_before_spawn(
            self,
            &emp_id,
            bwrap_masked_siblings.as_ref(),
        )?;
        let spawned = self.spawn_and_persist(
            launch.prepared,
            &emp_id,
            &run_id,
            &session_id,
            &mark_str,
            &objective,
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
            bwrap_masked_siblings,
            connection_id,
            route_digest,
            oauth_expires_at_ms,
            oauth_token_fp,
            oauth_refresh_attempted,
            workspace_arg,
            permit: permit_guard.take(),
            task_ref,
            tried_connections,
            home,
            tmpdir,
            binding_file,
            preset_cfg,
            spec_hash,
            objective,
            persisted_stalls,
            spawned,
            session,
            lifecycle: launch.lifecycle,
        })
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "spawn persistence records prepared process, identifiers, mark, objective, stalls, and runtime session"
    )]
    pub(super) fn spawn_and_persist(
        &self,
        prepared: org_supervise::PreparedSpawn,
        emp_id: &str,
        run_id: &str,
        session_id: &str,
        mark_str: &str,
        objective: &str,
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
}

pub(super) fn ensure_live_handle_not_recovery_required(
    state: &SessionRuntimeState,
) -> Result<(), ApiError> {
    if state.recovery_required || state.state == SessionState::RecoveryRequired {
        return Err(ApiError::new(
            ErrorCode::Conflict,
            "agent session requires manual recovery before starting",
        ));
    }
    Ok(())
}

pub(super) fn is_bwrap_topology_stale(
    office: &Office,
    emp_id: &str,
    handle: &LiveSessionHandle,
) -> bool {
    handle.bwrap_masked_siblings.as_ref().is_some_and(|masked| {
        crate::bwrap_isolation::has_unmasked_bwrap_sibling_in_office(office, emp_id, masked)
    })
}

pub(super) fn ensure_warm_bwrap_not_stale(bwrap_topology_stale: bool) -> Result<(), ApiError> {
    if bwrap_topology_stale {
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

pub(super) fn build_initial_runtime_state(
    office_id: String,
    stall_timeout: Duration,
    prepared: &PreparedSessionLaunch,
) -> SessionRuntimeState {
    SessionRuntimeState {
        employee_id: prepared.emp_id.clone(),
        office_id,
        state: SessionState::Starting,
        run_id: Some(prepared.run_id.clone()),
        session_id: Some(prepared.session_id.clone()),
        runtime_id: Some(prepared.runtime_id.clone()),
        runtime_session_id: prepared
            .lifecycle
            .runtime_session_id
            .as_ref()
            .map(|s| s.as_str().to_owned()),
        private_home: prepared.home.display().to_string(),
        permission_preset: prepared.preset_cfg.preset.as_str().to_owned(),
        sandbox_enabled: prepared.preset_cfg.sandbox.enabled,
        spec_hash: prepared.spec_hash.clone(),
        watchdog: StallWatchdog::from_persisted(
            stall_timeout,
            Instant::now(),
            prepared.persisted_stalls,
            false,
        ),
        recovery_required: false,
        last_reply: None,
        last_error: None,
        backgrounds: BackgroundRegistry::new(),
        pending_approvals: Vec::new(),
        policy: prepared.preset_cfg.policy.clone(),
        preset: prepared.preset_cfg.preset,
        allowed_tools: prepared.preset_cfg.allowed_tools.clone(),
        writable_roots: effective_approval_writable_roots(prepared),
        suspended_on_asks: Vec::new(),
        waiting_for_member_id: None,
        waiting_for_name: None,
        suspended_since: None,
        pending_wake_count: 0,
        pending_wake_reason: None,
    }
}

pub(super) fn effective_approval_writable_roots(prepared: &PreparedSessionLaunch) -> Vec<SpecPath> {
    effective_approval_writable_roots_from(
        prepared.preset_cfg.preset,
        &prepared.preset_cfg.sandbox.writable_roots,
        &prepared.tmpdir,
    )
}

pub(crate) fn effective_approval_writable_roots_from(
    preset: PermissionPreset,
    sandbox_writable_roots: &[SpecPath],
    tmpdir: &Path,
) -> Vec<SpecPath> {
    let mut roots = sandbox_writable_roots.to_vec();
    if preset == PermissionPreset::ReadOnly {
        return roots;
    }
    let canon_tmp = canonicalize_with_tail(tmpdir);
    if let Some(spec) = canon_tmp
        .to_str()
        .and_then(|s| SpecPath::try_from(s).ok())
        .filter(|spec| !roots.contains(spec))
    {
        roots.push(spec);
    }
    roots
}
