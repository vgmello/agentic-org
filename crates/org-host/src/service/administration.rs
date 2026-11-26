//! The registered instance's administration on `host.sock`: `instance.view`, the
//! `https` listener's TLS pair (`tls.fetch`, `tls.set`), the administrator link calls and
//! recovery.

use std::sync::Arc;

use org_host_control::{Administrator, InstanceMode};
use org_proto::{ApiError, ErrorCode, Request, Response};

use crate::peer::Received;

use super::listener::Connection;
use super::reply::{answer, not_authorized, ok, params_dont_fit, secret_response};
use super::Service;

impl Service {
    /// `tls.renew` to the web process: its pair was replaced or renewed, and it fetches it
    /// again. Only the admitted web connection subscribes to these events.
    pub(crate) fn announce_tls_renewal(&self) {
        self.tls_renewal_retries
            .store(0, std::sync::atomic::Ordering::SeqCst);
        if let Some(provisioning) = &self.provisioning {
            provisioning
                .web()
                .publish_named(org_host_control::event::TLS_RENEW, serde_json::json!({}));
        }
    }
    /// Bounded retry when the web process reports a transient `tls.fetch` failure after
    /// `tls.renew` (X7 P3 / TASK-62): re-announces `tls.renew` up to 3 times after a short delay.
    pub(crate) fn schedule_tls_renewal_retry(self: &Arc<Self>) {
        const MAX_RETRIES: usize = 3;
        let attempt = self
            .tls_renewal_retries
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        if attempt >= MAX_RETRIES {
            return;
        }
        let weak = Arc::downgrade(self);
        let delay = std::time::Duration::from_secs(2 * (attempt as u64 + 1));
        let _ = std::thread::Builder::new()
            .name("org-tls-renew-retry".into())
            .spawn(move || {
                std::thread::sleep(delay);
                if let Some(service) = weak.upgrade() {
                    if let Some(provisioning) = &service.provisioning {
                        provisioning.web().publish_named(
                            org_host_control::event::TLS_RENEW,
                            serde_json::json!({}),
                        );
                    }
                }
            });
    }
    /// `instance.view` on the web connection: the name, mode, web state and whether an
    /// administrator exists. Nothing else about the host.
    pub(super) fn instance_view(
        &self,
    ) -> Result<org_host_control::browser::InstanceViewResult, ApiError> {
        let name = self.instance_name.get().cloned().ok_or_else(|| {
            ApiError::new(ErrorCode::Unavailable, "the instance view is unavailable")
        })?;
        let auth_enabled = self.web_tls.is_some()
            || self
                .authentication
                .as_ref()
                .and_then(|a| a.auth_enabled().ok())
                .unwrap_or(true);
        Ok(org_host_control::browser::InstanceViewResult {
            name,
            status: InstanceMode::Service,
            web_status: *crate::sync::lock(&self.gateway),
            administrator: self.administrator()?,
            auth_enabled,
            tls: self.web_tls.as_ref().map(|tls| {
                crate::web_tls::now()
                    .map_or(org_proto::web::TlsStatus::Missing, |now| tls.status(now))
            }),
        })
    }
    /// `tls.set` (`host.sock` only; the signed `agentic-org` peer; an administrator's
    /// session; journaled; X-02 §6.2): the pair is checked and installed by
    /// [`crate::web_tls::WebTls::set`], and a pair it installs is announced to the web
    /// process with `tls.renew`. Refused with `not_authorized` on the web connection, without
    /// a registry, for a peer the administrator calls' admission refuses
    /// ([`crate::admin::Admin::admit_peer`]) or for an account that is not the
    /// administrator; `unavailable` before the administrator service is installed and on an
    /// instance whose listener is not `https`.
    pub(super) fn set_tls(
        &self,
        request: &Request,
        connection: &Arc<Connection>,
        received: Received,
    ) -> bool {
        use org_host_control::ops::TlsSet;
        if !request.is::<TlsSet>() {
            return false;
        }
        let (None, Some(authentication)) = (connection.web_listen(), &self.authentication) else {
            connection.send_response(not_authorized(request.id));
            return true;
        };
        connection.send_response(answer::<TlsSet>(request, |params| {
            let admin = self.admin.get().ok_or_else(|| {
                ApiError::new(
                    ErrorCode::Unavailable,
                    "administrator calls are unavailable on this host",
                )
            })?;
            admin.admit_peer(connection, received)?;
            let account = authentication.authorize(connection, &params.session)?;
            if !account.admin() {
                return Err(ApiError::new(
                    ErrorCode::NotAuthorized,
                    "only the instance's administrator sets its TLS pair",
                ));
            }
            let tls = self.web_tls.as_ref().ok_or_else(|| {
                ApiError::new(
                    ErrorCode::Unavailable,
                    "this instance's listener is not https; it has no TLS pair",
                )
            })?;
            let now = crate::web_tls::now()
                .ok_or_else(|| ApiError::new(ErrorCode::Unavailable, "clock unavailable"))?;
            let (result, installed) = tls.set(&params, account.account_id(), now)?;
            if installed {
                self.announce_tls_renewal();
            }
            Ok(result)
        }));
        true
    }
    /// `tls.fetch` (the web connection only): the listener's snapshot and, in `https`, the
    /// stored pair, re-checked for the snapshot's origin. A pair that does not cover it is
    /// `unavailable` with `tls_name_mismatch`; one that cannot be read (its key is gone) is
    /// `storage_error`. An answer with a key goes out on the secret-line path.
    pub(super) fn fetch_tls(&self, request: &Request, connection: &Arc<Connection>) {
        use crate::web_tls::Unservable;
        use org_host_control::listen::ListenMode;
        use org_host_control::ops::{TlsFetch, TlsFetchResult};
        if request.params::<TlsFetch>().is_err() {
            connection.send_response(params_dont_fit(request.id));
            return;
        }
        let Some(snapshot) = connection.web_listen() else {
            connection.send_response(Response::error(
                request.id,
                ApiError::new(
                    ErrorCode::NotAuthorized,
                    "private gateway connection required",
                ),
            ));
            return;
        };
        // The two plain HTTP modes carry no pair. Which of them a listener has was judged
        // against the instance's service mode when the host started (`registered`), before
        // the listener was bound.
        let (origin, hsts) = (snapshot.origin().clone(), snapshot.hsts());
        let answer = match snapshot.scheme() {
            ListenMode::HttpLoopback => TlsFetchResult::HttpLoopback { origin, hsts },
            ListenMode::HttpPublished => TlsFetchResult::HttpPublished { origin, hsts },
            ListenMode::Https => match self.web_tls.as_deref().map(|tls| tls.fetch_for(&origin)) {
                Some(Ok((cert, key))) => TlsFetchResult::Https {
                    cert,
                    key,
                    origin,
                    hsts,
                },
                Some(Err(Unservable::Failure(failure))) => {
                    connection.send_response(ok::<TlsFetch>(
                        request.id,
                        &TlsFetchResult::Unavailable(failure),
                    ));
                    return;
                }
                Some(Err(Unservable::Missing | Unservable::NotValid | Unservable::Storage))
                | None => {
                    connection.send_response(Response::error(
                        request.id,
                        ApiError::new(ErrorCode::StorageError, "the TLS pair cannot be read"),
                    ));
                    return;
                }
            },
        };
        connection.note_listener_fetched();
        if matches!(answer, TlsFetchResult::Https { .. }) {
            secret_response(connection, request.id, &answer);
        } else {
            connection.send_response(ok::<TlsFetch>(request.id, &answer));
        }
    }
    pub(super) fn administer(
        &self,
        request: &Request,
        connection: &Arc<Connection>,
        received: Received,
    ) -> bool {
        use org_host_control::admin::{AdminLinkInitial, AdminReset};
        let initial = request.is::<AdminLinkInitial>();
        if !initial && !request.is::<AdminReset>() {
            return false;
        }
        let Some(admin) = self.admin.get() else {
            connection.send_response(Response::error(
                request.id,
                ApiError::new(
                    ErrorCode::Unavailable,
                    "administrator links are unavailable on this host",
                ),
            ));
            return true;
        };
        let result = if initial {
            request
                .params::<AdminLinkInitial>()
                .map_err(|_| {
                    ApiError::new(
                        ErrorCode::InvalidRequest,
                        "the params don't fit this method",
                    )
                })
                .and_then(|params| admin.initial(connection, received, params))
        } else {
            request
                .params::<AdminReset>()
                .map_err(|_| {
                    ApiError::new(
                        ErrorCode::InvalidRequest,
                        "the params don't fit this method",
                    )
                })
                .and_then(|_| admin.reset(connection, received))
        };
        match result {
            Ok(result) => secret_response(connection, request.id, &result),
            Err(error) => connection.send_response(Response::error(request.id, error)),
        }
        true
    }
    /// Whether the registry holds an administrator; a host without a registry has none.
    pub(super) fn administrator(&self) -> Result<Administrator, ApiError> {
        let Some(authentication) = &self.authentication else {
            return Ok(Administrator::None);
        };
        match authentication
            .provisioning_storage()
            .0
            .administrator_exists()
        {
            Ok(true) => Ok(Administrator::Present),
            Ok(false) => Ok(Administrator::None),
            Err(_) => Err(ApiError::new(
                ErrorCode::Unavailable,
                "the host registry is unavailable",
            )),
        }
    }
    pub(super) fn recover(
        &self,
        request: &Request,
        connection: &Arc<Connection>,
        received: Received,
    ) -> bool {
        use org_host_control::{
            ops::{RecoveryAcknowledge, RecoveryRelease},
            recovery::{Package, Status},
        };
        if !crate::recovery::handles(request) {
            return false;
        }
        if request.is::<RecoveryAcknowledge>() && request.params::<RecoveryAcknowledge>().is_err() {
            connection.clear_recovery_permission();
        }
        let Some(recovery) = &self.recovery else {
            connection.send_response(Response::error(
                request.id,
                ApiError::new(ErrorCode::Unavailable, "recovery unavailable"),
            ));
            return true;
        };
        if request.is::<RecoveryRelease>() {
            connection.clear_recovery_permission();
            let result = request
                .params::<RecoveryRelease>()
                .map_err(|_| ApiError::new(ErrorCode::InvalidRequest, "invalid recovery request"))
                .and_then(|p| recovery.release(connection, received, p));
            match result {
                Ok(result) => secret_response(connection, request.id, &result),
                Err(e) => connection.send_response(Response::error(request.id, e)),
            }
        } else if request.is::<RecoveryAcknowledge>() {
            connection.send_response(answer::<RecoveryAcknowledge>(request, |p| {
                recovery.acknowledge(connection, received, p)
            }));
        } else if request.is::<Status>() {
            connection.send_response(answer::<Status>(request, |p| {
                recovery.status(connection, received, p)
            }));
        } else {
            connection.send_response(answer::<Package>(request, |p| {
                recovery.package(connection, received, p)
            }));
        }
        true
    }
}
