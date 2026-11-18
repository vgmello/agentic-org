//! Bounded lingering close.
//!
//! Hyper can answer before it reads a request body: a 404 or 403 decided from
//! the head alone, a body-limit refusal, or its own 400/431 parser refusal.
//! When the connection then closes while body bytes are still unread (or still
//! arriving), the kernel answers them with a TCP reset, and a client can lose
//! the reply it had not yet read, seeing a connection reset instead.
//!
//! After Hyper finishes, the connection therefore half-closes its write side
//! (the reply is complete and the peer sees EOF) and reads and discards input
//! until the peer closes, a fixed byte count is consumed, or a fixed deadline
//! passes, whichever comes first. Only then is the socket dropped. A peer that
//! keeps sending is cut off at those bounds and may then still see a reset.

use std::io;
use std::pin::Pin;
use std::sync::{Arc, Mutex, PoisonError};
use std::task::{Context, Poll};
use std::time::Duration;

use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, ReadBuf};
use tokio::time;

/// Longest a closing connection keeps its permit after Hyper ends. It covers a
/// loopback or LAN peer whose remaining body is in flight; it is small next to
/// the 10-second header deadline any unauthenticated peer can already hold.
pub(super) const LINGER_TIME: Duration = Duration::from_millis(250);
/// Most input discarded while lingering. Time is the primary bound; this caps
/// the work per connection (one 4 KiB buffer, no retained data). It is twice the
/// 1 MiB business-body limit, so an upload refused with 413 just above that limit
/// still sees its reply; a larger upload still unread at the cap may be reset.
pub(super) const LINGER_BYTES: usize = 2 * 1024 * 1024;
const DISCARD_CHUNK: usize = 4096;

/// Half-close unless `shut_down`, then discard input within `LINGER_TIME` and
/// `LINGER_BYTES`. Returns the number of bytes discarded. The stream drops when
/// this returns.
///
/// Hyper already shuts the write side down on a graceful close. A second
/// `shutdown(SHUT_WR)` is not repeated: on BSD-derived stacks it can mark the
/// socket unable to receive once the FIN is acknowledged, which would drop the
/// late bytes and end the linger at once.
pub(super) async fn linger<T: AsyncRead + AsyncWrite + Unpin>(
    mut stream: T,
    shut_down: bool,
) -> usize {
    let mut discarded = 0;
    // One deadline covers the shutdown write (TLS close_notify) and every read.
    let _ = time::timeout(LINGER_TIME, async {
        if !shut_down && stream.shutdown().await.is_err() {
            return;
        }
        let mut chunk = [0; DISCARD_CHUNK];
        while discarded < LINGER_BYTES {
            let room = (LINGER_BYTES - discarded).min(DISCARD_CHUNK);
            match stream.read(&mut chunk[..room]).await {
                Ok(0) | Err(_) => return,
                Ok(read) => discarded += read,
            }
        }
    })
    .await;
    discarded
}

/// Hands the wrapped stream back through `slot` when Hyper drops it, on every
/// exit path (graceful close, parse error, timeout), so it can linger. The
/// slot also records whether the write side was already shut down.
pub(super) struct Reclaim<T> {
    inner: Option<T>,
    shut_down: bool,
    slot: Slot<T>,
}

pub(super) type Slot<T> = Arc<Mutex<Option<(T, bool)>>>;

impl<T> Reclaim<T> {
    pub(super) fn new(inner: T) -> (Self, Slot<T>) {
        let slot = Arc::new(Mutex::new(None));
        let reclaim = Self {
            inner: Some(inner),
            shut_down: false,
            slot: Arc::clone(&slot),
        };
        (reclaim, slot)
    }

    fn io(&mut self) -> &mut T {
        // `inner` is taken only in `drop`, after which no poll can occur.
        self.inner.as_mut().expect("stream present until drop")
    }
}

impl<T> Drop for Reclaim<T> {
    fn drop(&mut self) {
        if let Some(inner) = self.inner.take() {
            *self.slot.lock().unwrap_or_else(PoisonError::into_inner) =
                Some((inner, self.shut_down));
        }
    }
}

/// Take the stream Hyper released, if it was released, and whether its write
/// side was already shut down.
pub(super) fn reclaimed<T>(slot: &Mutex<Option<(T, bool)>>) -> Option<(T, bool)> {
    slot.lock().unwrap_or_else(PoisonError::into_inner).take()
}

impl<T: AsyncRead + Unpin> AsyncRead for Reclaim<T> {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buffer: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        Pin::new(self.get_mut().io()).poll_read(cx, buffer)
    }
}

impl<T: AsyncWrite + Unpin> AsyncWrite for Reclaim<T> {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        bytes: &[u8],
    ) -> Poll<io::Result<usize>> {
        Pin::new(self.get_mut().io()).poll_write(cx, bytes)
    }

    fn poll_write_vectored(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buffers: &[io::IoSlice<'_>],
    ) -> Poll<io::Result<usize>> {
        Pin::new(self.get_mut().io()).poll_write_vectored(cx, buffers)
    }

    fn is_write_vectored(&self) -> bool {
        self.inner.as_ref().is_some_and(T::is_write_vectored)
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(self.get_mut().io()).poll_flush(cx)
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        let this = self.get_mut();
        let result = Pin::new(this.io()).poll_shutdown(cx);
        if matches!(result, Poll::Ready(Ok(()))) {
            this.shut_down = true;
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::duplex;

    fn virtual_time(test: impl std::future::Future<Output = ()>) {
        tokio::runtime::Builder::new_current_thread()
            .enable_time()
            .build()
            .unwrap()
            .block_on(async {
                time::pause();
                test.await;
            });
    }

    #[test]
    fn a3_b1_a_peer_that_closes_ends_lingering_at_once_after_eof() {
        virtual_time(async {
            let started = time::Instant::now();
            let (server, mut client) = duplex(1024);
            client.write_all(b"late body").await.unwrap();
            let lingering = tokio::spawn(linger(server, false));
            // The half-close reaches the peer as EOF before any input is read.
            let mut rest = Vec::new();
            client.read_to_end(&mut rest).await.unwrap();
            assert!(rest.is_empty());
            drop(client);
            assert_eq!(lingering.await.unwrap(), b"late body".len());
            assert!(started.elapsed() < LINGER_TIME);
        });
    }

    #[test]
    fn a3_b1_a_peer_that_keeps_sending_is_cut_off_at_the_deadline() {
        virtual_time(async {
            let started = time::Instant::now();
            let (server, mut client) = duplex(1024);
            let sender = tokio::spawn(async move {
                // One byte every 10 ms never reaches the byte bound in time.
                while client.write_all(b"x").await.is_ok() {
                    time::sleep(Duration::from_millis(10)).await;
                }
            });
            let discarded = linger(server, false).await;
            let elapsed = started.elapsed();
            assert!(elapsed >= LINGER_TIME && elapsed < LINGER_TIME + Duration::from_millis(20));
            assert!(discarded > 0 && discarded < LINGER_BYTES);
            // Dropping the server end stops the sender with a write error.
            sender.await.unwrap();
        });
    }

    #[test]
    fn a3_b1_a_peer_that_floods_is_cut_off_at_the_byte_bound() {
        virtual_time(async {
            let started = time::Instant::now();
            let (server, mut client) = duplex(16 * 1024);
            let sender = tokio::spawn(async move {
                let flood = vec![b'x'; 10 * 1024 * 1024];
                let _ = client.write_all(&flood).await;
            });
            assert_eq!(linger(server, false).await, LINGER_BYTES);
            assert!(started.elapsed() < LINGER_TIME);
            sender.await.unwrap();
        });
    }

    #[test]
    fn a3_b1_the_stream_is_reclaimed_when_hyper_drops_it() {
        let (reclaim, slot) = Reclaim::new(7_u8);
        assert!(reclaimed(&slot).is_none());
        drop(reclaim);
        assert_eq!(reclaimed(&slot), Some((7, false)));
        assert!(reclaimed(&slot).is_none());
    }

    #[test]
    fn a3_b1_a_completed_shutdown_is_recorded_and_not_repeated() {
        virtual_time(async {
            let (server, mut client) = duplex(1024);
            let (mut reclaim, slot) = Reclaim::new(server);
            reclaim.shutdown().await.unwrap();
            drop(reclaim);
            let (server, shut_down) = reclaimed(&slot).unwrap();
            assert!(shut_down);
            client.write_all(b"late").await.unwrap();
            drop(client);
            assert_eq!(linger(server, shut_down).await, b"late".len());
        });
    }
}
