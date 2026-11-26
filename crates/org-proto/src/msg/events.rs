//! Message events: `message.received`, `message.state_changed` and
//! `message.delivery_failed`.

use serde::{Deserialize, Serialize};

use super::message::{DeliveryFailedReason, MessageHeader, MessageLifecycleState};
use crate::events::EventKind;

/// `message.received`: a message was committed for a recipient at this office. Carries only
/// the header, so frames stay small and bodies never reach subscribers.
///
/// Sent to app-bound connections, and to other member-bound connections only for messages
/// their member sent or received; never to unbound ones (see [Events](super#events)).
pub struct MessageReceived;

/// The data of a `message.received` event.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MessageReceivedData {
    pub header: MessageHeader,
}

impl EventKind for MessageReceived {
    const NAME: &'static str = "message.received";
    type Data = MessageReceivedData;
}

/// `message.state_changed`: a message's state changed (read, snoozed, released, done,
/// expired...), or an ask to a person or the owner passed its deadline
/// (`expired_waiting_person`).
///
/// Sent to app-bound connections, and to other member-bound connections only for messages
/// their member sent or received; never to unbound ones (see [Events](super#events)).
pub struct MessageStateChanged;

/// The data of a `message.state_changed` event.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MessageStateChangedData {
    pub id: String,
    pub thread_id: String,
    pub to_member_id: String,
    pub state: MessageLifecycleState,
    pub triage_gen: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub snoozed_until: Option<String>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub snooze_after_task: bool,
    /// A snooze or done note, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    /// As [`MessageHeader::expired_waiting_person`]: the state stays `unread`.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub expired_waiting_person: bool,
    pub updated_at: String,
}

impl EventKind for MessageStateChanged {
    const NAME: &'static str = "message.state_changed";
    type Data = MessageStateChangedData;
}

/// `message.delivery_failed`: a message couldn't be delivered. The server also sends its
/// sender a `delivery_failed` message (from [`OFFICE_SENDER`], replying to the original), which
/// answers a held `msg.ask`.
///
/// Sent to app-bound connections, and to other member-bound connections only for messages
/// their member sent or received; never to unbound ones (see [Events](super#events)).
pub struct MessageDeliveryFailed;

/// The data of a `message.delivery_failed` event.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MessageDeliveryFailedData {
    /// The `delivery_failed` message's id.
    pub id: String,
    /// The message that couldn't be delivered.
    pub original_id: String,
    pub thread_id: String,
    /// The original's sender, who gets the notice.
    pub from_member_id: String,
    pub recipient_id: String,
    /// The recipient's name as addressed at send time; at most [`MAX_HANDLE_BYTES`].
    pub recipient_name: String,
    pub reason: DeliveryFailedReason,
}

impl EventKind for MessageDeliveryFailed {
    const NAME: &'static str = "message.delivery_failed";
    type Data = MessageDeliveryFailedData;
}
