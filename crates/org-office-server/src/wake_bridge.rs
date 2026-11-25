//! Bridge connecting `ChatService` and `who` in `org-office-server` to `SessionManager`
//! autonomous wake-ups, required-ask suspend/resume, and live presence (`TASK-47`).

use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use org_office::directory::{MemberRuntime, RuntimeView};
use org_office::messaging::asks::{AnswerCommitted, AskCommitted, WithdrawCommitted};
use org_office::messaging::features::ChatSendCommitted;
use org_office::organization_family::records::EmployeeKind;
use org_office::Office;
use org_office_sessions::{
    PendingWakeItem, SessionManager, SuspendedAskEntry, WakeKind, WakeOutcome,
};
use org_proto::chat::{ConversationKind, DeliveredBecause};
use org_proto::directory::RecipientPresence;

const KIND_ANNOUNCEMENT: &str = "announcement";
const KIND_HANDOFF: &str = "handoff";

/// Live presence overlay feeding `SessionManager` runtime state into `Office::who_with_runtime`.
pub(crate) struct SessionRuntimeOverlay {
    sessions: Arc<SessionManager>,
}

impl SessionRuntimeOverlay {
    pub(crate) fn new(sessions: Arc<SessionManager>) -> Self {
        Self { sessions }
    }
}

impl RuntimeView for SessionRuntimeOverlay {
    fn runtime_of(&self, member_id: &str) -> Option<MemberRuntime> {
        self.sessions.member_runtime_presence(member_id)
    }
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as u64)
}

fn sender_meta(office: &Office, sender_id: &str) -> (String, bool) {
    match office.load_employee(sender_id).ok().flatten() {
        Some(emp) => (emp.name, emp.kind == EmployeeKind::Person),
        None => (sender_id.to_owned(), sender_id == office.owner_member_id()),
    }
}

/// Processes a committed `chat.send`: wakes eligible idle agents (`WAKE-2`, `WAKE-7`, `AVAIL-1`,
/// `AVAIL-3`, `LIVE-7`) and computes `AVAIL-5` recipient presence.
pub(crate) fn on_chat_send_committed(
    office: &Office,
    sessions: &SessionManager,
    sender_id: &str,
    committed: &ChatSendCommitted,
) -> (Vec<String>, Vec<RecipientPresence>) {
    let mut recipient_ids: Vec<String> = committed
        .recipients
        .iter()
        .map(|(id, _)| id.as_str().to_owned())
        .collect();
    if recipient_ids.is_empty() {
        for m in &committed.result.mentions {
            if m.employee_id != sender_id && !recipient_ids.contains(&m.employee_id) {
                recipient_ids.push(m.employee_id.clone());
            }
        }
    }
    let presence = sessions.recipient_presence_for(&recipient_ids);
    if !committed.inserted || committed.message.kind == KIND_ANNOUNCEMENT {
        return (Vec::new(), presence);
    }
    let (sender_name, sender_is_person) = sender_meta(office, sender_id);
    let ts = now_ms();
    let mut woke = Vec::new();
    for (recip_id, state) in &committed.recipients {
        let target_id = recip_id.as_str();
        if target_id == sender_id || state.owner_office_id.as_str() != office.info().office_id {
            continue;
        }
        let is_mentioned = committed
            .result
            .mentions
            .iter()
            .any(|m| m.employee_id == target_id);
        let kind = wake_kind_for_send(committed, target_id, state.delivered_because, is_mentioned);
        let Some(kind) = kind else { continue };
        let item = PendingWakeItem {
            item_id: format!("msg:{}:{target_id}", committed.message.id),
            kind,
            sender_id: sender_id.to_owned(),
            sender_name: sender_name.clone(),
            sender_is_person,
            conversation_id: Some(committed.result.conversation_id.clone()),
            source_id: committed.message.id.clone(),
            summary: committed.message.body_markdown.clone(),
            enqueued_at_ms: ts,
        };
        if sessions.enqueue_wake(target_id, item) == WakeOutcome::Started {
            woke.push(target_id.to_owned());
        }
    }
    (woke, presence)
}

fn wake_kind_for_send(
    committed: &ChatSendCommitted,
    target_id: &str,
    delivered_because: DeliveredBecause,
    is_mentioned: bool,
) -> Option<WakeKind> {
    if committed.message.kind == KIND_HANDOFF
        && (delivered_because == DeliveredBecause::Addressed
            || matches!(
                committed.conversation_kind,
                ConversationKind::Dm | ConversationKind::GroupDm
            ))
    {
        return Some(WakeKind::Handoff);
    }
    // Thread reply in an agent's OWN open question wakes the asker for continuation (`ADR-0023` d2, `LIVE-7`).
    if committed.open_question_asker_id.as_deref() == Some(target_id) {
        return Some(WakeKind::AskAnswer);
    }
    if delivered_because == DeliveredBecause::Mention || is_mentioned {
        return Some(WakeKind::Mention);
    }
    if matches!(
        committed.conversation_kind,
        ConversationKind::Dm | ConversationKind::GroupDm
    ) {
        return Some(WakeKind::DirectMessage);
    }
    None
}

/// Processes a committed `chat.ask`: registers open required asks (`ASK-3`, `ASK-11`), wakes
/// addressed agent recipients (`WAKE-1`, `ASK-9`), and computes `AVAIL-5` recipient presence.
pub(crate) fn on_chat_ask_committed(
    office: &Office,
    sessions: &SessionManager,
    sender_id: &str,
    committed: &AskCommitted,
) -> (Vec<String>, Vec<RecipientPresence>) {
    let mut recipient_ids: Vec<String> = committed
        .recipients
        .iter()
        .map(|(id, _)| id.as_str().to_owned())
        .collect();
    if recipient_ids.is_empty() {
        if let Some(target) = &committed.card.addressed_to {
            if target != sender_id {
                recipient_ids.push(target.clone());
            }
        }
    }
    let presence = sessions.recipient_presence_for(&recipient_ids);
    if !committed.inserted {
        return (Vec::new(), presence);
    }
    let (sender_name, sender_is_person) = sender_meta(office, sender_id);
    let question_summary = committed
        .card
        .questions
        .iter()
        .map(|q| q.prompt.as_str())
        .collect::<Vec<_>>()
        .join(" / ");
    let ts = now_ms();

    if committed.card.required && !sender_is_person {
        let waiting_id = committed
            .card
            .addressed_to
            .clone()
            .or_else(|| recipient_ids.first().cloned());
        let waiting_name = waiting_id.as_deref().map(|id| sender_meta(office, id).0);
        let asked_by_subagent = committed
            .message
            .metadata
            .get("asked_by_subagent")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false);
        sessions.on_required_ask_opened(
            sender_id,
            SuspendedAskEntry {
                ask_id: committed.result.ask_id.clone(),
                conversation_id: committed.result.conversation_id.clone(),
                question_summary: question_summary.clone(),
                waiting_for_member_id: waiting_id,
                waiting_for_name: waiting_name,
                asked_by_subagent,
                opened_at_ms: ts,
            },
        );
    }

    let mut woke = Vec::new();
    for (recip_id, _) in &committed.recipients {
        let target_id = recip_id.as_str();
        if target_id == sender_id {
            continue;
        }
        let item = PendingWakeItem {
            item_id: format!("ask:{}:{target_id}", committed.result.ask_id),
            kind: WakeKind::Ask,
            sender_id: sender_id.to_owned(),
            sender_name: sender_name.clone(),
            sender_is_person,
            conversation_id: Some(committed.result.conversation_id.clone()),
            source_id: committed.result.ask_id.clone(),
            summary: question_summary.clone(),
            enqueued_at_ms: ts,
        };
        if sessions.enqueue_wake(target_id, item) == WakeOutcome::Started {
            woke.push(target_id.to_owned());
        }
    }
    (woke, presence)
}

/// Processes a committed `chat.answer`: resumes a suspended asker (`ASK-5..9`) or wakes an idle asker (`WAKE-7`).
pub(crate) fn on_chat_answer_committed(
    office: &Office,
    sessions: &SessionManager,
    responder_id: &str,
    committed: &AnswerCommitted,
) {
    let Some(asker_id) = committed.asker_id.as_deref() else {
        return;
    };
    let (responder_name, responder_is_person) = sender_meta(office, responder_id);
    let _ = sessions.on_ask_answered(
        asker_id,
        &committed.result.ask_id,
        &committed.question_summary,
        &committed.answer_summary,
        responder_id,
        &responder_name,
        responder_is_person,
        false,
    );
}

/// Processes a committed `msg.withdraw`: clears suspended state and pending wakes for the ask.
pub(crate) fn on_msg_withdraw_committed(
    sessions: &SessionManager,
    caller_id: &str,
    committed: &WithdrawCommitted,
) {
    let asker = committed.asker_id.as_deref().unwrap_or(caller_id);
    sessions.on_ask_withdrawn(asker, &committed.result.ask_id);
}
