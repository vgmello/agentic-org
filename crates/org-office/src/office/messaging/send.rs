//! Sending on [`Office`]: `chat.send`, `msg.reply`, `chat.ask` and `chat.answer`, each with
//! up to ten files (`TASK-98.2`). The named staged uploads are checked and their bytes promoted
//! before the journal commits; the decision binds them in the commit; once it is committed
//! they leave staging.

use org_proto::ask::{ChatAnswerParams, ChatAskParams};
use org_proto::chat::ChatSendParams;
use org_proto::msg::MsgReplyParams;
use org_proto::{ApiError, ErrorCode};

use super::parse_member_id;
use crate::{
    error::OfficeError,
    messaging::{
        asks::{AnswerCommitted, AskCommitted},
        features::ChatSendCommitted,
        files::MessageFiles,
    },
    office::{storage, Office},
    store::open::{ChatAnswerOp, ChatReplyOp, ChatSendOp, MsgAskOp},
    time,
};

/// A call's outcome: the answer, the caller's refusal, or a storage failure.
type Answer<T> = Result<Result<T, ApiError>, OfficeError>;

impl Office {
    /// The caller's files ready for the commit, then the commit, then staging cleaned up.
    fn with_files<T>(
        &self,
        caller: &str,
        (record_id, attachment_ids): (&str, &[String]),
        commit: impl FnOnce(MessageFiles) -> Answer<(T, MessageFiles)>,
    ) -> Answer<T> {
        let files = match self.prepare_message_files(caller, record_id, attachment_ids)? {
            Ok(files) => files,
            Err(refusal) => return Ok(Err(refusal)),
        };
        match commit(files)? {
            Ok((committed, files)) => {
                self.release_message_files(&files);
                Ok(Ok(committed))
            }
            Err(refusal) => Ok(Err(refusal)),
        }
    }

    pub fn chat_send(&self, caller: &str, params: ChatSendParams) -> Answer<ChatSendCommitted> {
        let Ok(caller_id) = parse_member_id(caller) else {
            return Ok(Err(ApiError::new(ErrorCode::NotBound, "invalid caller")));
        };
        let org_seq = self.org_snapshot()?.position.seq;
        let (id, ids) = (params.id.clone(), params.attachment_ids.clone());
        self.with_files(caller, (&id, &ids), |files| {
            let now_utc = time::format_rfc3339_ms(self.monotonic_now_ms());
            let committed = self
                .store
                .run(ChatSendOp {
                    here: self.identity.clone(),
                    owner: self.effective_owner_for_ops(),
                    caller: caller_id,
                    params,
                    files: files.clone(),
                    org_seq,
                    now_utc,
                })
                .map_err(storage)?;
            Ok(committed.map(|c| (c, files)))
        })
    }

    pub fn chat_reply(&self, caller: &str, params: MsgReplyParams) -> Answer<ChatSendCommitted> {
        let Ok(caller_id) = parse_member_id(caller) else {
            return Ok(Err(ApiError::new(ErrorCode::NotBound, "invalid caller")));
        };
        let org_seq = self.org_snapshot()?.position.seq;
        let (id, ids) = (params.id.clone(), params.attachment_ids.clone());
        self.with_files(caller, (&id, &ids), |files| {
            let now_utc = time::format_rfc3339_ms(self.monotonic_now_ms());
            let committed = self
                .store
                .run(ChatReplyOp {
                    here: self.identity.clone(),
                    owner: self.effective_owner_for_ops(),
                    caller: caller_id,
                    params,
                    files: files.clone(),
                    org_seq,
                    now_utc,
                })
                .map_err(storage)?;
            Ok(committed.map(|c| (c, files)))
        })
    }

    pub fn msg_ask(&self, caller: &str, params: ChatAskParams) -> Answer<AskCommitted> {
        let Ok(caller_id) = parse_member_id(caller) else {
            return Ok(Err(ApiError::new(ErrorCode::NotBound, "invalid caller")));
        };
        let org_seq = self.org_snapshot()?.position.seq;
        let (id, ids) = (params.id.clone(), params.attachment_ids.clone());
        self.with_files(caller, (&id, &ids), |files| {
            let committed = self
                .store
                .run(MsgAskOp {
                    here: self.identity.clone(),
                    owner: self.effective_owner_for_ops(),
                    caller: caller_id,
                    params,
                    files: files.clone(),
                    org_seq,
                    now_ms: self.monotonic_now_ms(),
                })
                .map_err(storage)?;
            Ok(committed.map(|c| (c, files)))
        })
    }

    pub fn chat_answer(&self, caller: &str, params: ChatAnswerParams) -> Answer<AnswerCommitted> {
        let Ok(caller_id) = parse_member_id(caller) else {
            return Ok(Err(ApiError::new(ErrorCode::NotBound, "invalid caller")));
        };
        let org_seq = self.org_snapshot()?.position.seq;
        let (id, ids) = (params.ask_id.clone(), params.attachment_ids.clone());
        self.with_files(caller, (&id, &ids), |files| {
            let now_ms = self.monotonic_now_ms();
            let now_utc = time::format_rfc3339_ms(now_ms);
            let reply_id = crate::messaging::records::derive_conversation_id(
                "ask_reply",
                &format!(
                    "{}:{}:{caller_id}:{now_ms}",
                    self.identity.organization_id, params.ask_id
                ),
            )
            .as_str()
            .to_owned();
            let committed = self
                .store
                .run(ChatAnswerOp {
                    here: self.identity.clone(),
                    owner: self.effective_owner_for_ops(),
                    caller: caller_id,
                    params,
                    files: files.clone(),
                    reply_id,
                    org_seq,
                    now_utc,
                })
                .map_err(storage)?;
            Ok(committed.map(|c| (c, files)))
        })
    }
}
