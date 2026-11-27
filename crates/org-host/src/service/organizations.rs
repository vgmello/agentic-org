//! Organization calls for a signed-in browser: `organization.create`, `organization.list`
//! and `operation.lookup` ([`org_host_control::organization`]). One entry, [`dispatch`].
//!
//! They are served only on the admitted web process's control connection. On `host.sock`
//! or any other connection each is refused with `not_authorized` before its params are
//! read, so nothing is stored. The account is the one the browser pair names in the host's
//! own session records; the web process supplies no account id.
use std::sync::Arc;

use org_host_control::browser::{SessionEnded, SessionValidateParams, SESSION_ENDED};
use org_host_control::journal;
use org_host_control::listen::ListenMode;
use org_host_control::organization::{
    BrowserAuth, Create, List, OpenExisting, OpenPreview, OperationLookup,
};
use org_proto::{ApiError, Call, ErrorCode, Request, Response};
use serde_json::Value;

use crate::authentication::Authentication;
use crate::hostdb::accounts::Account;
use crate::provisioning::Provisioning;

/// Answers `request` if it is one of this module's calls; `None` for any other method.
/// `web` says whether the request arrived on the admitted web connection. `push` queues an
/// event on that connection ahead of the returned response (a `session.ended` for a session
/// this call found dead).
#[cfg(test)]
pub(crate) fn dispatch(
    request: &Request,
    web: bool,
    authentication: Option<&Authentication>,
    provisioning: Option<&Arc<Provisioning>>,
    push: &mut dyn FnMut(&'static str, Value),
) -> Option<Response> {
    dispatch_with_listen(
        request,
        web.then_some(ListenMode::Https),
        authentication,
        provisioning,
        push,
    )
}

pub(crate) fn dispatch_with_listen(
    request: &Request,
    web_listen: Option<ListenMode>,
    authentication: Option<&Authentication>,
    provisioning: Option<&Arc<Provisioning>>,
    push: &mut dyn FnMut(&'static str, Value),
) -> Option<Response> {
    if !(request.is::<Create>()
        || request.is::<List>()
        || request.is::<OperationLookup>()
        || request.is::<OpenPreview>()
        || request.is::<OpenExisting>())
    {
        return None;
    }
    let reply = answer(request, web_listen, authentication, provisioning, push);
    Some(reply.unwrap_or_else(|error| Response::error(request.id, error)))
}

fn answer(
    request: &Request,
    web_listen: Option<ListenMode>,
    authentication: Option<&Authentication>,
    provisioning: Option<&Arc<Provisioning>>,
    push: &mut dyn FnMut(&'static str, Value),
) -> Result<Response, ApiError> {
    let denied = || ApiError::new(ErrorCode::NotAuthorized, "not authorized");
    let (Some(listen), Some(authentication)) = (web_listen, authentication) else {
        return Err(denied());
    };
    let invalid = |_| ApiError::new(ErrorCode::InvalidRequest, "invalid organization request");
    let unavailable = || {
        ApiError::new(
            ErrorCode::Unavailable,
            "organization creation is unavailable",
        )
    };
    let mut account = |auth: &BrowserAuth| -> Result<Account, ApiError> {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .ok()
            .and_then(|wall| i64::try_from(wall.as_millis()).ok())
            .ok_or_else(|| ApiError::new(ErrorCode::Unavailable, "clock unavailable"))?;
        if auth.local_authless {
            if !matches!(listen, ListenMode::HttpLoopback | ListenMode::HttpPublished) {
                return Err(denied());
            }
            let db = authentication.accounts_store();
            let id = db
                .local_authless_account()
                .map_err(|_| unavailable())?
                .ok_or_else(denied)?;
            return db
                .account_authority(&id)
                .map_err(|_| unavailable())?
                .filter(|a| a.revoked_at_ms().is_none())
                .ok_or_else(denied);
        }
        let mut ended = Vec::new();
        let account = authentication.browser_account(
            &SessionValidateParams {
                cookie_digest: auth.cookie_digest,
                proof_digest: auth.proof_digest,
                origin: auth.origin.clone(),
            },
            now,
            &mut ended,
        );
        authentication.revoke_browser_sessions(&ended);
        for session in ended {
            let event = SessionEnded {
                session_id: session.id,
            };
            if let Ok(data) = serde_json::to_value(&event) {
                push(SESSION_ENDED, data);
            }
        }
        account
    };
    if request.is::<Create>() {
        let params = request.params::<Create>().map_err(invalid)?;
        let account = account(&params.auth)?;
        let fingerprint = journal::fingerprint(&params).map_err(invalid)?;
        let result = provisioning.ok_or_else(unavailable)?.create(
            &account,
            &params.operation_id,
            &params.plan,
            fingerprint,
        )?;
        encode::<Create>(request.id, &result)
    } else if request.is::<OperationLookup>() {
        let params = request.params::<OperationLookup>().map_err(invalid)?;
        let account = account(&params.auth)?;
        let result = provisioning
            .ok_or_else(unavailable)?
            .lookup(&account, &params.operation_id)?;
        encode::<OperationLookup>(request.id, &result)
    } else if request.is::<OpenPreview>() {
        let params = request.params::<OpenPreview>().map_err(invalid)?;
        let account = account(&params.auth)?;
        let result = provisioning.ok_or_else(unavailable)?.open_preview(
            &account,
            &params.repository_remote_url,
            &params.state_branch,
        )?;
        encode::<OpenPreview>(request.id, &result)
    } else if request.is::<OpenExisting>() {
        let params = request.params::<OpenExisting>().map_err(invalid)?;
        let account = account(&params.auth)?;
        let fingerprint = journal::fingerprint(&params).map_err(invalid)?;
        let result =
            provisioning
                .ok_or_else(unavailable)?
                .open_existing(&account, &params, fingerprint)?;
        encode::<OpenExisting>(request.id, &result)
    } else {
        let params = request.params::<List>().map_err(invalid)?;
        let account = account(&params.auth)?;
        let result = provisioning.ok_or_else(unavailable)?.list(&account)?;
        encode::<List>(request.id, &result)
    }
}

fn encode<C: Call>(id: u64, result: &C::Result) -> Result<Response, ApiError> {
    Response::ok::<C>(id, result)
        .map_err(|_| ApiError::new(ErrorCode::StorageError, "the answer couldn't be encoded"))
}

#[cfg(test)]
#[path = "organizations/tests.rs"]
mod tests;
