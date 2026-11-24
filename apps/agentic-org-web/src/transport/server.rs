use std::convert::Infallible;
use std::future::{poll_fn, Future};
use std::io;
use std::net::{SocketAddr, TcpListener};
use std::pin::{pin, Pin};
use std::sync::Arc;
use std::task::Poll;
use std::time::{Duration, Instant};

use http_body_util::Full;
use hyper::body::{Bytes, Incoming};
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper::{Request, Response};
use hyper_util::rt::{TokioIo, TokioTimer};
use org_host_control::listen::{self, ListenOrigin};
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::sync::Semaphore;
use tokio::task::JoinSet;
use tokio::time;
use tokio_rustls::TlsAcceptor;

use super::linger::{linger, reclaimed, Reclaim};
use super::write_stall::WriteStall;
use crate::origin::check_listener;
use crate::tls::TlsCertificates;

#[derive(Clone)]
pub enum ListenMode {
    HttpLoopback,
    /// Plain HTTP on a published port of a foreground instance: served exactly as loopback
    /// HTTP is. The host says which of the two plain modes the listener has; this process
    /// refuses to start unless the origin fits its listener in that mode
    /// (`ListenOrigin::check`): a published listener is not on a loopback address and its
    /// origin's host is a loopback name.
    HttpPublished,
    Https(Arc<TlsCertificates>),
}

impl ListenMode {
    /// The listen mode `ListenOrigin::check` fits the origin to.
    fn origin_mode(&self) -> listen::ListenMode {
        match self {
            ListenMode::HttpLoopback => listen::ListenMode::HttpLoopback,
            ListenMode::HttpPublished => listen::ListenMode::HttpPublished,
            ListenMode::Https(_) => listen::ListenMode::Https,
        }
    }
}

/// Runtime limits are checked before constructing a runtime or accepting peers.
#[derive(Clone, Copy)]
pub struct HttpLimits {
    pub connections: usize,
    pub header_bytes: usize,
    pub header_count: usize,
    pub header_timeout: Duration,
    pub handshake_timeout: Duration,
    pub write_stall: Duration,
}

impl Default for HttpLimits {
    fn default() -> Self {
        Self {
            connections: 128,
            header_bytes: 32 * 1024,
            header_count: 64,
            header_timeout: Duration::from_secs(10),
            handshake_timeout: Duration::from_secs(10),
            write_stall: Duration::from_secs(30),
        }
    }
}

impl HttpLimits {
    fn validate(&self) -> io::Result<()> {
        let deadlines = [
            self.header_timeout,
            self.handshake_timeout,
            self.write_stall,
        ];
        if self.connections == 0
            || self.connections > Semaphore::MAX_PERMITS
            || self.header_bytes < 8192
            || self.header_count == 0
            || self.header_count > self.header_bytes
            || deadlines.iter().any(|duration| {
                duration.is_zero() || Instant::now().checked_add(*duration).is_none()
            })
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "invalid HTTP service limits",
            ));
        }
        Ok(())
    }
}

/// Starts only after the verified host handoff, consuming its already-bound fd.
/// No production bind/listen occurs here. The factory runs inside the runtime,
/// so native clients, dedicated workers and control-fd futures are built only
/// after handoff too. It returns an application handler and shutdown future.
/// HTTP/1.1 is the only advertised/served application protocol.
pub fn run_http<H, F, S>(
    listener: TcpListener,
    mode: ListenMode,
    origin: ListenOrigin,
    limits: HttpLimits,
    initialize: impl FnOnce() -> (H, S),
) -> io::Result<()>
where
    H: Fn(Request<Incoming>) -> F + Send + Sync + 'static,
    F: Future<Output = Result<Response<Full<Bytes>>, Infallible>> + Send + 'static,
    S: Future<Output = ()>,
{
    try_run_http(listener, mode, origin, limits, || Ok(initialize()))
}

pub(crate) fn try_run_http<H, F, B, S>(
    listener: TcpListener,
    mode: ListenMode,
    origin: ListenOrigin,
    limits: HttpLimits,
    initialize: impl FnOnce() -> io::Result<(H, S)>,
) -> io::Result<()>
where
    H: Fn(Request<Incoming>) -> F + Send + Sync + 'static,
    F: Future<Output = Result<Response<B>, Infallible>> + Send + 'static,
    B: hyper::body::Body<Data = Bytes, Error = Infallible> + Send + 'static,
    S: Future<Output = ()>,
{
    limits.validate()?;
    let address = listener.local_addr()?;
    check_listener(&origin, mode.origin_mode(), address)?;
    listener.set_nonblocking(true)?;
    // X-02: no runtime/thread pool may exist before the host's step-7 handoff.
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .max_blocking_threads(2)
        .thread_name("gateway-http")
        .enable_io()
        .enable_time()
        .build()?;
    runtime.block_on(async move {
        let listener = tokio::net::TcpListener::from_std(listener)?;
        let (handler, shutdown) = initialize()?;
        let handler = Arc::new(handler);
        let mut shutdown = pin!(shutdown);
        let capacity = Arc::new(Semaphore::new(limits.connections));
        let mut connections = JoinSet::new();
        loop {
            while connections.try_join_next().is_some() {}
            let Some(permit) =
                until_shutdown(Arc::clone(&capacity).acquire_owned(), shutdown.as_mut()).await
            else {
                break;
            };
            let permit = permit.map_err(|_| io::Error::other("connection budget closed"))?;
            let Some(accepted) = until_shutdown(listener.accept(), shutdown.as_mut()).await else {
                break;
            };
            let (stream, peer) = accepted?;
            let handler = Arc::clone(&handler);
            let mode = mode.clone();
            connections.spawn(async move {
                let _permit = permit;
                let Ok(stream) = WriteStall::new(stream, limits.write_stall) else {
                    return;
                };
                match mode {
                    ListenMode::HttpLoopback | ListenMode::HttpPublished => {
                        serve_then_linger(stream, peer, handler, limits).await;
                    }
                    ListenMode::Https(certificates) => {
                        let accept = TlsAcceptor::from(certificates.server_config());
                        if let Ok(Ok(stream)) =
                            time::timeout(limits.handshake_timeout, accept.accept(stream)).await
                        {
                            serve_then_linger(stream, peer, handler, limits).await;
                        }
                    }
                }
            });
        }
        connections.abort_all();
        while connections.join_next().await.is_some() {}
        Ok(())
    })
}

// Both semaphore acquisition and accept are cancellation-safe. Poll shutdown
// first, so a ready stop never admits one more connection.
async fn until_shutdown<F: Future, S: Future<Output = ()>>(
    future: F,
    mut shutdown: Pin<&mut S>,
) -> Option<F::Output> {
    let mut future = pin!(future);
    poll_fn(|cx| {
        if shutdown.as_mut().poll(cx).is_ready() {
            return Poll::Ready(None);
        }
        future.as_mut().poll(cx).map(Some)
    })
    .await
}

/// Serve one connection, then close it with a bounded linger (see `linger`),
/// so an early reply to a request with unread body bytes is not reset.
async fn serve_then_linger<T, H, F, B>(
    stream: T,
    peer: SocketAddr,
    handler: Arc<H>,
    limits: HttpLimits,
) where
    T: AsyncRead + AsyncWrite + Unpin + Send + 'static,
    H: Fn(Request<Incoming>) -> F + Send + Sync + 'static,
    F: Future<Output = Result<Response<B>, Infallible>> + Send + 'static,
    B: hyper::body::Body<Data = Bytes, Error = Infallible> + Send + 'static,
{
    let (stream, slot) = Reclaim::new(stream);
    let _ = serve_connection(stream, Some(peer), handler, limits).await;
    if let Some((stream, shut_down)) = reclaimed(&slot) {
        linger(stream, shut_down).await;
    }
}

async fn serve_connection<T, H, F, B>(
    stream: T,
    peer: Option<SocketAddr>,
    handler: Arc<H>,
    limits: HttpLimits,
) -> Result<(), hyper::Error>
where
    T: AsyncRead + AsyncWrite + Unpin + Send + 'static,
    H: Fn(Request<Incoming>) -> F + Send + Sync + 'static,
    F: Future<Output = Result<Response<B>, Infallible>> + Send + 'static,
    B: hyper::body::Body<Data = Bytes, Error = Infallible> + Send + 'static,
{
    let mut builder = http1::Builder::new();
    builder
        .timer(TokioTimer::new())
        .max_headers(limits.header_count)
        .max_header_size(limits.header_bytes)
        .max_buf_size(limits.header_bytes)
        .header_read_timeout(limits.header_timeout)
        .keep_alive(true);
    // Hyper owns malformed-HTTP400/431: empty bodies, connection closed,
    // and no application handler invocation. Application headers are separate.
    builder
        .serve_connection(
            TokioIo::new(stream),
            service_fn(move |mut request: Request<Incoming>| {
                // The browser's address is the accepted socket's peer, never a header.
                if let Some(peer) = peer {
                    request.extensions_mut().insert(PeerAddress(peer));
                }
                handler(request)
            }),
        )
        .await
}

/// The TCP peer of the connection a request arrived on, set by the transport
/// from `accept()`. Request headers (`X-Forwarded-For`, `Forwarded`) never set it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PeerAddress(pub SocketAddr);

#[cfg(test)]
mod tests;
