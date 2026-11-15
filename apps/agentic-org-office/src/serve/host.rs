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
