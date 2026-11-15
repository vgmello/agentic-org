//! Fail a write/flush that makes no progress within the configured duration.

use std::future::Future;
use std::io::{self, IoSlice};
use std::pin::Pin;
use std::task::{Context, Poll};
use std::time::Duration;

use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio::time::{self, Sleep};

/// A progressing response has no overall duration cap. The timer exists only
/// while an output operation is pending, and resets when bytes are accepted.
/// Wrap the TCP stream before TLS so TLS's own writes have the same bound.
pub struct WriteStall<T> {
    inner: T,
    timeout: Duration,
    pending_since: Option<PendingOutput>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum OutputOperation {
    Write,
    Flush,
    Shutdown,
}

struct PendingOutput {
    operation: OutputOperation,
    timer: Pin<Box<Sleep>>,
}

impl<T> WriteStall<T> {
    pub fn new(inner: T, timeout: Duration) -> io::Result<Self> {
        if timeout.is_zero() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "zero write timeout",
            ));
        }
        Ok(Self {
            inner,
            timeout,
            pending_since: None,
        })
    }

    fn stalled(
        &mut self,
        cx: &mut Context<'_>,
        operation: OutputOperation,
    ) -> Poll<io::Result<()>> {
        let timer = self.pending_since.get_or_insert_with(|| PendingOutput {
            operation,
            timer: Box::pin(time::sleep(self.timeout)),
        });
        if timer.timer.as_mut().poll(cx).is_ready() {
            Poll::Ready(Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "response write stalled",
            )))
        } else {
            Poll::Pending
        }
    }

    fn written(
        &mut self,
        cx: &mut Context<'_>,
        result: Poll<io::Result<usize>>,
    ) -> Poll<io::Result<usize>> {
        match result {
            Poll::Ready(Ok(bytes)) => {
                if bytes > 0 {
                    self.pending_since = None;
                }
                Poll::Ready(Ok(bytes))
            }
            Poll::Ready(Err(error)) => Poll::Ready(Err(error)),
            Poll::Pending => self.stalled(cx, OutputOperation::Write).map_ok(|_| 0),
        }
    }

    fn flushed(
        &mut self,
        cx: &mut Context<'_>,
        result: Poll<io::Result<()>>,
        operation: OutputOperation,
    ) -> Poll<io::Result<()>> {
        match result {
            Poll::Ready(result) => {
                // TCP's empty flush is immediately Ready, even while a write
                // remains blocked. Only completing the operation that started
                // this timer can clear it without accepted write bytes.
                if self
                    .pending_since
                    .as_ref()
                    .is_some_and(|pending| pending.operation == operation)
                {
                    self.pending_since = None;
                }
                Poll::Ready(result)
            }
            Poll::Pending => self.stalled(cx, operation),
        }
    }
}

impl<T: AsyncRead + Unpin> AsyncRead for WriteStall<T> {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buffer: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        Pin::new(&mut self.get_mut().inner).poll_read(cx, buffer)
    }
}

impl<T: AsyncWrite + Unpin> AsyncWrite for WriteStall<T> {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        bytes: &[u8],
    ) -> Poll<io::Result<usize>> {
        let this = self.get_mut();
        let result = Pin::new(&mut this.inner).poll_write(cx, bytes);
        this.written(cx, result)
    }

    fn poll_write_vectored(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        bytes: &[IoSlice<'_>],
    ) -> Poll<io::Result<usize>> {
        let this = self.get_mut();
        let result = Pin::new(&mut this.inner).poll_write_vectored(cx, bytes);
        this.written(cx, result)
    }

    fn is_write_vectored(&self) -> bool {
        self.inner.is_write_vectored()
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        let this = self.get_mut();
        let result = Pin::new(&mut this.inner).poll_flush(cx);
        this.flushed(cx, result, OutputOperation::Flush)
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        let this = self.get_mut();
        let result = Pin::new(&mut this.inner).poll_shutdown(cx);
        this.flushed(cx, result, OutputOperation::Shutdown)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::future::poll_fn;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    fn run(test: impl Future<Output = ()>) {
        tokio::runtime::Builder::new_current_thread()
            .enable_time()
            .build()
            .unwrap()
            .block_on(async {
                time::pause();
                test.await;
            });
    }

    async fn pending<F: Future>(mut future: Pin<&mut F>) {
        poll_fn(|cx| {
            assert!(future.as_mut().poll(cx).is_pending());
            Poll::Ready(())
        })
        .await;
    }

    #[test]
    fn stalled_output_fails_at_the_injected_deadline() {
        run(async {
            let (writer, _reader) = tokio::io::duplex(1);
            let mut writer = WriteStall::new(writer, Duration::from_secs(30)).unwrap();
            writer.write_all(&[1]).await.unwrap();
            let mut write = Box::pin(writer.write_all(&[2]));
            pending(write.as_mut()).await;
            time::advance(Duration::from_secs(29)).await;
            pending(write.as_mut()).await;
            time::advance(Duration::from_secs(1)).await;
            assert_eq!(write.await.unwrap_err().kind(), io::ErrorKind::TimedOut);
        });
    }

    #[test]
    fn an_empty_successful_flush_is_not_write_progress() {
        run(async {
            let (writer, _reader) = tokio::io::duplex(1);
            let mut writer = WriteStall::new(writer, Duration::from_secs(30)).unwrap();
            writer.write_all(&[1]).await.unwrap();
            poll_fn(|cx| {
                assert!(Pin::new(&mut writer).poll_write(cx, &[2]).is_pending());
                Poll::Ready(())
            })
            .await;
            writer.flush().await.unwrap();
            time::advance(Duration::from_secs(30)).await;
            let error = time::timeout(Duration::from_secs(1), writer.write_all(&[2]))
                .await
                .expect("empty flush must not reset an existing write deadline")
                .unwrap_err();
            assert_eq!(error.kind(), io::ErrorKind::TimedOut);
        });
    }

    #[test]
    fn slow_continuous_progress_resets_the_stall_deadline() {
        run(async {
            let (writer, mut reader) = tokio::io::duplex(1);
            let mut writer = WriteStall::new(writer, Duration::from_secs(30)).unwrap();
            writer.write_all(&[1]).await.unwrap();
            let mut write = Box::pin(writer.write_all(&[2]));
            pending(write.as_mut()).await;
            time::advance(Duration::from_secs(20)).await;
            assert_eq!(reader.read_u8().await.unwrap(), 1);
            write.await.unwrap();
            let mut write = Box::pin(writer.write_all(&[3]));
            pending(write.as_mut()).await;
            time::advance(Duration::from_secs(20)).await;
            pending(write.as_mut()).await;
            assert_eq!(reader.read_u8().await.unwrap(), 2);
            write.await.unwrap();
            assert_eq!(reader.read_u8().await.unwrap(), 3);
        });
    }

    #[test]
    fn reads_are_unaffected_and_zero_timeouts_are_refused() {
        run(async {
            let (stream, mut peer) = tokio::io::duplex(16);
            let mut stream = WriteStall::new(stream, Duration::from_secs(30)).unwrap();
            time::advance(Duration::from_secs(60)).await;
            peer.write_all(b"ok").await.unwrap();
            let mut bytes = [0; 2];
            stream.read_exact(&mut bytes).await.unwrap();
            assert_eq!(&bytes, b"ok");
        });
        let (stream, _) = tokio::io::duplex(1);
        assert!(WriteStall::new(stream, Duration::ZERO).is_err());
    }
}
