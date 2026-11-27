//! Host-channel negotiation and per-request, grant-verified owner authority.
use std::os::unix::net::UnixStream;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use org_proto::{frame, ApiError, ErrorCode, Hello, HelloResult, Request, Response};

use super::auth::Binding;
use super::shared::Shared;
use super::wire::{internal, send};
use crate::follow::Timing;
mod authority;
use crate::revocations::Revocations;

/// Maximum concurrent push-woken `chat.events_poll` waiters on one multiplexed host channel.
const MAX_HELD_EVENT_POLLS: usize = 16;

pub(crate) struct HostChannel {
    pub _channel_id: String,
    pub _kind: org_office_launch::ChannelKind,
    pub hello: HelloResult,
    pub _revocations: Arc<Mutex<Revocations>>,
}

struct HeldPollSlot<'a> {
    active: &'a AtomicUsize,
}

impl Drop for HeldPollSlot<'_> {
    fn drop(&mut self) {
        self.active.fetch_sub(1, Ordering::SeqCst);
    }
}

fn is_blocking_events_poll(request: &Request) -> bool {
    let chat_poll = request.is::<org_proto::chat::ChatEventsPoll>()
        && request
            .params::<org_proto::chat::ChatEventsPoll>()
            .ok()
            .and_then(|p| p.wait_ms)
            .is_some_and(|ms| ms > 0);
    let tess_poll = request.is::<org_proto::tess::TessTurnEvents>()
        && request
            .params::<org_proto::tess::TessTurnEvents>()
            .ok()
            .and_then(|p| p.wait_ms)
            .is_some_and(|ms| ms > 0);
    chat_poll || tess_poll
}

fn dispatch_held_poll(
    shared: &Shared,
    owner: &str,
    channel_stop: &Arc<std::sync::atomic::AtomicBool>,
    request: &Request,
) -> Response {
    let id = request.id;
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let dispatched = if request.is::<org_proto::tess::TessTurnEvents>() {
            shared
                .ports
                .tess
                .as_ref()
                .and_then(|tess| tess(Some(owner), true, channel_stop, None, request))
        } else {
            shared
                .ports
                .chat
                .as_ref()
                .and_then(|chat| chat(Some(owner), true, channel_stop, request))
        };
        dispatched.unwrap_or_else(|| {
            Response::error(
                id,
                ApiError::new(
                    ErrorCode::UnknownMethod,
                    format!("this office does not know `{}`", request.method),
                ),
            )
        })
    }))
    .unwrap_or_else(|_| internal(id))
}

fn spawn_held_events_poll<'scope, 'env>(
    scope: &'scope std::thread::Scope<'scope, 'env>,
    request: Request,
    shared: &'env Shared,
    context: &'env HostChannel,
    out: &'env Mutex<UnixStream>,
    channel_stop: &'env Arc<std::sync::atomic::AtomicBool>,
    held_polls: &'env AtomicUsize,
) -> bool {
    let Some(office) = shared.hello.office.as_ref() else {
        return send(
            out,
            request.id,
            &refuse(request.id, ErrorCode::NotAuthorized),
            Timing::default(),
        );
    };
    let checks = authority::Live {
        shared,
        revocations: &context._revocations,
    };
    let owner = match authority::authorize(&request, &context._channel_id, &office.office, &checks)
    {
        Ok(owner) => owner,
        Err(code) => {
            return send(
                out,
                request.id,
                &refuse(request.id, code),
                Timing::default(),
            )
        }
    };
    if !matches!(allowed(&request), HostPolicy::Serve) {
        return send(
            out,
            request.id,
            &refuse(request.id, ErrorCode::NotAuthorized),
            Timing::default(),
        );
    }
    if held_polls.fetch_add(1, Ordering::SeqCst) >= MAX_HELD_EVENT_POLLS {
        held_polls.fetch_sub(1, Ordering::SeqCst);
        let busy = Response::error(
            request.id,
            ApiError::new(
                ErrorCode::Unavailable,
                "too many concurrent event streams on this channel",
            ),
        );
        return send(out, request.id, &busy, Timing::default());
    }
    let office_info = office.office.clone();
    let request_id = request.id;
    let started = std::thread::Builder::new()
        .name("org-host-events-poll".into())
        .spawn_scoped(scope, move || {
            let _slot = HeldPollSlot { active: held_polls };
            let id = request.id;
            let answer = dispatch_held_poll(shared, &owner, channel_stop, &request);
            // Re-run full live authorization (session revocation, grant expiry, and live person
            // membership) before emitting any held event frame so offboarded/removed persons and
            // revoked browser sessions never receive post-revocation events (`doc-7` #9).
            let post_checks = authority::Live {
                shared,
                revocations: &context._revocations,
            };
            let still_authorized =
                authority::authorize(&request, &context._channel_id, &office_info, &post_checks)
                    .is_ok();
            let final_resp = if still_authorized {
                answer
            } else {
                refuse(id, ErrorCode::NotAuthorized)
            };
            let _ = send(out, id, &final_resp, Timing::default());
        });
    if started.is_err() {
        held_polls.fetch_sub(1, Ordering::SeqCst);
        let busy = Response::error(
            request_id,
            ApiError::new(ErrorCode::Unavailable, "the office is busy"),
        );
        return send(out, request_id, &busy, Timing::default());
    }
    true
}

pub(super) fn serve(
    stream: UnixStream,
    shared: &Shared,
    context: HostChannel,
    stopping: &std::sync::atomic::AtomicBool,
) {
    let Ok(out) = stream.try_clone() else { return };
    let out = Mutex::new(out);
    let channel_stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let held_polls = AtomicUsize::new(0);
    std::thread::scope(|scope| {
        let mut reader = org_socket::Ticked::new(stream, || stopping.load(Ordering::SeqCst));
        let mut negotiated = false;
        while let Ok(Some(request)) = frame::read_message::<Request>(&mut reader) {
            if negotiated && is_blocking_events_poll(&request) {
                if !spawn_held_events_poll(
                    scope,
                    request,
                    shared,
                    &context,
                    &out,
                    &channel_stop,
                    &held_polls,
                ) {
                    break;
                }
                continue;
            }
            let (response, close) = if !negotiated && !request.is::<Hello>() {
                (refuse(request.id, ErrorCode::UnsupportedVersion), true)
            } else if request.is::<Hello>() {
                match request.params::<Hello>() {
                    Ok(params) if params.api_range().contains(2) => {
                        negotiated = true;
                        (
                            Response::ok::<Hello>(request.id, &context.hello)
                                .unwrap_or_else(|_| internal(request.id)),
                            false,
                        )
                    }
                    Ok(_) => (refuse(request.id, ErrorCode::UnsupportedVersion), true),
                    Err(_) => (refuse(request.id, ErrorCode::InvalidRequest), false),
                }
            } else {
                (
                    dispatch(
                        &request,
                        shared,
                        &context,
                        &authority::Live {
                            shared,
                            revocations: &context._revocations,
                        },
                    ),
                    false,
                )
            };
            if !send(&out, request.id, &response, Timing::default()) || close {
                break;
            }
        }
        // Wake and drain all held event-poll threads cleanly before exiting the scope.
        channel_stop.store(true, Ordering::SeqCst);
    });
}
fn dispatch(
    request: &Request,
    shared: &Shared,
    context: &HostChannel,
    checks: &impl authority::Checks,
) -> Response {
    if matches!(
        request.method.as_str(),
        "msg.bind" | "msg.wait" | "status.set"
    ) {
        return refuse(request.id, ErrorCode::InvalidRequest);
    }
    let Some(office) = shared.hello.office.as_ref() else {
        return refuse(request.id, ErrorCode::NotAuthorized);
    };
    let owner = match authority::authorize(request, &context._channel_id, &office.office, checks) {
        Ok(owner) => owner,
        Err(code) => return refuse(request.id, code),
    };
    match allowed(request) {
        HostPolicy::Serve => {}
        HostPolicy::Unserved => {
            return Response::error(
                request.id,
                ApiError::new(
                    ErrorCode::UnknownMethod,
                    format!("this office does not know `{}`", request.method),
                ),
            );
        }
        _ => return refuse(request.id, ErrorCode::NotAuthorized),
    }
    // This value lives for this call only. Host channels never acquire a binding.
    let response = super::answer(
        request,
        shared,
        &mut Binding::Member {
            id: owner,
            via_grant: true,
        },
    );
    // Inbox may recheck live ownership after authorization; keep that refusal opaque too.
    if request.is::<org_proto::inbox::InboxList>()
        && matches!(&response.outcome, org_proto::Outcome::Error(error) if error.code == ErrorCode::NotAuthorized)
    {
        return refuse(request.id, ErrorCode::NotAuthorized);
    }
    response
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
enum HostPolicy {
    Serve,
    Unserved,
    Refuse,
    Hello,
    InvalidRequest,
}
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Operation {
    method: String,
    policy: HostPolicy,
}
fn operations() -> &'static [Operation] {
    static OPERATIONS: std::sync::OnceLock<Vec<Operation>> = std::sync::OnceLock::new();
    OPERATIONS.get_or_init(|| {
        serde_json::from_str(include_str!("host/operations.json"))
            .expect("the checked-in host operation policy is valid")
    })
}
fn allowed(request: &Request) -> HostPolicy {
    operations()
        .iter()
        .find(|row| row.method == request.method)
        .map_or(HostPolicy::Refuse, |row| row.policy)
}

fn refuse(id: u64, code: ErrorCode) -> Response {
    Response::error(id, ApiError::new(code, "request refused on host channel"))
}

#[cfg(test)]
mod tests;
