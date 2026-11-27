//! Per-caller session partitions of the Chief of Staff (`tess:ask:<id>`, `tess:dm:<id>`, `tess:wake`).

use std::collections::HashMap;
use std::fmt;
use std::sync::{Arc, Mutex};

pub const TESS_ASK_PREFIX: &str = "tess:ask:";
pub const TESS_WAKE_PARTITION: &str = "tess:wake";
pub const TESS_DM_PREFIX: &str = "tess:dm:";
pub const ROTATE_AFTER_TURNS: u32 = 20;
pub const ROTATE_AT_CONTEXT_PERCENT: u64 = 60;
pub const DEFAULT_CONTEXT_WINDOW_TOKENS: u64 = 200_000;

/// A partition of the Chief of Staff's sessions.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct PartitionKey(String);

impl PartitionKey {
    pub fn tess_ask(caller_id: &str) -> Self {
        Self(format!("{TESS_ASK_PREFIX}{caller_id}"))
    }

    pub fn tess_wake() -> Self {
        Self(TESS_WAKE_PARTITION.to_owned())
    }

    pub fn tess_dm(sender_id: &str) -> Self {
        Self(format!("{TESS_DM_PREFIX}{sender_id}"))
    }

    pub fn for_caller(caller_id: Option<&str>) -> Self {
        caller_id.map_or_else(Self::tess_wake, Self::tess_ask)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn caller_id(&self) -> Option<&str> {
        self.0.strip_prefix(TESS_ASK_PREFIX)
    }

    pub fn acting_for_caller_id(&self) -> Option<&str> {
        self.caller_id()
    }

    pub fn dm_sender_id(&self) -> Option<&str> {
        self.0.strip_prefix(TESS_DM_PREFIX)
    }

    pub fn delivery_scope(&self) -> org_office::attachments::DeliveryScope {
        if let Some(caller_id) = self.caller_id() {
            org_office::attachments::DeliveryScope::ActingForPerson(caller_id.to_owned())
        } else if let Some(sender_id) = self.dm_sender_id() {
            org_office::attachments::DeliveryScope::DirectSender(sender_id.to_owned())
        } else {
            org_office::attachments::DeliveryScope::None
        }
    }
}

impl fmt::Display for PartitionKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PartitionState {
    pub runtime_session_id: Option<String>,
    pub turns: u32,
    pub context_tokens: u64,
    pub context_window_tokens: Option<u64>,
    pub rotations: u32,
}

impl PartitionState {
    pub fn needs_rotation(&self) -> bool {
        let context_full = self.context_window_tokens.is_some_and(|window| {
            window > 0
                && self.context_tokens.saturating_mul(100)
                    >= window.saturating_mul(ROTATE_AT_CONTEXT_PERCENT)
        });
        self.turns >= ROTATE_AFTER_TURNS || context_full
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PartitionTurn {
    pub key: PartitionKey,
    pub resume_runtime_session_id: Option<String>,
    pub rotated: bool,
    pub turn: u32,
}

#[derive(Clone, Default)]
pub struct PartitionLedger {
    inner: Arc<Mutex<HashMap<(String, PartitionKey), PartitionState>>>,
}

impl PartitionLedger {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn begin_turn(&self, employee_id: &str, key: &PartitionKey) -> PartitionTurn {
        let mut map = self.inner.lock().unwrap_or_else(|p| p.into_inner());
        let state = map
            .entry((employee_id.to_owned(), key.clone()))
            .or_default();
        let rotated = state.needs_rotation();
        if rotated {
            *state = PartitionState {
                rotations: state.rotations.saturating_add(1),
                ..PartitionState::default()
            };
        }
        state.turns = state.turns.saturating_add(1);
        PartitionTurn {
            key: key.clone(),
            resume_runtime_session_id: state.runtime_session_id.clone(),
            rotated,
            turn: state.turns,
        }
    }

    pub fn needs_rotation(&self, employee_id: &str, key: &PartitionKey) -> bool {
        self.state(employee_id, key)
            .is_some_and(|state| state.needs_rotation())
    }

    pub fn record_runtime_session(
        &self,
        employee_id: &str,
        key: &PartitionKey,
        runtime_session_id: &str,
    ) {
        let mut map = self.inner.lock().unwrap_or_else(|p| p.into_inner());
        if let Some(state) = map.get_mut(&(employee_id.to_owned(), key.clone())) {
            state.runtime_session_id = Some(runtime_session_id.to_owned());
        }
    }

    pub fn observe_context(
        &self,
        employee_id: &str,
        key: &PartitionKey,
        context_tokens: u64,
        context_window_tokens: Option<u64>,
    ) {
        let mut map = self.inner.lock().unwrap_or_else(|p| p.into_inner());
        if let Some(state) = map.get_mut(&(employee_id.to_owned(), key.clone())) {
            state.context_tokens = state.context_tokens.max(context_tokens);
            state.context_window_tokens = context_window_tokens.or(state.context_window_tokens);
        }
    }

    pub fn state(&self, employee_id: &str, key: &PartitionKey) -> Option<PartitionState> {
        let map = self.inner.lock().unwrap_or_else(|p| p.into_inner());
        map.get(&(employee_id.to_owned(), key.clone())).cloned()
    }

    pub fn clear_partition(&self, employee_id: &str, key: &PartitionKey) -> bool {
        let mut map = self.inner.lock().unwrap_or_else(|p| p.into_inner());
        map.remove(&(employee_id.to_owned(), key.clone())).is_some()
    }
}

#[derive(Clone)]
pub struct PartitionHandle {
    ledger: PartitionLedger,
    employee_id: String,
    key: PartitionKey,
}

impl PartitionHandle {
    pub fn new(ledger: PartitionLedger, employee_id: impl Into<String>, key: PartitionKey) -> Self {
        Self {
            ledger,
            employee_id: employee_id.into(),
            key,
        }
    }

    pub fn key(&self) -> &PartitionKey {
        &self.key
    }

    pub fn record_runtime_session(&self, runtime_session_id: &str) {
        self.ledger
            .record_runtime_session(&self.employee_id, &self.key, runtime_session_id);
    }

    pub fn observe_usage(&self, usage: &org_agent_runtime::Usage) {
        let context = usage
            .input
            .saturating_add(usage.cache_read)
            .saturating_add(usage.cache_write);
        self.ledger.observe_context(
            &self.employee_id,
            &self.key,
            context,
            Some(DEFAULT_CONTEXT_WINDOW_TOKENS),
        );
    }
}

#[cfg(test)]
mod tests;
