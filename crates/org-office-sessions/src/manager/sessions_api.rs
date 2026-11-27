//! Session start, inspection, listing, transcript reads, grants, and approval decisions.

use org_agent_runtime::Decision;
use org_office::organization_family::records::Employee;
use org_proto::sessions::{
    MemberSessionListParams, MemberSessionStartParams, MemberSessionTranscriptParams,
    SessionGrantsResult, SessionInspectResult, SessionListResult, SessionState,
    SessionTranscriptResult, SessionView,
};
use org_proto::{ApiError, ErrorCode};

use crate::credentials::rfc3339_now;
use crate::partition::PartitionKey;
use crate::preset::build_grants_summary;
use crate::pump::PumpCommand;

use super::affinity::{validate_start_affinity_params, WarmReuseControl};
use super::lifecycle_ops::reap_replaced_handle;
use super::spec::canonical_or_spec;
use super::views::overlay_durable_wake_state;
use super::warm::LaunchRouteOpts;
use super::{next_entropy_hex, SessionManager, StartSlotGuard};

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
        let control = validate_start_affinity_params(params)?;
        let employee = self.require_active_agent(&params.employee_id)?;
        let emp_id = employee.id.as_str();
        self.check_not_recovery_required(emp_id)?;
        self.clear_persisted_stall_only(emp_id);

        // The Chief of Staff answers each caller in their own partition (`TASK-80.1`).
        let partition = Self::turn_partition(&employee, Some(actor_member_id));
        // Composed once, for a live session and a new one alike: an owner instruction the HQ
        // proxied (`hq_attested_owner`) is badged `owner` either way.
        let objective = params.instruction.as_deref().map(|text| {
            self.objective_text_attested(&employee, actor_member_id, hq_attested_owner, text)
        });
        let reservation = self.reserve_start_or_send_with_control(
            &employee,
            objective.clone(),
            Vec::new(),
            None,
            params.workspace.as_deref(),
            partition.as_ref(),
            control,
        )?;
        if let Some(view) = reservation.live_view {
            return Ok(view);
        }
        let slot = StartSlotGuard::new(&self.registry, emp_id.to_owned());
        reap_replaced_handle(self, emp_id, reservation.finished_handle)?;
        self.check_not_recovery_required(emp_id)?;
        self.clear_persisted_stall_only(emp_id);
        let tokens = self.bindings.active_tokens();
        let refs: Vec<&str> = tokens.iter().map(String::as_str).collect();
        let turn = reservation.partition_turn.as_ref();
        let _ = self.store.append_history(
            emp_id,
            &turn_trigger_entry(actor_member_id, &params.command_id, turn),
            &refs,
        );
        let explicit_resume = params.resume_runtime_session_id.as_deref();
        let resume = if params.fresh {
            None
        } else if partition.is_some() {
            turn.and_then(|turn| turn.resume_runtime_session_id.as_deref())
        } else {
            explicit_resume.or_else(|| turn.and_then(|t| t.resume_runtime_session_id.as_deref()))
        };
        let opts = LaunchRouteOpts::fresh(resume, None, reservation.start_stop_epoch, partition)
            .with_affinity(params.fresh, explicit_resume.is_some());
        let prepared = self.prepare_and_spawn_raw(
            &employee,
            params,
            objective.unwrap_or_default(),
            Vec::new(),
            opts,
        )?;
        self.install_live_session(prepared, slot)
    }

    /// Starts or resumes a supervised session autonomously (C6 required-ask resume, C8 peer wake,
    /// C10 always-available wake). Uses the exact same runtime, binding, and sandbox path as `Run`.
    /// `objective` is the complete turn input: queued wakes compose it, task process deltas
    /// included, through `lifecycle::wake_turn`; `task_ref` only scopes route admission and the
    /// turn permit.
    ///
    /// The Chief of Staff's autonomous turns run in `wake_partition`, the one partition the
    /// claimed wakes belong to ([`Self::wake_partition`]), or in `tess:wake`, acting for nobody,
    /// when it is `None` (`TASK-80.1`): they resume only that partition's own conversation,
    /// never the one `resume_runtime_session_id` names (the last run's or a suspended run's,
    /// which may be another caller's), and never reuse a process serving another partition.
    /// Anyone else ignores `wake_partition`.
    #[cfg(test)]
    pub(crate) fn spawn_or_send_autonomous(
        &self,
        employee: &Employee,
        objective: String,
        resume_runtime_session_id: Option<&str>,
        task_ref: Option<&str>,
        trigger_entry: serde_json::Value,
        wake_partition: Option<PartitionKey>,
    ) -> Result<SessionView, ApiError> {
        self.spawn_or_send_autonomous_with_attachments(
            employee,
            objective,
            Vec::new(),
            resume_runtime_session_id,
            task_ref,
            trigger_entry,
            wake_partition,
        )
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "autonomous session launch threads employee, objective, lightweight attachment ids, resume id, task ref, trigger, and partition"
    )]
    pub(crate) fn spawn_or_send_autonomous_with_attachments(
        &self,
        employee: &Employee,
        objective: String,
        attachment_ids: Vec<String>,
        resume_runtime_session_id: Option<&str>,
        task_ref: Option<&str>,
        trigger_entry: serde_json::Value,
        wake_partition: Option<PartitionKey>,
    ) -> Result<SessionView, ApiError> {
        self.spawn_or_send_autonomous_with_affinity(
            employee,
            objective,
            attachment_ids,
            resume_runtime_session_id,
            false,
            false,
            task_ref,
            trigger_entry,
            wake_partition,
        )
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "autonomous session launch with review affinity threads employee, turn payload, resume/fresh controls, task ref, trigger, and partition"
    )]
    pub(crate) fn spawn_or_send_autonomous_with_affinity(
        &self,
        employee: &Employee,
        objective: String,
        attachment_ids: Vec<String>,
        resume_runtime_session_id: Option<&str>,
        force_fresh: bool,
        enforce_affinity_guard: bool,
        task_ref: Option<&str>,
        trigger_entry: serde_json::Value,
        wake_partition: Option<PartitionKey>,
    ) -> Result<SessionView, ApiError> {
        let emp_id = employee.id.as_str();
        self.check_not_blocked(emp_id)?;
        let partition =
            Self::turn_partition(employee, None).map(|wake| wake_partition.unwrap_or(wake));
        let params = MemberSessionStartParams {
            command_id: format!("auto-{}", next_entropy_hex(8)?),
            employee_id: emp_id.to_owned(),
            instruction: Some(objective.clone()),
            workspace: None,
            resume_runtime_session_id: (!force_fresh)
                .then(|| resume_runtime_session_id.map(str::to_owned))
                .flatten(),
            fresh: force_fresh,
        };
        let control = if force_fresh {
            WarmReuseControl::ForceFresh
        } else if enforce_affinity_guard {
            resume_runtime_session_id
                .map(WarmReuseControl::RequireSession)
                .unwrap_or(WarmReuseControl::Default)
        } else {
            WarmReuseControl::Default
        };
        let reservation = self.reserve_start_or_send_with_control(
            employee,
            Some(objective.clone()),
            attachment_ids.clone(),
            task_ref,
            None,
            partition.as_ref(),
            control,
        )?;
        if let Some(view) = reservation.live_view {
            let tokens = self.bindings.active_tokens();
            let refs: Vec<&str> = tokens.iter().map(String::as_str).collect();
            let _ = self.store.append_history(emp_id, &trigger_entry, &refs);
            return Ok(view);
        }
        let slot = StartSlotGuard::new(&self.registry, emp_id.to_owned());
        reap_replaced_handle(self, emp_id, reservation.finished_handle)?;
        self.check_not_blocked(emp_id)?;
        let tokens = self.bindings.active_tokens();
        let refs: Vec<&str> = tokens.iter().map(String::as_str).collect();
        let _ = self.store.append_history(emp_id, &trigger_entry, &refs);
        let resume = if force_fresh {
            None
        } else {
            autonomous_resume(
                reservation.partition_turn.as_ref(),
                partition.is_some(),
                resume_runtime_session_id,
            )
        };
        let opts =
            LaunchRouteOpts::fresh(resume, task_ref, reservation.start_stop_epoch, partition)
                .with_affinity(force_fresh, enforce_affinity_guard);
        let prepared =
            self.prepare_and_spawn_raw(employee, &params, objective, attachment_ids, opts)?;
        self.install_live_session(prepared, slot)
    }

    /// Returns an agent's current live or last recorded `runtime_session_id`, used by
    /// `ReviewService` to stamp session affinity when a reviewer commits a verdict (`TASK-92.1`).
    pub fn current_runtime_session_id(&self, employee_id: &str) -> Option<String> {
        {
            let reg = self.registry.lock().unwrap_or_else(|p| p.into_inner());
            if let Some(handle) = reg.live.get(employee_id) {
                let guard = handle.state.lock().unwrap_or_else(|p| p.into_inner());
                if let Some(rsid) = guard.runtime_session_id.clone() {
                    return Some(rsid);
                }
            }
        }
        self.store
            .read_active_turn(employee_id)
            .ok()
            .flatten()
            .and_then(|r| r.runtime_session_id)
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
            self.authorize_chief_of_staff_target_inner(caller_id, hq_attested_owner, target)?;
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
            self.authorize_chief_of_staff_target_inner(caller_id, hq_attested_owner, target)?;
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
            &self.config.runtimes,
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
            self.authorize_chief_of_staff_target_inner(caller_id, hq_attested_owner, target)?;
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
            &self.config.runtimes,
            params.session_id.as_deref(),
            live_sid.as_deref(),
            query,
            &refs,
        )
        .map_err(|_| ApiError::new(ErrorCode::StorageError, "could not read session transcript"))?
        .ok_or_else(|| ApiError::new(ErrorCode::NotFound, "session transcript not found"))
    }

    pub(crate) fn active_redaction_secrets(
        &self,
        employee_id: &str,
    ) -> Vec<zeroize::Zeroizing<String>> {
        let mut secrets: Vec<zeroize::Zeroizing<String>> = self
            .bindings
            .active_tokens()
            .into_iter()
            .map(zeroize::Zeroizing::new)
            .collect();
        let Ok(Some(exec)) = self.office.load_agent_execution(employee_id) else {
            return secrets;
        };
        let Ok(Some(prof)) = self
            .office
            .load_model_profile(exec.model_profile_id.as_str())
        else {
            return secrets;
        };
        let conn_ids = prof
            .generated_for_connection_id
            .iter()
            .chain(prof.connection_ids.iter());
        for conn_id in conn_ids {
            if let Ok(Some(raw)) = self.enrollment.vault().load_unwrapped(conn_id.as_str()) {
                crate::host_auth::collect_vault_redaction_secrets(&raw, &mut secrets);
            }
        }
        secrets
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

/// The conversation an autonomous start resumes: for the Chief of Staff (`partitioned`) only her
/// partition's own, from the partition ledger, and never `requested` (the last run's or a
/// suspended run's, which may be a member's); for anyone else, `requested` (`TASK-80.1`).
pub(crate) fn autonomous_resume<'a>(
    partition_turn: Option<&'a crate::partition::PartitionTurn>,
    partitioned: bool,
    requested: Option<&'a str>,
) -> Option<&'a str> {
    if partitioned {
        return partition_turn.and_then(|turn| turn.resume_runtime_session_id.as_deref());
    }
    requested
}

/// The history entry of a person's `Run`: who asked, and for the Chief of Staff the partition
/// and the caller her turn acts for, so her turns are on record under her own employee ID with
/// the member they were for (`TASK-80.1`).
fn turn_trigger_entry(
    actor_member_id: &str,
    command_id: &str,
    partition: Option<&crate::partition::PartitionTurn>,
) -> serde_json::Value {
    let mut entry = serde_json::json!({
        "type": "turn_trigger",
        "kind": "run",
        "sender_id": actor_member_id,
        "source_id": command_id,
        "at": rfc3339_now(),
    });
    if let Some(turn) = partition {
        entry["acting_for_caller_id"] = serde_json::json!(actor_member_id);
        entry["partition"] = serde_json::json!(turn.key.as_str());
        entry["partition_turn"] = serde_json::json!(turn.turn);
    }
    entry
}
