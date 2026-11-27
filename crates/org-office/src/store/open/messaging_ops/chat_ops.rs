//! `StoreOp` wrappers for chat, DM, channel, ask and answer decisions.

use org_proto::ask::{ChatAnswerParams, ChatAskParams, MsgWithdrawParams};
use org_proto::chat::{
    ChatChannelCreateParams, ChatChannelCreateResult, ChatChannelMembershipParams,
    ChatChannelMembershipResult, ChatMarkReadParams, ChatMarkReadResult, ChatMuteParams,
    ChatMuteResult, ChatReactParams, ChatReactResult, ChatSendParams, DmOpenParams, DmOpenResult,
    MsgAcceptParams, MsgAcceptResult,
};
use org_proto::msg::MsgReplyParams;
use org_proto::ApiError;
use org_store::{Id, OfficeIdentity, Store, StoreError};

use crate::messaging::{
    asks::{self, AnswerCommitted, AskCommitted, WithdrawCommitted},
    features::{self, ChatSendCommitted},
    files::MessageFiles,
};
use crate::store::open::{OfficeEngine, StoreOp};

use super::unwrap_reply;

fn resolve_owner_for_op(
    read: &impl crate::organization::ports::OrganizationRead,
    configured: &Id,
) -> Result<Id, org_store::Refusal> {
    crate::organization::owner::effective_owner_id(read, configured).map_err(|_| {
        org_store::Refusal::rejected(
            serde_json::to_vec(&ApiError::new(
                org_proto::ErrorCode::StorageError,
                "failed to resolve organization owner",
            ))
            .expect("ApiError serializes"),
        )
    })
}

pub struct DmOpenOp {
    pub here: OfficeIdentity,
    pub owner: Id,
    pub caller: Id,
    pub params: DmOpenParams,
    pub now_utc: String,
}

impl StoreOp for DmOpenOp {
    type Out = Result<Result<DmOpenResult, ApiError>, StoreError>;
    fn run<E: OfficeEngine>(self, store: &Store<E>) -> Self::Out {
        let out = std::sync::Arc::new(std::sync::Mutex::new(None));
        let slot = std::sync::Arc::clone(&out);
        let reply = store.submit(None, false, move |uow| {
            let owner = resolve_owner_for_op(uow.view(), &self.owner)?;
            let (change, res) = features::decide_dm_open(
                uow,
                &self.here,
                &owner,
                &self.caller,
                &self.params,
                &self.now_utc,
            )?;
            *slot.lock().unwrap_or_else(|p| p.into_inner()) = Some(res);
            Ok(change)
        });
        unwrap_reply(reply, out)
    }
}

pub struct ChannelCreateOp {
    pub here: OfficeIdentity,
    pub owner: Id,
    pub caller: Id,
    pub params: ChatChannelCreateParams,
    pub now_utc: String,
}

impl StoreOp for ChannelCreateOp {
    type Out = Result<Result<ChatChannelCreateResult, ApiError>, StoreError>;
    fn run<E: OfficeEngine>(self, store: &Store<E>) -> Self::Out {
        let out = std::sync::Arc::new(std::sync::Mutex::new(None));
        let slot = std::sync::Arc::clone(&out);
        let reply = store.submit(None, false, move |uow| {
            let owner = resolve_owner_for_op(uow.view(), &self.owner)?;
            let (change, res) = features::decide_channel_create(
                uow,
                &self.here,
                &owner,
                &self.caller,
                &self.params,
                &self.now_utc,
            )?;
            *slot.lock().unwrap_or_else(|p| p.into_inner()) = Some(res);
            Ok(change)
        });
        unwrap_reply(reply, out)
    }
}

pub struct ChannelMembershipOp {
    pub owner: Id,
    pub caller: Id,
    pub params: ChatChannelMembershipParams,
    pub join: bool,
    pub now_utc: String,
}

impl StoreOp for ChannelMembershipOp {
    type Out = Result<Result<ChatChannelMembershipResult, ApiError>, StoreError>;
    fn run<E: OfficeEngine>(self, store: &Store<E>) -> Self::Out {
        let out = std::sync::Arc::new(std::sync::Mutex::new(None));
        let slot = std::sync::Arc::clone(&out);
        let reply = store.submit(None, false, move |uow| {
            let owner = resolve_owner_for_op(uow.view(), &self.owner)?;
            let (change, res) = features::decide_channel_membership(
                uow,
                &owner,
                &self.caller,
                &self.params,
                self.join,
                &self.now_utc,
            )?;
            *slot.lock().unwrap_or_else(|p| p.into_inner()) = Some(res);
            Ok(change)
        });
        unwrap_reply(reply, out)
    }
}

pub struct ChatSendOp {
    pub here: OfficeIdentity,
    pub owner: Id,
    pub caller: Id,
    pub params: ChatSendParams,
    pub files: MessageFiles,
    pub org_seq: u64,
    pub now_utc: String,
}

impl StoreOp for ChatSendOp {
    type Out = Result<Result<ChatSendCommitted, ApiError>, StoreError>;
    fn run<E: OfficeEngine>(self, store: &Store<E>) -> Self::Out {
        let out = std::sync::Arc::new(std::sync::Mutex::new(None));
        let slot = std::sync::Arc::clone(&out);
        let reply = store.submit(None, false, move |uow| {
            let owner = resolve_owner_for_op(uow.view(), &self.owner)?;
            let (change, res) = features::decide_chat_send(
                uow,
                &self.here,
                &owner,
                &self.caller,
                &self.params,
                &self.files,
                self.org_seq,
                &self.now_utc,
            )?;
            *slot.lock().unwrap_or_else(|p| p.into_inner()) = Some(res);
            Ok(change)
        });
        unwrap_reply(reply, out)
    }
}

pub struct ChatReplyOp {
    pub here: OfficeIdentity,
    pub owner: Id,
    pub caller: Id,
    pub params: MsgReplyParams,
    pub files: MessageFiles,
    pub org_seq: u64,
    pub now_utc: String,
}

impl StoreOp for ChatReplyOp {
    type Out = Result<Result<ChatSendCommitted, ApiError>, StoreError>;
    fn run<E: OfficeEngine>(self, store: &Store<E>) -> Self::Out {
        let out = std::sync::Arc::new(std::sync::Mutex::new(None));
        let slot = std::sync::Arc::clone(&out);
        let reply = store.submit(None, false, move |uow| {
            let owner = resolve_owner_for_op(uow.view(), &self.owner)?;
            let (change, res) = features::decide_chat_reply(
                uow,
                &self.here,
                &owner,
                &self.caller,
                &self.params,
                &self.files,
                self.org_seq,
                &self.now_utc,
            )?;
            *slot.lock().unwrap_or_else(|p| p.into_inner()) = Some(res);
            Ok(change)
        });
        unwrap_reply(reply, out)
    }
}

pub struct ChatMarkReadOp {
    pub here: OfficeIdentity,
    pub owner: Id,
    pub caller: Id,
    pub params: ChatMarkReadParams,
    pub now_utc: String,
}

impl StoreOp for ChatMarkReadOp {
    type Out = Result<Result<ChatMarkReadResult, ApiError>, StoreError>;
    /// A marker that already covers the message is answered from a snapshot, so an idempotent
    /// repeat writes no journal entry. A move is decided on the writer; a dry run decides and
    /// validates there, then undoes everything (`Store::check`).
    fn run<E: OfficeEngine>(self, store: &Store<E>) -> Self::Out {
        let planned = store.read(|snapshot| {
            let owner = crate::organization::owner::effective_owner_id(snapshot, &self.owner)?;
            Ok(features::plan_mark_read(
                snapshot,
                &owner,
                &self.caller,
                &self.params,
                &self.now_utc,
            ))
        });
        match planned {
            Ok((Ok(features::MarkReadPlan::Unchanged(result)), _)) => return Ok(Ok(result)),
            Ok((Err(refusal), _)) => return Ok(Err(features::decode_refusal(&refusal))),
            Ok((Ok(features::MarkReadPlan::Advance { .. }), _)) | Err(_) => {}
        }
        let out = std::sync::Arc::new(std::sync::Mutex::new(None));
        let slot = std::sync::Arc::clone(&out);
        let dry_run = self.params.dry_run;
        let decide = move |uow: &mut org_store::UnitOfWork<'_, E>| {
            let owner = resolve_owner_for_op(uow.view(), &self.owner)?;
            let (change, res) = features::decide_mark_read(
                uow,
                &self.here,
                &owner,
                &self.caller,
                &self.params,
                &self.now_utc,
            )?;
            *slot.lock().unwrap_or_else(|p| p.into_inner()) = Some(res);
            Ok(change)
        };
        let reply = if dry_run {
            store.check(decide)
        } else {
            store.submit(None, false, decide)
        };
        unwrap_reply(reply, out)
    }
}

pub struct ChatMuteOp {
    pub here: OfficeIdentity,
    pub owner: Id,
    pub caller: Id,
    pub params: ChatMuteParams,
    pub now_utc: String,
}

impl StoreOp for ChatMuteOp {
    type Out = Result<Result<ChatMuteResult, ApiError>, StoreError>;
    fn run<E: OfficeEngine>(self, store: &Store<E>) -> Self::Out {
        let out = std::sync::Arc::new(std::sync::Mutex::new(None));
        let slot = std::sync::Arc::clone(&out);
        let reply = store.submit(None, false, move |uow| {
            let owner = resolve_owner_for_op(uow.view(), &self.owner)?;
            let (change, res) = features::decide_mute(
                uow,
                &self.here,
                &owner,
                &self.caller,
                &self.params,
                &self.now_utc,
            )?;
            *slot.lock().unwrap_or_else(|p| p.into_inner()) = Some(res);
            Ok(change)
        });
        unwrap_reply(reply, out)
    }
}

pub struct ChatReactOp {
    pub here: OfficeIdentity,
    pub owner: Id,
    pub caller: Id,
    pub params: ChatReactParams,
    pub now_utc: String,
}

impl StoreOp for ChatReactOp {
    type Out = Result<Result<ChatReactResult, ApiError>, StoreError>;
    fn run<E: OfficeEngine>(self, store: &Store<E>) -> Self::Out {
        let out = std::sync::Arc::new(std::sync::Mutex::new(None));
        let slot = std::sync::Arc::clone(&out);
        let reply = store.submit(None, false, move |uow| {
            let owner = resolve_owner_for_op(uow.view(), &self.owner)?;
            let (change, res) = features::decide_react(
                uow,
                &self.here,
                &owner,
                &self.caller,
                &self.params,
                &self.now_utc,
            )?;
            *slot.lock().unwrap_or_else(|p| p.into_inner()) = Some(res);
            Ok(change)
        });
        unwrap_reply(reply, out)
    }
}

pub struct MsgAcceptOp {
    pub here: OfficeIdentity,
    pub caller: Id,
    pub params: MsgAcceptParams,
    pub org_seq: u64,
    pub now_utc: String,
}

impl StoreOp for MsgAcceptOp {
    type Out = Result<Result<MsgAcceptResult, ApiError>, StoreError>;
    fn run<E: OfficeEngine>(self, store: &Store<E>) -> Self::Out {
        let out = std::sync::Arc::new(std::sync::Mutex::new(None));
        let slot = std::sync::Arc::clone(&out);
        let reply = store.submit(None, false, move |uow| {
            let (change, res) = features::decide_accept_handoff(
                uow,
                &self.here,
                &self.caller,
                &self.params,
                self.org_seq,
                &self.now_utc,
            )?;
            *slot.lock().unwrap_or_else(|p| p.into_inner()) = Some(res);
            Ok(change)
        });
        unwrap_reply(reply, out)
    }
}

pub struct MsgAskOp {
    pub here: OfficeIdentity,
    pub owner: Id,
    pub caller: Id,
    pub params: ChatAskParams,
    pub files: MessageFiles,
    pub org_seq: u64,
    pub now_ms: u64,
}

impl StoreOp for MsgAskOp {
    type Out = Result<Result<AskCommitted, ApiError>, StoreError>;
    fn run<E: OfficeEngine>(self, store: &Store<E>) -> Self::Out {
        let out = std::sync::Arc::new(std::sync::Mutex::new(None));
        let slot = std::sync::Arc::clone(&out);
        let reply = store.submit(None, false, move |uow| {
            let owner = resolve_owner_for_op(uow.view(), &self.owner)?;
            let (change, res) = asks::decide_msg_ask(
                uow,
                &self.here,
                &owner,
                &self.caller,
                &self.params,
                &self.files,
                self.org_seq,
                self.now_ms,
            )?;
            *slot.lock().unwrap_or_else(|p| p.into_inner()) = Some(res);
            Ok(change)
        });
        unwrap_reply(reply, out)
    }
}

pub struct ChatAnswerOp {
    pub here: OfficeIdentity,
    pub owner: Id,
    pub caller: Id,
    pub params: ChatAnswerParams,
    pub files: MessageFiles,
    pub reply_id: String,
    pub org_seq: u64,
    pub now_utc: String,
}

impl StoreOp for ChatAnswerOp {
    type Out = Result<Result<AnswerCommitted, ApiError>, StoreError>;
    fn run<E: OfficeEngine>(self, store: &Store<E>) -> Self::Out {
        let out = std::sync::Arc::new(std::sync::Mutex::new(None));
        let slot = std::sync::Arc::clone(&out);
        let reply = store.submit(None, false, move |uow| {
            let owner = resolve_owner_for_op(uow.view(), &self.owner)?;
            let (change, res) = asks::decide_chat_answer(
                uow,
                &self.here,
                &owner,
                &self.caller,
                &self.params,
                &self.files,
                &self.reply_id,
                self.org_seq,
                &self.now_utc,
            )?;
            *slot.lock().unwrap_or_else(|p| p.into_inner()) = Some(res);
            Ok(change)
        });
        unwrap_reply(reply, out)
    }
}

pub struct MsgWithdrawOp {
    pub here: OfficeIdentity,
    pub owner: Id,
    pub caller: Id,
    pub params: MsgWithdrawParams,
    pub org_seq: u64,
    pub now_utc: String,
    pub removed_offices: std::collections::BTreeSet<String>,
}

impl StoreOp for MsgWithdrawOp {
    type Out = Result<Result<WithdrawCommitted, ApiError>, StoreError>;
    fn run<E: OfficeEngine>(self, store: &Store<E>) -> Self::Out {
        let out = std::sync::Arc::new(std::sync::Mutex::new(None));
        let slot = std::sync::Arc::clone(&out);
        let reply = store.submit(None, false, move |uow| {
            let owner = resolve_owner_for_op(uow.view(), &self.owner)?;
            let (change, res) = asks::decide_msg_withdraw(
                uow,
                &self.here,
                &owner,
                &self.caller,
                &self.params,
                self.org_seq,
                &self.now_utc,
                &self.removed_offices,
            )?;
            *slot.lock().unwrap_or_else(|p| p.into_inner()) = Some(res);
            Ok(change)
        });
        unwrap_reply(reply, out)
    }
}

/// Atomically delivers validated `answer_candidate` inbox entries (`ADR-0024` d11, `WATCH-3/7`).
pub struct DeliverAnswerCandidatesOp {
    pub here: OfficeIdentity,
    pub owner: Id,
    pub conversation_id: Id,
    pub deliveries: Vec<features::AnswerCandidateDelivery>,
    pub org_seq: u64,
    pub now_utc: String,
}

impl StoreOp for DeliverAnswerCandidatesOp {
    type Out = Result<Result<Vec<(String, Id)>, ApiError>, StoreError>;

    fn run<E: OfficeEngine>(self, store: &Store<E>) -> Self::Out {
        let out = std::sync::Arc::new(std::sync::Mutex::new(None));
        let slot = std::sync::Arc::clone(&out);
        let reply = store.submit(None, false, move |uow| {
            let owner = resolve_owner_for_op(uow.view(), &self.owner)?;
            let (change, delivered) = features::deliver_answer_candidates(
                uow,
                &self.here,
                &owner,
                &self.conversation_id,
                &self.deliveries,
                self.org_seq,
                &self.now_utc,
            )?;
            *slot.lock().unwrap_or_else(|p| p.into_inner()) = Some(delivered);
            Ok(change)
        });
        unwrap_reply(reply, out)
    }
}
