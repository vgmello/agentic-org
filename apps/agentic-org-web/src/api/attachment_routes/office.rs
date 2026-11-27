//! The office side of the attachment routes: one narrow port ([`AttachmentOffice`]) that the
//! upload, download and file slices call, its production adapter over the organization's
//! HQ office ([`HqOffice`]), and the one mapping of an office answer to an HTTP reply.
//!
//! Who may read or change which file is decided by the office's `AuthorizationPolicy` on
//! every call; this side only forwards the caller's session.

use std::future::Future;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use hyper::header::{HeaderValue, RETRY_AFTER};
use hyper::StatusCode;
use org_proto::web::{Milliseconds, ReadResponse, WebError, WebErrorCode};
use org_proto::{Call, ErrorCode};
use serde_json::Value;

use super::super::hq_context::{resolve_hq_office, revalidate_and_resolve_hq};
use super::super::{failure, unavailable, CallerAuth, SessionApi};
use crate::host::Failure;
use crate::office::OfficeFailure;
use crate::service::ApiReply;

/// The deadline of one ordinary office call (inspect, list, one chunk, attach, update).
pub(super) const OFFICE_CALL: Duration = Duration::from_secs(5);

/// One typed `attachment.*` call to the caller's organization office. `retry` allows one
/// retry after a transport failure (reads only); `deadline` bounds the office's answer.
pub(super) trait AttachmentOffice: Send + Sync + 'static {
    fn call<C>(
        &self,
        params: C::Params,
        retry: bool,
        deadline: Duration,
    ) -> impl Future<Output = Result<C::Result, OfficeFailure>> + Send
    where
        C: Call,
        C::Params: Send + Sync,
        C::Result: Send;
}

/// The organization's HQ office as the caller's session reaches it.
pub(super) struct HqOffice {
    api: Arc<SessionApi>,
    caller: CallerAuth,
    org: String,
    office: Mutex<String>,
}

impl HqOffice {
    /// Validates the caller's session and finds the organization's HQ office: an
    /// organization the session isn't linked to is `404`.
    pub(super) async fn resolve(
        api: Arc<SessionApi>,
        caller: CallerAuth,
        org: String,
    ) -> Result<Self, ApiReply> {
        let office = resolve_hq_office(&api, caller, &org)
            .await
            .map_err(hq_failure)?;
        Ok(Self {
            api,
            caller,
            org,
            office: Mutex::new(office),
        })
    }

    fn office(&self) -> String {
        self.office
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }
}

fn hq_failure(error: Failure) -> ApiReply {
    match error {
        Failure::Refused(_) => unavailable(),
        other => failure(other),
    }
}

impl AttachmentOffice for HqOffice {
    async fn call<C>(
        &self,
        params: C::Params,
        retry: bool,
        deadline: Duration,
    ) -> Result<C::Result, OfficeFailure>
    where
        C: Call,
        C::Params: Send + Sync,
        C::Result: Send,
    {
        let auth = self.api.auth(&self.caller);
        let reply = self
            .api
            .offices
            .call_typed_with_timeout::<C>(&auth, &self.office(), &params, retry, deadline)
            .await;
        // A grant minted for a session that changed since: revalidate once and ask again
        // (a refusal is never applied, so the retry is safe for every call).
        if !matches!(&reply, Err(OfficeFailure::Refused(e)) if e.code == ErrorCode::NotAuthorized) {
            return reply;
        }
        let office = revalidate_and_resolve_hq(&self.api, self.caller, &self.org)
            .await
            .map_err(OfficeFailure::BeforeDispatch)?;
        *self
            .office
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = office.clone();
        self.api
            .offices
            .call_typed_with_timeout::<C>(&auth, &office, &params, retry, deadline)
            .await
    }
}

/// The HTTP reply for an office failure. The office's code picks the status and its
/// message (written for people, never paths or SQL) is kept; a temporary refusal that
/// says when to retry is `503` (or `429`) with `Retry-After`.
pub(super) fn office_reply(error: OfficeFailure) -> ApiReply {
    let error = match error {
        OfficeFailure::BeforeDispatch(failed) => return failure(failed),
        OfficeFailure::Transport => return unavailable(),
        OfficeFailure::Refused(error) => error,
    };
    let retry_after = error.retry_after_ms;
    let status = match error.code {
        ErrorCode::InvalidRequest | ErrorCode::InvalidName | ErrorCode::ClockSkew => {
            StatusCode::BAD_REQUEST
        }
        ErrorCode::NotAuthorized | ErrorCode::OwnerProtected => StatusCode::FORBIDDEN,
        ErrorCode::NotFound => StatusCode::NOT_FOUND,
        ErrorCode::TooLarge => StatusCode::PAYLOAD_TOO_LARGE,
        ErrorCode::Unsupported => StatusCode::UNSUPPORTED_MEDIA_TYPE,
        ErrorCode::Conflict | ErrorCode::AlreadyExists | ErrorCode::OperationReused => {
            StatusCode::CONFLICT
        }
        ErrorCode::Expired => StatusCode::GONE,
        ErrorCode::RateLimited => StatusCode::TOO_MANY_REQUESTS,
        ErrorCode::Unavailable | ErrorCode::NotReady if retry_after.is_some() => {
            StatusCode::SERVICE_UNAVAILABLE
        }
        _ => return unavailable(),
    };
    let mut web = WebError::from(error);
    if status == StatusCode::SERVICE_UNAVAILABLE {
        web.code = WebErrorCode::Unavailable;
        web.retry_after_ms = retry_after.and_then(|ms| Milliseconds::new(ms).ok());
    }
    let reply = ApiReply::json(
        status,
        serde_json::to_value(ReadResponse::<Value>::Error(web)).unwrap_or(Value::Null),
    );
    match retry_after {
        Some(ms) if matches!(status.as_u16(), 429 | 503) => {
            // Whole seconds, rounded up, at least one.
            let seconds = ms.div_ceil(1000).max(1);
            reply.with_header(RETRY_AFTER, HeaderValue::from(seconds))
        }
        _ => reply,
    }
}
