//! Session start, inspection, listing, transcript reads, grants, and approval decisions.

use super::*;

impl SessionManager {
    /// Starts (or sends a direct turn to) an agent's supervised session (`AC #1`, `LIVE-9..13`).
    /// Per `ADR-0030` d12 (`LIVE-13`), a person's explicit `Run` clears a `stall_locked` state
    /// while still requiring explicit clearance for `recovery_required`.
    pub fn start_session(
        &self,
        actor_member_id: &str,
        params: &MemberSessionStartParams,
    ) -> Result<SessionView, ApiError> {
        self.start_session_inner(actor_member_id, false, params)
    }

    pub(super) fn start_session_inner(
        &self,
        actor_member_id: &str,
        hq_attested_owner: bool,
        params: &MemberSessionStartParams,
    ) -> Result<SessionView, ApiError> {
        self.authorize_owner_or_person_inner(actor_member_id, hq_attested_owner)?;
        let employee = self.require_active_agent(&params.employee_id)?;
        let emp_id = employee.id.as_str();
        self.check_not_recovery_required(emp_id)?;
        self.clear_persisted_stall_only(emp_id);

        let initial_obj = params
            .instruction
            .as_deref()
            .map(|text| build_objective_text(&employee, actor_member_id, text));
        let (live_view, finished_handle) =
            self.reserve_start_or_send(&employee, initial_obj, None, params.workspace.as_deref())?;
        if let Some(view) = live_view {
            return Ok(view);
        }
        let _guard = StartSlotGuard::new(&self.registry, emp_id.to_owned());
        reap_replaced_handle(self, emp_id, finished_handle)?;
        self.check_not_recovery_required(emp_id)?;
        self.clear_persisted_stall_only(emp_id);
        let tokens = self.bindings.active_tokens();
        let refs: Vec<&str> = tokens.iter().map(String::as_str).collect();
        let _ = self.store.append_history(
            emp_id,
            &serde_json::json!({
                "type": "turn_trigger",
                "kind": "run",
                "sender_id": actor_member_id,
                "source_id": params.command_id,
                "at": rfc3339_now(),
            }),
            &refs,
        );
        let prepared = self.prepare_and_spawn(actor_member_id, &employee, params, None)?;
        self.install_live_session(prepared)
    }

    /// Starts or resumes a supervised session autonomously (C6 required-ask resume, C8 peer wake,
    /// C10 always-available wake). Uses the exact same runtime, binding, and sandbox path as `Run`.
    pub(crate) fn spawn_or_send_autonomous(
        &self,
        employee: &Employee,
        objective: String,
        resume_runtime_session_id: Option<&str>,
        task_ref: Option<&str>,
        trigger_entry: serde_json::Value,
    ) -> Result<SessionView, ApiError> {
        let emp_id = employee.id.as_str();
        self.check_not_blocked(emp_id)?;
        let params = MemberSessionStartParams {
            command_id: format!("auto-{}", next_entropy_hex(8)?),
            employee_id: emp_id.to_owned(),
            instruction: Some(objective.clone()),
            workspace: None,
        };
        let (live_view, finished_handle) =
            self.reserve_start_or_send(employee, Some(objective.clone()), task_ref, None)?;
        if let Some(view) = live_view {
            let tokens = self.bindings.active_tokens();
            let refs: Vec<&str> = tokens.iter().map(String::as_str).collect();
            let _ = self.store.append_history(emp_id, &trigger_entry, &refs);
            return Ok(view);
        }
        let _guard = StartSlotGuard::new(&self.registry, emp_id.to_owned());
        reap_replaced_handle(self, emp_id, finished_handle)?;
        self.check_not_blocked(emp_id)?;
        let tokens = self.bindings.active_tokens();
        let refs: Vec<&str> = tokens.iter().map(String::as_str).collect();
        let _ = self.store.append_history(emp_id, &trigger_entry, &refs);
        let prepared = self.prepare_and_spawn_raw(
            employee,
            &params,
            objective,
            resume_runtime_session_id,
            task_ref,
        )?;
        self.install_live_session(prepared)
    }

    /// Inspects an agent's live or last session (`CLI-9`). Agents may only inspect themselves.
    pub fn inspect_session(
        &self,
        caller_id: &str,
        employee_id: Option<&str>,
    ) -> Result<SessionInspectResult, ApiError> {
        self.inspect_session_inner(caller_id, false, employee_id)
    }

    pub(super) fn inspect_session_inner(
        &self,
        caller_id: &str,
        hq_attested_owner: bool,
        employee_id: Option<&str>,
    ) -> Result<SessionInspectResult, ApiError> {
        let target = employee_id.unwrap_or(caller_id);
        if target != caller_id {
            self.authorize_owner_or_person_inner(caller_id, hq_attested_owner)?;
        }
        let employee = self.require_agent(target)?;
        let accepted = self.bindings.accepted_commands(target).len() as u64;
        if let Some(handle) = self
            .registry
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .live
            .get(target)
        {
            let guard = handle.state.lock().unwrap_or_else(|p| p.into_inner());
            let mut view = guard.to_view();
            let spec_hash = guard.spec_hash.clone();
            drop(guard);
            overlay_durable_wake_state(&self.store, target, &mut view);
            return Ok(SessionInspectResult {
                session: view,
                accepted_commands_count: accepted,
                spec_hash,
            });
        }
        let view = self.idle_view_for(employee.id.as_str())?;
        Ok(SessionInspectResult {
            session: view,
            accepted_commands_count: accepted,
            spec_hash: String::new(),
        })
    }

    /// Lists discovered live and historical conversation sessions in an agent's private home
    /// (`REQ-05`, `FC-150`). Only authorized people/owner or the agent itself may inspect.
    pub fn list_sessions(
        &self,
        caller_id: &str,
        params: &MemberSessionListParams,
    ) -> Result<SessionListResult, ApiError> {
        self.list_sessions_inner(caller_id, false, params)
    }

    pub(super) fn list_sessions_inner(
        &self,
        caller_id: &str,
        hq_attested_owner: bool,
        params: &MemberSessionListParams,
    ) -> Result<SessionListResult, ApiError> {
        let target = params.employee_id.as_deref().unwrap_or(caller_id);
        if target != caller_id {
            self.authorize_owner_or_person_inner(caller_id, hq_attested_owner)?;
        }
        if params.limit == Some(0) {
            return Err(ApiError::new(
                ErrorCode::InvalidRequest,
                "limit must be greater than zero",
            ));
        }
        let employee = self.require_agent(target)?;
        let home = self.office.agent_private_home_path(employee.id.as_str());
        let live_sid = self.live_runtime_session_id(employee.id.as_str());
        let secrets = self.active_redaction_secrets(employee.id.as_str());
        let refs: Vec<&str> = secrets.iter().map(|s| s.as_str()).collect();
        Ok(crate::transcript::list_agent_sessions(
            employee.id.as_str(),
            &home,
            live_sid.as_deref(),
            params.limit,
            &refs,
        ))
    }

    /// Reads, parses, nests sub-agents (`SUB-6`), filters, and redacts a conversation transcript
    /// on the office (`REQ-05`, `FC-150`).
    pub fn read_session_transcript(
        &self,
        caller_id: &str,
        params: &MemberSessionTranscriptParams,
    ) -> Result<SessionTranscriptResult, ApiError> {
        self.read_session_transcript_inner(caller_id, false, params)
    }

    pub(super) fn read_session_transcript_inner(
        &self,
        caller_id: &str,
        hq_attested_owner: bool,
        params: &MemberSessionTranscriptParams,
    ) -> Result<SessionTranscriptResult, ApiError> {
        let target = params.employee_id.as_deref().unwrap_or(caller_id);
        if target != caller_id {
            self.authorize_owner_or_person_inner(caller_id, hq_attested_owner)?;
        }
        if params.limit == Some(0) {
            return Err(ApiError::new(
                ErrorCode::InvalidRequest,
                "limit must be greater than zero",
            ));
        }
        if let Some(ref sid) = params.session_id {
            if !crate::transcript::is_safe_session_id(sid) {
                return Err(ApiError::new(
                    ErrorCode::InvalidRequest,
                    "invalid session_id",
                ));
            }
        }
        let employee = self.require_agent(target)?;
        let home = self.office.agent_private_home_path(employee.id.as_str());
        let live_sid = self.live_runtime_session_id(employee.id.as_str());
        let query = crate::transcript::TranscriptQuery {
            include_tool_calls: params.include_tool_calls.unwrap_or(true),
            include_file_changes: params.include_file_changes.unwrap_or(true),
            include_subagents: params.include_subagents.unwrap_or(true),
            cursor: params.cursor.unwrap_or(0),
            limit: params
                .limit
                .map(|n| n as usize)
                .unwrap_or(crate::transcript::DEFAULT_TRANSCRIPT_LIMIT),
        };
        let secrets = self.active_redaction_secrets(employee.id.as_str());
        let refs: Vec<&str> = secrets.iter().map(|s| s.as_str()).collect();
        crate::transcript::load_agent_transcript(
            employee.id.as_str(),
            &home,
            params.session_id.as_deref(),
            live_sid.as_deref(),
            query,
            &refs,
        )
        .map_err(|_| ApiError::new(ErrorCode::StorageError, "could not read session transcript"))?
        .ok_or_else(|| ApiError::new(ErrorCode::NotFound, "session transcript not found"))
    }

    pub(super) fn active_redaction_secrets(
        &self,
        employee_id: &str,
    ) -> Vec<zeroize::Zeroizing<String>> {
        let mut secrets: Vec<zeroize::Zeroizing<String>> = self
            .bindings
            .active_tokens()
            .into_iter()
            .map(zeroize::Zeroizing::new)
            .collect();
        let Ok(exec_opt) = self.office.load_agent_execution(employee_id) else {
            return secrets;
        };
        let Some(exec) = exec_opt else {
            // Unconfigured agents fall back to Claude host subscription auth (`router.rs`),
            // so redact host Claude OAuth if present.
            if let Some(tok) = crate::host_auth::resolve_host_claude_oauth_token() {
                secrets.push(tok);
            }
            return secrets;
        };
        let Ok(Some(prof)) = self
            .office
            .load_model_profile(exec.model_profile_id.as_str())
        else {
            return secrets;
        };
        let mut uses_claude_host_subscription = false;
        let conn_ids = prof
            .generated_for_connection_id
            .iter()
            .chain(prof.connection_ids.iter());
        for conn_id in conn_ids {
            let mut has_vault_secret = false;
            if let Ok(Some(raw)) = self.enrollment.vault().load_unwrapped(conn_id.as_str()) {
                if let Ok(text) = std::str::from_utf8(&raw) {
                    has_vault_secret = true;
                    secrets.push(zeroize::Zeroizing::new(text.to_owned()));
                }
            }
            if !has_vault_secret && self.is_claude_subscription_connection(conn_id.as_str()) {
                uses_claude_host_subscription = true;
            }
        }
        if uses_claude_host_subscription {
            if let Some(tok) = crate::host_auth::resolve_host_claude_oauth_token() {
                secrets.push(tok);
            }
        }
        secrets
    }

    pub(super) fn is_claude_subscription_connection(&self, connection_id: &str) -> bool {
        let Ok(Some(conn)) = self.office.load_provider_connection(connection_id) else {
            return false;
        };
        conn.auth_mode
            == org_office::organization_family::records::ProviderAuthMode::SubscriptionCli
            && matches!(
                conn.runtime_driver.trim().to_ascii_lowercase().as_str(),
                "anthropic" | "claude" | "claude-code"
            )
    }

    pub(super) fn live_runtime_session_id(&self, employee_id: &str) -> Option<String> {
        let reg = self.registry.lock().unwrap_or_else(|p| p.into_inner());
        let handle = reg.live.get(employee_id)?;
        let guard = handle.state.lock().unwrap_or_else(|p| p.into_inner());
        if !matches!(
            guard.state,
            SessionState::Starting | SessionState::Running | SessionState::WaitingApproval
        ) {
            return None;
        }
        guard
            .runtime_session_id
            .clone()
            .or_else(|| guard.session_id.clone())
    }

    /// Lists effective permission grants & sandbox constraints (`CLI-10`). Agents may only read self.
    pub fn list_grants(
        &self,
        caller_id: &str,
        employee_id: Option<&str>,
    ) -> Result<SessionGrantsResult, ApiError> {
        self.list_grants_inner(caller_id, false, employee_id)
    }

    pub(super) fn list_grants_inner(
        &self,
        caller_id: &str,
        hq_attested_owner: bool,
        employee_id: Option<&str>,
    ) -> Result<SessionGrantsResult, ApiError> {
        let target = employee_id.unwrap_or(caller_id);
        if target != caller_id {
            self.authorize_owner_or_person_inner(caller_id, hq_attested_owner)?;
        }
        let employee = self.require_agent(target)?;
        let home = self.office.agent_private_home_path(employee.id.as_str());
        let workspace = canonical_or_spec(&home)?;
        let (preset, sandbox_enabled) = self.load_preset_and_sandbox(employee.id.as_str())?;
        let cfg = self.build_employee_preset_cfg(
            employee.id.as_str(),
            preset,
            sandbox_enabled,
            &workspace,
            false,
        )?;
        Ok(build_grants_summary(employee.id.as_str(), &cfg, &[]))
    }

    pub(super) fn build_employee_preset_cfg(
        &self,
        employee_id: &str,
        preset: PermissionPreset,
        sandbox_enabled: bool,
        workspace: &SpecPath,
        create_root: bool,
    ) -> Result<PresetConfig, ApiError> {
        let socket_str = self.config.office_socket.display().to_string();
        let socket_spec = SpecPath::try_from(socket_str.as_str()).ok();
        let denied_globs = self.denied_office_globs_for(Some(employee_id));
        let mut cfg = build_session_policy(
            preset,
            sandbox_enabled,
            workspace,
            socket_spec.as_ref(),
            &denied_globs,
            1,
        )
        .map_err(|_| ApiError::new(ErrorCode::InvalidRequest, "invalid permission preset"))?;
        let (wt_writable, wt_write_denies) = self.worktree_sandbox_grants(employee_id, create_root);
        apply_worktree_sandbox_rules(&mut cfg, &wt_writable, &wt_write_denies);
        Ok(cfg)
    }

    /// Owner approves or denies a pending escalation (`allow_once`, `allow_always`, `deny`).
    pub fn decide_approval(
        &self,
        actor_member_id: &str,
        employee_id: &str,
        approval_id: &str,
        decision_str: &str,
        reason: Option<String>,
    ) -> Result<SessionView, ApiError> {
        self.decide_approval_inner(
            actor_member_id,
            false,
            employee_id,
            approval_id,
            decision_str,
            reason,
        )
    }

    pub(super) fn decide_approval_inner(
        &self,
        actor_member_id: &str,
        hq_attested_owner: bool,
        employee_id: &str,
        approval_id: &str,
        decision_str: &str,
        reason: Option<String>,
    ) -> Result<SessionView, ApiError> {
        self.authorize_owner_only_inner(actor_member_id, hq_attested_owner)?;
        let decision = parse_decision(decision_str, reason)?;
        let reg = self.registry.lock().unwrap_or_else(|p| p.into_inner());
        let Some(handle) = reg.live.get(employee_id) else {
            return Err(ApiError::new(
                ErrorCode::NotFound,
                "no live session for this agent",
            ));
        };
        let guard = handle.state.lock().unwrap_or_else(|p| p.into_inner());
        let pending = guard
            .pending_approvals
            .iter()
            .find(|p| p.approval_id == approval_id)
            .cloned()
            .ok_or_else(|| ApiError::new(ErrorCode::NotFound, "approval request not found"))?;
        if decision_str == "allow_always" && pending.escalation != "none" {
            return Err(ApiError::new(
                ErrorCode::InvalidRequest,
                "allow_always is forbidden for escalated approval requests; use allow_once or deny",
            ));
        }
        let allow_always_tool = (decision_str == "allow_always" && pending.escalation == "none")
            .then_some(pending.tool);
        let view = guard.to_view();
        drop(guard);
        let _ = handle.tx.send(PumpCommand::AnswerApproval {
            approval_id: approval_id.to_owned(),
            decision,
            allow_always_tool,
        });
        Ok(view)
    }

    pub(super) fn idle_view_for(&self, employee_id: &str) -> Result<SessionView, ApiError> {
        if let Some(mut last) = self
            .last_states
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .get(employee_id)
            .cloned()
        {
            overlay_durable_wake_state(&self.store, employee_id, &mut last);
            return Ok(last);
        }
        let (preset, sandbox_enabled) = self
            .load_preset_and_sandbox(employee_id)
            .unwrap_or((PermissionPreset::SafeAsk, true));
        Ok(build_idle_session_view(
            &self.store,
            employee_id,
            &self.office.info().office_id,
            self.office.agent_private_home_path(employee_id),
            preset,
            sandbox_enabled,
        ))
    }
}

pub(super) fn overlay_durable_wake_state(
    store: &SessionStore,
    employee_id: &str,
    view: &mut SessionView,
) {
    let suspended = store.read_suspended_state(employee_id).ok().flatten();
    let pending_wakes = store.read_pending_wakes(employee_id).ok().flatten();
    let blocked = view.stall_locked || view.recovery_required;
    if let Some(ref susp) = suspended {
        if !blocked
            && !matches!(
                view.state,
                SessionState::Starting | SessionState::Running | SessionState::WaitingApproval
            )
        {
            view.state = SessionState::Suspended;
        }
        let (w_id, w_name) = susp.primary_waiting_target();
        view.suspended_on_asks = susp.asks.iter().map(|a| a.ask_id.clone()).collect();
        view.waiting_for_member_id = w_id;
        view.waiting_for_name = w_name;
        view.suspended_since = Some(susp.suspended_since.clone());
        if view.runtime_session_id.is_none() {
            view.runtime_session_id = susp.runtime_session_id.clone();
        }
    } else {
        view.suspended_on_asks.clear();
        view.waiting_for_member_id = None;
        view.waiting_for_name = None;
        view.suspended_since = None;
    }
    let (count, reason) = extract_pending_wake_fields(pending_wakes.as_ref(), suspended.as_ref());
    view.pending_wake_count = count;
    view.pending_wake_reason = reason;
}

pub(super) fn build_idle_session_view(
    store: &SessionStore,
    employee_id: &str,
    office_id: &str,
    private_home: PathBuf,
    preset: PermissionPreset,
    sandbox_enabled: bool,
) -> SessionView {
    let suspended = store.read_suspended_state(employee_id).ok().flatten();
    let pending_wakes = store.read_pending_wakes(employee_id).ok().flatten();
    let persisted = store.read_active_turn(employee_id).ok().flatten();
    let (mut state, consecutive_stalls, stall_locked, recovery_required, run_id, session_id, rsid) =
        extract_persisted_turn(persisted);
    let (suspended_on_asks, waiting_for_member_id, waiting_for_name, suspended_since) =
        extract_suspended_fields(
            suspended.as_ref(),
            stall_locked || recovery_required,
            &mut state,
        );
    let (pending_wake_count, pending_wake_reason) =
        extract_pending_wake_fields(pending_wakes.as_ref(), suspended.as_ref());
    SessionView {
        employee_id: employee_id.to_owned(),
        office_id: office_id.to_owned(),
        state,
        run_id,
        session_id,
        runtime_id: None,
        runtime_session_id: rsid.or_else(|| suspended.and_then(|s| s.runtime_session_id)),
        private_home: private_home.display().to_string(),
        same_user_isolation_notice: SAME_USER_ISOLATION_NOTICE.to_owned(),
        permission_preset: preset.as_str().to_owned(),
        sandbox_enabled,
        consecutive_stalls,
        stall_locked,
        recovery_required,
        last_reply: None,
        last_error: None,
        background_tasks: Vec::new(),
        pending_approvals: Vec::new(),
        usage: Default::default(),
        suspended_on_asks,
        waiting_for_member_id,
        waiting_for_name,
        suspended_since,
        pending_wake_count,
        pending_wake_reason,
    }
}

pub(super) fn extract_persisted_turn(
    persisted: Option<ActiveTurnRecord>,
) -> (
    SessionState,
    u32,
    bool,
    bool,
    Option<String>,
    Option<String>,
    Option<String>,
) {
    let Some(rec) = persisted else {
        return (SessionState::Idle, 0, false, false, None, None, None);
    };
    let state = if rec.stall_locked {
        SessionState::StallLocked
    } else if rec.recovery_required {
        SessionState::RecoveryRequired
    } else {
        SessionState::Idle
    };
    (
        state,
        rec.consecutive_stalls,
        rec.stall_locked,
        rec.recovery_required,
        Some(rec.run_id),
        Some(rec.session_id),
        rec.runtime_session_id,
    )
}

pub(super) fn extract_suspended_fields(
    suspended: Option<&crate::wake::SuspendedStateRecord>,
    blocked: bool,
    state: &mut SessionState,
) -> (Vec<String>, Option<String>, Option<String>, Option<String>) {
    let Some(susp) = suspended else {
        return (Vec::new(), None, None, None);
    };
    if !blocked {
        *state = SessionState::Suspended;
    }
    let (w_id, w_name) = susp.primary_waiting_target();
    (
        susp.asks.iter().map(|a| a.ask_id.clone()).collect(),
        w_id,
        w_name,
        Some(susp.suspended_since.clone()),
    )
}

pub(super) fn extract_pending_wake_fields(
    pending_wakes: Option<&crate::wake::PendingWakeQueueRecord>,
    suspended: Option<&crate::wake::SuspendedStateRecord>,
) -> (u32, Option<String>) {
    match pending_wakes {
        Some(q) if !q.items.is_empty() => (
            q.items.len() as u32,
            q.hold_reason.map(|r| r.as_str().to_owned()),
        ),
        _ => (
            0,
            suspended.and_then(|s| s.hold_reason.map(|r| r.as_str().to_owned())),
        ),
    }
}

pub(super) fn parse_decision(
    decision_str: &str,
    reason: Option<String>,
) -> Result<Decision, ApiError> {
    match decision_str {
        "allow_once" | "allow_always" => Ok(Decision::AllowOnce),
        "deny" => Ok(Decision::Deny {
            message: reason.or_else(|| Some("Denied by organization owner.".to_owned())),
            stop_turn: false,
        }),
        _ => Err(ApiError::new(
            ErrorCode::InvalidRequest,
            "decision must be `allow_once`, `allow_always`, or `deny`",
        )),
    }
}
