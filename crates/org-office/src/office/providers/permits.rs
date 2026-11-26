//! Dual-authority quota permits and cooldowns, locally and for satellite offices.

use org_proto::providers::{
    ConnectionQuotaStatus, PermitAuthority, ProviderCooldownRecordParams,
    ProviderCooldownRecordResult, ProviderQuotaStatusParams, ProviderQuotaStatusResult,
    TurnPermitState,
};
use org_proto::{ApiError, ErrorCode};
use org_store::Sha256;

use crate::error::OfficeError;
use crate::office::Office;
use crate::providers::{
    quota_scope_key, ConnectionQuotaState, PermitAcquireOutcome, PermitAcquireRequest,
    DEFAULT_COOLDOWN_SECS, PERMIT_LEASE_MS,
};
use crate::time::format_rfc3339_ms;

use super::require_office_operator;

impl Office {
    // ---- Internal supervisor APIs (`org-office-sessions`) ----

    /// Acquires a turn permit on `connection_id` under `authority` (`hq_pool` or `office_local`).
    /// When `is_failover_switch == false`, checks unreleased budget holds first (`doc-88` §6 step 3);
    /// when `is_failover_switch == true`, skips budget holds so a running turn can fail over cleanly.
    pub fn acquire_turn_permit(
        &self,
        connection_id: &str,
        employee_id: &str,
        task_id: Option<&str>,
        model_id: &str,
        authority: PermitAuthority,
        is_failover_switch: bool,
    ) -> Result<Result<PermitAcquireOutcome, ApiError>, OfficeError> {
        op_acquire_turn_permit(
            self,
            connection_id,
            employee_id,
            task_id,
            model_id,
            authority,
            is_failover_switch,
        )
    }

    /// Renews an active turn permit's lease **in memory only** (zero durable/SQLite writes; `TASK-2`).
    pub fn renew_turn_permit(
        &self,
        permit_id: &str,
        coordinator_generation: u64,
    ) -> Result<Result<u64, ApiError>, OfficeError> {
        self.providers.renew_permit_in_memory(
            permit_id,
            coordinator_generation,
            self.monotonic_now_ms(),
        )
    }

    /// Renews an HQ-issued `HqPool` permit only when it belongs to `office_id` and matches
    /// `coordinator_generation` (`TASK-52` #12).
    pub fn renew_turn_permit_for_office(
        &self,
        office_id: &str,
        permit_id: &str,
        coordinator_generation: u64,
    ) -> Result<Result<u64, ApiError>, OfficeError> {
        if self.info.role != crate::OfficeRole::Hq {
            return Ok(Err(ApiError::new(
                ErrorCode::NotAuthorized,
                "only HQ renews cross-office HqPool turn permits",
            )));
        }
        let now_ms = self.monotonic_now_ms();
        let snap = self
            .providers
            .quota_snapshot(&self.info.org_id, None, now_ms)?;
        let owned = snap.permits.iter().any(|p| {
            p.id == permit_id
                && p.office_id == office_id
                && p.coordinator_generation == coordinator_generation
                && p.state == org_proto::providers::TurnPermitState::Active
        });
        if !owned {
            return Ok(Err(ApiError::new(
                ErrorCode::NotFound,
                "permit not found for authenticated office and generation",
            )));
        }
        self.renew_turn_permit(permit_id, coordinator_generation)
    }

    pub fn mark_turn_permit_release_pending(&self, permit_id: &str) -> Result<bool, OfficeError> {
        self.providers.mark_permit_release_pending(permit_id)
    }

    pub fn release_turn_permit(&self, permit_id: &str) -> Result<bool, OfficeError> {
        self.providers.release_permit(permit_id)
    }

    pub fn record_connection_cooldown_internal(
        &self,
        connection_id: &str,
        model_id: Option<&str>,
        cooldown_until: Option<&str>,
        retry_after_seconds: Option<u64>,
    ) -> Result<ConnectionQuotaState, OfficeError> {
        record_cooldown_inner(
            self,
            connection_id,
            model_id,
            cooldown_until,
            retry_after_seconds,
        )
    }
}

pub(super) fn op_quota_status(
    office: &Office,
    actor: &str,
    params: ProviderQuotaStatusParams,
) -> Result<Result<ProviderQuotaStatusResult, ApiError>, OfficeError> {
    if let Err(e) = require_office_operator(office, actor)? {
        return Ok(Err(e));
    }
    let snap = office.providers.quota_snapshot(
        &office.info.org_id,
        params.connection_id.as_deref(),
        office.monotonic_now_ms(),
    )?;
    let mut active = Vec::new();
    let mut release_pending = Vec::new();
    for p in snap.permits {
        match p.state {
            TurnPermitState::Active => active.push(p.to_view()),
            TurnPermitState::ReleasePending => release_pending.push(p.to_view()),
            TurnPermitState::Released | _ => {}
        }
    }
    Ok(Ok(ProviderQuotaStatusResult {
        coordinator_generation: snap.coordinator_generation,
        cooldowns: snap.cooldowns.iter().map(|c| c.to_view()).collect(),
        active_permits: active,
        release_pending_permits: release_pending,
    }))
}

pub(super) fn op_cooldown_record(
    office: &Office,
    actor: &str,
    params: ProviderCooldownRecordParams,
) -> Result<Result<ProviderCooldownRecordResult, ApiError>, OfficeError> {
    if let Err(e) = require_office_operator(office, actor)? {
        return Ok(Err(e));
    }
    if office
        .load_provider_connection(&params.connection_id)?
        .is_none()
    {
        return Ok(Err(ApiError::new(
            ErrorCode::NotFound,
            "provider connection not found",
        )));
    }
    if let Some(ref until) = params.cooldown_until {
        if org_proto::parse_rfc3339_ms(until).is_none() {
            return Ok(Err(ApiError::new(
                ErrorCode::InvalidRequest,
                "cooldown_until must be a valid RFC 3339 timestamp",
            )));
        }
    }
    let state = record_cooldown_inner(
        office,
        &params.connection_id,
        params.model_id.as_deref(),
        params.cooldown_until.as_deref(),
        params.retry_after_seconds,
    )?;
    Ok(Ok(ProviderCooldownRecordResult {
        quota: state.to_view(),
    }))
}

fn record_cooldown_inner(
    office: &Office,
    connection_id: &str,
    model_id: Option<&str>,
    cooldown_until: Option<&str>,
    retry_after_seconds: Option<u64>,
) -> Result<ConnectionQuotaState, OfficeError> {
    let now_ms = office.monotonic_now_ms();
    let until_ms = cooldown_until
        .and_then(org_proto::parse_rfc3339_ms)
        .unwrap_or_else(|| {
            let secs = retry_after_seconds.unwrap_or(DEFAULT_COOLDOWN_SECS).max(1);
            now_ms.saturating_add(secs.saturating_mul(1000))
        });
    let state = ConnectionQuotaState {
        organization_id: office.info.org_id.clone(),
        connection_id: connection_id.to_owned(),
        scope_key: quota_scope_key(model_id),
        model_id: model_id.map(str::to_owned),
        status: ConnectionQuotaStatus::Cooldown,
        cooldown_until: Some(format_rfc3339_ms(until_ms)),
        observed_at: format_rfc3339_ms(now_ms),
        observed_by_office_id: office.info.office_id.clone(),
        coordinator_generation: office.providers.coordinator_generation()?,
    };
    office.providers.record_cooldown(&state)?;
    Ok(state)
}

/// Parameters for HQ acquiring a turn permit on behalf of an authenticated office (`TASK-52`).
pub struct OfficePermitSpec<'a> {
    pub target_office_id: &'a str,
    pub connection_id: &'a str,
    pub employee_id: &'a str,
    pub task_id: Option<&'a str>,
    pub model_id: &'a str,
    pub authority: PermitAuthority,
    pub is_failover_switch: bool,
}

impl Office {
    pub fn acquire_turn_permit_for_office(
        &self,
        mut spec: OfficePermitSpec<'_>,
    ) -> Result<Result<PermitAcquireOutcome, ApiError>, OfficeError> {
        if !org_proto::is_uuid_v7(spec.target_office_id) {
            return Ok(Err(ApiError::new(
                ErrorCode::InvalidRequest,
                "invalid target office id",
            )));
        }
        // Failover switch exemption is ONLY honored when HQ already holds an active permit for
        // `(target_office_id, employee_id)`; an untrusted peer cannot bypass budget holds by
        // setting `is_failover_switch = true` on a cold turn (`TASK-52` review F6).
        spec.is_failover_switch = if spec.is_failover_switch {
            let snap =
                self.providers
                    .quota_snapshot(&self.info.org_id, None, self.monotonic_now_ms())?;
            snap.permits.iter().any(|p| {
                p.office_id == spec.target_office_id
                    && p.employee_id == spec.employee_id
                    && p.state == TurnPermitState::Active
            })
        } else {
            false
        };
        op_acquire_turn_permit_for(self, spec)
    }

    pub fn release_turn_permit_for_office(
        &self,
        target_office_id: &str,
        permit_id: &str,
    ) -> Result<bool, OfficeError> {
        let snap =
            self.providers
                .quota_snapshot(&self.info.org_id, None, self.monotonic_now_ms())?;
        let owned = snap.permits.iter().any(|p| {
            p.id == permit_id
                && p.office_id == target_office_id
                && p.state != TurnPermitState::Released
        });
        if !owned {
            return Ok(false);
        }
        self.providers.release_permit(permit_id)?;
        Ok(true)
    }

    pub fn record_satellite_cooldown(
        &self,
        satellite_office_id: &str,
        connection_id: &str,
        cooldown_until: Option<&str>,
    ) -> Result<Result<ConnectionQuotaState, ApiError>, OfficeError> {
        if !org_proto::is_uuid_v7(satellite_office_id) {
            return Ok(Err(ApiError::new(
                ErrorCode::InvalidRequest,
                "invalid satellite office id",
            )));
        }
        if self.load_provider_connection(connection_id)?.is_none() {
            return Ok(Err(ApiError::new(
                ErrorCode::NotFound,
                "provider connection not found",
            )));
        }
        if let Some(until) = cooldown_until {
            if org_proto::parse_rfc3339_ms(until).is_none() {
                return Ok(Err(ApiError::new(
                    ErrorCode::InvalidRequest,
                    "cooldown_until must be valid RFC 3339",
                )));
            }
        }
        let now_ms = self.monotonic_now_ms();
        let until_ms = cooldown_until
            .and_then(org_proto::parse_rfc3339_ms)
            .unwrap_or_else(|| now_ms.saturating_add(DEFAULT_COOLDOWN_SECS.saturating_mul(1000)));
        let state = ConnectionQuotaState {
            organization_id: self.info.org_id.clone(),
            connection_id: connection_id.to_owned(),
            scope_key: quota_scope_key(None),
            model_id: None,
            status: ConnectionQuotaStatus::Cooldown,
            cooldown_until: Some(format_rfc3339_ms(until_ms)),
            observed_at: format_rfc3339_ms(now_ms),
            observed_by_office_id: satellite_office_id.to_owned(),
            coordinator_generation: self.providers.coordinator_generation()?,
        };
        self.providers.record_cooldown(&state)?;
        Ok(Ok(state))
    }
}

fn op_acquire_turn_permit(
    office: &Office,
    connection_id: &str,
    employee_id: &str,
    task_id: Option<&str>,
    model_id: &str,
    authority: PermitAuthority,
    is_failover_switch: bool,
) -> Result<Result<PermitAcquireOutcome, ApiError>, OfficeError> {
    op_acquire_turn_permit_for(
        office,
        OfficePermitSpec {
            target_office_id: &office.info.office_id,
            connection_id,
            employee_id,
            task_id,
            model_id,
            authority,
            is_failover_switch,
        },
    )
}

fn op_acquire_turn_permit_for(
    office: &Office,
    spec: OfficePermitSpec<'_>,
) -> Result<Result<PermitAcquireOutcome, ApiError>, OfficeError> {
    let OfficePermitSpec {
        target_office_id,
        connection_id,
        employee_id,
        task_id,
        model_id,
        authority,
        is_failover_switch,
    } = spec;
    // `doc-88` §6 step 3: serialize the final budget hold check with first-permit acquisition
    // under `budget_op_lock` so a concurrent budget hold cannot slip in between check and grant,
    // while preserving the §7 exemption for replacement switch permits on an already-running turn.
    let _budget_guard = (!is_failover_switch).then(|| office.lock_budget_ops());
    if !is_failover_switch {
        let holds = crate::office::budgets::op_active_holds(office, Some(employee_id), task_id)?;
        if let Some(first) = holds.first() {
            return Ok(Err(ApiError::new(
                ErrorCode::Conflict,
                format!("BUDGET_HELD: {}", first.budget_id),
            )));
        }
    }
    let Some(conn) = office.load_provider_connection(connection_id)? else {
        return Ok(Err(ApiError::new(
            ErrorCode::NotFound,
            "provider connection not found",
        )));
    };
    if conn.archived || !conn.enabled {
        return Ok(Err(ApiError::new(
            ErrorCode::Conflict,
            "provider connection is disabled or archived",
        )));
    }
    let now_ms = office.monotonic_now_ms();
    let req = PermitAcquireRequest {
        organization_id: office.info.org_id.clone(),
        permit_id: office.next_uuid_v7(),
        connection_id: connection_id.to_owned(),
        office_id: target_office_id.to_owned(),
        employee_id: employee_id.to_owned(),
        placement_generation: 1,
        model_id: model_id.to_owned(),
        authority,
        max_concurrent_turns: conn.max_concurrent_turns,
        now_ms,
        issued_at: format_rfc3339_ms(now_ms),
        expires_at: format_rfc3339_ms(now_ms.saturating_add(PERMIT_LEASE_MS)),
    };
    Ok(Ok(office.providers.acquire_permit(req)?))
}

pub(super) fn hex_sha256(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let digest = Sha256::of(bytes);
    let mut out = String::with_capacity(64);
    for &b in digest.as_bytes() {
        out.push(HEX[(b >> 4) as usize] as char);
        out.push(HEX[(b & 0x0f) as usize] as char);
    }
    out
}
