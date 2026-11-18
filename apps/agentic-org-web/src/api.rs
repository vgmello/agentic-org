//! The production `/api/v1` backend (web-api-v1 §2 and §4, ADR-0013): the public
//! version constant, sign-in, registration-link redemption, the session view,
//! sign-out, the instance view and the organization list. Every other `/api/v1` path is a JSON 404.
//!
//! The web process mints each browser cookie and proof, sends only their digests
//! to the host over its admitted control connection, and caches a validated
//! session in memory for the connection generation that validated it. The
//! browser's address comes from the accepted socket ([`PeerAddress`]), never a
//! header. Nothing else forwards to the host or a local socket.
use crate::{
    body::BusinessBody,
    host::{Failure, HostControl},
    path::RequestPath,
    service::{ApiBackend, ApiReply, PreparedCall},
    session::{rfc3339_millis, BrowserPair, CookieName, PresentedPair, SessionCache},
    transport::PeerAddress,
};
use hyper::{header::SET_COOKIE, http::request::Parts, Method, StatusCode};
use org_host_control::{
    browser::{
        BrowserOrigin, BrowserSession, BrowserSessionEnd, BrowserSessionEndParams, InstanceView,
        InstanceViewResult, LinkRegister, LinkRegisterParams, SessionSignIn, SessionSignInParams,
        SessionValidate, SessionValidateParams,
    },
    listen::{ListenOrigin, OriginScheme},
    GatewayState, LinkToken, Password,
};
use org_proto::gateway::{
    self as gateway, AccountView, GatewayError, GatewayErrorCode, GatewayStatus, ListenScheme,
    Milliseconds, OrganizationLink, ReadResponse, SessionEstablished, SessionProof, SessionView,
    Version,
};
use serde::Deserialize;
use std::{
    net::IpAddr,
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};

/// The session routes over the web process's admitted host connection.
pub struct SessionApi {
    host: Arc<HostControl>,
    cache: Arc<SessionCache>,
    cookie: CookieName,
    origin: BrowserOrigin,
    listen_scheme: ListenScheme,
}

impl SessionApi {
    /// `origin` is the configured listen origin; its scheme decides the cookie's `Secure`.
    pub fn new(host: Arc<HostControl>, cache: Arc<SessionCache>, origin: &ListenOrigin) -> Self {
        let scheme = origin.scheme().as_str();
        Self {
            listen_scheme: match origin.scheme() {
                OriginScheme::Https => ListenScheme::Https,
                _ => ListenScheme::HttpLoopback,
            },
            cookie: CookieName::new(host.instance(), scheme == "https"),
            origin: BrowserOrigin {
                scheme: scheme.to_owned(),
                host: origin.host().to_string(),
                port: origin.port(),
            },
            host,
            cache,
        }
    }
}

pub enum Route {
    Version,
    SignIn(IpAddr),
    Register(IpAddr),
    Session(PresentedPair),
    SignOut(PresentedPair),
    Instance(PresentedPair),
    Organizations(PresentedPair),
}

pub enum Call {
    Version,
    SignIn(IpAddr, Credentials),
    Register(IpAddr, LinkToken, Credentials),
    Session(PresentedPair),
    SignOut(PresentedPair),
    Instance(PresentedPair),
    Organizations(PresentedPair),
}

/// A login and password as the browser sent them; the password stays zeroizing.
pub struct Credentials {
    login: String,
    password: Password,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SignInBody {
    login: String,
    #[serde(deserialize_with = "crate::secret_json::password")]
    password: Password,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RegisterBody {
    #[serde(deserialize_with = "crate::secret_json::link")]
    link: LinkToken,
    login: String,
    #[serde(deserialize_with = "crate::secret_json::password")]
    password: Password,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EmptyBody {}

fn invalid() -> ApiReply {
    ApiReply::error(
        StatusCode::BAD_REQUEST,
        GatewayErrorCode::InvalidRequest,
        "Invalid request.",
    )
}
fn not_found() -> ApiReply {
    ApiReply::error(
        StatusCode::NOT_FOUND,
        GatewayErrorCode::NotFound,
        "Not found.",
    )
}
fn signed_out() -> ApiReply {
    ApiReply::error(
        StatusCode::UNAUTHORIZED,
        GatewayErrorCode::NotAuthorized,
        "Sign in to continue.",
    )
}
fn unavailable() -> ApiReply {
    ApiReply::error(
        StatusCode::SERVICE_UNAVAILABLE,
        GatewayErrorCode::Unavailable,
        "Service unavailable.",
    )
}
fn result(value: impl serde::Serialize) -> ApiReply {
    ApiReply::json(
        StatusCode::OK,
        serde_json::to_value(ReadResponse::Result(value)).expect("result serializes"),
    )
}

/// web-api-v1 §5: 401 for no valid session, 404 for a link that doesn't redeem, a
/// credential rule refusal (400 with `field`/`rule`), a rate limit (429 with
/// `retry_after_ms`), 503 for everything else. Only `code`, `field`, `rule` and
/// `retry_after_ms` come from the host; the messages are this process's own, so the
/// web-to-host contract's wording never reaches the browser. An `invalid_request`
/// without a rule pair is a web-to-host fault, not the browser's: 503.
fn failure(failure: Failure) -> ApiReply {
    match failure {
        Failure::Denied => signed_out(),
        Failure::NotFound => not_found(),
        Failure::Refused(error) => {
            let host = GatewayError::from(*error);
            let (status, mut error) = match (host.code, host.field, host.rule) {
                (GatewayErrorCode::InvalidRequest, Some(field), Some(rule)) => {
                    let mut error = GatewayError::new(
                        GatewayErrorCode::InvalidRequest,
                        "This doesn't meet the sign-in rules.",
                    );
                    (error.field, error.rule) = (Some(field), Some(rule));
                    (StatusCode::BAD_REQUEST, error)
                }
                (GatewayErrorCode::RateLimited, _, _) if host.retry_after_ms.is_some() => (
                    StatusCode::TOO_MANY_REQUESTS,
                    GatewayError::new(
                        GatewayErrorCode::RateLimited,
                        "Too many attempts. Try again later.",
                    ),
                ),
                _ => return unavailable(),
            };
            if status == StatusCode::TOO_MANY_REQUESTS {
                error.retry_after_ms = host.retry_after_ms;
            }
            ApiReply::json(
                status,
                serde_json::to_value(ReadResponse::<()>::Error(error))
                    .expect("gateway error serializes"),
            )
        }
        Failure::Unavailable => unavailable(),
    }
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
        .unwrap_or(0)
}

/// The public instance view of the host's `instance.view` (web-api-v1 §3). `web_status`
/// is `ok` only while the host reports the web process running. `tls` stays absent
/// until TLS lands; no path, socket or certificate material is reported.
fn instance_view(
    instance_id: &str,
    listen_scheme: ListenScheme,
    host: InstanceViewResult,
) -> Result<gateway::InstanceView, Failure> {
    Ok(gateway::InstanceView {
        instance_id: instance_id.to_owned(),
        name: host.name,
        server_version: env!("CARGO_PKG_VERSION").to_owned(),
        web_status: match host.web_status {
            GatewayState::Running => GatewayStatus::Ok,
            _ => GatewayStatus::Degraded,
        },
        listen_scheme,
        tls: None,
        server_time_ms: Milliseconds::new(now_ms()).map_err(|_| Failure::Unavailable)?,
    })
}

/// The public session view of a host session record. Never a digest or handle.
fn view(instance_id: &str, session: BrowserSession) -> Result<SessionView, Failure> {
    Ok(SessionView {
        instance_id: instance_id.to_owned(),
        account: AccountView {
            id: session.account.id,
            login: session.account.login,
        },
        admin: session.admin,
        organizations: session
            .orgs
            .into_iter()
            .map(|o| OrganizationLink {
                org_id: o.org_id,
                employee_id: o.employee_id,
                office_ids: o.office_ids,
            })
            .collect(),
        // The reply's time, not the record's: a cached session may be hours old.
        server_time_ms: Milliseconds::new(now_ms()).map_err(|_| Failure::Unavailable)?,
        idle_expires_at: rfc3339_millis(session.idle_expires_at_ms),
    })
}

impl SessionApi {
    /// The accepted socket's peer; absent only off the real transport (fails closed).
    fn peer(request: &Parts) -> Result<IpAddr, ApiReply> {
        request
            .extensions
            .get::<PeerAddress>()
            .map(|p| p.0.ip())
            .ok_or_else(unavailable)
    }

    /// A protected route needs exactly one cookie of this instance's name and one
    /// proof header; anything else is 401 before any host call.
    fn pair(&self, request: &Parts) -> Result<PresentedPair, ApiReply> {
        PresentedPair::read(&self.cookie, &request.headers).ok_or_else(signed_out)
    }

    /// Inserts a successful reply into the cache on the host reader thread, under
    /// this connection's generation, before any later host frame is applied: a
    /// `session.ended` that follows the reply removes the entry it made.
    fn caching(
        &self,
        pair: &PresentedPair,
    ) -> impl FnOnce(&Result<BrowserSession, Failure>) + Send + 'static {
        let (cache, generation, pair) = (Arc::clone(&self.cache), self.host.generation(), *pair);
        move |reply| {
            if let Ok(session) = reply {
                cache.insert(
                    pair.cookie_digest,
                    pair.proof_digest,
                    session.clone(),
                    generation,
                );
            }
        }
    }

    /// The live session for a presented pair: the cache for this connection's
    /// generation, else one `session.validate`.
    fn validate(&self, pair: PresentedPair) -> Result<BrowserSession, Failure> {
        let generation = self.host.generation();
        if let Some(session) = self.cache.lookup(
            &pair.cookie_digest,
            &pair.proof_digest,
            generation,
            now_ms(),
        ) {
            return Ok(session);
        }
        self.host.request_then::<SessionValidate>(
            &SessionValidateParams {
                cookie_digest: pair.cookie_digest,
                proof_digest: pair.proof_digest,
                origin: self.origin.clone(),
            },
            self.caching(&pair),
        )
    }

    /// Sign-in and registration: mint a fresh pair, let the host create the
    /// session against its digests, then return the view with the proof and set
    /// the cookie. A refused call sets no cookie.
    fn establish(
        &self,
        create: impl FnOnce(&PresentedPair) -> Result<BrowserSession, Failure>,
    ) -> ApiReply {
        let Ok(minted) = BrowserPair::mint() else {
            return unavailable();
        };
        let pair = PresentedPair {
            cookie_digest: minted.cookie_digest(),
            proof_digest: minted.proof_digest(),
        };
        let session = match create(&pair) {
            Ok(session) => session,
            Err(e) => return failure(e),
        };
        let Ok(proof) = SessionProof::parse(minted.proof().to_owned()) else {
            return unavailable();
        };
        match view(self.host.instance(), session) {
            Ok(view) => result(SessionEstablished::new(view, proof))
                .with_header(SET_COOKIE, self.cookie.set(&minted)),
            Err(e) => failure(e),
        }
    }

    fn clear(&self, reply: ApiReply) -> ApiReply {
        reply.with_header(SET_COOKIE, self.cookie.clear())
    }
}

impl ApiBackend for SessionApi {
    type Route = Route;
    type Call = Call;
    fn route(&self, request: &Parts, path: &RequestPath) -> Result<Route, ApiReply> {
        let query = request.uri.query().is_some();
        // HEAD is GET without the body (`into_http` drops it), as `Allow` advertises.
        let get = matches!(request.method, Method::GET | Method::HEAD) && !query;
        let post = request.method == Method::POST && !query;
        let segments: Vec<&str> = path.segments().iter().map(String::as_str).collect();
        Ok(match segments.as_slice() {
            ["api", "v1", "version"] if get => Route::Version,
            ["api", "v1", "session", "sign-in"] if post => Route::SignIn(Self::peer(request)?),
            ["api", "v1", "links", "register"] if post => Route::Register(Self::peer(request)?),
            ["api", "v1", "session"] if get => Route::Session(self.pair(request)?),
            ["api", "v1", "session", "sign-out"] if post => {
                Route::SignOut(self.pair(request).map_err(|r| self.clear(r))?)
            }
            ["api", "v1", "organizations"] if get => Route::Organizations(self.pair(request)?),
            ["api", "v1", "instance"] if get => Route::Instance(self.pair(request)?),
            ["api", "v1", "version"]
            | ["api", "v1", "instance"]
            | ["api", "v1", "session", "sign-in"]
            | ["api", "v1", "links", "register"]
            | ["api", "v1", "session"]
            | ["api", "v1", "session", "sign-out"]
            | ["api", "v1", "organizations"] => return Err(invalid()),
            _ => return Err(not_found()),
        })
    }
    fn prepare(&self, route: Route, body: BusinessBody) -> Result<PreparedCall<Call>, ApiReply> {
        let bytes = body.as_bytes();
        let empty = || bytes.is_empty().then_some(()).ok_or_else(invalid);
        let call = match route {
            Route::Version => empty().map(|()| Call::Version)?,
            Route::Session(pair) => empty().map(|()| Call::Session(pair))?,
            Route::Organizations(pair) => empty().map(|()| Call::Organizations(pair))?,
            Route::Instance(pair) => empty().map(|()| Call::Instance(pair))?,
            Route::SignOut(pair) => {
                serde_json::from_slice::<EmptyBody>(bytes).map_err(|_| invalid())?;
                Call::SignOut(pair)
            }
            Route::SignIn(address) => {
                let SignInBody { login, password } =
                    serde_json::from_slice(bytes).map_err(|_| invalid())?;
                Call::SignIn(address, Credentials { login, password })
            }
            Route::Register(address) => {
                let RegisterBody {
                    link,
                    login,
                    password,
                } = serde_json::from_slice(bytes).map_err(|_| invalid())?;
                Call::Register(address, link, Credentials { login, password })
            }
        };
        Ok(PreparedCall::read(call, bytes.len()))
    }
    fn execute(&self, call: Call) -> ApiReply {
        match call {
            Call::Version => result(Version::default()),
            Call::Session(pair) => match self
                .validate(pair)
                .and_then(|s| view(self.host.instance(), s))
            {
                Ok(view) => result(view),
                Err(e) => failure(e),
            },
            Call::Instance(pair) => match self
                .validate(pair)
                .and_then(|_| self.host.request::<InstanceView>(&Default::default()))
                .and_then(|v| instance_view(self.host.instance(), self.listen_scheme, v))
            {
                Ok(view) => result(view),
                Err(e) => failure(e),
            },
            // No organization exists before A4: an authenticated empty list, no host call.
            Call::Organizations(pair) => match self.validate(pair) {
                Ok(_) => result(serde_json::json!({ "organizations": [] })),
                Err(e) => failure(e),
            },
            Call::SignOut(pair) => {
                // The host deletes the record and pushes `session.ended` before this
                // reply; the event sink drops the cache entry on the reader thread.
                let ended = self.validate(pair).and_then(|session| {
                    self.host
                        .request::<BrowserSessionEnd>(&BrowserSessionEndParams {
                            session_id: session.session_id,
                        })
                });
                self.clear(match ended {
                    Ok(_) => result(serde_json::json!({})),
                    Err(e) => failure(e),
                })
            }
            Call::SignIn(client_address, Credentials { login, password }) => {
                self.establish(|pair| {
                    self.host.request_then::<SessionSignIn>(
                        &SessionSignInParams {
                            login,
                            password,
                            client_address,
                            cookie_digest: pair.cookie_digest,
                            proof_digest: pair.proof_digest,
                            origin: self.origin.clone(),
                        },
                        self.caching(pair),
                    )
                })
            }
            Call::Register(client_address, link, Credentials { login, password }) => self
                .establish(|pair| {
                    self.host.request_then::<LinkRegister>(
                        &LinkRegisterParams {
                            link,
                            login,
                            password,
                            client_address,
                            cookie_digest: pair.cookie_digest,
                            proof_digest: pair.proof_digest,
                            origin: self.origin.clone(),
                        },
                        self.caching(pair),
                    )
                }),
        }
    }
}

#[cfg(test)]
#[path = "api/tests.rs"]
mod tests;
