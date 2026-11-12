//! Host-channel negotiation and default-deny authority until grant verification.
use super::*;
use crate::revocations::Revocations;

pub(crate) struct HostChannel {
    pub _channel_id: String,
    pub _kind: org_office_launch::v3::ChannelKind,
    pub hello: HelloResult,
    pub _revocations: Arc<Mutex<Revocations>>,
}
pub(super) fn serve(
    stream: UnixStream,
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
            let code = if matches!(
                request.method.as_str(),
                "msg.bind" | "msg.wait" | "status.set"
            ) {
                ErrorCode::InvalidRequest
            } else {
                ErrorCode::NotAuthorized
            };
            (refuse(request.id, code), false)
        };
        if !send(&out, request.id, &response, Timing::default()) || close {
            break;
        }
    }
}
fn refuse(id: u64, code: ErrorCode) -> Response {
    Response::error(id, ApiError::new(code, "request refused on host channel"))
}

#[cfg(test)]
mod tests;
