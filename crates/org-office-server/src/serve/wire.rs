//! Writing answers to a connection: framed sends, revocation and snapshot-unavailable refusals,
//! and the internal-error answer.

use std::os::unix::net::UnixStream;
use std::sync::atomic::Ordering;
use std::sync::Mutex;

use crate::bounded;
use crate::follow::Timing;
use org_proto::{frame, ApiError, ErrorCode, OfficeRole, Response};

use super::held::Control;
use super::shared::Shared;
use super::SnapshotFailure;

pub(super) fn internal(id: u64) -> Response {
    Response::error(
        id,
        ApiError::new(ErrorCode::StorageError, "the office could not answer"),
    )
}

pub(super) fn snapshot_unavailable_response(shared: &Shared, id: u64) -> Option<Response> {
    // Only satellite offices can be blocked awaiting their initial HQ snapshot (`TASK-52`);
    // checking `ports.snapshot()` on HQ would pay a full org snapshot read on every subscribe,
    // wait, or mailbox RPC and violates inbox fixtures that forbid snapshot reads.
    if !shared
        .hello
        .office
        .as_ref()
        .is_some_and(|o| o.office.role == OfficeRole::Satellite)
    {
        return None;
    }
    if matches!((shared.ports.snapshot)(), Err(SnapshotFailure::Unavailable)) {
        Some(Response::error(
            id,
            ApiError::new(
                ErrorCode::Unavailable,
                "organization snapshot is not available yet",
            ),
        ))
    } else {
        None
    }
}

pub(super) fn refuse_revoked(out: &Mutex<UnixStream>, id: u64, control: &Control, timing: Timing) {
    control.generation.fetch_add(1, Ordering::SeqCst);
    control.held.cancel_all();
    let refusal = bind_error(
        id,
        ErrorCode::NotBound,
        "this connection's member binding was revoked",
    );
    let _ = send(out, id, &refusal, timing);
}

/// Writes `response` to the client; `false` when the connection is gone.
pub(super) fn send(out: &Mutex<UnixStream>, id: u64, response: &Response, timing: Timing) -> bool {
    let stream = out.lock().unwrap_or_else(|p| p.into_inner());
    match bounded::write_message(&stream, response, timing.write) {
        Ok(()) => true,
        // The answer does not fit in a frame (nothing was written): say so, and carry on.
        Err(frame::FrameError::TooLargeToSend) => {
            let error = Response::error(
                id,
                ApiError::new(ErrorCode::StorageError, "the answer is too large to send"),
            );
            bounded::write_message(&stream, &error, timing.write).is_ok()
        }
        // A frame may be half written: nothing more goes out on this connection, not even an
        // event from the follower once it gets the lock.
        Err(_) => {
            let _ = stream.shutdown(std::net::Shutdown::Both);
            false
        }
    }
}

pub(super) fn bind_error(id: u64, code: ErrorCode, message: &str) -> Response {
    Response::error(id, ApiError::new(code, message))
}
