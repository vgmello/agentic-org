//! Installing a spawned session: pre-pump isolation checks, pump thread, shutdown aborts.

use super::*;

impl SessionManager {
    pub(super) fn install_live_session(
        &self,
        prepared: PreparedSessionLaunch,
    ) -> Result<SessionView, ApiError> {
        let runtime_state = Arc::new(Mutex::new(build_initial_runtime_state(
            self.office.info().office_id.clone(),
            self.config.stall_timeout,
            &prepared,
        )));
        let PreparedSessionLaunch {
            emp_id,
            run_id,
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
            binding_file,
            objective,
            spawned,
            session,
            lifecycle,
            ..
        } = prepared;
        let active_permit = Arc::new(Mutex::new(permit));
        let (tx, rx) = mpsc::channel();
        let hooks = self.build_pump_hooks(
            &emp_id,
            route_digest.clone(),
            oauth_expires_at_ms,
            bwrap_masked_siblings.clone(),
        );
        let pump_ctx = PumpContext {
            employee_id: emp_id.clone(),
            run_id,
            runtime,
            supports_mid_turn,
            connection_id: connection_id.clone(),
            active_permit: Arc::clone(&active_permit),
            workspace: workspace_arg.clone(),
            task_ref,
            tried_connections,
            oauth_refresh_attempted,
            oauth_expires_at_ms,
            oauth_token_fp,
            store: self.store.clone(),
            bindings: self.bindings.clone(),
            state: Arc::clone(&runtime_state),
            stop_plan: lifecycle.stop,
            cleanup_paths: lifecycle.cleanup_after_exit,
            binding_file,
            hooks,
        };
        let pre_pump_meta = PrePumpHandleMeta {
            tx: tx.clone(),
            state: Arc::clone(&runtime_state),
            connection_id: connection_id.clone(),
            route_digest: route_digest.clone(),
            oauth_expires_at_ms,
            workspace: workspace_arg.clone(),
            active_permit: Arc::clone(&active_permit),
            supports_mid_turn,
            bwrap_masked_siblings: bwrap_masked_siblings.clone(),
        };
        let ready =
            ensure_pre_pump_isolation(self, &emp_id, spawned, session, pump_ctx, pre_pump_meta)?;
        // Enqueue the initial turn BEFORE starting the pump thread so `run_pump` always sees
        // Turn 1 immediately upon `Ready` without transitioning `Starting -> Idle` prematurely.
        if !objective.is_empty() {
            let _ = tx.send(PumpCommand::SendTurn(objective));
        }
        let thread = spawn_pump_thread(
            self,
            &emp_id,
            ready.spawned,
            ready.session,
            rx,
            ready.pump_ctx,
            ready.meta,
        )?;
        let view = runtime_state
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .to_view();
        register_live_session_or_abort(
            self,
            emp_id,
            LiveSessionHandle {
                state: runtime_state,
                tx,
                thread: Some(thread),
                connection_id,
                route_digest,
                oauth_expires_at_ms,
                workspace: workspace_arg,
                active_permit,
                supports_mid_turn,
                bwrap_masked_siblings,
            },
        )?;
        Ok(view)
    }
}

pub(crate) struct PrePumpHandleMeta {
    pub(crate) tx: Sender<PumpCommand>,
    pub(crate) state: Arc<Mutex<SessionRuntimeState>>,
    pub(crate) connection_id: Option<String>,
    pub(crate) route_digest: String,
    pub(crate) oauth_expires_at_ms: Option<u64>,
    pub(crate) workspace: Option<String>,
    pub(crate) active_permit: Arc<Mutex<Option<(String, u64)>>>,
    pub(crate) supports_mid_turn: bool,
    pub(crate) bwrap_masked_siblings: Option<std::collections::BTreeSet<String>>,
}

pub(super) struct VerifiedPrePump {
    pub(super) spawned: org_supervise::Spawned,
    pub(super) session: Box<dyn org_agent_runtime::Session>,
    pub(super) pump_ctx: PumpContext,
    pub(super) meta: PrePumpHandleMeta,
}

pub(super) fn ensure_pre_pump_isolation(
    mgr: &SessionManager,
    emp_id: &str,
    spawned: org_supervise::Spawned,
    mut session: Box<dyn org_agent_runtime::Session>,
    pump_ctx: PumpContext,
    meta: PrePumpHandleMeta,
) -> Result<VerifiedPrePump, ApiError> {
    let bwrap_pre_raced = crate::bwrap_isolation::verify_bwrap_masked_before_spawn(
        mgr,
        emp_id,
        meta.bwrap_masked_siblings.as_ref(),
    )
    .is_err();
    let shutting_down = is_admissions_closed(mgr);
    if !shutting_down && !bwrap_pre_raced {
        return Ok(VerifiedPrePump {
            spawned,
            session,
            pump_ctx,
            meta,
        });
    }
    let cleanup_verified = abort_pre_pump_shutdown(
        mgr,
        emp_id,
        spawned,
        &mut session,
        &pump_ctx,
        &meta.active_permit,
    );
    if !cleanup_verified && !shutting_down {
        install_unrecovered_pre_pump_handle(mgr, emp_id, meta);
        return Err(ApiError::new(
            ErrorCode::Conflict,
            format!(
                "could not verify teardown of aborted sandboxed session `{emp_id}`; resolve recovery first"
            ),
        ));
    }
    Err(if bwrap_pre_raced {
        ApiError::new(
            ErrorCode::Conflict,
            "a newly started sibling agent was not masked in this sandbox; retry launch",
        )
    } else {
        ApiError::new(
            ErrorCode::Unavailable,
            "office session manager is shutting down",
        )
    })
}

pub(crate) fn install_unrecovered_pre_pump_handle(
    mgr: &SessionManager,
    emp_id: &str,
    meta: PrePumpHandleMeta,
) {
    let (run_id, session_id, runtime_session_id, view) = {
        let mut st = meta.state.lock().unwrap_or_else(|p| p.into_inner());
        st.recovery_required = true;
        st.state = SessionState::RecoveryRequired;
        (
            st.run_id.clone(),
            st.session_id.clone(),
            st.runtime_session_id.clone(),
            st.to_view(),
        )
    };
    mgr.last_states
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .insert(emp_id.to_owned(), view);
    let _ = mgr.store.persist_recovery_required(
        emp_id,
        run_id.as_deref(),
        session_id.as_deref(),
        runtime_session_id.as_deref(),
    );
    // Check admissions under the same `registry` lock that inserts, so a drain or shutdown that
    // starts in between can never miss this handle (Pass 6).
    let mut reg = mgr.registry.lock().unwrap_or_else(|p| p.into_inner());
    if reg.draining || reg.shutting_down {
        return;
    }
    let handle = LiveSessionHandle {
        state: meta.state,
        tx: meta.tx,
        thread: None,
        connection_id: meta.connection_id,
        route_digest: meta.route_digest,
        oauth_expires_at_ms: meta.oauth_expires_at_ms,
        workspace: meta.workspace,
        active_permit: meta.active_permit,
        supports_mid_turn: meta.supports_mid_turn,
        bwrap_masked_siblings: meta.bwrap_masked_siblings,
    };
    reg.live.insert(emp_id.to_owned(), handle);
}

pub(super) fn spawn_pump_thread(
    mgr: &SessionManager,
    emp_id: &str,
    spawned: org_supervise::Spawned,
    session: Box<dyn org_agent_runtime::Session>,
    rx: mpsc::Receiver<PumpCommand>,
    pump_ctx: PumpContext,
    meta: PrePumpHandleMeta,
) -> Result<JoinHandle<()>, ApiError> {
    let thread_name = format!("org-session-{}", &emp_id[..emp_id.len().min(8)]);
    let payload = Arc::new(Mutex::new(Some((spawned, session, rx, pump_ctx))));
    let worker_payload = Arc::clone(&payload);
    match thread::Builder::new().name(thread_name).spawn(move || {
        let Some((spawned, session, rx, pump_ctx)) = worker_payload
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .take()
        else {
            return;
        };
        run_pump(spawned, session, rx, pump_ctx);
    }) {
        Ok(handle) => Ok(handle),
        Err(_) => {
            let cleanup_verified = if let Some((spawned, mut session, _rx, pump_ctx)) =
                payload.lock().unwrap_or_else(|p| p.into_inner()).take()
            {
                crate::pump::finalize_child(
                    spawned.child,
                    spawned.stdin,
                    &mut session,
                    &pump_ctx,
                    org_supervise::StopMode::Graceful,
                    false,
                )
            } else {
                mgr.bindings.revoke(emp_id, None);
                false
            };
            if let Some((pid, _)) = meta
                .active_permit
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .take()
            {
                if cleanup_verified {
                    crate::router::release_turn_permit_any(
                        &mgr.office,
                        mgr.config.hq_permits.as_deref(),
                        &pid,
                    );
                } else {
                    let _ = mgr.office.mark_turn_permit_release_pending(&pid);
                }
            }
            if !cleanup_verified {
                install_unrecovered_pre_pump_handle(mgr, emp_id, meta);
            }
            Err(ApiError::new(
                ErrorCode::Unavailable,
                "could not start session pump thread",
            ))
        }
    }
}

pub(super) fn abort_pre_pump_shutdown(
    mgr: &SessionManager,
    emp_id: &str,
    spawned: org_supervise::Spawned,
    session: &mut Box<dyn org_agent_runtime::Session>,
    pump_ctx: &PumpContext,
    active_permit: &Arc<Mutex<Option<(String, u64)>>>,
) -> bool {
    let cleanup_verified = crate::pump::finalize_child(
        spawned.child,
        spawned.stdin,
        session,
        pump_ctx,
        org_supervise::StopMode::Graceful,
        false,
    );
    if let Some((pid, _)) = active_permit
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .take()
    {
        if cleanup_verified {
            crate::router::release_turn_permit_any(
                &mgr.office,
                mgr.config.hq_permits.as_deref(),
                &pid,
            );
        } else {
            let _ = mgr.office.mark_turn_permit_release_pending(&pid);
        }
    }
    if !cleanup_verified {
        let session_id = pump_ctx
            .state
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .session_id
            .clone();
        let _ = mgr.store.persist_recovery_required(
            emp_id,
            Some(&pump_ctx.run_id),
            session_id.as_deref(),
            None,
        );
    }
    let _ = persist_shutdown_cancellation(mgr, emp_id, &pump_ctx.run_id, !cleanup_verified);
    cleanup_verified
}

pub(super) fn register_live_session_or_abort(
    mgr: &SessionManager,
    emp_id: String,
    mut handle: LiveSessionHandle,
) -> Result<(), ApiError> {
    let disk_unmasked = handle.bwrap_masked_siblings.as_ref().is_some_and(|masked| {
        crate::bwrap_isolation::has_unmasked_bwrap_sibling_in_office(&mgr.office, &emp_id, masked)
    });
    let mut reg = mgr.registry.lock().unwrap_or_else(|p| p.into_inner());
    let bwrap_raced_unmasked = disk_unmasked
        || handle.bwrap_masked_siblings.as_ref().is_some_and(|masked| {
            reg.live
                .keys()
                .chain(reg.starting.iter())
                .any(|other| other != &emp_id && !masked.contains(other))
        });
    if reg.draining || reg.shutting_down || bwrap_raced_unmasked {
        let shutting_down = reg.draining || reg.shutting_down;
        drop(reg);
        if bwrap_raced_unmasked && !shutting_down {
            let _ = crate::bwrap_isolation::reap_live_handle(
                mgr,
                &emp_id,
                handle,
                Some("bwrap_sibling_race_abort"),
                crate::bwrap_isolation::ReapMode::FailClosedRestore {
                    conflict_message: format!(
                        "could not verify teardown of raced sandboxed session `{emp_id}`; resolve recovery first"
                    ),
                },
            )?;
            return Err(ApiError::new(
                ErrorCode::Conflict,
                "a newly started sibling agent was not masked in this sandbox; retry launch",
            ));
        }
        if let Some(thread) = handle.thread.take() {
            let _ = abort_post_spawn_shutdown(
                mgr,
                &emp_id,
                handle.tx,
                thread,
                &handle.state,
                &handle.active_permit,
            );
        }
        return Err(ApiError::new(
            ErrorCode::Unavailable,
            "office session manager is shutting down",
        ));
    }
    reg.live.insert(emp_id, handle);
    Ok(())
}

pub(super) fn abort_post_spawn_shutdown(
    mgr: &SessionManager,
    emp_id: &str,
    tx: Sender<PumpCommand>,
    thread: JoinHandle<()>,
    runtime_state: &Arc<Mutex<SessionRuntimeState>>,
    active_permit: &Arc<Mutex<Option<(String, u64)>>>,
) -> ApiError {
    mgr.bindings.revoke(emp_id, None);
    let _ = tx.send(PumpCommand::Stop {
        reason: "office shutdown".into(),
    });
    let joined_ok = thread.join().is_ok();
    let (cancelled_run_id, recovery_required) = {
        let st = runtime_state.lock().unwrap_or_else(|p| p.into_inner());
        (
            st.run_id.clone(),
            !joined_ok || st.recovery_required || st.state == SessionState::RecoveryRequired,
        )
    };
    if let Some((pid, _)) = active_permit
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .take()
    {
        // If `run_pump` did not finish cleanly to take the permit itself, fail closed (`AE-07`).
        let _ = mgr.office.mark_turn_permit_release_pending(&pid);
    }
    if recovery_required {
        let _ =
            mgr.store
                .persist_recovery_required(emp_id, cancelled_run_id.as_deref(), None, None);
    }
    if let Some(cancelled_run_id) = cancelled_run_id {
        return persist_shutdown_cancellation(mgr, emp_id, &cancelled_run_id, recovery_required);
    }
    ApiError::new(
        ErrorCode::Unavailable,
        "office session manager is shutting down",
    )
}

pub(super) fn persist_shutdown_cancellation(
    mgr: &SessionManager,
    emp_id: &str,
    run_id: &str,
    recovery_required: bool,
) -> ApiError {
    if let Err(io_err) = mgr.store.update_active_turn(emp_id, |rec| {
        if rec.run_id == run_id {
            rec.active = false;
            if recovery_required {
                rec.recovery_required = true;
            }
            rec.updated_at = rfc3339_now();
        }
    }) {
        eprintln!(
            "org-office-sessions: error: failed to persist shutdown cancellation for {emp_id}: {io_err}"
        );
        return ApiError::new(
            ErrorCode::StorageError,
            "could not persist session shutdown cancellation",
        );
    }
    ApiError::new(
        ErrorCode::Unavailable,
        "office session manager is shutting down",
    )
}
