//! Browser-session helpers shared by the browser ops: the clock their sessions are judged
//! by, and the `session.ended` pushes for sessions that ended.

use org_proto::{ApiError, ErrorCode};

use crate::authentication::{self, Authentication};
use crate::control_listener::Connection;

pub(super) fn browser_now_ms() -> Result<i64, ApiError> {
    authentication::wall_ms().and_then(|ms| {
        i64::try_from(ms).map_err(|_| ApiError::new(ErrorCode::Unavailable, "clock unavailable"))
    })
}

pub(super) fn finish_ended_browser_sessions(
    authentication: &Authentication,
    connection: &Connection,
    ended: Vec<crate::sessions::Ended>,
) {
    use org_host_control::browser::{SessionEnded, SESSION_ENDED};
    authentication.revoke_browser_sessions(&ended);
    for session in ended {
        let event = SessionEnded {
            session_id: session.id,
        };
        if let Ok(data) = serde_json::to_value(&event) {
            connection.send_event(SESSION_ENDED, data);
        }
    }
}
