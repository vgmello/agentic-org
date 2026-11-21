//! Socket and host-channel dispatch for `dm.open`, `chat.*`, and `msg.accept` (`doc-90`, `B6`).

use std::sync::Arc;

use org_mailbox::Mailbox;
use org_office::Office;
use org_proto::chat::{
    header_only_received, ChatChannelCreate, ChatChannelJoin, ChatChannelLeave, ChatChannels,
    ChatConversations, ChatEventsPoll, ChatEventsPollResult, ChatInbox, ChatMarkRead, ChatMessages,
    ChatMute, ChatReact, ChatSend, DmOpen, MsgAccept,
};
use org_proto::{ApiError, Call, ErrorCode, Request, Response};

use crate::live::{Live, LiveEvents};

/// Returns `true` if `request` names one of the B6 chat/channel/DM/accept/events-poll methods.
pub(crate) fn is_chat_method(request: &Request) -> bool {
    is_channel_or_dm_method(request)
        || is_message_chat_method(request)
        || request.is::<ChatEventsPoll>()
}

fn is_channel_or_dm_method(request: &Request) -> bool {
    request.is::<DmOpen>()
        || request.is::<ChatChannels>()
        || request.is::<ChatChannelCreate>()
        || request.is::<ChatChannelJoin>()
        || request.is::<ChatChannelLeave>()
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

/// Live chat/channel/events service composed over [`Office`], [`Mailbox`], and [`LiveEvents`].
pub(crate) struct ChatService {
    office: Arc<Office>,
    live: Arc<LiveEvents>,
    mailbox: Arc<Mailbox>,
    owner_id: String,
}

impl ChatService {
    pub(crate) fn new(office: Arc<Office>, live: Arc<LiveEvents>, mailbox: Arc<Mailbox>) -> Self {
        let owner_id = office.owner_member_id().to_owned();
        Self {
            office,
            live,
            mailbox,
            owner_id,
        }
    }

    /// Dispatches one chat/DM/accept/events-poll request for `member` (with `via_grant` authority).
    pub(crate) fn dispatch(
        &self,
        member: Option<&str>,
        via_grant: bool,
        request: &Request,
    ) -> Option<Response> {
        if !is_chat_method(request) {
            return None;
        }
        let Some(member) = member else {
            return Some(Response::error(
                request.id,
                ApiError::new(ErrorCode::NotBound, "connection is not bound"),
            ));
        };
        if let Some(resp) = self.dispatch_channel_or_dm(member, request) {
            return Some(resp);
        }
        if let Some(resp) = self.dispatch_message_ops(member, request) {
            return Some(resp);
        }
        if request.is::<ChatEventsPoll>() {
            return Some(self.handle_events_poll(member, via_grant, request));
        }
        None
    }

    fn dispatch_channel_or_dm(&self, member: &str, request: &Request) -> Option<Response> {
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
            return Some(run_call::<ChatChannelLeave>(request, |p| {
                self.office.set_channel_membership(member, p, false)
            }));
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
        None
    }

    fn handle_chat_send(&self, member: &str, request: &Request) -> Response {
        let Ok(params) = request.params::<ChatSend>() else {
            return invalid_params(request.id, ChatSend::METHOD);
        };
        if !params.to.trim().starts_with('#') {
            match self.office.check_hr_message_allowed(member, &params.to) {
                Ok(Ok(())) => {}
                Ok(Err(refusal)) => return Response::error(request.id, refusal),
                Err(error) => {
                    crate::logged("chat.send.hr_guard", &error);
                    return storage_err(request.id);
                }
            }
        }
        match self.office.chat_send(member, params) {
            Ok(Ok(committed)) => {
                if committed.inserted {
                    let mut owner_touched = false;
                    for header in committed.delivered_headers() {
                        self.mailbox.notify_member(&header.to_member_id);
                        if header.to_member_id == self.owner_id {
                            owner_touched = true;
                        }
                        self.live
                            .publish_live(Live::Received(Box::new(header_only_received(header))));
                    }
                    if owner_touched {
                        self.live.record_mailbox_change();
                    }
                }
                ok_reply::<ChatSend>(request.id, &committed.result)
            }
            Ok(Err(refusal)) => Response::error(request.id, refusal),
            Err(error) => {
                crate::logged("chat.send", &error);
                storage_err(request.id)
            }
        }
    }

    fn handle_msg_accept(&self, member: &str, request: &Request) -> Response {
        let Ok(params) = request.params::<MsgAccept>() else {
            return invalid_params(request.id, MsgAccept::METHOD);
        };
        match self.office.msg_accept(member, params) {
            Ok(Ok(result)) => {
                if member == self.owner_id {
                    self.live.record_mailbox_change();
                }
                ok_reply::<MsgAccept>(request.id, &result)
            }
            Ok(Err(refusal)) => Response::error(request.id, refusal),
            Err(error) => {
                crate::logged("msg.accept", &error);
                storage_err(request.id)
            }
        }
    }

    fn poll_org_changes(
        &self,
        is_owner: bool,
        after_org_seq: Option<u64>,
        org_head_seq: u64,
    ) -> Result<(Vec<org_proto::org::OrgChangedData>, bool), org_office::OfficeError> {
        match after_org_seq {
            Some(after) if is_owner && after > org_head_seq => Ok((Vec::new(), true)),
            Some(after) if is_owner && after < org_head_seq => {
                Ok((self.office.org_changes_after(after, 100)?, false))
            }
            _ => Ok((Vec::new(), false)),
        }
    }

    fn collect_live_events(
        &self,
        member: &str,
        is_owner: bool,
        after_live_seq: Option<u64>,
        live_head: u64,
    ) -> (
        Vec<org_proto::msg::MessageReceivedData>,
        Vec<org_proto::msg::MessageStateChangedData>,
        Vec<org_proto::inbox::InboxChangedData>,
        bool,
    ) {
        let mut messages_received = Vec::new();
        let mut message_states = Vec::new();
        let mut inbox_changes = Vec::new();
        let Some(after) = after_live_seq else {
            return (messages_received, message_states, inbox_changes, false);
        };
        let batch = self.live.since(after, live_head);
        for ev in batch.events {
            match ev {
                Live::Received(data) if is_owner || data.header.to_member_id == member => {
                    messages_received.push(*data);
                }
                Live::StateChanged(data) if is_owner || data.to_member_id == member => {
                    message_states.push(data);
                }
                Live::InboxChanged(data) if is_owner => inbox_changes.push(data),
                _ => {}
            }
        }
        (messages_received, message_states, inbox_changes, batch.lost)
    }

    fn handle_events_poll(&self, member: &str, via_grant: bool, request: &Request) -> Response {
        let Ok(params) = request.params::<ChatEventsPoll>() else {
            return invalid_params(request.id, ChatEventsPoll::METHOD);
        };
        let Ok(snapshot) = self.office.org_snapshot() else {
            return storage_err(request.id);
        };
        let org_head_seq = snapshot.position.seq;
        let is_owner = via_grant && member == self.owner_id;
        let (org_changes, snapshot_required) =
            match self.poll_org_changes(is_owner, params.after_org_seq, org_head_seq) {
                Ok(v) => v,
                Err(error) => {
                    crate::logged("chat.events_poll.org", &error);
                    return storage_err(request.id);
                }
            };
        let head = self.live.head();
        let (messages_received, message_states, inbox_changes, live_lost) =
            self.collect_live_events(member, is_owner, params.after_live_seq, head.messages);
        let presence_changes = params
            .after_presence_seq
            .map(|after| self.live.presence_since(after, head.presence).0)
            .unwrap_or_default();
        ok_reply::<ChatEventsPoll>(
            request.id,
            &ChatEventsPollResult {
                org_head_seq,
                live_head_seq: head.messages,
                presence_head_seq: head.presence,
                snapshot_required,
                live_lost,
                org_changes,
                messages_received,
                message_states,
                presence_changes,
                inbox_changes,
            },
        )
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
#[path = "chat_calls_tests.rs"]
mod tests;
