//! Per-caller session partitions of the built-in Chief of Staff (`tess:ask:<caller_id>`,
//! `TASK-80.1`, `doc-97`).
//!
//! Every member who asks Tess talks to their own conversation: the owner's context never
//! reaches another member's answers. A partition is keyed by the caller and holds its own
//! runtime conversation ID, so a switch between callers resumes the next caller's conversation
//! rather than continuing the last one. A wake with what one sender addressed to her (a DM, a
//! hand-off, an ask) runs in that sender's partition: a person's `tess:ask:<id>`, acting for
//! them, or anyone else's `tess:dm:<id>`, acting for nobody. Other turns with no caller (a
//! mention, an assignment, a resumed ask) run in the separate `tess:wake` partition, which
//! never receives a sender's private message.
//!
//! A partition **rotates** to a fresh conversation once it has run [`ROTATE_AFTER_TURNS`]
//! turns or filled [`ROTATE_AT_CONTEXT_PERCENT`] of its context window, so a long-lived
//! partition never drags an unbounded history into every turn.
//!
//! The ledger lives in RAM: after an office restart a partition starts a fresh conversation,
//! which is always safe (no context crosses callers).

use std::collections::HashMap;
use std::fmt;
use std::sync::{Arc, Mutex};

/// The prefix of a caller's partition key.
pub const TESS_ASK_PREFIX: &str = "tess:ask:";
/// The key of the partition for turns that act for nobody.
pub const TESS_WAKE_PARTITION: &str = "tess:wake";
/// The prefix of the partition of what one sender who is not a person addressed to her.
pub const TESS_DM_PREFIX: &str = "tess:dm:";
/// Turns a partition runs before it rotates to a fresh conversation.
pub const ROTATE_AFTER_TURNS: u32 = 20;
/// The share of the context window, in percent, at which a partition rotates.
pub const ROTATE_AT_CONTEXT_PERCENT: u64 = 60;
/// The context window assumed when the runtime does not report one: the smallest window of
/// the models the office runs (200k tokens). Too small a guess only rotates earlier, which is
/// always safe.
pub const DEFAULT_CONTEXT_WINDOW_TOKENS: u64 = 200_000;

/// A partition of the Chief of Staff's sessions.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct PartitionKey(String);

impl PartitionKey {
    /// The partition of `caller_id`'s requests: `tess:ask:<caller_id>`.
    pub fn tess_ask(caller_id: &str) -> Self {
        Self(format!("{TESS_ASK_PREFIX}{caller_id}"))
    }

    /// The partition of turns that act for nobody: `tess:wake`.
    pub fn tess_wake() -> Self {
        Self(TESS_WAKE_PARTITION.to_owned())
    }

    /// The partition of what `sender_id`, who is not a person, addressed to her:
    /// `tess:dm:<sender_id>`. It acts for nobody, and no other sender's message joins it.
    pub fn tess_dm(sender_id: &str) -> Self {
        Self(format!("{TESS_DM_PREFIX}{sender_id}"))
    }

    /// The partition of a run acting for `caller_id`, or for nobody.
    pub fn for_caller(caller_id: Option<&str>) -> Self {
        caller_id.map_or_else(Self::tess_wake, Self::tess_ask)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The caller a `tess:ask:` partition belongs to.
    pub fn caller_id(&self) -> Option<&str> {
        self.0.strip_prefix(TESS_ASK_PREFIX)
    }

    /// Alias for [`Self::caller_id`], naming the delegated human caller for `tess:ask:` turns.
    pub fn acting_for_caller_id(&self) -> Option<&str> {
        self.caller_id()
    }

    /// The non-person sender a `tess:dm:` partition belongs to.
    pub fn dm_sender_id(&self) -> Option<&str> {
        self.0.strip_prefix(TESS_DM_PREFIX)
    }

    /// Maps this Chief of Staff partition to the [`org_office::attachments::DeliveryScope`]
    /// enforced on turn attachment resolution (`TASK-80.1`, `TASK-98.3`).
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

/// What the office knows of one partition's conversation.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PartitionState {
    /// The runtime's conversation to resume, once one has run.
    pub runtime_session_id: Option<String>,
    /// Turns run in the current conversation.
    pub turns: u32,
    /// Context the current conversation fills, in tokens, as last observed.
    pub context_tokens: u64,
    /// The model's context window, in tokens, when known.
    pub context_window_tokens: Option<u64>,
    /// How many times the partition rotated to a fresh conversation.
    pub rotations: u32,
}

impl PartitionState {
    /// Whether the next turn must start a fresh conversation: [`ROTATE_AFTER_TURNS`] turns run,
    /// or [`ROTATE_AT_CONTEXT_PERCENT`] of a known context window filled.
    pub fn needs_rotation(&self) -> bool {
        let context_full = self.context_window_tokens.is_some_and(|window| {
            window > 0
                && self.context_tokens.saturating_mul(100)
                    >= window.saturating_mul(ROTATE_AT_CONTEXT_PERCENT)
        });
        self.turns >= ROTATE_AFTER_TURNS || context_full
    }
}

/// What a turn about to run in a partition resumes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PartitionTurn {
    pub key: PartitionKey,
    /// The partition's conversation to resume, `None` for a fresh one.
    pub resume_runtime_session_id: Option<String>,
    /// The partition rotated: its old conversation is left behind.
    pub rotated: bool,
    /// The turn's number in the partition's current conversation, from 1.
    pub turn: u32,
}

/// Every partition of every Chief of Staff on this office, in RAM.
#[derive(Clone, Default)]
pub struct PartitionLedger {
    inner: Arc<Mutex<HashMap<(String, PartitionKey), PartitionState>>>,
}

impl PartitionLedger {
    pub fn new() -> Self {
        Self::default()
    }

    /// Starts a turn of `employee_id` in partition `key`: rotates first when the partition
    /// needs it, counts the turn, and answers the conversation to resume.
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

    /// Whether `employee_id`'s partition `key` must rotate before its next turn.
    pub fn needs_rotation(&self, employee_id: &str, key: &PartitionKey) -> bool {
        self.state(employee_id, key)
            .is_some_and(|state| state.needs_rotation())
    }

    /// Records the runtime conversation partition `key` of `employee_id` holds, so the next
    /// turn in it resumes that conversation. Ignored when the partition is unknown.
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

    /// Records how much of its context window partition `key`'s conversation fills.
    pub fn observe_context(
        &self,
        employee_id: &str,
        key: &PartitionKey,
        context_tokens: u64,
        context_window_tokens: Option<u64>,
    ) {
        let mut map = self.inner.lock().unwrap_or_else(|p| p.into_inner());
        if let Some(state) = map.get_mut(&(employee_id.to_owned(), key.clone())) {
            // The fullest the conversation has been: a smaller report (one turn's share, say)
            // never postpones its rotation.
            state.context_tokens = state.context_tokens.max(context_tokens);
            state.context_window_tokens = context_window_tokens.or(state.context_window_tokens);
        }
    }

    /// A snapshot of one partition.
    pub fn state(&self, employee_id: &str, key: &PartitionKey) -> Option<PartitionState> {
        let map = self.inner.lock().unwrap_or_else(|p| p.into_inner());
        map.get(&(employee_id.to_owned(), key.clone())).cloned()
    }
}

/// One run's partition, as its pump sees it: the run records the conversation it holds the
/// moment the runtime names it (so stopping, exiting or crashing never loses a caller's
/// conversation) and how much of the context window each turn fills (so the partition rotates
/// at [`ROTATE_AT_CONTEXT_PERCENT`]).
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

    /// The partition the run serves.
    pub fn key(&self) -> &PartitionKey {
        &self.key
    }

    /// Records the runtime conversation the run holds (`SessionStarted`).
    pub fn record_runtime_session(&self, runtime_session_id: &str) {
        self.ledger
            .record_runtime_session(&self.employee_id, &self.key, runtime_session_id);
    }

    /// Records a turn's usage as the context its conversation fills: every input token the
    /// turn sent (fresh, cache read and cache written). A turn of several model calls counts
    /// each call's input, so it can only overstate, and an overstatement only rotates sooner.
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
