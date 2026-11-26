//! One connection's state and its outbound side: responses, events, secret lines and
//! passed descriptors, queued for its writer thread.

use std::collections::HashMap;
use std::io;
use std::os::unix::io::AsRawFd;
use std::os::unix::net::UnixStream;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, SyncSender, TrySendError};
use std::sync::{Arc, Mutex};

use org_proto::Response;
use serde_json::Value;

use crate::peer::Accepted;
use crate::sync::lock;

use super::outbound::{shut_down, Outbound, SecretLine};

/// Messages one connection may have waiting to be written. A client that lets this fill isn't
/// reading, and is disconnected rather than slowing anyone else down.
const OUTBOUND_QUEUE: usize = 256;

pub(super) struct Gateway {
    pub(super) transfer: Arc<std::os::unix::net::UnixDatagram>,
    pub(super) listen: org_host_control::listen::ListenConfig,
    /// A successful `tls.fetch` answered on this connection.
    pub(super) fetched: AtomicBool,
    /// The web process's one `web.serving` was accepted.
    pub(super) serving: AtomicBool,
    /// The host refused a `web.serving` as a protocol error and closed control.
    pub(super) serving_refused: AtomicBool,
}

/// How the host takes a `web.serving` notification.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ServingReport {
    /// Not the admitted web process connection.
    NotWeb,
    /// Accepted: the first one, after hello and a successful `tls.fetch`.
    Accepted,
    /// Repeated, or before hello/`tls.fetch`: a protocol error.
    OutOfOrder,
}

/// One client connection, as handlers see it: a place to queue responses and events.
pub(crate) struct Connection {
    outbound: SyncSender<Outbound>,
    stream: UnixStream,
    /// What the host noted about the client when it accepted the connection (`app_auth`).
    accepted: Accepted,
    /// Ends the ticked reader even if shutdown misses a platform wakeup.
    pub(super) closed: Arc<AtomicBool>,
    hello_completed: AtomicBool,
    /// The peer is root, admitted as the local authority for `admin.reset` alone.
    root_authority: AtomicBool,
    pub(super) descriptor_receipts:
        Mutex<HashMap<String, (mpsc::Sender<()>, crate::supervisor::channels::ChannelSlot)>>,
    pub(super) received_channels: Mutex<HashMap<String, crate::supervisor::channels::ChannelSlot>>,
    pub(super) gateway: Option<Gateway>,
    pub(crate) recovery_permission: Mutex<Option<crate::recovery::Permission>>,
}

#[cfg(test)]
impl Connection {
    /// A connection as the listener makes one for an accepted `stream`, for the tests of
    /// the calls that judge a connection (`admin`). Nothing serves it.
    pub(crate) fn accepted_for_tests(stream: &UnixStream) -> Arc<Self> {
        Self::new(stream.try_clone().unwrap(), Accepted::sample(stream)).0
    }
}

/// Capacity is a definite pre-enqueue refusal; all other delivery failures stay fatal.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum DescriptorError {
    Capacity,
    Transport,
}

impl Connection {
    pub(super) fn new(stream: UnixStream, accepted: Accepted) -> (Arc<Self>, Receiver<Outbound>) {
        let (outbound, queue) = mpsc::sync_channel(OUTBOUND_QUEUE);
        let connection = Self {
            outbound,
            stream,
            accepted,
            closed: Arc::new(AtomicBool::new(false)),
            hello_completed: AtomicBool::new(false),
            root_authority: AtomicBool::new(false),
            descriptor_receipts: Mutex::default(),
            received_channels: Mutex::default(),
            gateway: None,
            recovery_permission: Mutex::default(),
        };
        (Arc::new(connection), queue)
    }

    pub(in crate::service) fn liveness(&self) -> std::sync::Weak<AtomicBool> {
        Arc::downgrade(&self.closed)
    }

    /// Privileged inline handlers must observe EOF without consuming another
    /// request while waiting for OS authentication. Any pipelined byte denies.
    pub(crate) fn recovery_peer_idle(&self) -> bool {
        if self.closed.load(Ordering::SeqCst) {
            return false;
        }
        let mut byte = 0u8;
        // SAFETY: live socket and one writable byte, non-consuming/non-blocking.
        let n = unsafe {
            libc::recv(
                self.stream.as_raw_fd(),
                (&mut byte as *mut u8).cast(),
                1,
                libc::MSG_PEEK | libc::MSG_DONTWAIT,
            )
        };
        n < 0 && std::io::Error::last_os_error().kind() == io::ErrorKind::WouldBlock
    }
    pub(crate) fn clear_recovery_permission(&self) {
        *lock(&self.recovery_permission) = None;
    }
    pub(crate) fn hello_completed(&self) -> bool {
        self.hello_completed.load(Ordering::SeqCst)
    }
    /// Set once, when the connection is accepted, and never cleared.
    pub(crate) fn mark_root_authority(&self) {
        self.root_authority.store(true, Ordering::SeqCst);
    }
    /// Whether the kernel named root as this connection's peer when it was accepted, on a
    /// system where that makes it the local authority for `admin.reset`.
    pub(crate) fn root_authority(&self) -> bool {
        self.root_authority.load(Ordering::SeqCst)
    }
    pub(crate) fn channel_kind(&self) -> org_office_launch::ChannelKind {
        if self.gateway.is_some() {
            org_office_launch::ChannelKind::Web
        } else {
            org_office_launch::ChannelKind::Local
        }
    }
    pub(crate) fn web_listen(&self) -> Option<&org_host_control::listen::ListenConfig> {
        self.gateway.as_ref().map(|g| &g.listen)
    }
    /// Records a successful `tls.fetch` answer on the web process connection.
    pub(crate) fn note_listener_fetched(&self) {
        if let Some(web) = &self.gateway {
            web.fetched.store(true, Ordering::SeqCst);
        }
    }
    /// Takes the web process's `web.serving`: accepted once, after hello and `tls.fetch`.
    pub(crate) fn report_serving(&self) -> ServingReport {
        let Some(web) = &self.gateway else {
            return ServingReport::NotWeb;
        };
        if self.hello_completed()
            && web.fetched.load(Ordering::SeqCst)
            && web
                .serving
                .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
                .is_ok()
        {
            ServingReport::Accepted
        } else {
            ServingReport::OutOfOrder
        }
    }

    pub(crate) fn complete_hello(&self) {
        self.hello_completed.store(true, Ordering::SeqCst);
    }

    #[cfg(test)]
    pub(crate) fn for_test() -> Arc<Self> {
        let (stream, _peer) = UnixStream::pair().unwrap();
        Self::new(stream.try_clone().unwrap(), Accepted::sample(&stream)).0
    }
    #[cfg(test)]
    pub(in crate::service) fn record_received_channel_for_test(&self, channel: &str) {
        self.record_received_slot_for_test(
            channel,
            crate::supervisor::channels::ChannelSlot::for_test(),
        );
    }

    #[cfg(test)]
    pub(in crate::service) fn record_received_slot_for_test(
        &self,
        channel: &str,
        slot: crate::supervisor::channels::ChannelSlot,
    ) {
        lock(&self.received_channels).insert(channel.into(), slot);
    }

    /// Refuses a web process's `web.serving` as a protocol error, noting it so the
    /// supervisor logs that cause rather than only the control EOF that follows.
    pub(crate) fn refuse_serving(&self, response: Response) {
        if let Some(web) = &self.gateway {
            web.serving_refused.store(true, Ordering::SeqCst);
        }
        self.refuse_and_finish(response);
    }

    /// Drain the refusal before closing, but stop even already-buffered requests.
    pub(crate) fn refuse_and_finish(&self, response: Response) {
        self.closed.store(true, Ordering::SeqCst);
        self.enqueue(Outbound::Response(response));
        self.enqueue(Outbound::Close);
    }

    /// What the host noted about the client when it accepted the connection.
    pub(crate) fn accepted(&self) -> Accepted {
        self.accepted
    }

    /// The connection's socket, to identify the client (`app_auth`).
    pub(crate) fn socket(&self) -> &UnixStream {
        &self.stream
    }

    /// Queues an encoded response line that holds a secret. Never blocks, like
    /// [`Connection::send_response`].
    pub(crate) fn send_secret_line(&self, line: SecretLine) {
        self.enqueue(Outbound::Secret(line));
    }

    /// Queues a response. Never blocks: a client whose queue is full is disconnected.
    pub(crate) fn send_response(&self, response: Response) {
        self.enqueue(Outbound::Response(response));
    }
    pub(crate) fn send_descriptor(
        &self,
        response: Response,
        channel: String,
        descriptor: std::os::fd::OwnedFd,
        office: &str,
        witness: crate::supervisor::channels::ChannelSlot,
    ) -> Result<(), DescriptorError> {
        let (sent, receipt) = mpsc::channel();
        {
            // One order for every paired access. Pending reservations are never pruned.
            let mut pending = lock(&self.descriptor_receipts);
            let mut acknowledged = lock(&self.received_channels);
            acknowledged.retain(|_, witness| witness.is_live());
            if pending.contains_key(&channel) || acknowledged.contains_key(&channel) {
                return Err(DescriptorError::Transport);
            }
            if pending.len() >= 16 || pending.len() + acknowledged.len() >= 512 {
                return Err(DescriptorError::Capacity);
            }
            if self.closed.load(Ordering::SeqCst) {
                return Err(DescriptorError::Transport);
            }
            pending.insert(channel.clone(), (sent, witness));
        }
        self.enqueue(Outbound::Descriptor {
            response,
            descriptor,
            receipt,
            gateway: self.gateway.as_ref().map(|g| Arc::clone(&g.transfer)),
            office: office.into(),
            channel,
        });
        if self.closed.load(Ordering::SeqCst) {
            return Err(DescriptorError::Transport);
        }
        Ok(())
    }
    pub(crate) fn descriptor_received(&self, channel: &str) -> bool {
        let mut pending = lock(&self.descriptor_receipts);
        let mut acknowledged = lock(&self.received_channels);
        let Some((receipt, witness)) = pending.remove(channel) else {
            return false;
        };
        if receipt.send(()).is_err() {
            return false;
        }
        if witness.is_live() {
            acknowledged.insert(channel.into(), witness);
        }
        true
    }
    pub(crate) fn channel_received(&self, channel: &str) -> bool {
        lock(&self.received_channels)
            .get(channel)
            .is_some_and(|witness| witness.is_live())
    }

    /// Queues an event. Never blocks, like [`Connection::send_response`].
    pub(crate) fn send_event(&self, name: &str, data: Value) {
        self.enqueue(Outbound::Event {
            name: name.to_owned(),
            data,
        });
    }

    /// Closes the socket both ways, which ends the connection's threads.
    pub(crate) fn close(&self) {
        self.closed.store(true, Ordering::SeqCst);
        shut_down(&self.stream);
    }

    pub(super) fn finish(&self) {
        let _ = self.outbound.send(Outbound::Close);
    }

    fn enqueue(&self, message: Outbound) {
        match self.outbound.try_send(message) {
            Ok(()) => {}
            Err(TrySendError::Full(_) | TrySendError::Disconnected(_)) => self.close(),
        }
    }
}

impl crate::broadcast::Subscriber for Connection {
    fn send_event(&self, name: &str, data: Value) {
        Connection::send_event(self, name, data);
    }
}
