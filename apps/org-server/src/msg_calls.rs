//! `msg.send`, `msg.inbox`, `msg.read` and `msg.wait` on the socket: the connection's binding
//! says WHO, the params say what, and the office's `Mailbox` does the rest. The member is never
//! read from the call: a connection acts as the member it bound as (ADR-0003 M-B).
//!
//! A bad param is told in fixed words, never with the decoder's text, which can quote a body.

use std::time::{Duration, Instant};

use org_mailbox::{Cancellation, Mailbox, MailboxError};
use org_proto::msg::{MsgInbox, MsgRead, MsgSend, MsgWait, MsgWaitParams, MAX_WAIT_MS};
use org_proto::{ApiError, Call, ErrorCode, Request, Response};

/// Answers `request` if it is one of the calls answered at once (not `msg.wait`, which is held).
/// `member` is the bound member, `None` when the connection acts as no member.
pub(crate) fn answer(
    request: &Request,
    mailbox: &Mailbox,
    member: Option<&str>,
) -> Option<Response> {
    if request.is::<MsgSend>() {
        return Some(run::<MsgSend>(request, member, |member, params| {
            mailbox.send(member, params)
        }));
    }
    if request.is::<MsgInbox>() {
        return Some(run::<MsgInbox>(request, member, |member, params| {
            mailbox.inbox(member, params)
        }));
    }
    if request.is::<MsgRead>() {
        return Some(run::<MsgRead>(request, member, |member, params| {
            mailbox.read(member, params)
        }));
    }
    None
}

/// What a held `msg.wait` needs once it is allowed to start.
pub(crate) struct Wait {
    pub member: String,
    pub params: MsgWaitParams,
}

/// Reads a `msg.wait`: the binding first (`not_bound`), then the params. `Err` is the answer to
/// send now.
pub(crate) fn begin_wait(request: &Request, member: Option<&str>) -> Result<Wait, Response> {
    let Some(member) = member else {
        return Err(not_bound(request.id));
    };
    let params = request
        .params::<MsgWait>()
        .map_err(|_| invalid(request.id, "wait"))?;
    Ok(Wait {
        member: member.to_owned(),
        params,
    })
}

/// Holds the wait until a message, its timeout, or `cancellation` (a re-bind or the connection
/// ending, which end it with `not_bound`). The answer to send.
pub(crate) fn finish_wait(
    request_id: u64,
    mailbox: &Mailbox,
    wait: Wait,
    cancellation: &Cancellation,
) -> Response {
    // The mailbox takes the earlier of this and the call's own timeout, and refuses one over
    // the limit itself.
    let deadline = Instant::now() + Duration::from_millis(MAX_WAIT_MS);
    match mailbox.wait(&wait.member, wait.params, deadline, cancellation) {
        Ok(result) => reply::<MsgWait>(request_id, &result),
        Err(error) => refuse(request_id, error),
    }
}

fn run<C: Call>(
    request: &Request,
    member: Option<&str>,
    call: impl FnOnce(&str, C::Params) -> Result<C::Result, MailboxError>,
) -> Response {
    let Some(member) = member else {
        return not_bound(request.id);
    };
    let Ok(params) = request.params::<C>() else {
        return invalid(request.id, C::METHOD);
    };
    match call(member, params) {
        Ok(result) => reply::<C>(request.id, &result),
        Err(error) => refuse(request.id, error),
    }
}

fn reply<C: Call>(id: u64, result: &C::Result) -> Response {
    Response::ok::<C>(id, result).unwrap_or_else(|_| {
        Response::error(
            id,
            ApiError::new(ErrorCode::StorageError, "the office could not answer"),
        )
    })
}

/// A mailbox error is its identically named wire code with the error's fixed text.
fn refuse(id: u64, error: MailboxError) -> Response {
    let text = error.to_string();
    Response::error(id, ApiError::new(ErrorCode::from(error), text))
}

fn not_bound(id: u64) -> Response {
    refuse(id, MailboxError::NotBound)
}

fn invalid(id: u64, method: &str) -> Response {
    Response::error(
        id,
        ApiError::new(
            ErrorCode::InvalidRequest,
            format!("the params of `{method}` are not valid"),
        ),
    )
}
