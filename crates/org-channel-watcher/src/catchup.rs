//! What one `channel summarize` call digests: the question's recent window, or the caller's new
//! messages since its read marker (`--since-read`, `TASK-99` rung 3). An empty catch-up answers
//! here, before any watching runtime is resolved, so it calls no model and needs none.

use org_copilot::watcher::DERIVED_SUMMARY_NOTICE;
use org_office::messaging::records::{ConversationRecord, MessageRecord};
use org_office::Office;
use org_proto::chat::{ChatChannelSummarizeParams, ChatChannelSummarizeResult};
use org_proto::{ApiError, ErrorCode};

/// The question a `--since-read` digest answers when the caller asks none.
pub const CATCHUP_QUESTION: &str = "Catch me up on these new channel messages, oldest \
first. List, citing the supporting message IDs: decisions made; blockers raised; handoffs or \
requests for someone to act; open questions still unanswered. Write 'none' under a heading with \
nothing new. Report only what the messages say.";

/// The counts a `--since-read` digest answers with.
pub(super) struct CatchupCounts {
    pub(super) new_message_count: u64,
    pub(super) remaining_unread: u64,
    pub(super) through_message_id: Option<String>,
}

/// The messages and question one summarize run is given.
pub(super) struct Digest {
    pub(super) conversation: ConversationRecord,
    pub(super) question: String,
    pub(super) messages: Vec<MessageRecord>,
    pub(super) catchup: Option<CatchupCounts>,
}

impl Digest {
    pub(super) fn channel_label(&self) -> String {
        self.conversation
            .name
            .clone()
            .unwrap_or_else(|| self.conversation.id.as_str().to_owned())
    }

    /// The result fields a run fills in around the model's answer.
    pub(super) fn result(
        &self,
        summary: String,
        message_ids: Vec<String>,
        derived_notice: String,
        usage_record_id: Option<String>,
    ) -> ChatChannelSummarizeResult {
        let counts = self.catchup.as_ref();
        ChatChannelSummarizeResult {
            conversation_id: self.conversation.id.as_str().to_owned(),
            summary,
            message_ids,
            derived_notice,
            usage_record_id,
            new_message_count: counts.map(|c| c.new_message_count),
            remaining_unread: counts.map(|c| c.remaining_unread),
            through_message_id: counts.and_then(|c| c.through_message_id.clone()),
        }
    }
}

/// A summarize call's plan: answered already (nothing new), or a digest to run.
pub(super) enum Plan {
    Answered(Box<ChatChannelSummarizeResult>),
    Run(Box<Digest>),
}

/// The digest of `params` for `caller_id`, at most `limit` messages.
pub(super) fn plan_digest(
    office: &Office,
    caller_id: &str,
    params: &ChatChannelSummarizeParams,
    limit: usize,
) -> Result<Plan, ApiError> {
    if params.since_read {
        return plan_catchup(office, caller_id, params, limit);
    }
    let question = params.question.trim();
    if question.is_empty() {
        return Err(ApiError::new(
            ErrorCode::InvalidRequest,
            "question must not be empty (or set since_read for a catch-up digest)",
        ));
    }
    let (conversation, messages) = office
        .channel_messages_for_caller(caller_id, &params.channel, limit)
        .map_err(|_| ApiError::new(ErrorCode::StorageError, "failed to read channel"))??;
    Ok(Plan::Run(Box::new(Digest {
        conversation,
        question: question.to_owned(),
        messages,
        catchup: None,
    })))
}

fn plan_catchup(
    office: &Office,
    caller_id: &str,
    params: &ChatChannelSummarizeParams,
    limit: usize,
) -> Result<Plan, ApiError> {
    let window = office
        .channel_catchup_for_caller(caller_id, &params.channel, limit)
        .map_err(|_| ApiError::new(ErrorCode::StorageError, "failed to read channel"))??;
    let counts = CatchupCounts {
        new_message_count: window.messages.len() as u64,
        remaining_unread: window.remaining_unread(),
        through_message_id: window.through_message_id(),
    };
    let question = match params.question.trim() {
        "" => CATCHUP_QUESTION.to_owned(),
        asked => asked.to_owned(),
    };
    let digest = Digest {
        conversation: window.conversation,
        question,
        messages: window.messages,
        catchup: Some(counts),
    };
    if !digest.messages.is_empty() {
        return Ok(Plan::Run(Box::new(digest)));
    }
    let summary = format!(
        "No new messages in #{} since your last read.",
        digest.channel_label()
    );
    Ok(Plan::Answered(Box::new(digest.result(
        summary,
        Vec::new(),
        DERIVED_SUMMARY_NOTICE.to_owned(),
        None,
    ))))
}
