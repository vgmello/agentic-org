//! Socket and host-channel dispatch for `dm.open`, `chat.*`, and `msg.accept` (`doc-90`, `B6`).

mod ask;
mod channels;
mod events;
mod messages;

use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use crate::live::LiveEvents;
use org_mailbox::Mailbox;
use org_office::Office;
use org_proto::ask::{ChatAnswer, ChatAsk, ChatWaitAsk, MsgWithdraw};
use org_proto::chat::{
    ChatChannelCreate, ChatChannelJoin, ChatChannelLeave, ChatChannelSummarize, ChatChannels,
    ChatConversations, ChatEventsPoll, ChatInbox, ChatMarkRead, ChatMessages, ChatMute, ChatReact,
    ChatSend, ChatWatchingModelGet, ChatWatchingModelSet, DmOpen, MsgAccept,
};
use org_proto::{ApiError, Call, ErrorCode, Request, Response};

use self::ask::AskWaiters;

/// Returns `true` if `request` names one of the B6/B7 chat, channel, DM, ask, accept, or poll methods.
pub(crate) fn is_chat_method(request: &Request) -> bool {
    is_channel_or_dm_method(request)
        || is_message_chat_method(request)
        || is_ask_method(request)
        || request.is::<ChatEventsPoll>()
}

fn is_ask_method(request: &Request) -> bool {
    request.is::<ChatAsk>()
        || request.is::<ChatAnswer>()
        || request.is::<MsgWithdraw>()
        || request.is::<ChatWaitAsk>()
}

fn is_channel_or_dm_method(request: &Request) -> bool {
    request.is::<DmOpen>()
        || request.is::<ChatChannels>()
        || request.is::<ChatChannelCreate>()
        || request.is::<ChatChannelJoin>()
        || request.is::<ChatChannelLeave>()
        || request.is::<ChatChannelSummarize>()
        || request.is::<ChatWatchingModelGet>()
        || request.is::<ChatWatchingModelSet>()
        || request.is::<ChatConversations>()
}

fn is_message_chat_method(request: &Request) -> bool {
    request.is::<ChatSend>()
        || request.is::<ChatMessages>()
        || request.is::<ChatInbox>()
        || request.is::<ChatMarkRead>()
        || request.is::<ChatMute>()
        || request.is::<ChatReact>()
        || request.is::<MsgAccept>()
}

/// Live chat/channel/ask/events service composed over [`Office`], [`Mailbox`], [`LiveEvents`],
/// and optional [`org_office_sessions::SessionManager`] for C6/C8/C10 wakes.
pub(crate) struct ChatService {
    office: Arc<Office>,
    live: Arc<LiveEvents>,
    mailbox: Arc<Mailbox>,
    sessions: Option<Arc<org_office_sessions::SessionManager>>,
    watcher: Arc<crate::watcher::ChannelWatcher>,
    owner_id: String,
    ask_waiters: AskWaiters,
    sat_sync_wake: Option<Arc<dyn Fn() + Send + Sync>>,
    feed_wake: Option<Arc<crate::org_feed::FeedWake>>,
}

impl ChatService {
    pub(crate) fn with_watcher(
        office: Arc<Office>,
        live: Arc<LiveEvents>,
        mailbox: Arc<Mailbox>,
        sessions: Option<Arc<org_office_sessions::SessionManager>>,
        watcher: Arc<crate::watcher::ChannelWatcher>,
    ) -> Self {
        let owner_id = office.owner_member_id().to_owned();
        // Where the session manager meets the live feed: agents' coarse turn activity joins
        // presence for `chat.events_poll` (`TASK-103`).
        if let Some(sessions) = sessions.as_ref() {
            crate::live::follow_session_activity(&live, sessions.stream_hub());
        }
        Self {
            office,
            live,
            mailbox,
            sessions,
            watcher,
            owner_id,
            ask_waiters: AskWaiters::default(),
            sat_sync_wake: None,
            feed_wake: None,
        }
    }

    pub(crate) fn with_sat_sync_wake(mut self, wake: Arc<dyn Fn() + Send + Sync>) -> Self {
        self.sat_sync_wake = Some(wake);
        self
    }

    pub(crate) fn with_feed_wake(mut self, wake: Arc<crate::org_feed::FeedWake>) -> Self {
        self.feed_wake = Some(wake);
        self
    }

    /// Wakes any locally held `chat.ask` / `chat.wait_ask` callers (for inbound cross-office
    /// answers and withdrawals).
    pub(crate) fn notify_ask_waiters(&self) {
        self.ask_waiters.notify();
    }

    /// Returns `true` when `asker_id` currently holds an active short-hold `chat.ask` or
    /// `chat.wait_ask` waiter for `ask_id` on this office (`TASK-115` / `doc-101` §4A).
    pub(crate) fn has_active_ask_waiter(&self, asker_id: &str, ask_id: &str) -> bool {
        self.ask_waiters.has_active_waiter(asker_id, ask_id)
    }

    fn notify_sat_sync(&self) {
        if let Some(wake) = &self.sat_sync_wake {
            wake();
        }
    }

    /// Dispatches one chat/DM/ask/accept/events-poll request for `member` (with `via_grant` authority).
    pub(crate) fn dispatch(
        &self,
        member: Option<&str>,
        via_grant: bool,
        stopping: &AtomicBool,
        request: &Request,
    ) -> Option<Response> {
        if !is_chat_method(request)
            && !request.is::<org_proto::msg::MsgDone>()
            && !request.is::<org_proto::msg::MsgReply>()
            && !request.is::<org_proto::msg::MsgSend>()
        {
            return None;
        }
        // Refuse chat RPCs on a satellite awaiting a complete HQ snapshot (`TASK-52` P9-#3).
        if self
            .office
            .sync_status()
            .is_ok_and(|s| s.initial_snapshot_required)
        {
            return Some(Response::error(
                request.id,
                ApiError::new(
                    ErrorCode::Conflict,
                    "satellite office is awaiting a complete HQ snapshot",
                ),
            ));
        }
        let Some(member) = member else {
            return Some(Response::error(
                request.id,
                ApiError::new(ErrorCode::NotBound, "connection is not bound"),
            ));
        };
        if let Some(resp) = self.dispatch_channel_or_dm(member, via_grant, request) {
            return Some(resp);
        }
        if let Some(resp) = self.dispatch_message_ops(member, request) {
            return Some(resp);
        }
        if let Some(resp) = self.dispatch_ask_ops(member, via_grant, stopping, request) {
            return Some(resp);
        }
        if request.is::<ChatEventsPoll>() {
            return Some(self.handle_events_poll(member, via_grant, stopping, request));
        }
        None
    }

    fn dispatch_ask_ops(
        &self,
        member: &str,
        via_grant: bool,
        stopping: &AtomicBool,
        request: &Request,
    ) -> Option<Response> {
        if request.is::<ChatAsk>() {
            return Some(self.handle_msg_ask(member, via_grant, stopping, request));
        }
        if request.is::<ChatAnswer>() {
            return Some(self.handle_chat_answer(member, request));
        }
        if request.is::<MsgWithdraw>() {
            return Some(self.handle_msg_withdraw(member, request));
        }
        if request.is::<ChatWaitAsk>() {
            return Some(self.handle_wait_ask(member, via_grant, stopping, request));
        }
        None
    }

    fn dispatch_channel_or_dm(
        &self,
        member: &str,
        via_grant: bool,
        request: &Request,
    ) -> Option<Response> {
        if request.is::<DmOpen>() {
            return Some(run_call::<DmOpen>(request, |p| {
                self.office.dm_open(member, p)
            }));
        }
        if request.is::<ChatChannels>() {
            return Some(run_call::<ChatChannels>(request, |p| {
                self.office.list_channels(member, p)
            }));
        }
        if request.is::<ChatChannelCreate>() {
            return Some(run_call::<ChatChannelCreate>(request, |p| {
                self.office.create_channel(member, p)
            }));
        }
        if request.is::<ChatChannelJoin>() {
            return Some(run_call::<ChatChannelJoin>(request, |p| {
                self.office.set_channel_membership(member, p, true)
            }));
        }
        if request.is::<ChatChannelLeave>() {
            return Some(self.handle_channel_leave(member, request));
        }
        if request.is::<ChatChannelSummarize>() {
            return Some(self.handle_channel_summarize(member, request));
        }
        if request.is::<ChatWatchingModelGet>() {
            return Some(self.handle_watching_model_get(request));
        }
        if request.is::<ChatWatchingModelSet>() {
            return Some(self.handle_watching_model_set(member, via_grant, request));
        }
        if request.is::<ChatConversations>() {
            return Some(run_call::<ChatConversations>(request, |p| {
                self.office.list_conversations(member, p)
            }));
        }
        None
    }

    fn dispatch_message_ops(&self, member: &str, request: &Request) -> Option<Response> {
        if request.is::<ChatSend>() {
            return Some(self.handle_chat_send(member, request));
        }
        if request.is::<ChatMessages>() {
            return Some(run_call::<ChatMessages>(request, |p| {
                self.office.chat_messages(member, p)
            }));
        }
        if request.is::<ChatInbox>() {
            let _ = self.mailbox.release_due_snoozes_for(member);
            return Some(run_call::<ChatInbox>(request, |p| {
                self.office.chat_inbox(member, p)
            }));
        }
        if request.is::<ChatMarkRead>() {
            return Some(run_call::<ChatMarkRead>(request, |p| {
                self.office.chat_mark_read(member, p)
            }));
        }
        if request.is::<ChatMute>() {
            return Some(run_call::<ChatMute>(request, |p| {
                self.office.chat_mute(member, p)
            }));
        }
        if request.is::<ChatReact>() {
            return Some(run_call::<ChatReact>(request, |p| {
                self.office.chat_react(member, p)
            }));
        }
        if request.is::<MsgAccept>() {
            return Some(self.handle_msg_accept(member, request));
        }
        if request.is::<org_proto::msg::MsgDone>() {
            return Some(self.handle_msg_done(member, request));
        }
        if request.is::<org_proto::msg::MsgReply>() {
            return Some(self.handle_msg_reply(member, request));
        }
        if request.is::<org_proto::msg::MsgSend>() {
            return Some(self.handle_msg_send(member, request));
        }
        None
    }
}

fn run_call<C: Call>(
    request: &Request,
    run: impl FnOnce(C::Params) -> Result<Result<C::Result, ApiError>, org_office::OfficeError>,
) -> Response {
    let Ok(params) = request.params::<C>() else {
        return invalid_params(request.id, C::METHOD);
    };
    match run(params) {
        Ok(Ok(result)) => ok_reply::<C>(request.id, &result),
        Ok(Err(refusal)) => Response::error(request.id, refusal),
        Err(error) => {
            crate::logged(C::METHOD, &error);
            storage_err(request.id)
        }
    }
}

fn ok_reply<C: Call>(id: u64, result: &C::Result) -> Response {
    Response::ok::<C>(id, result).unwrap_or_else(|_| storage_err(id))
}

fn invalid_params(id: u64, method: &str) -> Response {
    Response::error(
        id,
        ApiError::new(
            ErrorCode::InvalidRequest,
            format!("the params of `{method}` are not valid"),
        ),
    )
}

fn storage_err(id: u64) -> Response {
    Response::error(
        id,
        ApiError::new(ErrorCode::StorageError, "the office could not answer"),
    )
}

#[cfg(test)]
mod tests;
