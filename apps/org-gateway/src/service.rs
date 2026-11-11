//! Compose static responses, bounded business calls and the shared response policy.

use std::convert::Infallible;
use std::future::Future;
use std::io;
use std::net::TcpListener;
use std::sync::Arc;
use std::time::Duration;

use http_body_util::Full;
use hyper::body::{Body, Bytes, Incoming};
use hyper::http::request::Parts;
use hyper::{Method, Request, Response, StatusCode};
use org_host_control::listen::ListenOrigin;
use org_proto::gateway::{
    GatewayError, GatewayErrorCode, MutationOutcome, OperationId, ReadResponse,
};
use serde_json::Value;

use crate::assets::BuildFiles;
use crate::blocking::BlockingPool;
use crate::body::{
    read_business_body, BodyError, BusinessBody, BODY_READ_TIMEOUT, MAX_BUSINESS_BODY_BYTES,
};
use crate::ingress::RequestAdmission;
use crate::path::RequestPath;
use crate::static_ui::response;
use crate::transport::{try_run_http, HttpLimits, ListenMode};
use crate::{Hsts, StaticUi};

#[cfg(test)]
mod admission_tests;

/// Routing and preparation perform no native I/O. Routing must authorize before
/// returning a business route. Credential routes use
/// their own zeroed ingress in B1-06 and must not enter this business-body path.
pub trait ApiBackend: Send + Sync + 'static {
    type Route: Send;
    type Call: Send + 'static;

    fn route(&self, request: &Parts, path: &RequestPath) -> Result<Self::Route, ApiReply>;
    fn prepare(
        &self,
        route: Self::Route,
        body: BusinessBody,
    ) -> Result<PreparedCall<Self::Call>, ApiReply>;
    /// Runs on a dedicated worker and must use finite native deadlines.
    fn execute(&self, call: Self::Call) -> ApiReply;
}

pub struct ApiReply {
    status: StatusCode,
    body: Bytes,
}

impl ApiReply {
    pub fn json(status: StatusCode, value: Value) -> Self {
        Self {
            status,
            body: Bytes::from(serde_json::to_vec(&value).expect("JSON value serializes")),
        }
    }

    pub fn error(status: StatusCode, code: GatewayErrorCode, message: &'static str) -> Self {
        let error = ReadResponse::<Value>::Error(GatewayError::new(code, message));
        Self::json(
            status,
            serde_json::to_value(error).expect("gateway error serializes"),
        )
    }

    fn into_http(self, head: bool) -> Response<Full<Bytes>> {
        response(
            self.status,
            self.body,
            head,
            "application/json; charset=utf-8",
            "no-store",
        )
    }
}

#[derive(Clone)]
enum CallKind {
    Read,
    Mutation(OperationId),
}

pub struct PreparedCall<T> {
    call: T,
    kind: CallKind,
    input_bytes: usize,
}

impl<T> PreparedCall<T> {
    /// Include all request-owned bytes retained by the prepared native call.
    pub fn read(call: T, input_bytes: usize) -> Self {
        Self {
            call,
            kind: CallKind::Read,
            input_bytes,
        }
    }
    pub fn mutation(call: T, operation_id: OperationId, input_bytes: usize) -> Self {
        Self {
            call,
            kind: CallKind::Mutation(operation_id),
            input_bytes,
        }
    }
}

impl CallKind {
    fn unavailable(&self) -> ApiReply {
        let error = GatewayError::new(GatewayErrorCode::Unavailable, "Service unavailable.");
        match self {
            Self::Read => ApiReply::error(
                StatusCode::SERVICE_UNAVAILABLE,
                GatewayErrorCode::Unavailable,
                "Service unavailable.",
            ),
            Self::Mutation(id) => ApiReply::json(
                StatusCode::SERVICE_UNAVAILABLE,
                serde_json::to_value(MutationOutcome::<Value>::Rejected {
                    operation_id: id.clone(),
                    error,
                })
                .expect("mutation refusal serializes"),
            ),
        }
    }

    fn unknown(&self) -> ApiReply {
        match self {
            Self::Read => ApiReply::error(
                StatusCode::SERVICE_UNAVAILABLE,
                GatewayErrorCode::Timeout,
                "Backend did not answer in time.",
            ),
            Self::Mutation(id) => ApiReply::json(
                StatusCode::GATEWAY_TIMEOUT,
                serde_json::to_value(MutationOutcome::<Value>::Unknown {
                    operation_id: id.clone(),
                })
                .expect("mutation outcome serializes"),
            ),
        }
    }
}

#[derive(Clone, Copy)]
pub struct ApiLimits {
    pub body_bytes: usize,
    pub body_timeout: Duration,
    pub backend_timeout: Duration,
    pub workers: usize,
    pub waiting_jobs: usize,
    pub waiting_bytes: usize,
}

impl Default for ApiLimits {
    fn default() -> Self {
        Self {
            body_bytes: MAX_BUSINESS_BODY_BYTES,
            body_timeout: BODY_READ_TIMEOUT,
            backend_timeout: Duration::from_secs(10),
            workers: 2,
            waiting_jobs: 32,
            waiting_bytes: 8 * 1024 * 1024,
        }
    }
}

impl ApiLimits {
    fn validate(self) -> io::Result<()> {
        if self.body_bytes == 0
            || self.body_timeout.is_zero()
            || self.backend_timeout.is_zero()
            || tokio::time::Instant::now()
                .checked_add(self.body_timeout)
                .is_none()
            || tokio::time::Instant::now()
                .checked_add(self.backend_timeout)
                .is_none()
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "API limits must be finite and nonzero",
            ));
        }
        if self.workers == 0 || self.waiting_jobs == 0 || self.waiting_bytes == 0 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "API worker limits must be nonzero",
            ));
        }
        Ok(())
    }
}

pub struct GatewayConfig {
    pub mode: ListenMode,
    pub origin: ListenOrigin,
    pub hsts: Hsts,
    pub http: HttpLimits,
    pub api: ApiLimits,
}

struct Gateway<B> {
    ui: StaticUi,
    admission: RequestAdmission,
    backend: Arc<B>,
    workers: BlockingPool,
    limits: ApiLimits,
}

impl<B: ApiBackend> Gateway<B> {
    fn new(
        ui: StaticUi,
        backend: B,
        limits: ApiLimits,
        admission: RequestAdmission,
    ) -> io::Result<Self> {
        limits.validate()?;
        Ok(Self {
            ui,
            admission,
            backend: Arc::new(backend),
            workers: BlockingPool::new(limits.workers, limits.waiting_jobs, limits.waiting_bytes)?,
            limits,
        })
    }

    async fn respond<T>(&self, request: Request<T>) -> Response<Full<Bytes>>
    where
        T: Body<Data = Bytes>,
        T::Error: Into<Box<dyn std::error::Error + Send + Sync>>,
    {
        let (parts, body) = request.into_parts();
        let head = parts.method == Method::HEAD;
        let unsupported_version = parts.version != hyper::Version::HTTP_11;
        let mut response = if !self.admission.accepts_target_and_host(&parts) {
            ApiReply::error(
                StatusCode::BAD_REQUEST,
                GatewayErrorCode::InvalidRequest,
                "Invalid request authority or target.",
            )
            .into_http(head)
        } else if unsupported_version {
            ApiReply::error(
                StatusCode::BAD_REQUEST,
                GatewayErrorCode::InvalidRequest,
                "HTTP/1.1 is required.",
            )
            .into_http(head)
        } else {
            match RequestPath::parse(parts.uri.path()) {
                Err(_) => ApiReply::error(
                    StatusCode::BAD_REQUEST,
                    GatewayErrorCode::InvalidRequest,
                    "Invalid request path.",
                )
                .into_http(head),
                Ok(path) if path.is_api() && !self.admission.accepts_api_origin(&parts) => {
                    ApiReply::error(
                        StatusCode::FORBIDDEN,
                        GatewayErrorCode::NotAuthorized,
                        "Request origin is not authorized.",
                    )
                    .into_http(head)
                }
                Ok(path) if matches!(path.segments(), [api, version, ..] if api == "api" && version == "v1") => {
                    self.api_response(&parts, path, body).await.into_http(head)
                }
                Ok(path) => self.ui.serve_static(&parts.method, &parts.headers, &path),
            }
        };
        self.ui.finish_response(&parts.headers, &mut response);
        if unsupported_version {
            *response.version_mut() = parts.version;
            response.headers_mut().insert(
                hyper::header::CONNECTION,
                hyper::header::HeaderValue::from_static("close"),
            );
        }
        response
    }

    async fn api_response<T>(&self, parts: &Parts, path: RequestPath, body: T) -> ApiReply
    where
        T: Body<Data = Bytes>,
        T::Error: Into<Box<dyn std::error::Error + Send + Sync>>,
    {
        let route = match self.backend.route(parts, &path) {
            Ok(route) => route,
            Err(reply) => return reply,
        };
        let body = match read_business_body(body, self.limits.body_bytes, self.limits.body_timeout)
            .await
        {
            Ok(body) => body,
            Err(error) => {
                return match error {
                    BodyError::TooLarge => ApiReply::error(
                        StatusCode::PAYLOAD_TOO_LARGE,
                        GatewayErrorCode::TooLarge,
                        "Request body is too large.",
                    ),
                    BodyError::TimedOut => ApiReply::error(
                        StatusCode::SERVICE_UNAVAILABLE,
                        GatewayErrorCode::Timeout,
                        "Request body timed out.",
                    ),
                    BodyError::Malformed => ApiReply::error(
                        StatusCode::BAD_REQUEST,
                        GatewayErrorCode::InvalidRequest,
                        "Invalid request body.",
                    ),
                    BodyError::InvalidLimits => ApiReply::error(
                        StatusCode::SERVICE_UNAVAILABLE,
                        GatewayErrorCode::Unavailable,
                        "Service unavailable.",
                    ),
                }
            }
        };
        let bytes = body.as_bytes().len();
        let prepared = match self.backend.prepare(route, body) {
            Ok(prepared) => prepared,
            Err(reply) => return reply,
        };
        let backend = Arc::clone(&self.backend);
        let call = prepared.call;
        let ticket = match self
            .workers
            .submit(bytes.max(prepared.input_bytes), move || {
                backend.execute(call)
            }) {
            Ok(ticket) => ticket,
            Err(_) => return prepared.kind.unavailable(),
        };
        match tokio::time::timeout(self.limits.backend_timeout, ticket.wait()).await {
            Ok(Ok(reply)) => reply,
            _ => prepared.kind.unknown(),
        }
    }
}

pub struct UnavailableApi;

impl ApiBackend for UnavailableApi {
    type Route = Infallible;
    type Call = Infallible;
    fn route(&self, _: &Parts, _: &RequestPath) -> Result<Self::Route, ApiReply> {
        Err(ApiReply::error(
            StatusCode::SERVICE_UNAVAILABLE,
            GatewayErrorCode::Unavailable,
            "Service unavailable.",
        ))
    }
    fn prepare(
        &self,
        route: Infallible,
        _: BusinessBody,
    ) -> Result<PreparedCall<Infallible>, ApiReply> {
        match route {}
    }
    fn execute(&self, call: Infallible) -> ApiReply {
        match call {}
    }
}

/// Production callers supply the verified host listener. Backend and workers are
/// constructed only inside the runtime, after transport validates that listener.
pub fn run_gateway<B, S>(
    listener: TcpListener,
    config: GatewayConfig,
    files: BuildFiles,
    initialize: impl FnOnce() -> io::Result<(B, S)>,
) -> io::Result<()>
where
    B: ApiBackend,
    S: Future<Output = ()>,
{
    config.api.validate()?;
    try_run_http(
        listener,
        config.mode,
        config.origin.clone(),
        config.http,
        move || {
            let (backend, shutdown) = initialize()?;
            let admission = RequestAdmission::new(config.origin.clone());
            let ui = StaticUi::new(files, config.origin, config.hsts);
            let service = Arc::new(Gateway::new(ui, backend, config.api, admission)?);
            let handler = move |request: Request<Incoming>| {
                let service = Arc::clone(&service);
                async move { Ok::<_, Infallible>(service.respond(request).await) }
            };
            Ok((handler, shutdown))
        },
    )
}
