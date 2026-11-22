//! In-memory [`ProviderPoolStore`] implementation for `EngineChoice::Memory` tests (`TASK-54`).

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

use org_proto::providers::TurnPermitState;
use org_proto::{ApiError, ErrorCode};

use super::{
    ConnectionQuotaState, OfficeConnectionBinding, OfficeConnectionModel, OfficeConnectionState,
    PermitAcquireOutcome, PermitAcquireRequest, PermitSnapshot, ProviderPoolStore,
    TurnPermitRecord, PERMIT_LEASE_MS,
};
use crate::error::OfficeError;

type BindKey = (String, String, String);
type QuotaKey = (String, String, String);

#[derive(Default)]
struct State {
    coordinator_generation: u64,
    bindings: BTreeMap<BindKey, OfficeConnectionBinding>,
    states: BTreeMap<BindKey, OfficeConnectionState>,
    models: BTreeMap<BindKey, Vec<OfficeConnectionModel>>,
    cooldowns: BTreeMap<QuotaKey, ConnectionQuotaState>,
    permits: BTreeMap<String, TurnPermitRecord>,
    /// In-memory lease deadlines (`permit_id -> expires_at_ms`), updated with ZERO durable writes.
    in_memory_deadlines: BTreeMap<String, u64>,
}

pub struct MemoryProviderPoolStore {
    state: Mutex<State>,
    durable_writes: AtomicU64,
}

impl Default for MemoryProviderPoolStore {
    fn default() -> Self {
        Self::new()
    }
}

impl MemoryProviderPoolStore {
    pub fn new() -> Self {
        Self {
            state: Mutex::new(State {
                coordinator_generation: 1,
                ..State::default()
            }),
            durable_writes: AtomicU64::new(0),
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|p| p.into_inner())
    }
}

impl ProviderPoolStore for MemoryProviderPoolStore {
    fn get_binding(
        &self,
        org_id: &str,
        office_id: &str,
        connection_id: &str,
    ) -> Result<Option<OfficeConnectionBinding>, OfficeError> {
        let key = (
            org_id.to_owned(),
            office_id.to_owned(),
            connection_id.to_owned(),
        );
        Ok(self.lock().bindings.get(&key).cloned())
    }

    fn list_bindings(
        &self,
        org_id: &str,
        office_id: &str,
    ) -> Result<Vec<OfficeConnectionBinding>, OfficeError> {
        Ok(self
            .lock()
            .bindings
            .values()
            .filter(|b| b.organization_id == org_id && b.office_id == office_id)
            .cloned()
            .collect())
    }

    fn save_binding_and_invalidate(
        &self,
        binding: &OfficeConnectionBinding,
    ) -> Result<(), OfficeError> {
        let mut s = self.lock();
        let key = (
            binding.organization_id.clone(),
            binding.office_id.clone(),
            binding.connection_id.clone(),
        );
        s.bindings.insert(key.clone(), binding.clone());
        s.states.remove(&key);
        s.models.remove(&key);
        self.durable_writes.fetch_add(1, Ordering::Relaxed);
        Ok(())
    }

    fn get_state(
        &self,
        org_id: &str,
        office_id: &str,
        connection_id: &str,
    ) -> Result<Option<OfficeConnectionState>, OfficeError> {
        let key = (
            org_id.to_owned(),
            office_id.to_owned(),
            connection_id.to_owned(),
        );
        Ok(self.lock().states.get(&key).cloned())
    }

    fn list_models(
        &self,
        org_id: &str,
        office_id: &str,
        connection_id: &str,
    ) -> Result<Vec<OfficeConnectionModel>, OfficeError> {
        let key = (
            org_id.to_owned(),
            office_id.to_owned(),
            connection_id.to_owned(),
        );
        Ok(self.lock().models.get(&key).cloned().unwrap_or_default())
    }

    fn save_observation(
        &self,
        state: &OfficeConnectionState,
        models: &[OfficeConnectionModel],
    ) -> Result<(), OfficeError> {
        let mut s = self.lock();
        let key = (
            state.organization_id.clone(),
            state.office_id.clone(),
            state.connection_id.clone(),
        );
        s.states.insert(key.clone(), state.clone());
        s.models.insert(key, models.to_vec());
        Ok(())
    }

    fn coordinator_generation(&self) -> Result<u64, OfficeError> {
        Ok(self.lock().coordinator_generation)
    }

    fn acquire_permit(
        &self,
        req: PermitAcquireRequest,
    ) -> Result<PermitAcquireOutcome, OfficeError> {
        let mut s = self.lock();
        let expired_any = expire_overdue_locked(&mut s, req.now_ms);
        if expired_any {
            self.durable_writes.fetch_add(1, Ordering::Relaxed);
        }
        if let Some(until) = active_cooldown_until(
            &s.cooldowns,
            &req.organization_id,
            &req.connection_id,
            &req.model_id,
            req.now_ms,
        ) {
            return Ok(PermitAcquireOutcome::CoolingDown {
                cooldown_until: until,
            });
        }
        let occupied = s
            .permits
            .values()
            .filter(|p| {
                p.organization_id == req.organization_id
                    && p.connection_id == req.connection_id
                    && p.state != TurnPermitState::Released
            })
            .count() as u64;
        if occupied >= req.max_concurrent_turns {
            return Ok(PermitAcquireOutcome::CapacityFull {
                occupied,
                max: req.max_concurrent_turns,
            });
        }
        let permit = TurnPermitRecord {
            organization_id: req.organization_id,
            id: req.permit_id.clone(),
            connection_id: req.connection_id,
            office_id: req.office_id,
            employee_id: req.employee_id,
            placement_generation: req.placement_generation,
            model_id: req.model_id,
            authority: req.authority,
            state: TurnPermitState::Active,
            issued_at: req.issued_at,
            expires_at: req.expires_at,
            coordinator_generation: s.coordinator_generation,
        };
        s.permits.insert(permit.id.clone(), permit.clone());
        s.in_memory_deadlines.insert(
            permit.id.clone(),
            req.now_ms.saturating_add(PERMIT_LEASE_MS),
        );
        self.durable_writes.fetch_add(1, Ordering::Relaxed);
        Ok(PermitAcquireOutcome::Granted(permit))
    }

    fn renew_permit_in_memory(
        &self,
        permit_id: &str,
        coordinator_generation: u64,
        now_ms: u64,
    ) -> Result<Result<u64, ApiError>, OfficeError> {
        let mut s = self.lock();
        let Some(p) = s.permits.get(permit_id) else {
            return Ok(Err(ApiError::new(ErrorCode::NotFound, "permit not found")));
        };
        if p.coordinator_generation != coordinator_generation || p.state != TurnPermitState::Active
        {
            return Ok(Err(ApiError::new(
                ErrorCode::Conflict,
                "permit is no longer active for this coordinator generation",
            )));
        }
        let Some(&current_deadline) = s.in_memory_deadlines.get(permit_id) else {
            return Ok(Err(ApiError::new(
                ErrorCode::Conflict,
                "PERMIT_EXPIRED: permit lease is no longer tracked",
            )));
        };
        if now_ms >= current_deadline {
            s.in_memory_deadlines.remove(permit_id);
            return Ok(Err(ApiError::new(
                ErrorCode::Conflict,
                "PERMIT_EXPIRED: turn permit lease has already expired",
            )));
        }
        let next_deadline = now_ms.saturating_add(PERMIT_LEASE_MS);
        s.in_memory_deadlines
            .insert(permit_id.to_owned(), next_deadline);
        // Zero durable writes!
        Ok(Ok(next_deadline))
    }

    fn mark_permit_release_pending(&self, permit_id: &str) -> Result<bool, OfficeError> {
        let mut s = self.lock();
        let Some(p) = s.permits.get_mut(permit_id) else {
            return Ok(false);
        };
        if p.state != TurnPermitState::Active {
            return Ok(false);
        }
        p.state = TurnPermitState::ReleasePending;
        s.in_memory_deadlines.remove(permit_id);
        self.durable_writes.fetch_add(1, Ordering::Relaxed);
        Ok(true)
    }

    fn release_permit(&self, permit_id: &str) -> Result<bool, OfficeError> {
        let mut s = self.lock();
        let Some(p) = s.permits.get_mut(permit_id) else {
            return Ok(false);
        };
        if p.state == TurnPermitState::Released {
            return Ok(false);
        }
        p.state = TurnPermitState::Released;
        s.in_memory_deadlines.remove(permit_id);
        self.durable_writes.fetch_add(1, Ordering::Relaxed);
        Ok(true)
    }

    fn record_cooldown(&self, state: &ConnectionQuotaState) -> Result<(), OfficeError> {
        let mut s = self.lock();
        let mut next = state.clone();
        next.coordinator_generation = s.coordinator_generation;
        s.cooldowns.insert(
            (
                next.organization_id.clone(),
                next.connection_id.clone(),
                next.scope_key.clone(),
            ),
            next,
        );
        self.durable_writes.fetch_add(1, Ordering::Relaxed);
        Ok(())
    }

    fn quota_snapshot(
        &self,
        org_id: &str,
        connection_id: Option<&str>,
        now_ms: u64,
    ) -> Result<PermitSnapshot, OfficeError> {
        let mut s = self.lock();
        if expire_overdue_locked(&mut s, now_ms) {
            self.durable_writes.fetch_add(1, Ordering::Relaxed);
        }
        let cooldowns = s
            .cooldowns
            .values()
            .filter(|c| {
                c.organization_id == org_id
                    && connection_id.is_none_or(|cid| c.connection_id == cid)
            })
            .cloned()
            .collect();
        let permits = s
            .permits
            .values()
            .filter(|p| {
                p.organization_id == org_id
                    && p.state != TurnPermitState::Released
                    && connection_id.is_none_or(|cid| p.connection_id == cid)
            })
            .cloned()
            .collect();
        Ok(PermitSnapshot {
            coordinator_generation: s.coordinator_generation,
            cooldowns,
            permits,
        })
    }

    fn durable_write_count(&self) -> u64 {
        self.durable_writes.load(Ordering::Relaxed)
    }
}

fn expire_overdue_locked(s: &mut State, now_ms: u64) -> bool {
    let mut expired_ids = Vec::new();
    for (id, p) in &s.permits {
        if p.state != TurnPermitState::Active {
            continue;
        }
        let deadline = s
            .in_memory_deadlines
            .get(id)
            .copied()
            .or_else(|| org_proto::parse_rfc3339_ms(&p.expires_at))
            .unwrap_or(0);
        if now_ms >= deadline {
            expired_ids.push(id.clone());
        }
    }
    for id in &expired_ids {
        if let Some(p) = s.permits.get_mut(id) {
            p.state = TurnPermitState::ReleasePending;
        }
        s.in_memory_deadlines.remove(id);
    }
    !expired_ids.is_empty()
}

fn active_cooldown_until(
    cooldowns: &BTreeMap<QuotaKey, ConnectionQuotaState>,
    org_id: &str,
    connection_id: &str,
    model_id: &str,
    now_ms: u64,
) -> Option<String> {
    let mut best: Option<(u64, String)> = None;
    for scope in ["connection".to_owned(), format!("model:{model_id}")] {
        let key = (org_id.to_owned(), connection_id.to_owned(), scope);
        let Some(q) = cooldowns.get(&key).filter(|q| q.is_cooling_down(now_ms)) else {
            continue;
        };
        let Some(until) = q.cooldown_until.as_deref() else {
            continue;
        };
        let Some(ms) = org_proto::parse_rfc3339_ms(until) else {
            continue;
        };
        if best.as_ref().is_none_or(|(b, _)| ms > *b) {
            best = Some((ms, until.to_owned()));
        }
    }
    best.map(|(_, s)| s)
}
