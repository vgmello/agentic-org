//! One connection's reader loop: framing, revocation refusal, and answer delivery.

use std::os::unix::net::UnixStream;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::follow;
use org_proto::msg::{MsgBind, MsgWait};
use org_proto::{frame, ApiError, ErrorCode, Hello, Request, Response};

use super::auth::Binding;
use super::binding::{is_gone, lookup_live_binding, Lookup};
use super::dispatch::{answer_and_record, hello_response};
use super::held::{CancelOnDrop, Control, Registration};
use super::messaging::{attach_waiting_messages_hint, handle_msg_wait_request};
use super::shared::{deliver_hint, Shared, Subscriber};
use super::wire::{bind_error, internal, refuse_revoked, send, snapshot_unavailable_response};
use super::{host, Context};

pub(super) fn serve_connection(
    stream: UnixStream,
    shared: &Shared,
    conn: u64,
    stopping: &std::sync::atomic::AtomicBool,
) {
    serve_context(stream, shared, conn, stopping, Context::OfficeSocket);
}

pub(crate) fn serve_context(
    stream: UnixStream,
    shared: &Shared,
    conn: u64,
    stopping: &std::sync::atomic::AtomicBool,
    context: Context,
) {
    match context {
        Context::OfficeSocket => serve_connection_with_poll(
            stream,
            shared,
            conn,
            stopping,
            org_socket::READ_TICK,
            org_socket::poll_readable,
        ),
        Context::HostChannel(context) => host::serve(stream, shared, *context, stopping),
    }
}

pub(super) fn serve_connection_with_poll(
    stream: UnixStream,
    shared: &Shared,
    conn: u64,
    stopping: &std::sync::atomic::AtomicBool,
    tick: Duration,
    poll: impl FnMut(std::os::fd::RawFd, Duration) -> std::io::Result<bool>,
) {
    serve_connection_as(
        stream,
        shared,
        conn,
        stopping,
        tick,
        poll,
        Binding::Anonymous,
    );
}

/// Serves an office-socket connection that starts as `initial`. The product always starts
/// [`Binding::Anonymous`]; the tests of the follow engine start a grant-bound subscriber here,
/// because no frame on the office socket binds one (the grant-bound `subscribe` on a host channel
/// arrives with A3's `/events`).
#[cfg(test)]
pub(crate) fn serve_grant_bound_stream_for_test(
    stream: UnixStream,
    shared: &Shared,
    conn: u64,
    stopping: &std::sync::atomic::AtomicBool,
    member: &str,
) {
    serve_connection_as(
        stream,
        shared,
        conn,
        stopping,
        org_socket::READ_TICK,
        org_socket::poll_readable,
        Binding::Member {
            id: member.to_owned(),
            via_grant: true,
        },
    );
}

pub(super) fn serve_connection_as(
    stream: UnixStream,
    shared: &Shared,
    conn: u64,
    stopping: &std::sync::atomic::AtomicBool,
    tick: Duration,
    poll: impl FnMut(std::os::fd::RawFd, Duration) -> std::io::Result<bool>,
    initial: Binding,
) {
    let peer_pid = org_socket::peer_pid(&stream).ok();
    let Ok(out) = stream.try_clone() else {
        return;
    };
    // One writer at a time: answers and pushed events never interleave. Every frame has a
    // deadline (`bounded`), so a client that does not read cannot hold the thread.
    let out = Mutex::new(out);
    // What the watcher may do to this connection, and its place in the registry.
    let control = Arc::new(Control::default());
    let generation = &control.generation;
    let held = &control.held;
    let mut registration = Registration {
        bound: &shared.bound,
        conn,
        control: Arc::clone(&control),
        member: None,
    };
    // A connection that starts bound is watched from its first frame, as one bound by a call is.
    registration.sync(initial.member());
    // The events sent on this connection, whichever subscription sent them.
    let sent = AtomicU64::new(0);
    // The subscription's thread is joined when the connection ends: it reads the office, which
    // must not be closing.
    std::thread::scope(|scope| {
        let _end_held_calls = CancelOnDrop(held);
        let mut reader = org_socket::Ticked::with_poll(
            stream,
            tick,
            || stopping.load(Ordering::SeqCst) || control.revoked.load(Ordering::SeqCst),
            poll,
        );
        let mut binding = initial;
        let mut bound_waiting: Option<(String, Arc<std::sync::atomic::AtomicU32>)> = None;
        let mut subscribed = false;
        // The last subscription's thread: a new one waits for it, so a client that binds and
        // subscribes in a loop never has more than two.
        let mut pusher: Option<std::thread::ScopedJoinHandle<'_, ()>> = None;
        // A line the framing refuses (too long, not JSON) ends the connection: the stream is out
        // of step and nothing in it can be trusted.
        while let Ok(Some(request)) = frame::read_message::<Request>(&mut reader) {
            // A dead binding cannot be kept alive by principal-bearing frames.
            if control.revoked.load(Ordering::SeqCst) {
                refuse_revoked(&out, request.id, &control, shared.timing);
                break;
            }
            // This is the office-socket loop; host channels authenticate principals separately.
            // Presence includes null and malformed values. Refuse before any binding effects.
            if request.principal.is_some() {
                let refusal = bind_error(
                    request.id,
                    ErrorCode::NotAuthorized,
                    "a principal is not allowed on the office socket",
                );
                if !send(&out, request.id, &refusal, shared.timing) {
                    break;
                }
                continue;
            }
            // The member this connection acts as is checked on every call, not only at bind, and
            // the watcher may have ended the connection's held calls already.
            let member_lookup = lookup_live_binding(shared, &binding, &control);
            if matches!(member_lookup, Some(Lookup::Not)) || control.revoked.load(Ordering::SeqCst)
            {
                refuse_revoked(&out, request.id, &control, shared.timing);
                break;
            }
            if !request.is::<MsgBind>() && matches!(member_lookup, Some(Lookup::Failed)) {
                binding = Binding::Anonymous;
                registration.sync(None);
                generation.fetch_add(1, Ordering::SeqCst);
                subscribed = false;
                held.cancel_all();
            }
            if request.is::<Hello>() {
                let response = hello_response(&request, shared);
                let unsupported = matches!(
                    &response.outcome,
                    org_proto::Outcome::Error(error) if error.code == ErrorCode::UnsupportedVersion
                );
                if !send(&out, request.id, &response, shared.timing) || unsupported {
                    break;
                }
                continue;
            }
            if request.is::<MsgBind>() {
                generation.fetch_add(1, Ordering::SeqCst);
                subscribed = false;
                // A bind, failed or not, ends the held calls of the binding before it.
                held.cancel_all();
            }
            if request.is::<MsgWait>() {
                if !handle_msg_wait_request(scope, &request, &binding, shared, &out, &control) {
                    break;
                }
                continue;
            }
            let feed = shared.ports.feed.as_deref();
            let response = if let Some(unavailable) = request
                .is::<org_proto::Subscribe>()
                .then(|| snapshot_unavailable_response(shared, request.id))
                .flatten()
            {
                unavailable
            } else if request.is::<org_proto::Subscribe>() && feed.is_none() {
                Response::error(
                    request.id,
                    ApiError::new(
                        ErrorCode::Unsupported,
                        "this office does not serve `subscribe` yet",
                    ),
                )
            } else if let (true, Some(feed)) = (request.is::<org_proto::Subscribe>(), feed) {
                match follow::begin(&request, feed, binding.via_grant(), subscribed) {
                    Ok(position) => {
                        // Live events start from here: what is published from now on.
                        let start = follow::Start {
                            position,
                            live: shared.ports.live.head(),
                        };
                        // The generation this subscription belongs to, taken before the answer
                        // goes out: a watcher's bump after this ends it.
                        let mine = generation.load(Ordering::SeqCst);
                        // The answer first, then the events.
                        let answer =
                            Response::ok::<org_proto::Subscribe>(request.id, &org_proto::Empty {})
                                .unwrap_or_else(|_| internal(request.id));
                        if !send(&out, request.id, &answer, shared.timing) {
                            break;
                        }
                        subscribed = true;
                        if let Some(old) = pusher.take() {
                            let _ = old.join();
                        }
                        let live = &*shared.ports.live;
                        let wire = follow::Wire {
                            out: &out,
                            generation,
                            mine,
                            seq: &sent,
                            timing: shared.timing,
                        };
                        let binding = binding.clone();
                        pusher = Some(scope.spawn(move || {
                            let subscriber = Subscriber {
                                binding: &binding,
                                wire: &wire,
                            };
                            follow::push_with_hint(feed, live, &wire, start, |data| {
                                deliver_hint(shared, &subscriber, data)
                            });
                        }));
                        continue;
                    }
                    Err(refusal) => Response::error(request.id, refusal),
                }
            } else {
                answer_and_record(&request, shared, &mut binding, peer_pid)
            };
            // The watcher finds this connection by the member it now acts as. Registered before
            // the answer goes out, so a client that has its bind's answer is already watched.
            if registration.sync(binding.member()) {
                // Registered after the member may already have gone: the watcher cannot have
                // seen this entry, so look once.
                if let Some(member) = binding.member() {
                    if is_gone(shared, member) {
                        control.held.cancel_all();
                        control.generation.fetch_add(1, Ordering::SeqCst);
                        control.revoked.store(true, Ordering::SeqCst);
                    }
                }
            }
            // A watcher can revoke the old binding while msg.bind resolves the new one.
            // Registration keeps that revocation sticky; never acknowledge a fresh bind.
            if request.is::<MsgBind>() && control.revoked.load(Ordering::SeqCst) {
                refuse_revoked(&out, request.id, &control, shared.timing);
                break;
            }
            let mut response = response;
            attach_waiting_messages_hint(shared, &binding, &mut bound_waiting, &mut response);
            if !send(&out, request.id, &response, shared.timing) {
                break;
            }
        }
        generation.fetch_add(1, Ordering::SeqCst);
    });
}
