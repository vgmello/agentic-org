//! Instance bootstrap access, opt-in authentication enablement, and opt-in HTTPS
//! configuration routes (`GET /api/v1/access`, `POST /api/v1/instance/auth/enable`,
//! `POST /api/v1/instance/tls`; ADR-0033 and ADR-0034).

use std::net::IpAddr;

use hyper::{header::SET_COOKIE, StatusCode};
use org_host_control::{
    browser::{
        BrowserAuthEnable, BrowserAuthEnableParams, BrowserTlsEnable, BrowserTlsEnableParams,
        SessionValidate, SessionValidateParams, TlsEnableMode,
    },
    listen::Hsts,
    TlsPrivateKey,
};
use org_proto::web::{
    AccessView, SessionEstablished, SessionProof, TlsEnableResult, WebError, WebErrorCode,
};
use serde::Deserialize;

use super::{
    failure, invalid, result, signed_out, unavailable, view, Call, Credentials, PresentedPair,
    SessionApi, SignInBody,
};
use crate::{
    host::Failure,
    service::{ApiReply, PreparedCall},
    session::BrowserPair,
};

/// How a browser request authenticates to protected routes.
///
/// `LocalAuthless` is only produced when this instance is serving local plain HTTP with
/// authentication not yet enabled (`auth_enabled == false`). The host independently
/// enforces the same condition on every call.
#[derive(Clone, Copy)]
pub enum CallerAuth {
    Pair(PresentedPair),
    LocalAuthless,
}

/// Decoded request parameters for `POST /api/v1/instance/tls`.
pub struct TlsEnableInput {
    pub(super) mode: TlsEnableMode,
    pub(super) hsts: Option<Hsts>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TlsEnableBody {
    mode: String,
    #[serde(default)]
    cert: Option<String>,
    #[serde(
        default,
        deserialize_with = "crate::secret_json::optional_tls_private_key"
    )]
    key: Option<TlsPrivateKey>,
    #[serde(default)]
    hsts: Option<Hsts>,
}

pub(super) fn prepare_auth_enable(
    peer: IpAddr,
    pair: Option<PresentedPair>,
    bytes: &[u8],
) -> Result<PreparedCall<Call>, ApiReply> {
    let SignInBody { login, password } = serde_json::from_slice(bytes).map_err(|_| invalid())?;
    Ok(PreparedCall::read(
        Call::EnableAuth(peer, pair, Credentials { login, password }),
        bytes.len(),
    ))
}

pub(super) fn prepare_tls_enable(
    caller: CallerAuth,
    bytes: &[u8],
) -> Result<PreparedCall<Call>, ApiReply> {
    let body: TlsEnableBody = serde_json::from_slice(bytes).map_err(|_| invalid())?;
    let mode = match (body.mode.as_str(), body.cert, body.key) {
        ("self_signed", None, None) => TlsEnableMode::SelfSigned,
        ("supplied", Some(cert), Some(key)) => TlsEnableMode::Supplied { cert, key },
        _ => return Err(invalid()),
    };
    Ok(PreparedCall::read(
        Call::EnableTls(
            caller,
            TlsEnableInput {
                mode,
                hsts: body.hsts,
            },
        ),
        bytes.len(),
    ))
}

pub(super) fn execute_access(api: &SessionApi) -> ApiReply {
    result(AccessView {
        auth_enabled: api.is_auth_enabled(),
        listen_scheme: api.listen_scheme,
    })
}

pub(super) fn execute_auth_enable(
    api: &SessionApi,
    client_address: IpAddr,
    pair: Option<PresentedPair>,
    credentials: Credentials,
) -> ApiReply {
    if let Some(presented) = pair {
        if let Err(error) = api.validate(CallerAuth::Pair(presented)) {
            return failure(error);
        }
    }
    let Ok(minted) = BrowserPair::mint() else {
        return unavailable();
    };
    let pair = PresentedPair {
        cookie_digest: minted.cookie_digest(),
        proof_digest: minted.proof_digest(),
    };
    let reply = api.host.request_then::<BrowserAuthEnable>(
        &BrowserAuthEnableParams {
            login: credentials.login,
            password: credentials.password,
            client_address,
            cookie_digest: pair.cookie_digest,
            proof_digest: pair.proof_digest,
            origin: api.origin.clone(),
        },
        api.caching(&pair),
    );
    let session = match reply {
        Ok(session) => {
            api.set_auth_enabled(true);
            session
        }
        Err(Failure::Refused(error)) if error.code == org_proto::ErrorCode::Conflict => {
            api.set_auth_enabled(true);
            return ApiReply::error(
                StatusCode::CONFLICT,
                WebErrorCode::Conflict,
                "Authentication is already enabled.",
            );
        }
        Err(error) => return failure(error),
    };
    let Ok(proof) = SessionProof::parse(minted.proof().to_owned()) else {
        return unavailable();
    };
    match view(api.host.instance(), session) {
        Ok(view) => result(SessionEstablished::new(view, proof))
            .with_header(SET_COOKIE, api.cookie.set(&minted)),
        Err(error) => failure(error),
    }
}

pub(super) fn execute_tls_enable(
    api: &SessionApi,
    caller: CallerAuth,
    input: TlsEnableInput,
) -> ApiReply {
    if let Err(error) = api.validate(caller) {
        return failure(error);
    }
    let reply = api
        .host
        .request::<BrowserTlsEnable>(&BrowserTlsEnableParams {
            auth: api.auth(&caller),
            mode: input.mode,
            hsts: input.hsts,
        });
    match reply {
        Ok(enabled) => result(TlsEnableResult {
            scheme: enabled.scheme,
            origin: enabled.origin,
            fingerprint: enabled.fingerprint,
            source: enabled.source,
            restart_required: enabled.restart_required,
        }),
        Err(Failure::Denied) => handle_tls_denied(api, caller),
        Err(Failure::Refused(error)) => map_tls_refusal(*error),
        Err(error) => failure(error),
    }
}

fn handle_tls_denied(api: &SessionApi, caller: CallerAuth) -> ApiReply {
    match caller {
        CallerAuth::LocalAuthless => {
            api.set_auth_enabled(true);
            signed_out()
        }
        CallerAuth::Pair(pair) => {
            let recheck = api.host.request_then::<SessionValidate>(
                &SessionValidateParams {
                    cookie_digest: pair.cookie_digest,
                    proof_digest: pair.proof_digest,
                    origin: api.origin.clone(),
                },
                api.caching(&pair),
            );
            match recheck {
                Ok(_) => ApiReply::error(
                    StatusCode::FORBIDDEN,
                    WebErrorCode::NotAuthorized,
                    "Only the instance administrator can configure HTTPS.",
                ),
                Err(error) => failure(error),
            }
        }
    }
}

fn map_tls_refusal(error: org_proto::ApiError) -> ApiReply {
    let host = WebError::from(error);
    match host.code {
        WebErrorCode::InvalidRequest => {
            let mut web = WebError::new(
                WebErrorCode::InvalidRequest,
                "The TLS certificate or private key is invalid.",
            );
            web.field = host.field;
            web.rule = host.rule;
            ApiReply::json(
                StatusCode::BAD_REQUEST,
                serde_json::to_value(org_proto::web::ReadResponse::<()>::Error(web))
                    .expect("gateway error serializes"),
            )
        }
        WebErrorCode::Conflict => ApiReply::error(
            StatusCode::CONFLICT,
            WebErrorCode::Conflict,
            "TLS configuration could not be updated.",
        ),
        _ => unavailable(),
    }
}
