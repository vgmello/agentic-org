//! Asks: the short-hold ask waiters, `msg.ask`, `chat.answer`, `msg.withdraw`, and `chat.wait_ask`.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Condvar, Mutex};
use std::time::{Duration, Instant};

use crate::live::Live;
use org_proto::ask::{
    AskStatus, ChatAnswer, ChatAsk, ChatAskResult, ChatWaitAsk, ChatWaitAskResult, MsgWithdraw,
    MAX_SHORT_HOLD_MS,
};
use org_proto::chat::header_only_received;
use org_proto::{ApiError, Call, Request, Response};

use super::{invalid_params, ok_reply, storage_err, ChatService};

/// Bounded tick interval while waiting on `AskWaiters` so server shutdown wakes promptly.
pub(super) const ASK_WAIT_TICK: Duration = Duration::from_millis(100);

#[derive(Default)]
struct AskWaitersState {
    gen: u64,
    active: HashMap<(String, String), usize>,
}

/// In-memory condvar & active-waiter registry waking short-hold `chat.ask` / `chat.wait_ask`
/// callers when an answer or withdrawal commits (`ASK-10`, `TASK-115` / `ADR-0023` d6).
#[derive(Default)]
pub(super) struct AskWaiters {
    state: Mutex<AskWaitersState>,
    cv: Condvar,
}

struct AskWaiterGuard<'a> {
    waiters: &'a AskWaiters,
    key: (String, String),
}

impl Drop for AskWaiterGuard<'_> {
    fn drop(&mut self) {
        let mut guard = self.waiters.state.lock().unwrap_or_else(|p| p.into_inner());
        if let Some(count) = guard.active.get_mut(&self.key) {
            if *count <= 1 {
                guard.active.remove(&self.key);
            } else {
                *count -= 1;
            }
        }
    }
}

impl AskWaiters {
    fn generation(&self) -> u64 {
        self.state.lock().unwrap_or_else(|p| p.into_inner()).gen
    }

    fn register(&self, asker_id: &str, ask_id: &str) -> AskWaiterGuard<'_> {
        let key = (asker_id.to_owned(), ask_id.to_owned());
        let mut guard = self.state.lock().unwrap_or_else(|p| p.into_inner());
        *guard.active.entry(key.clone()).or_insert(0) += 1;
        AskWaiterGuard { waiters: self, key }
    }

    pub(super) fn has_active_waiter(&self, asker_id: &str, ask_id: &str) -> bool {
        let guard = self.state.lock().unwrap_or_else(|p| p.into_inner());
        guard
            .active
            .get(&(asker_id.to_owned(), ask_id.to_owned()))
            .is_some_and(|&n| n > 0)
    }

    pub(super) fn notify(&self) {
        let mut guard = self.state.lock().unwrap_or_else(|p| p.into_inner());
        guard.gen = guard.gen.saturating_add(1);
        self.cv.notify_all();
    }

    fn wait_change(&self, seen: u64, deadline: Instant, stopping: &AtomicBool) -> u64 {
        let mut guard = self.state.lock().unwrap_or_else(|p| p.into_inner());
        loop {
            if guard.gen != seen || stopping.load(Ordering::SeqCst) {
                return guard.gen;
            }
            let now = Instant::now();
            if now >= deadline {
                return guard.gen;
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

impl ChatService {
    fn hold_for_ask(
        &self,
        member: &str,
        ask_id: &str,
        hold_ms: Option<u64>,
        stopping: &AtomicBool,
    ) -> Result<Result<ChatWaitAskResult, ApiError>, org_office::OfficeError> {
        let max_hold = hold_ms.unwrap_or(MAX_SHORT_HOLD_MS).min(MAX_SHORT_HOLD_MS);
        let _waiter_guard = (max_hold > 0).then(|| self.ask_waiters.register(member, ask_id));
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
        // Unregister BEFORE the final check so any responder that observed `has_active_waiter ==
        // true` and skipped wake enqueue has already committed its answer and is returned inline
        // (`TASK-115` zero-loss handoff).
        drop(_waiter_guard);
        if max_hold > 0 && latest.status == AskStatus::Waiting {
            latest = match self.office.inspect_ask_status(member, ask_id)? {
                Ok(v) => v,
                Err(e) => return Ok(Err(e)),
            };
        }
        Ok(Ok(latest))
    }

    pub(super) fn handle_msg_ask(
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
                crate::federation::sync_native_message_relays(&self.office);
                self.notify_sat_sync();
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

    pub(super) fn handle_chat_answer(&self, member: &str, request: &Request) -> Response {
        let Ok(params) = request.params::<ChatAnswer>() else {
            return invalid_params(request.id, ChatAnswer::METHOD);
        };
        match self.office.chat_answer(member, params) {
            Ok(Ok(committed)) => {
                let satisfied_active_waiter = committed.asker_id.as_deref().is_some_and(|asker| {
                    self.ask_waiters
                        .has_active_waiter(asker, &committed.result.ask_id)
                });
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
                crate::federation::sync_native_message_relays(&self.office);
                self.notify_sat_sync();
                if let Some(sessions) = &self.sessions {
                    crate::wake_bridge::on_chat_answer_committed(
                        &self.office,
                        sessions,
                        member,
                        &committed,
                        satisfied_active_waiter,
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

    pub(super) fn handle_msg_withdraw(&self, member: &str, request: &Request) -> Response {
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
                crate::federation::sync_native_message_relays(&self.office);
                self.notify_sat_sync();
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

    pub(super) fn handle_wait_ask(
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
}
