//! `StoreOp` and `ReadOp` wrappers for the messaging family, `org-mailbox` storage primitives,
//! and `OrgFeed` committed organization change replay (`doc-90`, `B6`).

use org_proto::{ApiError, ErrorCode};
use org_store::{Outcome, Refusal, Reply, StoreError};

use crate::messaging::features::decode_refusal;

mod chat_ops;
mod chat_reads;
mod mailbox_commit;
mod mailbox_reads;
mod mailbox_state;
mod org_feed;

pub use chat_ops::{
    ChannelCreateOp, ChannelMembershipOp, ChatAnswerOp, ChatMarkReadOp, ChatMuteOp, ChatReactOp,
    ChatReplyOp, ChatSendOp, DeliverAnswerCandidatesOp, DmOpenOp, MsgAcceptOp, MsgAskOp,
    MsgWithdrawOp,
};
pub use chat_reads::{
    ChannelMessagesForCallerRead, ChannelMessagesRawRead, ChatInboxRead, ChatMessagesRead,
    ConversationByIdRead, InspectAskStatusRead, ListChannelsRead, ListConversationsRead,
    OpenOwnerInboxRead, OwnerInboxEntry,
};
pub use mailbox_commit::{
    ClaimMailboxReadOp, CommitMailboxMessageOp, MailboxCommitInput, MailboxCommitOut, MailboxStored,
};
pub use mailbox_reads::{
    FindReplyForAskRead, GetMailboxMessageRead, ListActiveSnoozeDeadlinesRead,
    ListMailboxHeadersRead, PeekUnreadOut, PeekUnreadRead,
};
pub use mailbox_state::{
    LocalTransitionInput, MailboxCheckNotifyOp, MailboxDeferOp, MailboxUnsnoozeOp,
    MailboxUpdateStateOp, NotifyCandidateOut,
};
pub use org_feed::OrgChangesAfterRead;

fn storage_refusal(msg: &str) -> Refusal {
    Refusal::rejected(
        serde_json::to_vec(&ApiError::new(ErrorCode::StorageError, msg)).expect("serializes"),
    )
}

fn unwrap_reply<T>(
    outcome: Result<Reply, StoreError>,
    slot: std::sync::Arc<std::sync::Mutex<Option<T>>>,
) -> Result<Result<T, ApiError>, StoreError> {
    let reply = outcome?;
    match reply.outcome {
        Outcome::Completed => {
            let val = slot
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .take()
                .ok_or_else(|| StoreError::invalid("missing committed result"))?;
            Ok(Ok(val))
        }
        Outcome::Rejected => Ok(Err(decode_refusal(&Refusal::Rejected(reply.bytes)))),
    }
}
