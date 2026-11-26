//! Local-HQ authless session, opt-in authentication enablement, and opt-in HTTPS enablement
//! handlers on the admitted web process control connection (ADR-0033 / TASK-62 & ADR-0034 / TASK-76).

use org_host_control::browser::{
    AuthModeChanged, BrowserAuthEnable, BrowserLocalSession, BrowserTlsEnable,
    BrowserTlsEnableParams, BrowserTlsEnableResult, TlsEnableMode,
};
use org_host_control::listen::{Hsts, ListenConfig, ListenMode};
use org_host_control::ops::TlsSetParams;
use org_host_control::HostOpResult;
use org_proto::web::ListenScheme;
use org_proto::{ApiError, ErrorCode, Request, Response};

use super::authentication::Authentication;
use super::browser_sessions::{browser_now_ms, finish_ended_browser_sessions};
use super::listener::Connection;
use super::reply::ok;
use super::Service;

pub(super) fn handle_local_session(
    authentication: &Authentication,
    request: &Request,
    connection: &Connection,
) {
    let reply = browser_now_ms()
        .and_then(|now| {
            request
                .params::<BrowserLocalSession>()
                .map_err(|_| ApiError::new(ErrorCode::InvalidRequest, "invalid local session"))?;
            authentication.local_authless_session(connection, now)
        })
        .map(|session| ok::<BrowserLocalSession>(request.id, &session))
        .unwrap_or_else(|error| Response::error(request.id, error));
    connection.send_response(reply);
}

pub(super) fn handle_auth_enable(
    service: &Service,
    authentication: &Authentication,
    request: &Request,
    connection: &Connection,
) {
    let mut ended = Vec::new();
    let result = browser_now_ms().and_then(|now| {
        let params = request
            .params::<BrowserAuthEnable>()
            .map_err(|_| ApiError::new(ErrorCode::InvalidRequest, "invalid auth enable request"))?;
        authentication.enable_local_auth(connection, &params, now, &mut ended)
    });
    finish_ended_browser_sessions(authentication, connection, ended);
    match result {
        Ok(session) => {
            if let Some(provisioning) = &service.provisioning {
                if let Ok(data) = serde_json::to_value(AuthModeChanged { auth_enabled: true }) {
                    provisioning
                        .web()
                        .publish_named(org_host_control::event::AUTH_MODE_CHANGED, data);
                }
            }
            connection.send_response(ok::<BrowserAuthEnable>(request.id, &session));
        }
        Err(error) => connection.send_response(Response::error(request.id, error)),
    }
}

pub(super) fn handle_tls_enable(
    service: &Service,
    authentication: &Authentication,
    request: &Request,
    connection: &Connection,
) {
    let mut ended = Vec::new();
    let result = browser_now_ms().and_then(|now| {
        let params = request
            .params::<BrowserTlsEnable>()
            .map_err(|_| ApiError::new(ErrorCode::InvalidRequest, "invalid tls enable request"))?;
        let account =
            authentication.browser_auth_account(connection, &params.auth, now, &mut ended)?;
        if !account.admin() {
            return Err(ApiError::new(
                ErrorCode::NotAuthorized,
                "administrator required to manage TLS settings",
            ));
        }
        execute_tls_enable(service, authentication, account.account_id(), params)
    });
    finish_ended_browser_sessions(authentication, connection, ended);
    match result {
        Ok(res) => connection.send_response(ok::<BrowserTlsEnable>(request.id, &res)),
        Err(error) => connection.send_response(Response::error(request.id, error)),
    }
}

fn execute_tls_enable(
    service: &Service,
    authentication: &Authentication,
    account_id: &str,
    params: BrowserTlsEnableParams,
) -> Result<BrowserTlsEnableResult, ApiError> {
    let _guard = crate::sync::lock(&service.tls_upgrading);
    let now_secs = crate::web_tls::now()
        .ok_or_else(|| ApiError::new(ErrorCode::Unavailable, "clock unavailable"))?;
    let instance = service
        .instance
        .as_ref()
        .ok_or_else(|| ApiError::new(ErrorCode::Unavailable, "instance unavailable"))?;
    let current = instance
        .browser()
        .map_err(|_| ApiError::new(ErrorCode::StorageError, "could not read browser config"))?
        .ok_or_else(|| ApiError::new(ErrorCode::Unavailable, "no browser listener configured"))?;

    let target_hsts = params.hsts.unwrap_or_else(|| current.hsts());
    let https_listen = build_https_listen(&current, target_hsts)?;

    // If the running host already serves HTTPS, rotate certificates live with tls.renew.
    if let Some(web_tls) = &service.web_tls {
        let hsts_changed = target_hsts != current.hsts();
        return rotate_live_https(
            service,
            instance,
            web_tls,
            account_id,
            &https_listen,
            hsts_changed,
            params.mode,
            now_secs,
        );
    }

    // Refuse HTTP -> HTTPS upgrade while authless mode is active: restarting onto HTTPS
    // disables local_authless and would lock the browser out with zero password accounts.
    let (db, secrets) = authentication.provisioning_storage();
    if db
        .local_authless_account()
        .map_err(|_| ApiError::new(ErrorCode::StorageError, "could not read auth mode"))?
        .is_some()
    {
        return Err(ApiError::new(
            ErrorCode::Conflict,
            "enable authentication before switching to HTTPS",
        ));
    }

    // Upgrading from HTTP -> HTTPS: build/validate the pair, persist key + pair, and update instance.json.
    let (checked, source) = match params.mode {
        TlsEnableMode::SelfSigned => (
            crate::web_tls::make(https_listen.origin(), now_secs).map_err(|_| {
                ApiError::new(ErrorCode::Unavailable, "could not generate certificate")
            })?,
            crate::web_tls::Source::Made,
        ),
        TlsEnableMode::Supplied { cert, key } => {
            let checked = crate::web_tls::check(
                &cert,
                key.expose(),
                &[https_listen.origin()],
                Some(now_secs),
            )
            .map_err(|refusal| {
                ApiError::new(
                    ErrorCode::InvalidRequest,
                    crate::web_tls::refusal_text(&refusal),
                )
            })?;
            (checked, crate::web_tls::Source::Supplied)
        }
    };
    let fp = crate::web_tls::WebTls::install_for_upgrade(&db, &*secrets, &checked, source)
        .map_err(|_| ApiError::new(ErrorCode::StorageError, "could not store TLS certificate"))?;
    instance
        .upgrade_browser_to_https(&https_listen)
        .map_err(|_| ApiError::new(ErrorCode::StorageError, "could not update listener config"))?;

    Ok(BrowserTlsEnableResult {
        scheme: ListenScheme::Https,
        origin: https_listen.origin().as_str().to_owned(),
        fingerprint: fp.to_string(),
        source: source.as_str().to_owned(),
        restart_required: true,
    })
}

#[expect(
    clippy::too_many_arguments,
    reason = "live HTTPS rotation coordinates service, registry, TLS state, account, listen config, mode, and clock"
)]
fn rotate_live_https(
    service: &Service,
    instance: &crate::instance::registry::Instance,
    web_tls: &crate::web_tls::WebTls,
    account_id: &str,
    https_listen: &ListenConfig,
    hsts_changed: bool,
    mode: TlsEnableMode,
    now_secs: i64,
) -> Result<BrowserTlsEnableResult, ApiError> {
    let result = match mode {
        TlsEnableMode::SelfSigned => {
            let fp = web_tls.install_made(now_secs).map_err(|_| {
                ApiError::new(ErrorCode::StorageError, "could not renew certificate")
            })?;
            service.announce_tls_renewal();
            BrowserTlsEnableResult {
                scheme: ListenScheme::Https,
                origin: https_listen.origin().as_str().to_owned(),
                fingerprint: fp.to_string(),
                source: crate::web_tls::Source::Made.as_str().to_owned(),
                restart_required: hsts_changed,
            }
        }
        TlsEnableMode::Supplied { cert, key } => {
            let op_id = crate::ids::uuid_v7()
                .map_err(|_| ApiError::new(ErrorCode::Unavailable, "id generator unavailable"))?;
            let set_params = TlsSetParams {
                session: org_host_control::SessionHandle::new("browser-tls-enable".into())
                    .expect("non-empty"),
                operation_id: op_id,
                cert,
                key,
            };
            let (outcome, announce) = web_tls.set(&set_params, account_id, now_secs)?;
            match outcome {
                HostOpResult::Completed { .. } => {
                    if announce {
                        service.announce_tls_renewal();
                    }
                    BrowserTlsEnableResult {
                        scheme: ListenScheme::Https,
                        origin: https_listen.origin().as_str().to_owned(),
                        fingerprint: web_tls.stored_fingerprint().unwrap_or_default(),
                        source: crate::web_tls::Source::Supplied.as_str().to_owned(),
                        restart_required: hsts_changed,
                    }
                }
                HostOpResult::Rejected { refusal } => {
                    return Err(ApiError::new(
                        ErrorCode::InvalidRequest,
                        crate::web_tls::refusal_text(&refusal),
                    ));
                }
                _ => {
                    return Err(ApiError::new(
                        ErrorCode::Unavailable,
                        "TLS update did not complete",
                    ));
                }
            }
        }
    };
    if hsts_changed {
        instance
            .upgrade_browser_to_https(https_listen)
            .map_err(|_| {
                ApiError::new(ErrorCode::StorageError, "could not update listener config")
            })?;
    }
    Ok(result)
}

fn build_https_listen(current: &ListenConfig, hsts: Hsts) -> Result<ListenConfig, ApiError> {
    let host = current.origin().host().to_string();
    let port = current.origin().port();
    let origin = if port == 443 {
        format!("https://{host}")
    } else {
        format!("https://{host}:{port}")
    };
    let hsts_str = match hsts {
        Hsts::Off => "off",
        Hsts::On => "on",
        Hsts::Clear => "clear",
    };
    let value = serde_json::json!({
        "address": current.address().to_string(),
        "port": current.port(),
        "scheme": "https",
        "origin": origin,
        "hsts": hsts_str,
    });
    let listen: ListenConfig = serde_json::from_value(value)
        .map_err(|_| ApiError::new(ErrorCode::InvalidRequest, "could not build HTTPS listener"))?;
    debug_assert_eq!(listen.scheme(), ListenMode::Https);
    Ok(listen)
}
