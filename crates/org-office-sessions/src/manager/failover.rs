//! Pool failover relaunch, host Claude subscription refresh, and recovery wakes.

use super::*;

impl SessionManager {
    /// Handles §7 connection failover after a pre-side-effect `rate_limited`, `quota_exhausted`,
    /// `auth_expired`, or `model_unavailable` turn failure (`doc-88` §7).
    pub(crate) fn failover_relaunch(&self, req: FailoverRequest) {
        run_failover_relaunch(self, req);
    }
}

pub(crate) fn is_host_claude_subscription_refresh_eligible(
    mgr: &SessionManager,
    connection_id: &str,
) -> Option<PathBuf> {
    let conn = mgr.office.load_provider_connection(connection_id).ok()??;
    if conn.auth_mode != org_office::organization_family::records::ProviderAuthMode::SubscriptionCli
    {
        return None;
    }
    let adapter = mgr.config.runtimes.resolve(&conn.runtime_driver).ok()?;
    if adapter.id().as_str() != "claude" {
        return None;
    }
    if mgr
        .enrollment
        .vault()
        .load_unwrapped(conn.id.as_str())
        .ok()
        .flatten()
        .is_some()
    {
        return None;
    }
    mgr.config
        .runtimes
        .resolve_binary(adapter.as_ref(), None)
        .ok()
}

pub(super) fn assemble_launch_env(
    mut launch_env: Vec<(OsString, OsString)>,
    endpoint_env: Vec<(OsString, OsString)>,
    runtime_name: &str,
    allow_host_auth: bool,
    runtime_path: &Path,
    plan: &org_agent_runtime::LaunchPlan,
) -> (Vec<(OsString, OsString)>, Option<u64>, Option<String>) {
    let host_oauth = crate::host_auth::append_host_claude_oauth(
        runtime_name,
        allow_host_auth,
        runtime_path,
        plan,
        &mut launch_env,
    );
    let expires_at_ms = host_oauth.as_ref().and_then(|m| m.expires_at_ms);
    let token_fp = host_oauth.map(|m| m.token_fp);
    launch_env.extend(endpoint_env);
    (launch_env, expires_at_ms, token_fp)
}

pub(super) fn try_refresh_host_claude_subscription(
    mgr: &SessionManager,
    req: &FailoverRequest,
) -> bool {
    if req.reason != "auth_expired" || req.oauth_refresh_attempted {
        return false;
    }
    let Some(binary) = is_host_claude_subscription_refresh_eligible(mgr, &req.failed_connection_id)
    else {
        return false;
    };
    crate::host_auth::refresh_host_claude_oauth_reactive(&binary, req.oauth_token_fp.as_deref())
}

pub(super) fn collect_non_self_admin_names(
    wt_dir: &Path,
    repo_id: Option<&str>,
    git_state: &org_office::git::OfficeGitState,
    self_admins: &std::collections::BTreeSet<String>,
) -> std::collections::BTreeSet<String> {
    let mut other_admins = std::collections::BTreeSet::new();
    if let Ok(entries) = std::fs::read_dir(wt_dir) {
        for name in entries
            .flatten()
            .filter_map(|e| e.file_name().into_string().ok())
            .filter(|n| !self_admins.contains(n))
        {
            other_admins.insert(name);
        }
    }
    if let Some(rid) = repo_id {
        for ws in git_state.workspaces.values() {
            if ws.repository_id == rid
                && is_safe_worktree_admin_name(&ws.admin_dir_name)
                && !self_admins.contains(&ws.admin_dir_name)
            {
                other_admins.insert(ws.admin_dir_name.clone());
            }
        }
    }
    other_admins
}

pub(super) fn record_failover_connection_health(mgr: &SessionManager, req: &FailoverRequest) {
    match req.reason {
        "rate_limited" | "quota_exhausted" => {
            let _ = mgr.office.record_connection_cooldown_internal(
                &req.failed_connection_id,
                None,
                req.rate_limit_resets_at.as_deref(),
                None,
            );
            crate::router::report_cooldown_any(
                &mgr.office,
                mgr.config.hq_permits.as_deref(),
                &req.failed_connection_id,
                req.rate_limit_resets_at.as_deref(),
                Some(req.reason),
            );
        }
        "auth_expired" => {
            let _ = mgr
                .office
                .mark_connection_auth_expired_internal(&req.failed_connection_id, req.reason);
        }
        _ => {}
    }
}

pub(super) fn take_failed_live_handle(
    mgr: &SessionManager,
    req: &FailoverRequest,
) -> Option<Option<LiveSessionHandle>> {
    let mut reg = mgr.registry.lock().unwrap_or_else(|p| p.into_inner());
    if reg.draining || reg.shutting_down || reg.starting.contains(&req.employee_id) {
        return None;
    }
    if let Some(existing) = reg.live.get(&req.employee_id) {
        let st = existing.state.lock().unwrap_or_else(|p| p.into_inner());
        if st.recovery_required
            || st.state == SessionState::RecoveryRequired
            || st.run_id.as_deref() != Some(req.failed_run_id.as_str())
        {
            return None;
        }
    }
    let old = reg.live.remove(&req.employee_id);
    reg.starting.insert(req.employee_id.clone());
    Some(old)
}

pub(crate) fn run_failover_relaunch(mgr: &SessionManager, req: FailoverRequest) {
    if is_admissions_closed(mgr) {
        return;
    }
    let is_stale_recycle = req.reason == "warm_route_stale";
    let refreshed_same_conn = !is_stale_recycle && try_refresh_host_claude_subscription(mgr, &req);
    if !is_stale_recycle && !refreshed_same_conn {
        record_failover_connection_health(mgr, &req);
    }
    let Some(dead_handle) = take_failed_live_handle(mgr, &req) else {
        if is_stale_recycle && !is_admissions_closed(mgr) {
            enqueue_recovery_wake(mgr, &req, None);
        }
        return;
    };
    let _guard = StartSlotGuard::new(&mgr.registry, req.employee_id.clone());
    if let Some(old) = dead_handle {
        if crate::bwrap_isolation::reap_live_handle(
            mgr,
            &req.employee_id,
            old,
            Some("pool_failover"),
            crate::bwrap_isolation::ReapMode::FailClosedRestore {
                conflict_message: format!(
                    "could not verify teardown of failed session `{}`; resolve recovery first",
                    req.employee_id
                ),
            },
        )
        .is_err()
        {
            return;
        }
    }
    let Ok(employee) = mgr.require_active_agent(&req.employee_id) else {
        return;
    };
    let Ok(cmd_suffix) = next_entropy_hex(8) else {
        return;
    };
    let params = MemberSessionStartParams {
        command_id: format!("failover-{cmd_suffix}"),
        employee_id: req.employee_id.clone(),
        instruction: Some(req.objective.clone()),
        workspace: req.workspace.clone(),
    };
    let mut excluded: HashSet<String> = if is_stale_recycle {
        HashSet::new()
    } else {
        req.tried_connections.iter().cloned().collect()
    };
    if refreshed_same_conn {
        excluded.remove(&req.failed_connection_id);
    }
    let opts = LaunchRouteOpts {
        resume_runtime_session_id: None,
        task_ref: req.task_ref.as_deref(),
        is_failover_switch: !is_stale_recycle,
        oauth_refresh_attempted: req.oauth_refresh_attempted || refreshed_same_conn,
        excluded_connections: excluded,
        tried_connections: if is_stale_recycle {
            Vec::new()
        } else {
            req.tried_connections.clone()
        },
    };
    let objective = mgr.prepend_watch_notices(employee.id.as_str(), req.objective.clone());
    let launch_res = mgr
        .plan_session_launch(&employee, &params, opts)
        .and_then(|planned| mgr.issue_and_spawn_launch(objective, planned))
        .and_then(|prepared| mgr.install_live_session(prepared));
    if let Err(err) = launch_res {
        record_failover_exhausted(mgr, &req, &err);
    }
}

pub(super) fn record_failover_exhausted(
    mgr: &SessionManager,
    req: &FailoverRequest,
    err: &ApiError,
) {
    let note = serde_json::json!({
        "type": "pool_failover_exhausted",
        "failed_connection_id": req.failed_connection_id,
        "reason": req.reason,
        "error": err.message,
        "at": rfc3339_now(),
    });
    let _ = mgr.store.append_history(&req.employee_id, &note, &[]);
    let mut last_view = mgr.idle_view_for(&req.employee_id).unwrap_or_else(|_| {
        build_idle_session_view(
            &mgr.store,
            &req.employee_id,
            &mgr.office.info().office_id,
            mgr.office.agent_private_home_path(&req.employee_id),
            PermissionPreset::SafeAsk,
            true,
        )
    });
    last_view.last_error = Some(err.message.clone());
    mgr.last_states
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .insert(req.employee_id.clone(), last_view);
    // Queue a Recovery wake so the exhausted turn automatically retries when cooldown/capacity clears.
    enqueue_recovery_wake(
        mgr,
        req,
        Some(crate::wake::WakeHoldReason::AdmissionRefused),
    );
}

pub(super) fn enqueue_recovery_wake(
    mgr: &SessionManager,
    req: &FailoverRequest,
    hold: Option<crate::wake::WakeHoldReason>,
) {
    let ts = crate::credentials::now_ms();
    let _lock = mgr.employee_locks.lock(&req.employee_id);
    let mut queue = mgr
        .store
        .read_pending_wakes(&req.employee_id)
        .ok()
        .flatten()
        .unwrap_or_else(|| crate::wake::PendingWakeQueueRecord::empty(&req.employee_id));
    queue.push_dedup(crate::wake::PendingWakeItem {
        item_id: format!(
            "failover:{}:{}:{}",
            req.reason, req.failed_connection_id, ts
        ),
        kind: crate::wake::WakeKind::Recovery,
        sender_id: mgr.office.owner_member_id().to_owned(),
        sender_name: "Office Supervisor".into(),
        sender_handle: None,
        sender_relation: Some("office_supervisor".into()),
        sender_is_person: true,
        conversation_id: None,
        source_id: req.failed_connection_id.clone(),
        summary: req.objective.clone(),
        enqueued_at_ms: ts,
    });
    if let Some(reason) = hold {
        queue.set_hold(reason, ts, None);
    }
    let tokens = mgr.bindings.active_tokens();
    let refs: Vec<&str> = tokens.iter().map(String::as_str).collect();
    let _ = mgr.store.write_pending_wakes_redacted(&queue, &refs);
    set_waiting_messages_count(&mgr.waiting_messages, &req.employee_id, queue.items.len());
}
