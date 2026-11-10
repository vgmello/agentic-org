use std::convert::Infallible;
use std::future::{poll_fn, Future};
use std::io;
use std::net::TcpListener;
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
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::sync::Semaphore;
use tokio::task::JoinSet;
use tokio::time;
use tokio_rustls::TlsAcceptor;

use super::write_stall::WriteStall;
use crate::tls::TlsCertificates;
use crate::ConfiguredOrigin;

#[derive(Clone)]
pub enum ListenMode {
    HttpLoopback,
    Https(Arc<TlsCertificates>),
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
    origin: ConfiguredOrigin,
    limits: HttpLimits,
    initialize: impl FnOnce() -> (H, S),
) -> io::Result<()>
where
    H: Fn(Request<Incoming>) -> F + Send + Sync + 'static,
    F: Future<Output = Result<Response<Full<Bytes>>, Infallible>> + Send + 'static,
    S: Future<Output = ()>,
{
    limits.validate()?;
    let address = listener.local_addr()?;
    if matches!(mode, ListenMode::HttpLoopback) {
        origin
            .validate_http_loopback(address)
            .map_err(|message| io::Error::new(io::ErrorKind::PermissionDenied, message))?;
    } else {
        origin
            .validate_https(address)
            .map_err(|message| io::Error::new(io::ErrorKind::InvalidInput, message))?;
    }
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
        let (handler, shutdown) = initialize();
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
            let (stream, _) = accepted?;
            let handler = Arc::clone(&handler);
            let mode = mode.clone();
            connections.spawn(async move {
                let _permit = permit;
                let Ok(stream) = WriteStall::new(stream, limits.write_stall) else {
                    return;
                };
                match mode {
                    ListenMode::HttpLoopback => {
                        let _ = serve_connection(stream, handler, limits).await;
                    }
                    ListenMode::Https(certificates) => {
                        let accept = TlsAcceptor::from(certificates.server_config());
                        if let Ok(Ok(stream)) =
                            time::timeout(limits.handshake_timeout, accept.accept(stream)).await
                        {
                            let _ = serve_connection(stream, handler, limits).await;
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

async fn serve_connection<T, H, F>(
    stream: T,
    handler: Arc<H>,
    limits: HttpLimits,
) -> Result<(), hyper::Error>
where
    T: AsyncRead + AsyncWrite + Unpin + Send + 'static,
    H: Fn(Request<Incoming>) -> F + Send + Sync + 'static,
    F: Future<Output = Result<Response<Full<Bytes>>, Infallible>> + Send + 'static,
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
            service_fn(move |request| handler(request)),
        )
        .await
}

#[cfg(test)]
mod tests;
