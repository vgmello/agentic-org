//! Live events: one stream per machine. After `subscribe`, orgd sends each one as a JSON-RPC
//! notification whose method is [`crate::EVENT_NOTIFICATION`] and whose params are the event.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::hire::HireProgress;
use crate::ids::{AgentId, MachineId, Timestamp, WorkspacePath};
use crate::model::{ActivityEvent, Goal, Machine, NeedsYouItem, Presence, Session};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum NeedsYouClearReason {
    CarriedOn,
    Dismissed,
    Resumed,
    Resolved,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum OrgEvent {
    PresenceChanged {
        presence: Presence,
    },
    NeedsYouOpened {
        item: NeedsYouItem,
    },
    NeedsYouCleared {
        machine_id: MachineId,
        item_id: String,
        reason: NeedsYouClearReason,
        /// When it cleared (for `dismissed`: the item's `dismissed_at`).
        at: Timestamp,
    },
    SessionChanged {
        session: Session,
    },
    /// `goal: null`: the agent has no live goal any more (its session stopped, or the goal ended).
    GoalChanged {
        machine_id: MachineId,
        agent_id: AgentId,
        goal: Option<Goal>,
    },
    Activity {
        event: ActivityEvent,
    },
    FileChanged {
        machine_id: MachineId,
        path: WorkspacePath,
        agent_id: Option<AgentId>,
        by_open_session: bool,
    },
    MachineChanged {
        machine: Machine,
    },
    HireProgress {
        // Boxed: the hire payloads are much larger than the other events. Same JSON.
        progress: Box<HireProgress>,
    },
}
