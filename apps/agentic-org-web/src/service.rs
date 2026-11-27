//! Compose static responses, bounded business calls and the shared response policy.

use std::convert::Infallible;
use std::future::Future;
use std::io;
use std::net::TcpListener;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use hyper::body::{Body, Bytes, Incoming};
use hyper::header::{HeaderName, HeaderValue, ALLOW};
use hyper::http::request::Parts;
use hyper::{Method, Request, Response, StatusCode};
use org_host_control::listen::ListenOrigin;
use org_proto::web::{MutationOutcome, OperationId, ReadResponse, WebError, WebErrorCode};
use serde_json::Value;

use crate::assets::BuildFiles;
use crate::blocking::BlockingPool;
use crate::body::stream::{self, StreamedBody, STREAMED_BODY_TIMEOUT};
use crate::body::{
    read_business_body, BodyError, BusinessBody, BODY_READ_TIMEOUT, MAX_BUSINESS_BODY_BYTES,
};
use crate::ingress::{ApiRefusal, RequestAdmission};
use crate::path::RequestPath;
use crate::sse::WebBody;
use crate::static_ui::response;
use crate::transport::{try_run_http, HttpLimits, ListenMode};
use crate::{Hsts, StaticUi};

#[cfg(test)]
mod admission_tests;

/// Either a fixed JSON API reply or an async streaming HTTP response (SSE).
pub enum WebReply {
    Json(ApiReply),
    Stream(Response<WebBody>),
}

/// Optional async dispatch result: `Async` runs directly on Tokio (bypassing the
/// bounded host `BlockingPool`), while `Sync` hands the call back to `BlockingPool`.
pub enum AsyncDispatch<C> {
    Async {
        future: Pin<Box<dyn Future<Output = WebReply> + Send>>,
        /// Streaming responses (SSE) manage their own lifetime via heartbeats and
        /// session-ended events rather than `backend_timeout`.
        streaming: bool,
    },
    Sync(C),
}

/// Routing and preparation perform no native I/O. Routing must authorize before
/// returning a business route. Every body arrives in a zeroizing buffer
/// ([`BusinessBody`]), so credential routes decode secrets straight from it.
pub trait ApiBackend: Send + Sync + 'static {
    type Route: Send;
    type Call: Send + 'static;

    fn route(&self, request: &Parts, path: &RequestPath) -> Result<Self::Route, ApiReply>;
    fn prepare(
        &self,
        route: Self::Route,
        body: BusinessBody,
    ) -> Result<PreparedCall<Self::Call>, ApiReply>;
    /// Optional async execution path for multiplexed office RPCs and SSE streams
    /// that run directly on Tokio without occupying the host `BlockingPool`.
    fn dispatch_async(self: Arc<Self>, call: Self::Call) -> AsyncDispatch<Self::Call> {
        AsyncDispatch::Sync(call)
    }
    /// Runs on a dedicated worker and must use finite native deadlines.
    fn execute(&self, call: Self::Call) -> ApiReply;
    /// A route whose body streams to its call in bounded chunks instead of the buffered
    /// business body (attachment uploads): the exact length the route admitted. The
    /// default streams nothing.
    fn streamed_body(&self, _route: &Self::Route) -> Option<u64> {
        None
    }
    /// Prepares a route [`streamed_body`](Self::streamed_body) admitted. Its call must
    /// dispatch asynchronously; it reads `body` while the service feeds it.
    fn prepare_streamed(
        &self,
        _route: Self::Route,
        _body: StreamedBody,
    ) -> Result<PreparedCall<Self::Call>, ApiReply> {
        Err(ApiReply::error(
            StatusCode::NOT_FOUND,
            WebErrorCode::NotFound,
            "Not found.",
        ))
    }
}

/// The largest chunk a streamed body is cut into: one office IPC chunk (512 KiB).
const STREAMED_CHUNK_BYTES: usize = org_proto::attachments::ATTACHMENT_CHUNK_MAX_BYTES as usize;
/// How long a streamed call may outlive its body deadline before its answer is unknown.
const STREAMED_CALL_GRACE: Duration = Duration::from_secs(5);

pub struct ApiReply {
    status: StatusCode,
    body: Bytes,
    headers: Vec<(HeaderName, HeaderValue)>,
}

impl ApiReply {
    pub fn json(status: StatusCode, value: Value) -> Self {
        Self {
            status,
            body: Bytes::from(serde_json::to_vec(&value).expect("JSON value serializes")),
            headers: Vec::new(),
        }
    }

    /// Adds a response header (`Set-Cookie`, `Allow`). `Access-Control-*` is
    /// stripped from every response regardless.
    pub fn with_header(mut self, name: HeaderName, value: HeaderValue) -> Self {
        self.headers.push((name, value));
        self
    }

    pub fn error(status: StatusCode, code: WebErrorCode, message: &'static str) -> Self {
        let error = ReadResponse::<Value>::Error(WebError::new(code, message));
        Self::json(
            status,
            serde_json::to_value(error).expect("gateway error serializes"),
        )
    }

    #[cfg(test)]
    pub(crate) fn status(&self) -> StatusCode {
        self.status
    }

    #[cfg(test)]
    pub(crate) fn body_bytes(&self) -> &[u8] {
        &self.body
    }

    #[cfg(test)]
    pub(crate) fn header(&self, name: &str) -> Option<&HeaderValue> {
        self.headers
            .iter()
            .find(|(key, _)| key.as_str() == name)
            .map(|(_, value)| value)
    }

    fn into_http(self, head: bool) -> Response<WebBody> {
        let mut response = response(
            self.status,
            self.body,
            head,
            "application/json; charset=utf-8",
            "no-store",
        );
        for (name, value) in self.headers {
            response.headers_mut().append(name, value);
        }
        response.map(WebBody::from)
    }

    fn refusal(refusal: ApiRefusal) -> Self {
        match refusal {
            ApiRefusal::Options => Self::error(
                StatusCode::METHOD_NOT_ALLOWED,
                WebErrorCode::InvalidRequest,
                "Method not allowed.",
            )
            .with_header(ALLOW, HeaderValue::from_static("GET, HEAD, POST")),
            ApiRefusal::CrossSite | ApiRefusal::Origin => Self::error(
                StatusCode::FORBIDDEN,
                WebErrorCode::NotAuthorized,
                "Request origin is not authorized.",
            ),
            ApiRefusal::MediaType => Self::error(
                StatusCode::UNSUPPORTED_MEDIA_TYPE,
                WebErrorCode::InvalidRequest,
                "Content-Type must be application/json.",
            ),
        }
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
        let error = WebError::new(WebErrorCode::Unavailable, "Service unavailable.");
        match self {
            Self::Read => ApiReply::error(
                StatusCode::SERVICE_UNAVAILABLE,
                WebErrorCode::Unavailable,
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
                WebErrorCode::Timeout,
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

pub struct WebConfig {
    pub mode: ListenMode,
    pub origin: ListenOrigin,
    pub hsts: Hsts,
    pub http: HttpLimits,
    pub api: ApiLimits,
}

pub(crate) struct WebService<B> {
    ui: StaticUi,
    admission: RequestAdmission,
    backend: Arc<B>,
    workers: BlockingPool,
    limits: ApiLimits,
}

impl<B: ApiBackend> WebService<B> {
    pub(crate) fn new(
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

    pub(crate) async fn respond<T>(&self, request: Request<T>) -> Response<WebBody>
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
                WebErrorCode::InvalidRequest,
                "Invalid request authority or target.",
            )
            .into_http(head)
        } else if unsupported_version {
            ApiReply::error(
                StatusCode::BAD_REQUEST,
                WebErrorCode::InvalidRequest,
                "HTTP/1.1 is required.",
            )
            .into_http(head)
        } else {
            match RequestPath::parse(parts.uri.path()) {
                Err(_) => ApiReply::error(
                    StatusCode::BAD_REQUEST,
                    WebErrorCode::InvalidRequest,
                    "Invalid request path.",
                )
                .into_http(head),
                Ok(path) if path.is_api() => match self.admission.check_api(&parts, &path) {
                    Err(refusal) => ApiReply::refusal(refusal).into_http(head),
                    Ok(()) if matches!(path.segments(), [api, version, ..] if api == "api" && version == "v1") => {
                        match self.api_response(&parts, path, body).await {
                            WebReply::Json(reply) => reply.into_http(head),
                            WebReply::Stream(stream) => stream,
                        }
                    }
                    Ok(()) => {
                        ApiReply::error(StatusCode::NOT_FOUND, WebErrorCode::NotFound, "Not found.")
                            .into_http(head)
                    }
                },
                Ok(path) => self
                    .ui
                    .serve_static(&parts.method, &parts.headers, &path)
                    .map(WebBody::from),
            }
        };
        self.ui.finish_response(&parts.headers, &mut response);
        // No response from the web process carries a CORS grant (web-api-v1 §2).
        let cors: Vec<HeaderName> = response
            .headers()
            .keys()
            .filter(|name| name.as_str().starts_with("access-control-"))
            .cloned()
            .collect();
        for name in cors {
            response.headers_mut().remove(name);
        }
        if unsupported_version {
            *response.version_mut() = parts.version;
            response.headers_mut().insert(
                hyper::header::CONNECTION,
                hyper::header::HeaderValue::from_static("close"),
            );
        }
        response
    }

    async fn api_response<T>(&self, parts: &Parts, path: RequestPath, body: T) -> WebReply
    where
        T: Body<Data = Bytes>,
        T::Error: Into<Box<dyn std::error::Error + Send + Sync>>,
    {
        let route = match self.backend.route(parts, &path) {
            Ok(route) => route,
            Err(reply) => return WebReply::Json(reply),
        };
        if let Some(declared) = self.backend.streamed_body(&route) {
            return self.streamed_response(route, declared, body).await;
        }
        let body = match read_business_body(body, self.limits.body_bytes, self.limits.body_timeout)
            .await
        {
            Ok(body) => body,
            Err(error) => {
                return WebReply::Json(match error {
                    BodyError::TooLarge => ApiReply::error(
                        StatusCode::PAYLOAD_TOO_LARGE,
                        WebErrorCode::TooLarge,
                        "Request body is too large.",
                    ),
                    BodyError::TimedOut => ApiReply::error(
                        StatusCode::SERVICE_UNAVAILABLE,
                        WebErrorCode::Timeout,
                        "Request body timed out.",
                    ),
                    BodyError::Malformed => ApiReply::error(
                        StatusCode::BAD_REQUEST,
                        WebErrorCode::InvalidRequest,
                        "Invalid request body.",
                    ),
                    BodyError::InvalidLimits => ApiReply::error(
                        StatusCode::SERVICE_UNAVAILABLE,
                        WebErrorCode::Unavailable,
                        "Service unavailable.",
                    ),
                });
            }
        };
        let bytes = body.as_bytes().len();
        let prepared = match self.backend.prepare(route, body) {
            Ok(prepared) => prepared,
            Err(reply) => return WebReply::Json(reply),
        };
        let backend = Arc::clone(&self.backend);
        let call = match backend.dispatch_async(prepared.call) {
            AsyncDispatch::Async { future, streaming } => {
                return if streaming {
                    future.await
                } else {
                    match tokio::time::timeout(self.limits.backend_timeout, future).await {
                        Ok(reply) => reply,
                        Err(_) => WebReply::Json(prepared.kind.unknown()),
                    }
                };
            }
            AsyncDispatch::Sync(call) => call,
        };
        let backend = Arc::clone(&self.backend);
        let ticket = match self
            .workers
            .submit(bytes.max(prepared.input_bytes), move || {
                backend.execute(call)
            }) {
            Ok(ticket) => ticket,
            Err(_) => return WebReply::Json(prepared.kind.unavailable()),
        };
        WebReply::Json(
            match tokio::time::timeout(self.limits.backend_timeout, ticket.wait()).await {
                Ok(Ok(reply)) => reply,
                _ => prepared.kind.unknown(),
            },
        )
    }
}

impl<B: ApiBackend> WebService<B> {
    /// The streamed-body path (attachment uploads): no buffered body and no worker. The
    /// declared length must be the body's exact framing; the body is fed to the call in
    /// bounded chunks while the call runs, and one deadline covers both.
    async fn streamed_response<T>(&self, route: B::Route, declared: u64, body: T) -> WebReply
    where
        T: Body<Data = Bytes>,
        T::Error: Into<Box<dyn std::error::Error + Send + Sync>>,
    {
        if body.size_hint().exact() != Some(declared) {
            return WebReply::Json(ApiReply::error(
                StatusCode::LENGTH_REQUIRED,
                WebErrorCode::InvalidRequest,
                "Content-Length is required.",
            ));
        }
        let deadline = tokio::time::Instant::now() + STREAMED_BODY_TIMEOUT;
        let (streamed, feeder) = stream::channel(declared, STREAMED_CHUNK_BYTES, deadline);
        let prepared = match self.backend.prepare_streamed(route, streamed) {
            Ok(prepared) => prepared,
            Err(reply) => return WebReply::Json(reply),
        };
        let mut future = match Arc::clone(&self.backend).dispatch_async(prepared.call) {
            AsyncDispatch::Async { future, .. } => future,
            AsyncDispatch::Sync(_) => return WebReply::Json(prepared.kind.unavailable()),
        };
        let mut pump = std::pin::pin!(feeder.pump(body));
        let mut pumping = true;
        // The call answers; the feeder only serves it. Once the call is done (even before
        // the body ends), the rest of the body is never read.
        let run = std::future::poll_fn(|cx| {
            if pumping && pump.as_mut().poll(cx).is_ready() {
                pumping = false;
            }
            future.as_mut().poll(cx)
        });
        match tokio::time::timeout_at(deadline + STREAMED_CALL_GRACE, run).await {
            Ok(reply) => reply,
            Err(_) => WebReply::Json(prepared.kind.unknown()),
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
            WebErrorCode::Unavailable,
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
pub fn run_web<B, S>(
    listener: TcpListener,
    config: WebConfig,
    files: BuildFiles,
    initialize: impl FnOnce() -> io::Result<(B, S)>,
) -> io::Result<()>
where
    B: ApiBackend,
    S: Future<Output = ()>,
{
    run_web_reporting(listener, config, files, initialize, || Ok(()))
}

/// [`run_web`], calling `serving` once after the runtime, listener
/// registration, backend and web process service are initialized and before the
/// first accept. Its error stops the web process before it serves any request.
pub fn run_web_reporting<B, S>(
    listener: TcpListener,
    config: WebConfig,
    files: BuildFiles,
    initialize: impl FnOnce() -> io::Result<(B, S)>,
    serving: impl FnOnce() -> io::Result<()>,
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
            let service = Arc::new(WebService::new(ui, backend, config.api, admission)?);
            serving()?;
            let handler = move |request: Request<Incoming>| {
                let service = Arc::clone(&service);
                async move { Ok::<_, Infallible>(service.respond(request).await) }
            };
            Ok((handler, shutdown))
        },
    )
}
