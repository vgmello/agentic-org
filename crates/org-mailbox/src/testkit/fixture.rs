//! The standard organization, clock, ids and recorders the conformance suite runs on.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, MutexGuard};

use org_proto::msg::{MessageDeliveryFailedData, MessageStateChangedData};
use org_proto::{AgentProfile, AgentRuntime, AgentStatus, Member, MemberKind, Position};

use crate::{Clock, MessageEvent, MessageEvents};

/// The office the suite's mailbox serves: the organization's HQ.
pub const OFFICE_ID: &str = "01JCOFFICEHQ00000000000000";
pub const OTHER_OFFICE_ID: &str = "01JCOFFICEFAR0000000000000";
pub const INCARNATION: &str = "01JCINCARNATION00000000000";
/// org.db's applied position the directory reports.
pub const POSITION: Position = Position { epoch: 1, seq: 42 };

/// The owner, a person: addressed only as `owner`, never by display name.
pub const OWNER_ID: &str = "01JCMOWNER0000000000000000";
pub const OWNER_NAME: &str = "Robin Owner";
/// A person; their name is their handle.
pub const PAT_ID: &str = "01JCMPAT000000000000000000";
pub const PAT_HANDLE: &str = "Pat";
/// A person whose handle isn't ASCII: handles fold case by Unicode lower case.
pub const EMILE_ID: &str = "01JCMEMILE0000000000000000";
pub const EMILE_HANDLE: &str = "\u{c9}mile";
/// An active agent at this office.
pub const ADA_ID: &str = "01JCMADA000000000000000000";
pub const ADA_HANDLE: &str = "ada";
pub const ADA_NAME: &str = "Ada Lovelace";
/// A paused agent at this office: it still binds and receives.
pub const REMY_ID: &str = "01JCMREMY00000000000000000";
pub const REMY_HANDLE: &str = "remy";
/// An offboarded agent at this office: refused as a recipient and as a binding.
pub const GONE_ID: &str = "01JCMGONE00000000000000000";
pub const GONE_HANDLE: &str = "gone";
/// An agent whose status this build can't read: treated like an offboarded one.
pub const ODD_ID: &str = "01JCMODD000000000000000000";
pub const ODD_HANDLE: &str = "odd";
/// An active agent placed in another office (cross-office routing is out of slice 1).
pub const FAR_ID: &str = "01JCMFAR000000000000000000";
pub const FAR_HANDLE: &str = "far";
/// An agent whose handle is spelled like Pat's id: Pat's exact id must still win. HQ refuses
/// an address-shaped handle (`invalid_name`), so a backend must seed [`members`] raw, not
/// through org-proto's validated commands.
pub const SHADOW_ID: &str = "01JCMSHADOW000000000000000";

/// 2026-11-08T12:00:00.000Z, the suite's wall clock.
pub const NOW_MS: u64 = 1_794_139_200_000;
pub const NOW_UTC: &str = "2026-11-08T12:00:00.000Z";

fn person(id: &str, name: &str, owner: bool) -> Member {
    Member {
        id: id.into(),
        kind: MemberKind::Person,
        name: name.into(),
        contact: None,
        owner,
        office_id: None,
        agent: None,
        reports_to_member_id: None,
        created_at: None,
        version: 1,
    }
}

fn agent(id: &str, name: &str, handle: &str, status: AgentStatus, office_id: &str) -> Member {
    Member {
        id: id.into(),
        kind: MemberKind::Agent,
        name: name.into(),
        contact: None,
        owner: false,
        office_id: Some(office_id.into()),
        agent: Some(AgentProfile {
            role: "software engineer".into(),
            title: "Engineer".into(),
            focus: None,
            runtime: AgentRuntime::ClaudeCode,
            model: None,
            run_mode: None,
            schedule: None,
            home: None,
            messaging_name: handle.into(),
            status,
            offboarded_at: (status == AgentStatus::Offboarded).then(|| NOW_UTC.into()),
        }),
        reports_to_member_id: None,
        created_at: None,
        version: 1,
    }
}

/// The organization every conformance case opens its backend with.
pub fn members() -> Vec<Member> {
    vec![
        person(OWNER_ID, OWNER_NAME, true),
        person(PAT_ID, PAT_HANDLE, false),
        person(EMILE_ID, EMILE_HANDLE, false),
        agent(ADA_ID, ADA_NAME, ADA_HANDLE, AgentStatus::Active, OFFICE_ID),
        agent(REMY_ID, "Remy", REMY_HANDLE, AgentStatus::Paused, OFFICE_ID),
        agent(
            GONE_ID,
            "Gone",
            GONE_HANDLE,
            AgentStatus::Offboarded,
            OFFICE_ID,
        ),
        agent(ODD_ID, "Odd", ODD_HANDLE, AgentStatus::Unknown, OFFICE_ID),
        agent(
            FAR_ID,
            "Far",
            FAR_HANDLE,
            AgentStatus::Active,
            OTHER_OFFICE_ID,
        ),
        agent(SHADOW_ID, "Shadow", PAT_ID, AgentStatus::Active, OFFICE_ID),
    ]
}

/// The `n`th client-made message id: a canonical ULID.
pub fn msg_id(n: u32) -> String {
    format!("01JCMSG{n:019}")
}

/// A wall clock the test sets. Blocking waits use `Instant`, not this.
#[derive(Debug)]
pub struct ManualClock(AtomicU64);

impl ManualClock {
    pub fn new(ms: u64) -> Self {
        Self(AtomicU64::new(ms))
    }

    pub fn set(&self, ms: u64) {
        self.0.store(ms, Ordering::SeqCst);
    }
}

impl Clock for ManualClock {
    fn now_ms(&self) -> u64 {
        self.0.load(Ordering::SeqCst)
    }
}

/// Records every event the mailbox publishes.
#[derive(Debug, Default)]
pub struct RecordingEvents(Mutex<Vec<MessageEvent>>);

impl RecordingEvents {
    fn events(&self) -> MutexGuard<'_, Vec<MessageEvent>> {
        self.0.lock().unwrap_or_else(|error| error.into_inner())
    }

    /// Ids of `message.received` events, in publish order.
    pub fn received(&self) -> Vec<String> {
        self.events()
            .iter()
            .filter_map(|event| match event {
                MessageEvent::Received(data) => Some(data.header.id.clone()),
                _ => None,
            })
            .collect()
    }

    /// Ids of `message.state_changed` events, in publish order.
    pub fn state_changed(&self) -> Vec<String> {
        self.events()
            .iter()
            .filter_map(|event| match event {
                MessageEvent::StateChanged(data) => Some(data.id.clone()),
                _ => None,
            })
            .collect()
    }
}

impl RecordingEvents {
    /// `message.state_changed` events, in publish order.
    pub fn state_changes(&self) -> Vec<MessageStateChangedData> {
        self.events()
            .iter()
            .filter_map(|event| match event {
                MessageEvent::StateChanged(data) => Some(data.clone()),
                _ => None,
            })
            .collect()
    }

    /// `message.delivery_failed` events, in publish order.
    pub fn delivery_failed(&self) -> Vec<MessageDeliveryFailedData> {
        self.events()
            .iter()
            .filter_map(|event| match event {
                MessageEvent::DeliveryFailed(data) => Some(data.clone()),
                _ => None,
            })
            .collect()
    }
}

impl MessageEvents for RecordingEvents {
    fn publish(&self, event: MessageEvent) {
        self.events().push(event);
    }
}
