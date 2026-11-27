//! Messaging and chat dispatch: held `msg.wait` calls, waiting-message hints, and chat overrides.

use std::os::unix::net::UnixStream;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};

use crate::msg_calls;
use org_mailbox::cancellation_pair;
use org_proto::{ApiError, ErrorCode, Request, Response};

use super::auth::Binding;
use super::held::{Control, Place};
use super::shared::{live_owner, Shared};
use super::wire::{internal, send, snapshot_unavailable_response};

pub(super) fn handle_msg_wait_request<'scope, 'env>(
    scope: &'scope std::thread::Scope<'scope, 'env>,
    request: &Request,
    binding: &Binding,
    shared: &'env Shared,
    out: &'env Mutex<UnixStream>,
    control: &'env Control,
) -> bool {
    let held = &control.held;
    if let Some(unavailable) = snapshot_unavailable_response(shared, request.id) {
        return send(out, request.id, &unavailable, shared.timing);
    }
    let Some(mailbox) = &shared.ports.mailbox else {
        return send(
            out,
            request.id,
            &msg_calls::unsupported(request.id),
            shared.timing,
        );
    };
    let wait = match msg_calls::begin_wait(request, binding.member()) {
        Ok(wait) => wait,
        Err(refusal) => return send(out, request.id, &refusal, shared.timing),
    };
    let (cancel, cancellation) = cancellation_pair();
    let Some(number) = held.start(cancel) else {
        let busy = Response::error(
            request.id,
            ApiError::new(
                ErrorCode::Unavailable,
                "too many calls are held on this connection",
            ),
        );
        return send(out, request.id, &busy, shared.timing);
    };
    // The watcher sets `revoked` before it cancels, so a wait that began
    // after its cancel still finds the flag and ends at once.
    if control.revoked.load(Ordering::SeqCst) {
        held.cancel_all();
    }
    let timing = shared.timing;
    let id = request.id;
    let started = std::thread::Builder::new()
        .name("org-office-wait".into())
        .spawn_scoped(scope, move || {
            let _place = Place(held, number);
            // A panic in the mailbox answers this call alone.
            let answer = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                msg_calls::finish_wait(id, mailbox, wait, &cancellation)
            }))
            .unwrap_or_else(|_| internal(id));
            send(out, id, &answer, timing);
        });
    if started.is_err() {
        held.finish(number);
        let busy = Response::error(
            request.id,
            ApiError::new(ErrorCode::Unavailable, "the office is busy"),
        );
        return send(out, request.id, &busy, timing);
    }
    true
}

pub(super) fn attach_waiting_messages_hint(
    shared: &Shared,
    binding: &Binding,
    cached: &mut Option<(String, Arc<std::sync::atomic::AtomicU32>)>,
    response: &mut Response,
) {
    if binding.via_grant() || !matches!(response.outcome, org_proto::Outcome::Result(_)) {
        return;
    }
    let (Some(member), Some(lookup)) = (binding.member(), &shared.ports.waiting_messages_counter)
    else {
        return;
    };
    // The Chief of Staff's count spans every caller's messages to her: never a side channel
    // into how many another member sent (`TASK-80.1`).
    if shared
        .ports
        .delegation
        .as_deref()
        .is_some_and(|port| port.is_chief_of_staff(member))
    {
        return;
    }
    let count = match cached {
        Some((id, counter)) if id == member => counter.load(Ordering::Relaxed),
        _ => {
            let counter = lookup(member);
            let n = counter.load(Ordering::Relaxed);
            *cached = Some((member.to_owned(), counter));
            n
        }
    };
    if count > 0 {
        response.waiting_messages = Some(count);
    }
}

fn try_chat_override(request: &Request, shared: &Shared, binding: &Binding) -> Option<Response> {
    let Some(chat) = &shared.ports.chat else {
        return None;
    };
    // `msg.send` naming files goes as `chat.send` does: the bare mailbox binds none (`TASK-98.2`).
    let sends_files = request.is::<org_proto::msg::MsgSend>()
        && request
            .params::<org_proto::msg::MsgSend>()
            .is_ok_and(|params| !params.attachment_ids.is_empty());
    if request.is::<org_proto::msg::MsgDone>()
        || request.is::<org_proto::msg::MsgReply>()
        || sends_files
    {
        return chat(
            binding.member(),
            binding.via_grant(),
            &shared.stopping,
            request,
        );
    }
    None
}

fn check_hr_message_guard(
    request: &Request,
    shared: &Shared,
    binding: &Binding,
) -> Option<Response> {
    if !request.is::<org_proto::msg::MsgSend>() {
        return None;
    }
    let (Some(guard), Some(member)) = (&shared.ports.hr_message_guard, binding.member()) else {
        return None;
    };
    let Ok(params) = request.params::<org_proto::msg::MsgSend>() else {
        return None;
    };
    match guard(member, &params.to) {
        Ok(Ok(())) => None,
        Ok(Err(refusal)) => Some(Response::error(request.id, refusal)),
        Err(_) => Some(internal(request.id)),
    }
}

pub(super) fn answer_message(
    request: &Request,
    shared: &Shared,
    binding: &Binding,
) -> Option<Response> {
    if crate::chat_calls::is_chat_method(request) {
        return Some(match &shared.ports.chat {
            Some(chat) => chat(
                binding.member(),
                binding.via_grant(),
                &shared.stopping,
                request,
            )
            .unwrap_or_else(|| msg_calls::unsupported(request.id)),
            None => msg_calls::unsupported(request.id),
        });
    }
    // When `ports.chat` is active, let `ChatService` handle `msg.done` (closing asker-owned
    // channel watches), `msg.reply` (native conversation-aware reply with wakes & cross-office
    // relay) and `msg.send` with files; when `ports.chat` is `None`, preserve the
    // `msg_calls::answer` fallback.
    if let Some(resp) = try_chat_override(request, shared, binding) {
        return Some(resp);
    }
    if let Some(resp) = check_hr_message_guard(request, shared, binding) {
        return Some(resp);
    }
    if !msg_calls::is_mailbox_call(request) {
        return None;
    }
    if let Some(unavailable) = snapshot_unavailable_response(shared, request.id) {
        return Some(unavailable);
    }
    let is_owner = binding.via_grant()
        && match live_owner(shared, binding) {
            Ok(is_owner) => is_owner,
            Err(_) => return Some(internal(request.id)),
        };
    msg_calls::answer(
        request,
        shared.ports.mailbox.as_deref(),
        binding.member(),
        is_owner,
    )
}
