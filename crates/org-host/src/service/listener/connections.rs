//! Single-connection listeners: the web process's connection and a local pair, each
//! served without an accept loop.

use std::io;
use std::os::unix::net::UnixStream;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};

use crate::peer::Accepted;

use super::connection::{Connection, Gateway};
use super::serve::serve;
use super::Handler;

/// Lifetime of the sole authenticated web process control connection. Constructed only by
/// the web process supervisor, after its image/reader/sandbox admission and handoff ACK.
/// This is the only production path that gives a connection web process privilege:
/// host.sock and setup connections stay `ChannelKind::Local`.
pub(crate) struct WebConnection {
    pub(super) connection: Arc<Connection>,
    thread: Option<JoinHandle<()>>,
}

impl WebConnection {
    pub(crate) fn start(
        stream: UnixStream,
        transfer: std::os::unix::net::UnixDatagram,
        listen: org_host_control::listen::ListenConfig,
        handler: Arc<dyn Handler>,
    ) -> io::Result<Self> {
        let (mut connection, queue) =
            Connection::new(stream.try_clone()?, Accepted::sample(&stream));
        Arc::get_mut(&mut connection)
            .ok_or_else(|| io::Error::other("shared admission"))?
            .gateway = Some(Gateway {
            transfer: Arc::new(transfer),
            listen,
            fetched: AtomicBool::new(false),
            serving: AtomicBool::new(false),
            serving_refused: AtomicBool::new(false),
        });
        let serving = Arc::clone(&connection);
        let thread = thread::Builder::new()
            .name("web-control".into())
            .spawn(move || serve(stream, serving, queue, handler, 4))?;
        Ok(Self {
            connection,
            thread: Some(thread),
        })
    }
    pub(crate) fn live(&self) -> bool {
        !self.connection.closed.load(Ordering::SeqCst)
    }
    /// The host refused the web process's `web.serving` as a protocol error.
    pub(crate) fn serving_refused(&self) -> bool {
        self.connection
            .gateway
            .as_ref()
            .is_some_and(|g| g.serving_refused.load(Ordering::SeqCst))
    }
    /// The web process reported, in order, that its HTTP server is serving.
    pub(crate) fn reported_serving(&self) -> bool {
        self.connection
            .gateway
            .as_ref()
            .is_some_and(|g| g.serving.load(Ordering::SeqCst))
    }
}

impl Drop for WebConnection {
    fn drop(&mut self) {
        self.connection.close();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// A served `Local` connection for tests, built like an accepted host.sock
/// connection. It has no web process privilege.
#[cfg(test)]
pub(crate) struct LocalConnection {
    pub(super) connection: Arc<Connection>,
    thread: Option<JoinHandle<()>>,
}

#[cfg(test)]
impl LocalConnection {
    pub(crate) fn start(stream: UnixStream, handler: Arc<dyn Handler>) -> io::Result<Self> {
        if !org_socket::peer_allowed(&stream, org_socket::effective_uid()) {
            return Err(io::Error::other("local peer refused"));
        }
        let (connection, queue) = Connection::new(stream.try_clone()?, Accepted::sample(&stream));
        let serving = Arc::clone(&connection);
        let thread = thread::Builder::new()
            .name("local-control".into())
            .spawn(move || serve(stream, serving, queue, handler, 1))?;
        Ok(Self {
            connection,
            thread: Some(thread),
        })
    }
    pub(crate) fn live(&self) -> bool {
        !self.connection.closed.load(Ordering::SeqCst)
    }
}

#[cfg(test)]
impl Drop for LocalConnection {
    fn drop(&mut self) {
        self.connection.close();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}
