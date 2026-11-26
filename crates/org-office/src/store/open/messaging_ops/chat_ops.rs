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
};
use crate::store::open::{OfficeEngine, StoreOp};

use super::unwrap_reply;

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
            let (change, res) = features::decide_dm_open(
                uow,
                &self.here,
                &self.owner,
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
            let (change, res) = features::decide_channel_create(
                uow,
                &self.here,
                &self.owner,
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
            let (change, res) = features::decide_channel_membership(
                uow,
                &self.owner,
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
    pub org_seq: u64,
    pub now_utc: String,
}

impl StoreOp for ChatSendOp {
    type Out = Result<Result<ChatSendCommitted, ApiError>, StoreError>;
    fn run<E: OfficeEngine>(self, store: &Store<E>) -> Self::Out {
        let out = std::sync::Arc::new(std::sync::Mutex::new(None));
        let slot = std::sync::Arc::clone(&out);
        let reply = store.submit(None, false, move |uow| {
            let (change, res) = features::decide_chat_send(
                uow,
                &self.here,
                &self.owner,
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

pub struct ChatReplyOp {
    pub here: OfficeIdentity,
    pub owner: Id,
    pub caller: Id,
    pub params: MsgReplyParams,
    pub org_seq: u64,
    pub now_utc: String,
}

impl StoreOp for ChatReplyOp {
    type Out = Result<Result<ChatSendCommitted, ApiError>, StoreError>;
    fn run<E: OfficeEngine>(self, store: &Store<E>) -> Self::Out {
        let out = std::sync::Arc::new(std::sync::Mutex::new(None));
        let slot = std::sync::Arc::clone(&out);
        let reply = store.submit(None, false, move |uow| {
            let (change, res) = features::decide_chat_reply(
                uow,
                &self.here,
                &self.owner,
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

pub struct ChatMarkReadOp {
    pub here: OfficeIdentity,
    pub owner: Id,
    pub caller: Id,
    pub params: ChatMarkReadParams,
    pub now_utc: String,
}

impl StoreOp for ChatMarkReadOp {
    type Out = Result<Result<ChatMarkReadResult, ApiError>, StoreError>;
    fn run<E: OfficeEngine>(self, store: &Store<E>) -> Self::Out {
        let out = std::sync::Arc::new(std::sync::Mutex::new(None));
        let slot = std::sync::Arc::clone(&out);
        let reply = store.submit(None, false, move |uow| {
            let (change, res) = features::decide_mark_read(
                uow,
                &self.here,
                &self.owner,
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
            let (change, res) = features::decide_mute(
                uow,
                &self.here,
                &self.owner,
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
            let (change, res) = features::decide_react(
                uow,
                &self.here,
                &self.owner,
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
    pub org_seq: u64,
    pub now_ms: u64,
}

impl StoreOp for MsgAskOp {
    type Out = Result<Result<AskCommitted, ApiError>, StoreError>;
    fn run<E: OfficeEngine>(self, store: &Store<E>) -> Self::Out {
        let out = std::sync::Arc::new(std::sync::Mutex::new(None));
        let slot = std::sync::Arc::clone(&out);
        let reply = store.submit(None, false, move |uow| {
            let (change, res) = asks::decide_msg_ask(
                uow,
                &self.here,
                &self.owner,
                &self.caller,
                &self.params,
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
            let (change, res) = asks::decide_chat_answer(
                uow,
                &self.here,
                &self.owner,
                &self.caller,
                &self.params,
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
            let (change, res) = asks::decide_msg_withdraw(
                uow,
                &self.here,
                &self.owner,
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
            let (change, delivered) = features::deliver_answer_candidates(
                uow,
                &self.here,
                &self.owner,
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
