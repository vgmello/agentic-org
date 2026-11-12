//! Host-channel negotiation and per-request, grant-verified owner authority.
use super::*;
mod authority;
use crate::revocations::Revocations;

pub(crate) struct HostChannel {
    pub _channel_id: String,
    pub _kind: org_office_launch::v3::ChannelKind,
    pub hello: HelloResult,
    pub _revocations: Arc<Mutex<Revocations>>,
}
pub(super) fn serve(
    stream: UnixStream,
    shared: &Shared,
    context: HostChannel,
    stopping: &std::sync::atomic::AtomicBool,
) {
    let Ok(out) = stream.try_clone() else { return };
    let out = Mutex::new(out);
    let mut reader = org_socket::Ticked::new(stream, || stopping.load(Ordering::SeqCst));
    let mut negotiated = false;
    while let Ok(Some(request)) = frame::read_message::<Request>(&mut reader) {
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
    if !allowed(request) {
        return refuse(request.id, ErrorCode::NotAuthorized);
    }
    // This value lives for this call only. Host channels never acquire a binding.
    let response = super::answer(
        request,
        shared,
        &mut Binding::Member {
            id: owner,
            with_app_token: true,
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
fn allowed(request: &Request) -> bool {
    let method = request.method.as_str();
    if method == "member.set_run_mode" {
        return false;
    }
    matches!(
        method,
        "org.snapshot"
            | "who"
            | "operation.get"
            | "inbox.list"
            | "msg.send"
            | "msg.inbox"
            | "msg.read"
            | "msg.ask"
            | "msg.reply"
            | "msg.defer"
            | "msg.done"
            | "msg.notify"
            | "msg.get"
            | "msg.list"
            | "msg.threads"
            | "msg.human_needs_you"
            | "dm.open"
    ) || [
        "work.",
        "task.",
        "bug.",
        "checklist.",
        "project.",
        "area.",
        "chat.",
    ]
    .iter()
    .any(|prefix| method.starts_with(prefix))
        || !matches!(OrgCommand::parse(request), Ok(None))
}

fn refuse(id: u64, code: ErrorCode) -> Response {
    Response::error(id, ApiError::new(code, "request refused on host channel"))
}

#[cfg(test)]
mod tests;
