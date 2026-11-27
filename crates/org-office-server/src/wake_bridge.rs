//! Bridge connecting `ChatService` and `who` in `org-office-server` to `SessionManager`
//! autonomous wake-ups, required-ask suspend/resume, and live presence (`TASK-47`).

use std::collections::BTreeSet;
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
use org_proto::tasks::TasksCommentResult;
use org_proto::{AgentStatus, MemberKind};

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
    let attachment_ids: Vec<String> = committed
        .message
        .attachments
        .iter()
        .map(|a| a.id.clone())
        .collect();
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
            sender_handle: None,
            sender_relation: None,
            sender_is_person,
            conversation_id: Some(committed.result.conversation_id.clone()),
            source_id: committed.message.id.clone(),
            summary: committed.message.body_markdown.clone(),
            attachment_ids: attachment_ids.clone(),
            session_affinity: None,
            enqueued_at_ms: ts,
            model_override: None,
            effort_override: None,
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
    // A DM is a direct message even when it `@mentions` her: a mention is a channel's wake,
    // and the Chief of Staff runs one in her shared wake partition (`TASK-80.1`).
    if matches!(
        committed.conversation_kind,
        ConversationKind::Dm | ConversationKind::GroupDm
    ) {
        return Some(WakeKind::DirectMessage);
    }
    if delivered_because == DeliveredBecause::Mention || is_mentioned {
        return Some(WakeKind::Mention);
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

    let attachment_ids: Vec<String> = committed
        .message
        .attachments
        .iter()
        .map(|a| a.id.clone())
        .collect();
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
            sender_handle: None,
            sender_relation: None,
            sender_is_person,
            conversation_id: Some(committed.result.conversation_id.clone()),
            source_id: committed.result.ask_id.clone(),
            summary: question_summary.clone(),
            attachment_ids: attachment_ids.clone(),
            session_affinity: None,
            enqueued_at_ms: ts,
            model_override: None,
            effort_override: None,
        };
        if sessions.enqueue_wake(target_id, item) == WakeOutcome::Started {
            woke.push(target_id.to_owned());
        }
    }
    (woke, presence)
}

/// Processes a committed `chat.answer`: resumes a suspended asker (`ASK-5..9`) or enqueues an
/// `AskAnswer` wake unless an active `msg ask`/`msg wait` waiter already received it (`TASK-115`).
pub(crate) fn on_chat_answer_committed(
    office: &Office,
    sessions: &SessionManager,
    responder_id: &str,
    committed: &AnswerCommitted,
    satisfied_active_waiter: bool,
) {
    let Some(asker_id) = committed.asker_id.as_deref() else {
        return;
    };
    let (responder_name, responder_is_person) = sender_meta(office, responder_id);
    let attachment_ids: Vec<String> = committed
        .result
        .answer
        .attachments
        .iter()
        .map(|a| a.id.clone())
        .collect();
    let _ = sessions.on_ask_answered_with_attachments(
        asker_id,
        &committed.result.ask_id,
        &committed.question_summary,
        &committed.answer_summary,
        (responder_id, &responder_name, responder_is_person),
        false,
        satisfied_active_waiter,
        &attachment_ids,
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

/// Processes a committed `tasks.comment`: wakes active agent assignee and watchers placed at this
/// office (excluding the comment author), carrying lightweight `attachment_ids` when files were
/// attached (never raw bytes or local paths, `TASK-98.4`).
pub(crate) fn on_task_comment_committed(
    office: &Office,
    sessions: &SessionManager,
    actor_id: &str,
    result: &TasksCommentResult,
) {
    let Ok(Ok(inspected)) = office.task_inspect(&result.task.id) else {
        return;
    };
    let mut candidates = BTreeSet::new();
    if let Some(assignee) = &inspected.task.assignee_id {
        candidates.insert(assignee.clone());
    }
    for watcher in &inspected.watchers {
        if watcher.watcher_kind == "employee" {
            if let Some(id) = &watcher.watcher_id {
                candidates.insert(id.clone());
            }
        }
    }
    candidates.remove(actor_id);
    if candidates.is_empty() {
        return;
    }
    let Ok(snapshot) = office.org_snapshot() else {
        return;
    };
    let local_office = office.info().office_id.as_str();
    let targets: Vec<String> = snapshot
        .members
        .iter()
        .filter(|m| {
            m.kind == MemberKind::Agent
                && m.agent
                    .as_ref()
                    .is_some_and(|a| a.status == AgentStatus::Active)
                && m.office_id.as_deref() == Some(local_office)
                && candidates.contains(&m.id)
        })
        .map(|m| m.id.clone())
        .collect();
    if targets.is_empty() {
        return;
    }
    let (sender_name, sender_is_person) = sender_meta(office, actor_id);
    let summary = comment_wake_summary(result);
    let attachment_ids: Vec<String> = result
        .comment
        .attachments
        .iter()
        .map(|a| a.id.clone())
        .collect();
    let task_override =
        org_office_sessions::parse_task_routing_labels_or_default(&inspected.task.labels);
    let ts = now_ms();
    for target_id in targets {
        let item = PendingWakeItem {
            item_id: format!("task-comment:{}:{target_id}", result.comment.id),
            kind: WakeKind::TaskAssignment,
            sender_id: actor_id.to_owned(),
            sender_name: sender_name.clone(),
            sender_handle: None,
            sender_relation: None,
            sender_is_person,
            conversation_id: None,
            source_id: result.task.key.clone(),
            summary: summary.clone(),
            attachment_ids: attachment_ids.clone(),
            session_affinity: None,
            enqueued_at_ms: ts,
            model_override: task_override.model.clone(),
            effort_override: task_override.effort_str().map(str::to_owned),
        };
        let _ = sessions.enqueue_wake(&target_id, item);
    }
}

fn comment_wake_summary(result: &TasksCommentResult) -> String {
    let trimmed = result.comment.body_markdown.trim();
    let mut summary = if trimmed.is_empty() {
        format!("Commented on {}", result.task.key)
    } else {
        let mut preview: String = trimmed.chars().take(240).collect();
        if trimmed.chars().count() > 240 {
            preview.push('…');
        }
        format!("Comment on {}: {preview}", result.task.key)
    };
    if !result.comment.attachments.is_empty() {
        let ids: Vec<&str> = result
            .comment
            .attachments
            .iter()
            .map(|a| a.id.as_str())
            .collect();
        summary.push_str(&format!(" [attachment_ids: {}]", ids.join(", ")));
    }
    summary
}

#[cfg(test)]
#[path = "wake_bridge_tests.rs"]
mod tests;
