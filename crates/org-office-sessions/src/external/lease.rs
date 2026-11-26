//! External lease lifecycle on the [`SessionManager`] (`TASK-137`): approval with supersede and
//! managed takeover, revocation, the operator's session list, and the single-active-lease checks
//! the wake queue and session admission consult.

use org_office::organization_family::records::{Employee, EmployeeKind, EmployeeStatus};
use org_proto::external_auth::{
    AuthWhoamiResult, ExternalSessionView, MemberExternalApproveParams, MemberExternalRevokeResult,
    MemberExternalSessionsResult, SessionMode, DEFAULT_LEASE_TTL_SECS, MAX_LEASE_TTL_SECS,
    MIN_LEASE_TTL_SECS,
};
use org_proto::{ApiError, ErrorCode};
use serde_json::json;

use crate::credentials::{format_rfc3339_ms, is_safe_id, now_ms, rfc3339_now};
use crate::manager::SessionManager;
use crate::provenance::{sender_in_ancestor_team_leads, sender_in_reports_to_chain};

use super::{decode_pubkey, pubkey_fingerprint, ExternalLeaseRecord, EXTERNAL_LEASE_VERSION};

/// Most history lines `member.external_sessions` answers.
const MAX_HISTORY_LIMIT: u32 = 200;
const DEFAULT_HISTORY_LIMIT: u32 = 20;

fn storage(context: &str) -> ApiError {
    ApiError::new(
        ErrorCode::StorageError,
        format!("could not {context} the external lease"),
    )
}

impl SessionManager {
    /// `employee_id`'s lease (active or not), from the RAM cache or, once, from disk. An
    /// unreadable lease is no lease: no external token can be minted against it either.
    pub fn external_lease(&self, employee_id: &str) -> Option<ExternalLeaseRecord> {
        if !is_safe_id(employee_id) {
            return None;
        }
        if let Some(cached) = self.external.cached(employee_id) {
            return cached;
        }
        let loaded = self
            .store
            .read_external_lease(employee_id)
            .unwrap_or_else(|e| {
                eprintln!("org-office-sessions: external lease of {employee_id} unreadable: {e}");
                None
            });
        self.external.fill(employee_id, loaded);
        self.external.cached(employee_id).flatten()
    }

    /// Whether `employee_id` holds an active (unrevoked, unexpired) external lease: while it
    /// does, autonomous wakes stay queued and no managed session starts (`TASK-137`).
    pub fn has_active_external_lease(&self, employee_id: &str) -> bool {
        self.external_lease(employee_id)
            .is_some_and(|lease| lease.is_active(now_ms()))
    }

    /// The lease session `token` belongs to, when it is `employee_id`'s live external fast
    /// token: RAM-only and constant-time (the bound hot path).
    pub fn verify_external_token(&self, employee_id: &str, token: Option<&str>) -> Option<String> {
        self.bindings
            .verify_external(employee_id, token, now_ms())
            .ok()
            .map(|binding| binding.session_id)
    }

    /// Whether an externally bound connection of lease `session_id` may keep acting.
    pub fn external_binding_active(&self, employee_id: &str, session_id: &str) -> bool {
        self.bindings
            .external_active(employee_id, session_id, now_ms())
    }

    /// `auth.whoami` for a bound connection: managed, or external with lease `session_id`.
    pub fn external_whoami(
        &self,
        employee_id: &str,
        external_session_id: Option<&str>,
    ) -> AuthWhoamiResult {
        let handle = self
            .office
            .load_employee(employee_id)
            .ok()
            .flatten()
            .map(|e| e.handle)
            .filter(|h| !h.is_empty());
        let office_id = self.office.info().office_id.clone();
        let Some(session_id) = external_session_id else {
            return AuthWhoamiResult {
                member_id: employee_id.to_owned(),
                handle,
                office_id,
                mode: SessionMode::Managed,
                session_id: None,
                expires_at: None,
                lease_expires_at: None,
            };
        };
        AuthWhoamiResult {
            member_id: employee_id.to_owned(),
            handle,
            office_id,
            mode: SessionMode::External,
            session_id: Some(session_id.to_owned()),
            expires_at: self
                .bindings
                .external_expiry(employee_id, session_id)
                .map(format_rfc3339_ms),
            lease_expires_at: self
                .external_lease(employee_id)
                .filter(|l| l.session_id == session_id)
                .map(|l| l.expires_at),
        }
    }

    /// Owner, or an active person who may hire, manages the agent, or leads its team chain.
    fn authorize_external_operator(&self, actor: &str, target: &Employee) -> Result<(), ApiError> {
        self.authorize_owner_or_person_inner(actor, false)?;
        if actor == self.office.owner_member_id() {
            return Ok(());
        }
        let person = self
            .office
            .load_employee(actor)
            .map_err(|_| ApiError::new(ErrorCode::StorageError, "could not verify caller"))?
            .filter(|e| e.kind == EmployeeKind::Person && e.status == EmployeeStatus::Active);
        let allowed = person.is_some_and(|p| {
            p.may_hire
                || sender_in_reports_to_chain(&self.office, target, actor)
                || sender_in_ancestor_team_leads(&self.office, target, actor)
        });
        if allowed {
            Ok(())
        } else {
            Err(ApiError::new(
                ErrorCode::NotAuthorized,
                "only the owner, a person who may hire, or the agent's manager or team lead may manage its external sessions",
            ))
        }
    }

    /// `member.external_approve`: turns a pending login into the agent's one active lease.
    /// Supersedes an older external lease; refuses a live managed turn unless `takeover` parks it.
    pub fn external_approve(
        &self,
        actor: &str,
        params: &MemberExternalApproveParams,
    ) -> Result<ExternalSessionView, ApiError> {
        let ttl = params.ttl_secs.unwrap_or(DEFAULT_LEASE_TTL_SECS);
        if !(MIN_LEASE_TTL_SECS..=MAX_LEASE_TTL_SECS).contains(&ttl) {
            return Err(ApiError::new(
                ErrorCode::InvalidRequest,
                format!("--ttl must be between {MIN_LEASE_TTL_SECS}s and {MAX_LEASE_TTL_SECS}s"),
            ));
        }
        // A caller who could never approve anything must not spend a request's attempts.
        self.authorize_owner_or_person_inner(actor, false)?;
        let _serial = self
            .external
            .lease_mutation
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        // Authority over the agent the code names first, the secret after: a person who may not
        // manage that agent learns nothing of the secret and never spends (or locks) its attempts.
        let target = self
            .external
            .pending
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .find_live(&params.code, now_ms())?;
        let employee = self.require_active_agent(&target.employee_id)?;
        self.authorize_external_operator(actor, &employee)?;
        let pending = self
            .external
            .pending
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .check_code(&params.code, &target, now_ms())?;
        let emp_id = employee.id.as_str();
        let now = now_ms();
        let session_id = format!("ext-{}", self.office.next_uuid_v7());
        let record = ExternalLeaseRecord {
            version: EXTERNAL_LEASE_VERSION,
            session_id: session_id.clone(),
            employee_id: emp_id.to_owned(),
            pubkey_ed25519_b64: pending.pubkey_ed25519_b64.clone(),
            workspace_label: pending.workspace_label.clone(),
            approved_by: actor.to_owned(),
            approved_at: format_rfc3339_ms(now),
            expires_at: format_rfc3339_ms(now.saturating_add(ttl * 1000)),
            revoked_at: None,
        };
        let (previous, took_over) =
            self.publish_external_lease(emp_id, &record, params.takeover)?;
        if let Err(e) = self.park_managed_for_external(emp_id) {
            self.external.restore(emp_id, previous);
            return Err(e);
        }
        if self.store.write_external_lease(&record).is_err() {
            self.external.restore(emp_id, previous);
            return Err(storage("save"));
        }
        // The new lease file replaced whatever revocation was still owed to the disk.
        self.external.settle_persisted(emp_id);
        // The new lease has no token yet: every older external token and nonce is void.
        self.bindings.revoke_external(emp_id, None);
        self.external
            .nonces
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .purge_member(emp_id);
        let superseded = previous
            .flatten()
            .filter(|old| old.is_active(now) && old.session_id != session_id)
            .map(|old| old.session_id);
        if let Some(old) = &superseded {
            self.record_external_event(emp_id, "superseded", Some(old), Some(actor), None);
        }
        let detail = format!(
            "{}{}",
            record.workspace_label,
            if took_over {
                " (took over a live managed turn)"
            } else {
                ""
            }
        );
        self.record_external_event(
            emp_id,
            "approved",
            Some(&session_id),
            Some(actor),
            Some(&detail),
        );
        self.external
            .pending
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .mark_approved(&pending.request_id, &session_id, &record.expires_at, now);
        let mut view = self.external_view(&employee, &record);
        view.superseded_session_id = superseded;
        view.took_over_managed = took_over;
        Ok(view)
    }

    /// Publishes `record` in the lease cache under the registry lock, refusing a live managed
    /// turn unless `takeover`. From here on every managed start and wake of this agent is held,
    /// so parking its managed session cannot race a fresh one. Answers the previous cache entry
    /// and whether a live managed turn is being taken over.
    fn publish_external_lease(
        &self,
        emp_id: &str,
        record: &ExternalLeaseRecord,
        takeover: bool,
    ) -> Result<(Option<Option<ExternalLeaseRecord>>, bool), ApiError> {
        let _ = self.external_lease(emp_id);
        let reg = self.registry.lock().unwrap_or_else(|p| p.into_inner());
        let live_turn = crate::lifecycle::managed_turn_live_in(&reg, &self.bindings, emp_id);
        if live_turn && !takeover {
            return Err(ApiError::new(
                ErrorCode::Conflict,
                "this agent has a live managed turn; approve with --takeover to park it first",
            ));
        }
        Ok((
            self.external.replace(emp_id, Some(record.clone())),
            live_turn,
        ))
    }

    /// Parks whatever managed session `emp_id` still has (a live or warm handle, an unmanaged
    /// binding); its queued wakes stay durable. Refuses when one is still there afterwards.
    fn park_managed_for_external(&self, emp_id: &str) -> Result<(), ApiError> {
        let has_managed = |mgr: &Self| {
            let reg = mgr.registry.lock().unwrap_or_else(|p| p.into_inner());
            reg.live.contains_key(emp_id)
                || reg.starting.contains(emp_id)
                || mgr.bindings.has_active(emp_id)
        };
        if has_managed(self) {
            self.stop_session(emp_id, "parked for an approved external session")?;
        }
        if has_managed(self) {
            return Err(ApiError::new(
                ErrorCode::Conflict,
                "the agent's managed session is still stopping; retry the approval",
            ));
        }
        Ok(())
    }

    /// `member.external_sessions`: the active lease and recent history.
    pub fn external_sessions(
        &self,
        actor: &str,
        employee_id: &str,
        limit: Option<u32>,
    ) -> Result<MemberExternalSessionsResult, ApiError> {
        let employee = self.require_agent(employee_id)?;
        self.authorize_external_operator(actor, &employee)?;
        let emp_id = employee.id.as_str();
        let limit = limit
            .unwrap_or(DEFAULT_HISTORY_LIMIT)
            .clamp(1, MAX_HISTORY_LIMIT) as usize;
        let history = self
            .store
            .read_external_history(emp_id, limit)
            .map_err(|_| storage("read the history of"))?;
        let active = self
            .external_lease(emp_id)
            .filter(|l| l.is_active(now_ms()))
            .map(|l| self.external_view(&employee, &l));
        Ok(MemberExternalSessionsResult {
            employee_id: emp_id.to_owned(),
            active,
            history,
        })
    }

    /// `member.external_revoke`: ends the agent's lease (only `session_id`, when given).
    pub fn external_revoke(
        &self,
        actor: &str,
        employee_id: &str,
        session_id: Option<&str>,
    ) -> Result<MemberExternalRevokeResult, ApiError> {
        let employee = self.require_agent(employee_id)?;
        self.authorize_external_operator(actor, &employee)?;
        let emp_id = employee.id.as_str();
        let revoked = self.end_external_lease(emp_id, session_id, Some(actor), "revoked")?;
        if revoked.is_some() {
            let _ = self.process_due_wakes_for(emp_id);
        }
        Ok(MemberExternalRevokeResult {
            employee_id: emp_id.to_owned(),
            revoked: revoked.is_some(),
            session_id: revoked,
        })
    }

    /// Ends `employee_id`'s lease without an operator (pause, offboard): best effort, logged.
    /// RAM is revoked whatever the disk does ([`Self::end_external_lease`]).
    pub fn revoke_external_lease(&self, employee_id: &str, reason: &str) {
        // The history names the lifecycle event, whatever words the caller's reason has.
        let event = if reason.contains("offboard") {
            "offboarded"
        } else if reason.contains("pause") {
            "paused"
        } else {
            "revoked"
        };
        if let Err(e) = self.end_external_lease(employee_id, None, None, event) {
            eprintln!("org-office-sessions: could not end external lease of {employee_id}: {e:?}");
            // Fail closed in RAM even when the file could not be rewritten.
            self.bindings.revoke_external(employee_id, None);
        }
    }

    /// Marks the active lease revoked (durably) and drops its token and nonces. Answers the
    /// ended session, `None` when nothing was active. Callers that end a lease of an agent that
    /// stays active run its held wakes afterwards.
    ///
    /// Fails closed: the RAM cache says revoked before the file is rewritten and stays revoked
    /// when the write fails, so no challenge, exchange or token is served for that lease from
    /// then on and its wakes are no longer held. The revocation the disk still owes is written
    /// by the next call for this agent, or replaced by the next approval's lease file.
    pub(crate) fn end_external_lease(
        &self,
        employee_id: &str,
        session_id: Option<&str>,
        actor: Option<&str>,
        reason: &str,
    ) -> Result<Option<String>, ApiError> {
        let _serial = self
            .external
            .lease_mutation
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        let Some(mut lease) = self.external_lease(employee_id) else {
            self.bindings.revoke_external(employee_id, None);
            return Ok(None);
        };
        if lease.revoked_at.is_some() {
            self.bindings.revoke_external(employee_id, None);
            if !self.external.owes_disk(employee_id) {
                return Ok(None);
            }
            // An earlier revocation reached RAM only: write it now.
            return self.persist_revocation(&lease, actor, reason).map(Some);
        }
        if session_id.is_some_and(|id| id != lease.session_id) {
            return Err(ApiError::new(
                ErrorCode::NotFound,
                "that external session is not this agent's current one",
            ));
        }
        lease.revoked_at = Some(rfc3339_now());
        self.external.replace(employee_id, Some(lease.clone()));
        self.external.mark_unpersisted(employee_id);
        self.bindings.revoke_external(employee_id, None);
        self.external
            .nonces
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .purge_member(employee_id);
        self.persist_revocation(&lease, actor, reason).map(Some)
    }

    /// Writes a lease RAM already holds revoked, then records it. On failure the debt stays.
    fn persist_revocation(
        &self,
        lease: &ExternalLeaseRecord,
        actor: Option<&str>,
        reason: &str,
    ) -> Result<String, ApiError> {
        if self.store.write_external_lease(lease).is_err() {
            return Err(storage("revoke"));
        }
        self.external.settle_persisted(&lease.employee_id);
        self.record_external_event(
            &lease.employee_id,
            reason,
            Some(&lease.session_id),
            actor,
            None,
        );
        Ok(lease.session_id.clone())
    }

    fn external_view(
        &self,
        employee: &Employee,
        lease: &ExternalLeaseRecord,
    ) -> ExternalSessionView {
        ExternalSessionView {
            session_id: lease.session_id.clone(),
            employee_id: lease.employee_id.clone(),
            handle: (!employee.handle.is_empty()).then(|| employee.handle.clone()),
            workspace_label: lease.workspace_label.clone(),
            pubkey_fingerprint: decode_pubkey(&lease.pubkey_ed25519_b64)
                .map(|k| pubkey_fingerprint(&k))
                .unwrap_or_default(),
            approved_by: lease.approved_by.clone(),
            approved_at: lease.approved_at.clone(),
            expires_at: lease.expires_at.clone(),
            revoked_at: lease.revoked_at.clone(),
            active: lease.is_active(now_ms()),
            superseded_session_id: None,
            took_over_managed: false,
        }
    }

    /// Appends one audit line to `external-history.jsonl`, every active token redacted.
    pub(crate) fn record_external_event(
        &self,
        employee_id: &str,
        event: &str,
        session_id: Option<&str>,
        actor_id: Option<&str>,
        detail: Option<&str>,
    ) {
        let tokens = self.bindings.active_tokens();
        let refs: Vec<&str> = tokens.iter().map(String::as_str).collect();
        let entry = json!({
            "at": rfc3339_now(),
            "event": event,
            "mode": "external",
            "session_id": session_id,
            "actor_id": actor_id,
            "detail": detail,
        });
        if let Err(e) = self
            .store
            .append_external_history(employee_id, &entry, &refs)
        {
            eprintln!("org-office-sessions: external history of {employee_id} not written: {e}");
        }
    }
}
