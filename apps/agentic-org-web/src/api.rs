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
mod account_routes;
mod attachment_routes;
mod chat_routes;
mod cost_routes;
mod doc_routes;
mod git_routes;
mod hiring_routes;
mod hq_context;
mod instance_routes;
mod note_routes;
mod office_routes;
mod open_routes;
mod org_routes;
mod process_routes;
mod provider_routes;
mod search_routes;
mod session_routes;
mod task_routes;
mod tess_routes;

pub use instance_routes::{CallerAuth, TlsEnableInput};

use crate::{
    event_hub::{EventBus, EventHub, InMemoryEventBus},
    host::HostControl,
    office::OfficeBridge,
    ports::{ControlPlaneTransport, Failure, OfficeTransport},
    service::ApiReply,
    session::{rfc3339_millis, CookieName, PresentedPair, SessionCache},
};
use hyper::StatusCode;
use org_host_control::{
    browser::{BrowserOrigin, BrowserSession, InstanceViewResult},
    listen::ListenOrigin,
    ops::RecoveryState as HostRecoveryState,
    organization::{self as host_org},
    HostOpResult, LinkToken, Password, WebState,
};
use org_proto::selected_bootstrap::PlanV1;
use org_proto::web::{
    self as gateway,
    org_mutations::{OrgMutation, OrgOperation},
    organizations::SelectedOrganizationView,
    AccountView, ListenScheme, Milliseconds, MutationOutcome, OfficeView, OperationId,
    OrganizationLink, ReadResponse, RecoveryState, SessionView, StreamRequest, WebError,
    WebErrorCode, WebStatus,
};
use serde::Deserialize;
use std::{
    net::IpAddr,
    sync::{atomic::AtomicBool, Arc},
    time::{SystemTime, UNIX_EPOCH},
};

mod backend;
mod sessions;

/// The session routes over the web process's consumer-owned transport and event-bus ports.
pub struct SessionApi {
    host: Arc<dyn ControlPlaneTransport>,
    offices: Arc<dyn OfficeTransport>,
    events: Arc<dyn EventBus>,
    cache: Arc<SessionCache>,
    cookie: CookieName,
    origin: BrowserOrigin,
    listen_scheme: ListenScheme,
    auth_enabled: Arc<AtomicBool>,
}

impl SessionApi {
    /// Constructs a [`SessionApi`] wired strictly through consumer-owned transport and event ports.
    pub fn from_ports(
        host: Arc<dyn ControlPlaneTransport>,
        offices: Arc<dyn OfficeTransport>,
        events: Arc<dyn EventBus>,
        cache: Arc<SessionCache>,
        origin: &ListenOrigin,
        mode: org_host_control::listen::ListenMode,
        auth_enabled: Arc<AtomicBool>,
    ) -> Self {
        use org_host_control::listen::ListenMode;
        let scheme = origin.scheme().as_str();
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
            events,
            cache,
            auth_enabled,
        }
    }

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
        Self::with_access(
            host,
            cache,
            origin,
            mode,
            hub,
            Arc::new(AtomicBool::new(true)),
        )
    }

    pub fn with_access(
        host: Arc<HostControl>,
        cache: Arc<SessionCache>,
        origin: &ListenOrigin,
        mode: org_host_control::listen::ListenMode,
        hub: EventHub,
        auth_enabled: Arc<AtomicBool>,
    ) -> Self {
        let events: Arc<dyn EventBus> = Arc::new(InMemoryEventBus::new(hub));
        let offices: Arc<dyn OfficeTransport> =
            Arc::new(OfficeBridge::new(Arc::clone(&host), Arc::clone(&events)));
        let host: Arc<dyn ControlPlaneTransport> = host;
        Self::from_ports(host, offices, events, cache, origin, mode, auth_enabled)
    }
}

pub enum Route {
    Version,
    Access,
    SignIn(IpAddr),
    Register(IpAddr),
    EnableAuth(IpAddr, Option<PresentedPair>),
    EnableTls(CallerAuth),
    Session(CallerAuth),
    SignOut(PresentedPair),
    Instance(CallerAuth),
    Organizations(CallerAuth),
    CreateOrganization(CallerAuth),
    OrgSnapshot(CallerAuth, String),
    OrgProjects(CallerAuth, String, bool),
    OrgProjectInspect(CallerAuth, String, String),
    OrgProjectCreate(CallerAuth, String),
    OrgOffices(CallerAuth, String),
    OfficeRestart(CallerAuth, String, String),
    OrgMutation(CallerAuth, String, OrgOperation),
    OrgOperation(CallerAuth, String, OperationId),
    EventsStream(CallerAuth),
    CliApprovalDecide(CallerAuth),
    Operation(CallerAuth, OperationId),
    Account(account_routes::AccountRoute),
    OpenOrg(open_routes::OpenRoute),
    Hiring(hiring_routes::HiringRoute),
    Sessions(session_routes::SessionRoute),
    Chat(chat_routes::ChatRoute),
    Tasks(task_routes::TaskRoute),
    Notes(note_routes::NoteRoute),
    Git(git_routes::GitRoute),
    Process(process_routes::ProcessRoute),
    Docs(doc_routes::DocRoute),
    Search(search_routes::SearchRoute),
    Cost(cost_routes::CostRoute),
    Provider(provider_routes::ProviderRoute),
    OfficeFederation(office_routes::OfficeFederationRoute),
    Attachments(attachment_routes::AttachmentRoute),
    Tess(tess_routes::TessRoute),
}

pub enum Call {
    Version,
    Access,
    SignIn(IpAddr, Credentials),
    Register(IpAddr, LinkToken, Credentials),
    EnableAuth(IpAddr, Option<PresentedPair>, Credentials),
    EnableTls(CallerAuth, TlsEnableInput),
    Session(CallerAuth),
    SignOut(PresentedPair),
    Instance(CallerAuth),
    Organizations(CallerAuth),
    CreateOrganization(CallerAuth, OperationId, PlanV1),
    OrgSnapshot(CallerAuth, String),
    OrgProjects(CallerAuth, String, bool),
    OrgProjectInspect(CallerAuth, String, String),
    OrgProjectCreate(
        CallerAuth,
        String,
        OperationId,
        org_proto::projects::ProjectCreateWebParams,
    ),
    OrgOffices(CallerAuth, String),
    OfficeRestart(CallerAuth, String, String, OperationId),
    OrgMutation(CallerAuth, String, OrgMutation),
    OrgOperation(CallerAuth, String, OperationId),
    EventsStream(CallerAuth, StreamRequest),
    CliApprovalDecide(CallerAuth, LinkToken, bool),
    Operation(CallerAuth, OperationId),
    Account(account_routes::AccountCall),
    OpenOrg(open_routes::OpenCall),
    Hiring(hiring_routes::HiringCall),
    Sessions(session_routes::SessionCall),
    Chat(chat_routes::ChatCall),
    Tasks(task_routes::TaskCall),
    Notes(note_routes::NoteCall),
    Git(git_routes::GitCall),
    Process(process_routes::ProcessCall),
    Docs(doc_routes::DocCall),
    Search(search_routes::SearchCall),
    Cost(cost_routes::CostCall),
    Provider(provider_routes::ProviderCall),
    OfficeFederation(office_routes::OfficeFederationCall),
    Attachments(attachment_routes::AttachmentCall),
    Tess(tess_routes::TessCall),
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
struct CliApprovalDecideBody {
    #[serde(deserialize_with = "crate::secret_json::link")]
    link: LinkToken,
    approve: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EmptyBody {}

pub(super) fn invalid() -> ApiReply {
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
                (WebErrorCode::Conflict, _, _) => (
                    StatusCode::CONFLICT,
                    WebError::new(
                        WebErrorCode::Conflict,
                        "This action conflicts with the current account state.",
                    ),
                ),
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
        auth_enabled: host.auth_enabled,
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
            time_zone: session.account.time_zone,
            last_org_id: session.account.last_org_id,
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

#[cfg(test)]
#[path = "api/tests.rs"]
mod tests;
