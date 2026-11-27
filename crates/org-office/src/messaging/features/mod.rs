//! Engine-independent domain decisions for the messaging family (`doc-90`, `ADR-0003`,
//! `ADR-0014`, `ADR-0024`).
//!
//! Every write commits through `org-store` with `Ack::Committed` (`durable = false`), writing
//! canonical JSON envelopes under `durable/messaging/conversations/...` and projecting into
//! `MessagingWrite` (`protected/messaging.db` on SQLite, `MessagingTables` on Memory).

use org_proto::chat::ConversationKind;
use org_proto::{ApiError, ErrorCode};
use org_store::{Change, CoreRead, Engine, Id, Refusal, UnitOfWork};

use crate::organization::records::{Employee, EmployeeStatus};

use super::{ports::MessagingRead, records::ConversationRecord};

mod answers;
mod auto_channels;
mod catchup;
mod channels;
mod conversations;
mod dm;
mod handoff;
mod inbox;
mod messages;
mod read_state;
mod recipients;
mod reply;
mod send;

pub use answers::{deliver_answer_candidates, AnswerCandidateDelivery};
pub use auto_channels::virtual_auto_channels;
pub(crate) use auto_channels::{effective_auto_members, slugify_channel};
pub use catchup::{channel_catchup_for_caller, snippet, ChannelCatchup};
pub use channels::{decide_channel_create, decide_channel_membership};
pub(crate) use conversations::all_conversations_including_auto;
pub use conversations::{list_channels, list_conversations};
pub use dm::decide_dm_open;
pub use handoff::decide_accept_handoff;
pub use inbox::list_chat_inbox;
pub use messages::{
    channel_messages_for_caller, is_reply_kind, list_chat_messages, to_wire_header,
};
pub use read_state::{decide_mark_read, decide_mute, decide_react, plan_mark_read, MarkReadPlan};
pub(super) use recipients::{append_send_recipient_states, resolve_send_target};
pub(crate) use recipients::{
    build_mention_candidates, collect_recipient_reasons, upsert_participant_snapshot,
};
pub use reply::decide_chat_reply;
pub use send::{decide_chat_send, ChatSendCommitted};

pub(super) fn refusal(code: ErrorCode, msg: impl Into<String>) -> Refusal {
    Refusal::rejected(serde_json::to_vec(&ApiError::new(code, msg)).expect("ApiError serializes"))
}

/// Decodes a `Refusal` encoded by this module back into an `ApiError`.
pub fn decode_refusal(r: &Refusal) -> ApiError {
    match r {
        Refusal::Rejected(bytes) => serde_json::from_slice(bytes)
            .unwrap_or_else(|_| ApiError::new(ErrorCode::InvalidRequest, "request refused")),
        Refusal::Invalid(err) => ApiError::new(ErrorCode::InvalidRequest, err.to_string()),
    }
}

pub(super) fn is_active_employee(emp: &Employee) -> bool {
    matches!(emp.status, EmployeeStatus::Active | EmployeeStatus::Paused)
}

pub(crate) fn resolve_employee<'a>(
    employees: &'a [Employee],
    owner: &Id,
    reference: &str,
) -> Result<&'a Employee, ApiError> {
    let trimmed = reference.trim();
    if trimmed.eq_ignore_ascii_case("office") {
        return Err(ApiError::new(
            ErrorCode::InvalidRequest,
            "the office cannot be addressed here",
        ));
    }
    if trimmed.eq_ignore_ascii_case("owner") {
        return employees
            .iter()
            .find(|e| e.id == *owner && is_active_employee(e))
            .ok_or_else(|| ApiError::new(ErrorCode::NotFound, "owner not found"));
    }
    if let Some(by_id) = employees.iter().find(|e| e.id.as_str() == trimmed) {
        return Ok(by_id);
    }
    let key = trimmed.to_lowercase();
    employees
        .iter()
        .find(|e| e.handle.to_lowercase() == key || e.name.trim().to_lowercase() == key)
        .ok_or_else(|| ApiError::new(ErrorCode::UnknownRecipient, "unknown recipient"))
}

/// Enforces `HIRE-2` (`doc-78`, ADR-0029 decision 3): only the owner, an active employee
/// who may hire somewhere ([`may_address_hr`]: never the built-in Chief of Staff), or HR
/// itself may address or `@mention` the builtin HR lead.
///
/// [`may_address_hr`]: crate::organization::features::authority::may_address_hr
pub(super) fn check_hr_access(
    read: &impl MessagingRead,
    employees: &[Employee],
    owner: &Id,
    caller: &Id,
    targets: impl IntoIterator<Item = Id>,
) -> Result<(), Refusal> {
    let hr_id = employees
        .iter()
        .find(|e| e.builtin_key == Some(crate::organization::records::BuiltinKey::Hr))
        .map(|e| &e.id);
    let Some(hr_id) = hr_id else {
        return Ok(());
    };
    if !targets.into_iter().any(|id| id == *hr_id) {
        return Ok(());
    }
    if caller == owner || caller == hr_id {
        return Ok(());
    }
    let Some(caller_emp) = employees
        .iter()
        .find(|e| e.id == *caller && is_active_employee(e))
    else {
        return Err(refusal(
            ErrorCode::NotAuthorized,
            crate::organization::features::HIRE_DENIED,
        ));
    };
    let allowed = crate::organization::features::authority::may_address_hr(read, owner, caller_emp)
        .map_err(|_| refusal(ErrorCode::StorageError, "failed to verify hire access"))?;
    if allowed {
        Ok(())
    } else {
        Err(refusal(
            ErrorCode::NotAuthorized,
            crate::organization::features::HIRE_DENIED,
        ))
    }
}

pub(super) fn require_conversation_access(
    read: &impl MessagingRead,
    owner: &Id,
    caller: &Id,
    conv_id: &Id,
    now_utc: &str,
) -> Result<(ConversationRecord, bool), Refusal> {
    let persisted = read
        .conversation(conv_id)
        .map_err(|_| refusal(ErrorCode::StorageError, "failed to read conversation"))?;
    let (conv, was_persisted) = match persisted {
        Some(c) => (c, true),
        None => {
            let virtuals = virtual_auto_channels(read, now_utc)
                .map_err(|_| refusal(ErrorCode::StorageError, "failed to load channels"))?;
            let found = virtuals
                .into_iter()
                .find(|c| c.id == *conv_id)
                .ok_or_else(|| refusal(ErrorCode::NotFound, "conversation not found"))?;
            (found, false)
        }
    };
    let active = effective_auto_members(read, &conv, owner)
        .map_err(|_| refusal(ErrorCode::StorageError, "failed to resolve members"))?;
    let allowed =
        conv.kind == ConversationKind::Channel || active.contains(caller) || caller == owner;
    if !allowed {
        return Err(refusal(ErrorCode::NotFound, "conversation not found"));
    }
    Ok((conv, was_persisted))
}

pub(super) fn put_or_create_file<E: Engine>(
    uow: &UnitOfWork<'_, E>,
    change: Change,
    path: org_store::DurablePath,
    bytes: Vec<u8>,
) -> Result<Change, Refusal> {
    let existed = uow
        .view()
        .source_hash(&path)
        .map_err(|_| refusal(ErrorCode::StorageError, "failed to check durable file"))?
        .is_some();
    Ok(if existed {
        change.put(path, bytes)
    } else {
        change.create(path, bytes)
    })
}
