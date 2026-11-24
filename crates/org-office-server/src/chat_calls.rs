//! Socket and host-channel dispatch for `dm.open`, `chat.*`, and `msg.accept` (`doc-90`, `B6`).

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use org_mailbox::Mailbox;
use org_office::Office;
use org_proto::ask::{
    AskStatus, ChatAnswer, ChatAsk, ChatAskResult, ChatWaitAsk, ChatWaitAskResult, MsgWithdraw,
    MAX_SHORT_HOLD_MS,
};
use org_proto::chat::{
    header_only_received, ChatChannelCreate, ChatChannelJoin, ChatChannelLeave,
    ChatChannelSummarize, ChatChannels, ChatConversations, ChatEventsPoll, ChatEventsPollResult,
    ChatInbox, ChatMarkRead, ChatMessages, ChatMute, ChatReact, ChatSend, ChatWatchingModelGet,
    ChatWatchingModelSet, DmOpen, MsgAccept,
};
use org_proto::{ApiError, Call, ErrorCode, Request, Response};

use crate::live::{Live, LiveEvents};

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

/// Bounded tick interval while waiting on `AskWaiters` so server shutdown wakes promptly.
const ASK_WAIT_TICK: Duration = Duration::from_millis(100);

/// In-memory condvar registry waking short-hold `chat.ask` / `chat.wait_ask` callers when an
/// answer or withdrawal commits (`ASK-10`). Uses a single monotonic generation counter (`O(1)`).
#[derive(Default)]
struct AskWaiters {
    gen: Mutex<u64>,
    cv: Condvar,
}

impl AskWaiters {
    fn generation(&self) -> u64 {
        *self.gen.lock().unwrap_or_else(|p| p.into_inner())
    }

    fn notify(&self) {
        let mut guard = self.gen.lock().unwrap_or_else(|p| p.into_inner());
        *guard = guard.saturating_add(1);
        self.cv.notify_all();
    }

    fn wait_change(&self, seen: u64, deadline: Instant, stopping: &AtomicBool) -> u64 {
        let mut guard = self.gen.lock().unwrap_or_else(|p| p.into_inner());
        loop {
            if *guard != seen || stopping.load(Ordering::SeqCst) {
                return *guard;
            }
            let now = Instant::now();
            if now >= deadline {
                return *guard;
            }
            let step = deadline.saturating_duration_since(now).min(ASK_WAIT_TICK);
            let (next, _) = self
                .cv
                .wait_timeout(guard, step)
                .unwrap_or_else(|p| p.into_inner());
            guard = next;
        }
    }
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
        Self {
            office,
            live,
            mailbox,
            sessions,
            watcher,
            owner_id,
            ask_waiters: AskWaiters::default(),
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
        if !is_chat_method(request) && !request.is::<org_proto::msg::MsgDone>() {
            return None;
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
            return Some(self.handle_events_poll(member, via_grant, request));
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
            Ok(Ok(mut committed)) => {
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
                    self.watcher
                        .on_channel_message_posted(&committed.message, committed.conversation_kind);
                }
                if let Some(sessions) = &self.sessions {
                    let (woke, presence) = crate::wake_bridge::on_chat_send_committed(
                        &self.office,
                        sessions,
                        member,
                        &committed,
                    );
                    committed.result.woke_agents = woke;
                    committed.result.recipient_presence = presence;
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

    fn handle_msg_done(&self, member: &str, request: &Request) -> Response {
        let target_id = request
            .params::<org_proto::msg::MsgDone>()
            .ok()
            .map(|p| p.id);
        // Capture related question IDs before `mailbox.done` marks an `answer_candidate` done.
        let linked_qids: Vec<String> = target_id
            .as_deref()
            .and_then(|id| {
                self.office
                    .chat_inbox(
                        member,
                        org_proto::chat::ChatInboxParams {
                            include_all: true,
                            state: None,
                            because: None,
                            from: None,
                            thread_id: None,
                            cursor: None,
                            limit: Some(100),
                        },
                    )
                    .ok()
                    .and_then(Result::ok)
                    .map(|page| {
                        page.items
                            .into_iter()
                            .find(|i| i.id == id)
                            .map(|i| i.related_message_ids)
                            .unwrap_or_default()
                    })
            })
            .unwrap_or_default();

        let resp = crate::msg_calls::answer(request, Some(&self.mailbox), Some(member), false)
            .unwrap_or_else(|| crate::msg_calls::unsupported(request.id));
        match &resp.outcome {
            org_proto::Outcome::Result(_) => {
                // High #1: only close watches owned by `member` (`asker_id == member`).
                if let Some(id) = target_id.as_deref() {
                    let _ = self.watcher.on_question_done_by(member, id);
                }
                for qid in &linked_qids {
                    let _ = self.watcher.on_question_done_by(member, qid);
                }
                resp
            }
            org_proto::Outcome::Error(err) if err.code == ErrorCode::NotFound => {
                // High #1: an asker has no recipient state for their own channel question; allow
                // `msg.done` on their own watched question ID to close that watch cleanly.
                if let Some(id) = target_id.as_deref() {
                    if self.watcher.on_question_done_by(member, id) {
                        let resolved_at = org_office::format_rfc3339_ms(
                            std::time::SystemTime::now()
                                .duration_since(std::time::UNIX_EPOCH)
                                .map_or(0, |d| d.as_millis() as u64),
                        );
                        return ok_reply::<org_proto::msg::MsgDone>(
                            request.id,
                            &org_proto::msg::MsgDoneResult {
                                id: id.to_owned(),
                                resolved_at,
                            },
                        );
                    }
                }
                resp
            }
            _ => resp,
        }
    }

    fn handle_channel_leave(&self, member: &str, request: &Request) -> Response {
        let Ok(params) = request.params::<ChatChannelLeave>() else {
            return invalid_params(request.id, ChatChannelLeave::METHOD);
        };
        match self.office.set_channel_membership(member, params, false) {
            Ok(Ok(res)) => {
                self.watcher.on_member_left_channel(member, &res.channel.id);
                ok_reply::<ChatChannelLeave>(request.id, &res)
            }
            Ok(Err(refusal)) => Response::error(request.id, refusal),
            Err(error) => {
                crate::logged(ChatChannelLeave::METHOD, &error);
                storage_err(request.id)
            }
        }
    }

    fn handle_channel_summarize(&self, member: &str, request: &Request) -> Response {
        let Ok(params) = request.params::<ChatChannelSummarize>() else {
            return invalid_params(request.id, ChatChannelSummarize::METHOD);
        };
        match self.watcher.summarize_channel(member, &params) {
            Ok(res) => ok_reply::<ChatChannelSummarize>(request.id, &res),
            Err(refusal) => Response::error(request.id, refusal),
        }
    }

    fn handle_watching_model_get(&self, request: &Request) -> Response {
        match self.office.watching_model_view() {
            Ok(res) => ok_reply::<ChatWatchingModelGet>(request.id, &res),
            Err(error) => {
                crate::logged(ChatWatchingModelGet::METHOD, &error);
                storage_err(request.id)
            }
        }
    }

    fn handle_watching_model_set(
        &self,
        member: &str,
        via_grant: bool,
        request: &Request,
    ) -> Response {
        if !via_grant {
            return Response::error(
                request.id,
                ApiError::new(
                    ErrorCode::NotAuthorized,
                    "only the authenticated organization owner may change the watching model",
                ),
            );
        }
        let Ok(params) = request.params::<ChatWatchingModelSet>() else {
            return invalid_params(request.id, ChatWatchingModelSet::METHOD);
        };
        match self.office.set_watching_model_setting(
            member,
            &params.command_id,
            params.model_profile_id.as_deref(),
        ) {
            Ok(Ok(res)) => ok_reply::<ChatWatchingModelSet>(request.id, &res),
            Ok(Err(refusal)) => Response::error(request.id, refusal),
            Err(error) => {
                crate::logged(ChatWatchingModelSet::METHOD, &error);
                storage_err(request.id)
            }
        }
    }

    fn open_channel_watch_if_applicable(
        &self,
        member: &str,
        committed: &org_office::messaging::asks::AskCommitted,
    ) -> (Option<bool>, Option<String>) {
        if !committed.inserted {
            return (None, None);
        }
        let conv_kind = self
            .office
            .conversation_by_id(&committed.result.conversation_id)
            .ok()
            .flatten()
            .map_or(org_proto::chat::ConversationKind::Dm, |c| c.kind);
        if conv_kind != org_proto::chat::ConversationKind::Channel {
            return (None, None);
        }
        let q_text = committed
            .card
            .questions
            .iter()
            .map(|q| q.prompt.as_str())
            .collect::<Vec<_>>()
            .join(" / ");
        let (watched, reason) = self.watcher.on_channel_ask_opened(
            member,
            &committed.result.ask_id,
            &committed.result.conversation_id,
            conv_kind,
            &q_text,
        );
        (Some(watched), reason)
    }

    fn hold_for_ask(
        &self,
        member: &str,
        ask_id: &str,
        hold_ms: Option<u64>,
        stopping: &AtomicBool,
    ) -> Result<Result<ChatWaitAskResult, ApiError>, org_office::OfficeError> {
        let max_hold = hold_ms.unwrap_or(MAX_SHORT_HOLD_MS).min(MAX_SHORT_HOLD_MS);
        let mut seen = self.ask_waiters.generation();
        let initial = match self.office.inspect_ask_status(member, ask_id)? {
            Ok(v) => v,
            Err(e) => return Ok(Err(e)),
        };
        if initial.status != AskStatus::Waiting || max_hold == 0 {
            return Ok(Ok(initial));
        }
        let bounded = max_hold.min(initial.remaining_ms.unwrap_or(max_hold));
        let deadline = Instant::now() + Duration::from_millis(bounded);
        let mut latest = initial;
        while latest.status == AskStatus::Waiting
            && Instant::now() < deadline
            && !stopping.load(Ordering::SeqCst)
        {
            seen = self.ask_waiters.wait_change(seen, deadline, stopping);
            latest = match self.office.inspect_ask_status(member, ask_id)? {
                Ok(v) => v,
                Err(e) => return Ok(Err(e)),
            };
        }
        Ok(Ok(latest))
    }

    fn handle_msg_ask(
        &self,
        member: &str,
        via_grant: bool,
        stopping: &AtomicBool,
        request: &Request,
    ) -> Response {
        let Ok(params) = request.params::<ChatAsk>() else {
            return invalid_params(request.id, ChatAsk::METHOD);
        };
        let hold_ms = if via_grant { Some(0) } else { params.hold_ms };
        match self.office.msg_ask(member, params) {
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
                let (woke, presence) = match &self.sessions {
                    Some(sessions) => crate::wake_bridge::on_chat_ask_committed(
                        &self.office,
                        sessions,
                        member,
                        &committed,
                    ),
                    None => (Vec::new(), Vec::new()),
                };
                let (watched, watch_reason) =
                    self.open_channel_watch_if_applicable(member, &committed);
                let mut res: ChatAskResult = committed.result;
                res.woke_agents = woke;
                res.recipient_presence = presence;
                res.watched = watched;
                res.watch_not_started_reason = watch_reason;
                if res.status == AskStatus::Waiting && hold_ms != Some(0) {
                    if let Ok(Ok(waited)) =
                        self.hold_for_ask(member, &res.ask_id, hold_ms, stopping)
                    {
                        res.status = waited.status;
                        res.remaining_ms = waited.remaining_ms;
                        res.answer = waited.answer;
                        res.withdrawal = waited.withdrawal;
                        res.defaults = waited.defaults;
                        res.required_notice = waited.required_notice;
                    }
                }
                if res.status == AskStatus::RequiredPending {
                    if let Some(sessions) = &self.sessions {
                        sessions.on_required_ask_wait_expired(member, &res.ask_id);
                    }
                }
                ok_reply::<ChatAsk>(request.id, &res)
            }
            Ok(Err(refusal)) => Response::error(request.id, refusal),
            Err(error) => {
                crate::logged("msg.ask", &error);
                storage_err(request.id)
            }
        }
    }

    fn handle_chat_answer(&self, member: &str, request: &Request) -> Response {
        let Ok(params) = request.params::<ChatAnswer>() else {
            return invalid_params(request.id, ChatAnswer::METHOD);
        };
        match self.office.chat_answer(member, params) {
            Ok(Ok(committed)) => {
                self.ask_waiters.notify();
                if let Some(header) = committed.reply_header.clone() {
                    self.mailbox.notify_member(&header.to_member_id);
                    self.live
                        .publish_live(Live::Received(Box::new(header_only_received(header))));
                }
                if committed
                    .affected_members
                    .iter()
                    .any(|m| m == &self.owner_id)
                {
                    self.live.record_mailbox_change();
                }
                self.watcher
                    .on_question_closed(&committed.result.ask_id, "answered");
                if let Some(sessions) = &self.sessions {
                    crate::wake_bridge::on_chat_answer_committed(
                        &self.office,
                        sessions,
                        member,
                        &committed,
                    );
                }
                ok_reply::<ChatAnswer>(request.id, &committed.result)
            }
            Ok(Err(refusal)) => Response::error(request.id, refusal),
            Err(error) => {
                crate::logged("chat.answer", &error);
                storage_err(request.id)
            }
        }
    }

    fn handle_msg_withdraw(&self, member: &str, request: &Request) -> Response {
        let Ok(params) = request.params::<MsgWithdraw>() else {
            return invalid_params(request.id, MsgWithdraw::METHOD);
        };
        match self.office.msg_withdraw(member, params) {
            Ok(Ok(committed)) => {
                self.ask_waiters.notify();
                if committed
                    .affected_members
                    .iter()
                    .any(|m| m == &self.owner_id)
                {
                    self.live.record_mailbox_change();
                }
                self.watcher
                    .on_question_closed(&committed.result.ask_id, "withdrawn");
                if let Some(sessions) = &self.sessions {
                    crate::wake_bridge::on_msg_withdraw_committed(sessions, member, &committed);
                }
                ok_reply::<MsgWithdraw>(request.id, &committed.result)
            }
            Ok(Err(refusal)) => Response::error(request.id, refusal),
            Err(error) => {
                crate::logged("msg.withdraw", &error);
                storage_err(request.id)
            }
        }
    }

    fn handle_wait_ask(
        &self,
        member: &str,
        via_grant: bool,
        stopping: &AtomicBool,
        request: &Request,
    ) -> Response {
        let Ok(params) = request.params::<ChatWaitAsk>() else {
            return invalid_params(request.id, ChatWaitAsk::METHOD);
        };
        let hold_ms = if via_grant { Some(0) } else { params.hold_ms };
        match self.hold_for_ask(member, &params.ask_id, hold_ms, stopping) {
            Ok(Ok(result)) => {
                if result.status == AskStatus::RequiredPending {
                    if let Some(sessions) = &self.sessions {
                        sessions.on_required_ask_wait_expired(member, &params.ask_id);
                    }
                }
                ok_reply::<ChatWaitAsk>(request.id, &result)
            }
            Ok(Err(refusal)) => Response::error(request.id, refusal),
            Err(error) => {
                crate::logged("chat.wait_ask", &error);
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
