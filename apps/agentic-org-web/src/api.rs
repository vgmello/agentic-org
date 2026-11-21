//! The production `/api/v1` backend (web-api-v1 §2 and §4, ADR-0013): the public
//! version constant, sign-in, registration-link redemption, the session view,
//! sign-out, the instance view, the organization list, organization creation and the
//! lookup of a host operation. Every other `/api/v1` path is a JSON 404.
//!
//! The web process mints each browser cookie and proof, sends only their digests
//! to the host over its admitted control connection, and caches a validated
//! session in memory for the connection generation that validated it. The
//! browser's address comes from the accepted socket ([`PeerAddress`]), never a
//! header. Nothing else forwards to the host or a local socket.
mod org_routes;

use crate::{
    body::BusinessBody,
    host::{Failure, HostControl},
    office::OfficeBridge,
    path::RequestPath,
    service::{ApiBackend, ApiReply, AsyncDispatch, PreparedCall, WebReply},
    session::{rfc3339_millis, BrowserPair, CookieName, PresentedPair, SessionCache},
    sse::{self, EventHub},
    transport::PeerAddress,
};
use hyper::{header::SET_COOKIE, http::request::Parts, Method, StatusCode};
use org_host_control::{
    browser::{
        BrowserOrigin, BrowserSession, BrowserSessionEnd, BrowserSessionEndParams, InstanceView,
        InstanceViewResult, LinkRegister, LinkRegisterParams, SessionSignIn, SessionSignInParams,
        SessionValidate, SessionValidateParams,
    },
    listen::ListenOrigin,
    ops::RecoveryState as HostRecoveryState,
    organization::{self as host_org, BrowserAuth},
    HostOpResult, LinkToken, Password, WebState,
};
use org_proto::selected_bootstrap::PlanV1;
use org_proto::web::{
    self as gateway,
    org_mutations::{OrgMutation, OrgOperation},
    organizations::{
        self, CreatingOrganization, SelectedCreateRequest, SelectedOrganizationList,
        SelectedOrganizationView,
    },
    AccountView, CreateState, ListenScheme, LookupQuery, Milliseconds, MutationOutcome, OfficeView,
    OperationId, OperationKind, OperationLookup, OrganizationLink, ReadResponse, RecoveryState,
    Scope, SessionEstablished, SessionProof, SessionView, StreamRequest, Version, WebError,
    WebErrorCode, WebStatus,
};
use serde::Deserialize;
use std::{
    collections::BTreeSet,
    net::IpAddr,
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};

/// The session routes over the web process's admitted host connection.
pub struct SessionApi {
    host: Arc<HostControl>,
    offices: OfficeBridge,
    hub: EventHub,
    cache: Arc<SessionCache>,
    cookie: CookieName,
    origin: BrowserOrigin,
    listen_scheme: ListenScheme,
}

impl SessionApi {
    /// `origin` is the configured listen origin; its scheme decides the cookie's `Secure`.
    /// `mode` is the listener's, as `tls.fetch` answered it: the instance view's
    /// `listen_scheme`.
    pub fn new(
        host: Arc<HostControl>,
        cache: Arc<SessionCache>,
        origin: &ListenOrigin,
        mode: org_host_control::listen::ListenMode,
    ) -> Self {
        Self::with_hub(host, cache, origin, mode, EventHub::default())
    }

    pub fn with_hub(
        host: Arc<HostControl>,
        cache: Arc<SessionCache>,
        origin: &ListenOrigin,
        mode: org_host_control::listen::ListenMode,
        hub: EventHub,
    ) -> Self {
        use org_host_control::listen::ListenMode;
        let scheme = origin.scheme().as_str();
        let offices = OfficeBridge::new(Arc::clone(&host), hub.clone());
        Self {
            listen_scheme: match mode {
                ListenMode::Https => ListenScheme::Https,
                ListenMode::HttpLoopback => ListenScheme::HttpLoopback,
                ListenMode::HttpPublished => ListenScheme::HttpPublished,
            },
            cookie: CookieName::new(host.instance(), scheme == "https"),
            origin: BrowserOrigin {
                scheme: scheme.to_owned(),
                host: origin.host().to_string(),
                port: origin.port(),
            },
            host,
            offices,
            hub,
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
    CreateOrganization(PresentedPair),
    OrgSnapshot(PresentedPair, String),
    OrgOffices(PresentedPair, String),
    OfficeRestart(PresentedPair, String, String),
    OrgMutation(PresentedPair, String, OrgOperation),
    OrgOperation(PresentedPair, String, OperationId),
    EventsStream(PresentedPair),
    Operation(PresentedPair, OperationId),
}

pub enum Call {
    Version,
    SignIn(IpAddr, Credentials),
    Register(IpAddr, LinkToken, Credentials),
    Session(PresentedPair),
    SignOut(PresentedPair),
    Instance(PresentedPair),
    Organizations(PresentedPair),
    CreateOrganization(PresentedPair, OperationId, PlanV1),
    OrgSnapshot(PresentedPair, String),
    OrgOffices(PresentedPair, String),
    OfficeRestart(PresentedPair, String, String, OperationId),
    OrgMutation(PresentedPair, String, OrgMutation),
    OrgOperation(PresentedPair, String, OperationId),
    EventsStream(PresentedPair, StreamRequest),
    Operation(PresentedPair, OperationId),
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
        WebErrorCode::InvalidRequest,
        "Invalid request.",
    )
}
fn not_found() -> ApiReply {
    ApiReply::error(StatusCode::NOT_FOUND, WebErrorCode::NotFound, "Not found.")
}
fn signed_out() -> ApiReply {
    ApiReply::error(
        StatusCode::UNAUTHORIZED,
        WebErrorCode::NotAuthorized,
        "Sign in to continue.",
    )
}
fn unavailable() -> ApiReply {
    ApiReply::error(
        StatusCode::SERVICE_UNAVAILABLE,
        WebErrorCode::Unavailable,
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
            let host = WebError::from(*error);
            let (status, mut error) = match (host.code, host.field, host.rule) {
                (WebErrorCode::InvalidRequest, Some(field), Some(rule)) => {
                    let mut error = WebError::new(
                        WebErrorCode::InvalidRequest,
                        "This doesn't meet the sign-in rules.",
                    );
                    (error.field, error.rule) = (Some(field), Some(rule));
                    (StatusCode::BAD_REQUEST, error)
                }
                (WebErrorCode::RateLimited, _, _) if host.retry_after_ms.is_some() => (
                    StatusCode::TOO_MANY_REQUESTS,
                    WebError::new(
                        WebErrorCode::RateLimited,
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

/// A refusal decided here, before any host call: the body or the path broke a rule.
fn bad_input(error: WebError) -> ApiReply {
    ApiReply::json(
        StatusCode::BAD_REQUEST,
        serde_json::to_value(ReadResponse::<()>::Error(error)).expect("gateway error serializes"),
    )
}

/// The browser's wording for a host refusal of a create: the code, `field` and `rule`
/// are the host's, the message is this process's own.
fn organization_error(host: WebError) -> WebError {
    use WebErrorCode::*;
    let message = match host.code {
        InvalidRequest => "This doesn't meet the organization rules.",
        AlreadyExists => "An organization with that name or slug already exists.",
        OperationReused => "This operation ID was already used for another request.",
        ClockSkew => "This device's clock differs too much from the server's.",
        NotAuthorized => "Only the instance administrator can create an organization.",
        _ => {
            return WebError::new(Unavailable, "Service unavailable.");
        }
    };
    let mut error = WebError::new(host.code, message);
    (error.field, error.rule) = (host.field, host.rule);
    error
}

fn recovery_state(state: HostRecoveryState) -> Result<RecoveryState, Failure> {
    match state {
        HostRecoveryState::Pending => Ok(RecoveryState::Pending),
        HostRecoveryState::Delivered => Ok(RecoveryState::Delivered),
        HostRecoveryState::Acknowledged => Ok(RecoveryState::Acknowledged),
        _ => Err(Failure::Unavailable),
    }
}

/// The public organization view of the host's: no private endpoint, no recovery code.
fn organization_view(
    host: host_org::OrganizationView,
) -> Result<SelectedOrganizationView, Failure> {
    Ok(SelectedOrganizationView {
        id: host.id,
        name: host.name,
        slug: host.slug,
        created_at: host.created_at,
        employee_id: host.employee_id,
        offices: host.offices.into_iter().map(OfficeView::from).collect(),
        recovery: recovery_state(host.recovery)?,
    })
}

/// What a host answer about one create means for the browser. A refusal the host stated
/// is `rejected`; a lost or unreadable answer is the caller's to report.
fn create_outcome(
    id: &OperationId,
    reply: Result<HostOpResult<host_org::OrganizationView>, Failure>,
) -> Result<MutationOutcome<SelectedOrganizationView>, Failure> {
    let operation_id = id.clone();
    let rejected = |error| MutationOutcome::Rejected {
        operation_id: id.clone(),
        error: organization_error(error),
    };
    Ok(match reply {
        Ok(HostOpResult::Accepted) => MutationOutcome::Accepted { operation_id },
        Ok(HostOpResult::Completed { result }) => MutationOutcome::Completed {
            operation_id,
            result: organization_view(result)?,
        },
        Ok(HostOpResult::Rejected { refusal }) => rejected(WebError::from(refusal)),
        Ok(_) => MutationOutcome::Unknown { operation_id },
        Err(Failure::Refused(error)) => {
            let error = WebError::from(*error);
            // Only a refusal this route names is final. Anything else (a rate limit, a
            // code from a newer host) leaves the outcome unknown: the same id asks again.
            if !matches!(
                error.code,
                WebErrorCode::InvalidRequest
                    | WebErrorCode::AlreadyExists
                    | WebErrorCode::OperationReused
                    | WebErrorCode::ClockSkew
            ) {
                return Err(Failure::Unavailable);
            }
            rejected(error)
        }
        Err(Failure::Denied) => rejected(WebError::new(WebErrorCode::NotAuthorized, "")),
        Err(other) => return Err(other),
    })
}

/// A mutation outcome with its HTTP class (202 when `accepted_allowed`, 200, the
/// refusal's authenticated class — 403 for `not_authorized`, never 401 — or 504 unknown).
fn mutation<T: serde::Serialize>(outcome: MutationOutcome<T>, accepted_allowed: bool) -> ApiReply {
    if !accepted_allowed && matches!(outcome, MutationOutcome::Accepted { .. }) {
        return unavailable();
    }
    let status = outcome
        .http_status(true)
        .and_then(|status| StatusCode::from_u16(status).ok())
        .unwrap_or(StatusCode::SERVICE_UNAVAILABLE);
    match serde_json::to_value(&outcome) {
        Ok(body) => ApiReply::json(status, body),
        Err(_) => unavailable(),
    }
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
        .unwrap_or(0)
}

/// The public instance view of the host's `instance.view` (web-api-v1 §3). `web_status`
/// is `ok` only while the host reports the web process running. `tls` is the host's word on
/// an `https` listener's certificate, absent for plain HTTP; no path, socket or certificate
/// material is reported.
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
            WebState::Running => WebStatus::Ok,
            _ => WebStatus::Degraded,
        },
        listen_scheme,
        tls: host.tls,
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

    fn auth(&self, pair: &PresentedPair) -> BrowserAuth {
        BrowserAuth {
            cookie_digest: pair.cookie_digest,
            proof_digest: pair.proof_digest,
            origin: self.origin.clone(),
        }
    }

    /// The lookup's query names this instance and nothing else (web-api-v1 §5): checked
    /// before any host call. Ids need no escaping, so an escape is refused.
    fn lookup_query(query: Option<&str>) -> Result<LookupQuery, ApiReply> {
        let query = query
            .filter(|q| !q.contains(['%', '+']))
            .ok_or_else(invalid)?;
        let fields = query
            .split('&')
            .map(|pair| pair.split_once('='))
            .collect::<Option<Vec<_>>>()
            .ok_or_else(invalid)?;
        LookupQuery::from_fields(fields).map_err(|_| invalid())
    }

    fn host_lookup_scope(&self, query: Option<&str>) -> Result<(), ApiReply> {
        Self::lookup_query(query)?
            .scope(OperationKind::Host, self.host.instance())
            .map(drop)
            .map_err(|_| invalid())
    }

    /// `GET /api/v1/operations/org/{id}` requires `instance_id` (matching this host) and
    /// `org_id`, with no `office_id` or unknown/repeated fields (web-api-v1 §5).
    fn org_lookup_scope(&self, query: Option<&str>) -> Result<String, ApiReply> {
        match Self::lookup_query(query)?
            .scope(OperationKind::Org, self.host.instance())
            .map_err(|_| invalid())?
        {
            Scope::Organization { org_id, .. } => {
                if !org_office_launch::is_storage_id(&org_id) {
                    return Err(invalid());
                }
                Ok(org_id)
            }
            _ => Err(invalid()),
        }
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

    /// Async validation for non-blocking office RPCs and SSE streams.
    pub(super) async fn validate_async(
        &self,
        pair: PresentedPair,
    ) -> Result<BrowserSession, Failure> {
        let generation = self.host.generation();
        if let Some(session) = self.cache.lookup(
            &pair.cookie_digest,
            &pair.proof_digest,
            generation,
            now_ms(),
        ) {
            return Ok(session);
        }
        self.revalidate_session_async(pair).await
    }

    /// Re-validates directly with the host, bypassing the in-memory session cache.
    pub(super) async fn revalidate_session_async(
        &self,
        pair: PresentedPair,
    ) -> Result<BrowserSession, Failure> {
        self.host
            .request_then_async::<SessionValidate>(
                &SessionValidateParams {
                    cookie_digest: pair.cookie_digest,
                    proof_digest: pair.proof_digest,
                    origin: self.origin.clone(),
                },
                Some(Box::new(self.caching(&pair))),
            )
            .await
    }

    async fn execute_events_stream(&self, pair: PresentedPair, request: StreamRequest) -> WebReply {
        let session = match self.validate_async(pair).await {
            Ok(session) => session,
            Err(error) => return WebReply::Json(failure(error)),
        };
        let list = match self
            .host
            .request_async::<host_org::List>(&host_org::ListParams {
                auth: self.auth(&pair),
            })
            .await
        {
            Ok(list) => list,
            Err(error) => return WebReply::Json(failure(error)),
        };
        let linked: BTreeSet<String> = list.organizations.into_iter().map(|o| o.id).collect();
        if !sse::authorize_subscriptions(&request, self.host.instance(), &linked) {
            return WebReply::Json(ApiReply::error(
                StatusCode::FORBIDDEN,
                WebErrorCode::NotAuthorized,
                "Subscription scope is not authorized.",
            ));
        }
        WebReply::Stream(sse::open_stream(
            &self.hub,
            self.host.instance().to_owned(),
            session.session_id,
            request,
        ))
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
            ["api", "v1", "organizations"] if post => {
                Route::CreateOrganization(self.pair(request)?)
            }
            ["api", "v1", "organizations", org, "snapshot"] if get => {
                let pair = self.pair(request)?;
                if !org_office_launch::is_storage_id(org) {
                    return Err(invalid());
                }
                Route::OrgSnapshot(pair, (*org).to_owned())
            }
            ["api", "v1", "organizations", org, "offices"] if get => {
                let pair = self.pair(request)?;
                if !org_office_launch::is_storage_id(org) {
                    return Err(invalid());
                }
                Route::OrgOffices(pair, (*org).to_owned())
            }
            ["api", "v1", "organizations", org, "offices", office, "restart"] if post => {
                let pair = self.pair(request)?;
                if !org_office_launch::is_storage_id(org)
                    || !org_office_launch::is_storage_id(office)
                {
                    return Err(invalid());
                }
                Route::OfficeRestart(pair, (*org).to_owned(), (*office).to_owned())
            }
            ["api", "v1", "organizations", org, "teams"] if post => {
                let pair = self.pair(request)?;
                if !org_office_launch::is_storage_id(org) {
                    return Err(invalid());
                }
                Route::OrgMutation(pair, (*org).to_owned(), OrgOperation::TeamCreate)
            }
            ["api", "v1", "organizations", org, resource @ ("teams" | "members"), action]
                if post =>
            {
                let Some(op) = org_routes::route_org_action(resource, action) else {
                    return Err(not_found());
                };
                let pair = self.pair(request)?;
                if !org_office_launch::is_storage_id(org) {
                    return Err(invalid());
                }
                Route::OrgMutation(pair, (*org).to_owned(), op)
            }
            ["api", "v1", "organizations", org, "projects", "add-team" | "remove-team"] if post => {
                let _ = self.pair(request)?;
                if !org_office_launch::is_storage_id(org) {
                    return Err(invalid());
                }
                return Err(unavailable());
            }
            ["api", "v1", "events", "stream"] if post => Route::EventsStream(self.pair(request)?),
            ["api", "v1", "operations", "host", id]
                if matches!(request.method, Method::GET | Method::HEAD) =>
            {
                let pair = self.pair(request)?;
                let id = organizations::host_operation_id(id).map_err(bad_input)?;
                self.host_lookup_scope(request.uri.query())?;
                Route::Operation(pair, id)
            }
            ["api", "v1", "operations", "org", id]
                if matches!(request.method, Method::GET | Method::HEAD) =>
            {
                let pair = self.pair(request)?;
                let id = organizations::host_operation_id(id).map_err(bad_input)?;
                let org_id = self.org_lookup_scope(request.uri.query())?;
                Route::OrgOperation(pair, org_id, id)
            }
            ["api", "v1", "instance"] if get => Route::Instance(self.pair(request)?),
            ["api", "v1", "version"]
            | ["api", "v1", "instance"]
            | ["api", "v1", "session", "sign-in"]
            | ["api", "v1", "links", "register"]
            | ["api", "v1", "session"]
            | ["api", "v1", "session", "sign-out"]
            | ["api", "v1", "organizations"]
            | ["api", "v1", "organizations", _, "snapshot"]
            | ["api", "v1", "organizations", _, "offices"]
            | ["api", "v1", "organizations", _, "offices", _, "restart"]
            | ["api", "v1", "organizations", _, "teams"]
            | ["api", "v1", "organizations", _, "teams", "create" | "rename" | "move" | "set-lead" | "delete" | "set-title"]
            | ["api", "v1", "organizations", _, "members", "add-person" | "update-person" | "remove" | "set-reports-to" | "move"]
            | ["api", "v1", "organizations", _, "projects", "add-team" | "remove-team"]
            | ["api", "v1", "events", "stream"]
            | ["api", "v1", "operations", "host", _]
            | ["api", "v1", "operations", "org", _] => return Err(invalid()),
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
            Route::OrgSnapshot(pair, org) => empty().map(|()| Call::OrgSnapshot(pair, org))?,
            Route::OrgOffices(pair, org) => empty().map(|()| Call::OrgOffices(pair, org))?,
            Route::OfficeRestart(pair, org, office) => {
                let raw: serde_json::Value =
                    serde_json::from_slice(bytes).map_err(|_| invalid())?;
                let id_str = raw
                    .get("operation_id")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or_default();
                let id = organizations::host_operation_id(id_str).map_err(bad_input)?;
                let request: gateway::OfficeRestartRequest =
                    gateway::decode_request(bytes).map_err(|_| invalid())?;
                let target = Scope::Office {
                    instance_id: self.host.instance().to_owned(),
                    org_id: org.clone(),
                    office_id: office.clone(),
                };
                request.scope.check_target(&target).map_err(|_| invalid())?;
                return Ok(PreparedCall::mutation(
                    Call::OfficeRestart(pair, org, office, id.clone()),
                    id,
                    bytes.len(),
                ));
            }
            Route::OrgMutation(pair, org, op) => {
                let scope = Scope::Organization {
                    instance_id: self.host.instance().to_owned(),
                    org_id: org.clone(),
                };
                let command = op.decode(bytes, &scope).map_err(bad_input)?;
                let id = command.operation_id().clone();
                return Ok(PreparedCall::mutation(
                    Call::OrgMutation(pair, org, command),
                    id,
                    bytes.len(),
                ));
            }
            Route::OrgOperation(pair, org, id) => {
                empty().map(|()| Call::OrgOperation(pair, org, id))?
            }
            Route::EventsStream(pair) => {
                let request: StreamRequest =
                    gateway::decode_request(bytes).map_err(|_| invalid())?;
                if request.subscriptions.is_empty() || request.subscriptions.len() > 32 {
                    return Err(invalid());
                }
                Call::EventsStream(pair, request)
            }
            Route::Instance(pair) => empty().map(|()| Call::Instance(pair))?,
            Route::Operation(pair, id) => empty().map(|()| Call::Operation(pair, id))?,
            Route::CreateOrganization(pair) => {
                // The operation id's format is decided first and named, then the envelope,
                // its scope and the payload's rules: all before any host call.
                let raw: serde_json::Value =
                    serde_json::from_slice(bytes).map_err(|_| invalid())?;
                // A missing id, or one that is not a string, is named the same way.
                let id = raw
                    .get("operation_id")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or_default();
                let id = organizations::host_operation_id(id).map_err(bad_input)?;
                let request: SelectedCreateRequest =
                    gateway::decode_request(bytes).map_err(|_| invalid())?;
                let instance = Scope::Instance {
                    instance_id: self.host.instance().to_owned(),
                };
                request
                    .scope
                    .check_target(&instance)
                    .map_err(|_| invalid())?;
                let plan = request.params.plan().map_err(bad_input)?;
                return Ok(PreparedCall::mutation(
                    Call::CreateOrganization(pair, id.clone(), plan),
                    id,
                    bytes.len(),
                ));
            }
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
    fn dispatch_async(self: Arc<Self>, call: Call) -> AsyncDispatch<Call> {
        match call {
            Call::OrgSnapshot(pair, org_id) => AsyncDispatch::Async {
                future: Box::pin(async move {
                    WebReply::Json(org_routes::execute_org_snapshot(&self, pair, &org_id).await)
                }),
                streaming: false,
            },
            Call::OrgOffices(pair, org_id) => AsyncDispatch::Async {
                future: Box::pin(async move {
                    WebReply::Json(org_routes::execute_org_offices(&self, pair, &org_id).await)
                }),
                streaming: false,
            },
            Call::OfficeRestart(pair, org_id, office_id, op_id) => AsyncDispatch::Async {
                future: Box::pin(async move {
                    WebReply::Json(
                        org_routes::execute_office_restart(&self, pair, &org_id, &office_id, op_id)
                            .await,
                    )
                }),
                streaming: false,
            },
            Call::OrgMutation(pair, org_id, command) => AsyncDispatch::Async {
                future: Box::pin(async move {
                    WebReply::Json(
                        org_routes::execute_org_mutation(&self, pair, &org_id, command).await,
                    )
                }),
                streaming: false,
            },
            Call::OrgOperation(pair, org_id, op_id) => AsyncDispatch::Async {
                future: Box::pin(async move {
                    WebReply::Json(
                        org_routes::execute_org_operation(&self, pair, &org_id, op_id).await,
                    )
                }),
                streaming: false,
            },
            Call::EventsStream(pair, request) => AsyncDispatch::Async {
                future: Box::pin(async move { self.execute_events_stream(pair, request).await }),
                streaming: true,
            },
            other => AsyncDispatch::Sync(other),
        }
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
            // Always the host's answer: what an account can open is never read from a cache.
            Call::Organizations(pair) => {
                let listed = self
                    .validate(pair)
                    .and_then(|_| {
                        self.host.request::<host_org::List>(&host_org::ListParams {
                            auth: self.auth(&pair),
                        })
                    })
                    .and_then(|list| {
                        Ok(SelectedOrganizationList {
                            organizations: list
                                .organizations
                                .into_iter()
                                .map(organization_view)
                                .collect::<Result<_, _>>()?,
                            creating: list
                                .creating
                                .into_iter()
                                .map(|c| {
                                    Ok(CreatingOrganization {
                                        operation_id: OperationId::parse(c.operation_id)
                                            .map_err(|_| Failure::Unavailable)?,
                                        name: c.name,
                                        slug: c.slug,
                                    })
                                })
                                .collect::<Result<_, Failure>>()?,
                        })
                    });
                match listed {
                    Ok(list) => result(list),
                    Err(Failure::Refused(_)) => unavailable(),
                    Err(e) => failure(e),
                }
            }
            Call::CreateOrganization(pair, id, plan) => {
                if let Err(e) = self.validate(pair) {
                    return failure(e);
                }
                // Let the host decide administrator authority authoritatively (L-05: no
                // duplicated BFF admin short-circuit).
                let reply = self
                    .host
                    .request::<host_org::Create>(&host_org::CreateParams {
                        auth: self.auth(&pair),
                        operation_id: id.as_str().to_owned(),
                        plan,
                    });
                // The host answers `expired` only for an id it has never seen (one it
                // knows answers its stored state at any age), so no create has this id:
                // a plain error, never an outcome of the operation.
                if matches!(&reply, Err(Failure::Refused(error))
                    if error.code == org_proto::ErrorCode::Expired)
                {
                    return ApiReply::error(
                        StatusCode::GONE,
                        WebErrorCode::Expired,
                        "This request expired. Start again.",
                    );
                }
                // No stated answer: the create may or may not have been admitted. The
                // same id asks again.
                mutation(
                    create_outcome(&id, reply)
                        .unwrap_or(MutationOutcome::Unknown { operation_id: id }),
                    true,
                )
            }
            Call::OrgSnapshot(_, _)
            | Call::OrgOffices(_, _)
            | Call::OfficeRestart(_, _, _, _)
            | Call::OrgMutation(_, _, _)
            | Call::OrgOperation(_, _, _)
            | Call::EventsStream(_, _) => {
                unreachable!("office RPCs and SSE streams dispatch asynchronously")
            }
            Call::Operation(pair, id) => {
                let reply = self.validate(pair).and_then(|_| {
                    self.host.request::<host_org::OperationLookup>(
                        &host_org::OperationLookupParams {
                            auth: self.auth(&pair),
                            operation_id: id.as_str().to_owned(),
                        },
                    )
                });
                let outcome = match reply {
                    Err(e @ (Failure::Denied | Failure::NotFound | Failure::Unavailable)) => {
                        return failure(e)
                    }
                    // A refused lookup is the lookup's own error, never a stored refusal
                    // of the create: `expired` is 410, anything else is unavailable. (The
                    // host looks a known operation up at any age and answers `not_found`
                    // for any other, so it sends no `expired` here today.)
                    Err(Failure::Refused(error)) => {
                        return if error.code == org_proto::ErrorCode::Expired {
                            ApiReply::error(
                                StatusCode::GONE,
                                WebErrorCode::Expired,
                                "This operation is too old to look up.",
                            )
                        } else {
                            unavailable()
                        };
                    }
                    stated => create_outcome(&id, stated),
                };
                let state = match &outcome {
                    Ok(MutationOutcome::Accepted { .. }) => Some(CreateState::Reserved),
                    Ok(MutationOutcome::Completed { .. }) => Some(CreateState::Linked),
                    Ok(MutationOutcome::Rejected { .. }) => None,
                    _ => return unavailable(),
                };
                match outcome {
                    Ok(outcome) => result(OperationLookup::Outcome {
                        kind: OperationKind::Host,
                        state,
                        outcome,
                    }),
                    Err(e) => failure(e),
                }
            }
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
