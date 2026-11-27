//! Streamed request bodies: the one ingress path that bypasses the buffered 1 MiB business
//! body ([`super::read_business_body`]), used only by attachment uploads.
//!
//! A [`BodyFeeder`] cuts the request body into chunks of at most `chunk_bytes` and hands
//! them over a one-slot channel, so the web process holds at most a few chunks of one
//! upload at a time, never the whole file. The body must carry exactly the declared
//! `Content-Length`; one absolute deadline covers the whole transfer and is never
//! restarted by chunk progress. No failure keeps codec error text or payload bytes.

use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::Arc;
use std::time::Duration;

use http_body_util::BodyExt;
use hyper::body::{Body, Bytes};
use tokio::sync::mpsc;
use tokio::time::Instant;

/// The total time a streamed body (and the calls it feeds) may take.
pub const STREAMED_BODY_TIMEOUT: Duration = Duration::from_secs(60);

/// Why a streamed body ended before its declared end.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StreamError {
    /// The deadline passed first.
    TimedOut,
    /// The client sent more or fewer bytes than it declared.
    LengthMismatch,
    /// The connection or its framing failed.
    Malformed,
}

impl StreamError {
    /// The code [`FeedFailure`] keeps; `0` is no failure.
    fn code(self) -> u8 {
        match self {
            Self::TimedOut => 1,
            Self::LengthMismatch => 2,
            Self::Malformed => 3,
        }
    }

    fn from_code(code: u8) -> Option<Self> {
        match code {
            1 => Some(Self::TimedOut),
            2 => Some(Self::LengthMismatch),
            3 => Some(Self::Malformed),
            _ => None,
        }
    }
}

/// How the feeder failed, shared with the receiver. The error the feeder also tries to
/// queue is lost when the one-slot channel is full, so the receiver checks this before it
/// hands out a chunk or reports the end: a failed body never reads as a complete one.
#[derive(Clone, Default)]
struct FeedFailure(Arc<AtomicU8>);

impl FeedFailure {
    /// Recorded before the feeder drops its sender, so a receiver that sees the channel
    /// close also sees the failure.
    fn record(&self, error: StreamError) {
        self.0.store(error.code(), Ordering::Release);
    }

    fn get(&self) -> Option<StreamError> {
        StreamError::from_code(self.0.load(Ordering::Acquire))
    }
}

/// The receiving half: chunks in order, then `None` once exactly the declared bytes arrived.
pub struct StreamedBody {
    chunks: mpsc::Receiver<Result<Bytes, StreamError>>,
    failure: FeedFailure,
    declared: u64,
    received: u64,
    deadline: Instant,
    ended: bool,
}

/// The sending half, driven by the service alongside the call that consumes the chunks.
pub struct BodyFeeder {
    chunks: mpsc::Sender<Result<Bytes, StreamError>>,
    failure: FeedFailure,
    declared: u64,
    chunk_bytes: usize,
    deadline: Instant,
}

/// A streamed body of exactly `declared` bytes, in chunks of at most `chunk_bytes`, that
/// must arrive before `deadline`.
pub fn channel(declared: u64, chunk_bytes: usize, deadline: Instant) -> (StreamedBody, BodyFeeder) {
    let (sender, receiver) = mpsc::channel(1);
    let failure = FeedFailure::default();
    (
        StreamedBody {
            chunks: receiver,
            failure: failure.clone(),
            declared,
            received: 0,
            deadline,
            ended: false,
        },
        BodyFeeder {
            chunks: sender,
            failure,
            declared,
            chunk_bytes: chunk_bytes.max(1),
            deadline,
        },
    )
}

impl StreamedBody {
    /// The length the request declared (and the route admitted).
    pub fn declared(&self) -> u64 {
        self.declared
    }

    /// What is left of the transfer's one deadline.
    pub fn remaining(&self) -> Duration {
        self.deadline.saturating_duration_since(Instant::now())
    }

    /// The next chunk; `None` only after every declared byte arrived. A feeder that stops
    /// early (dropped, failed, or out of time with a full channel) is an error here, never
    /// an end, even when the chunks it queued before failing add up to the declared length.
    pub async fn next_chunk(&mut self) -> Option<Result<Bytes, StreamError>> {
        if self.ended {
            return None;
        }
        let next = tokio::time::timeout_at(self.deadline, self.chunks.recv())
            .await
            .unwrap_or(Some(Err(StreamError::TimedOut)));
        let next = match next {
            Some(Ok(chunk)) => match self.received.checked_add(chunk.len() as u64) {
                Some(total) if total <= self.declared => match self.failure.get() {
                    Some(error) => Err(error),
                    None => {
                        self.received = total;
                        return Some(Ok(chunk));
                    }
                },
                _ => Err(StreamError::LengthMismatch),
            },
            Some(Err(error)) => Err(error),
            None => match self.failure.get() {
                Some(error) => Err(error),
                None if self.received == self.declared => {
                    self.ended = true;
                    return None;
                }
                None if Instant::now() >= self.deadline => Err(StreamError::TimedOut),
                None => Err(StreamError::Malformed),
            },
        };
        self.ended = true;
        Some(next)
    }
}

impl BodyFeeder {
    /// Reads `body` to its end (or the first failure) and forwards it in chunks. Returns when
    /// the body ends, fails, runs out of time, or the receiving call stops listening.
    pub async fn pump<B>(self, body: B)
    where
        B: Body<Data = Bytes>,
        B::Error: Into<Box<dyn std::error::Error + Send + Sync>>,
    {
        let outcome = tokio::time::timeout_at(self.deadline, self.forward(body))
            .await
            .unwrap_or(Err(Stop::Failed(StreamError::TimedOut)));
        if let Err(Stop::Failed(error)) = outcome {
            // Recorded first, so the failure survives a full channel; never wait past the
            // deadline to queue it as well.
            self.failure.record(error);
            let _ = self.chunks.try_send(Err(error));
        }
    }

    async fn forward<B>(&self, body: B) -> Result<(), Stop>
    where
        B: Body<Data = Bytes>,
        B::Error: Into<Box<dyn std::error::Error + Send + Sync>>,
    {
        let mut body = std::pin::pin!(body);
        let mut pending: Vec<u8> = Vec::new();
        let mut total: u64 = 0;
        while let Some(frame) = body.frame().await {
            let frame = frame.map_err(|_| Stop::Failed(StreamError::Malformed))?;
            let Ok(mut data) = frame.into_data() else {
                continue;
            };
            total = total
                .checked_add(data.len() as u64)
                .filter(|total| *total <= self.declared)
                .ok_or(Stop::Failed(StreamError::LengthMismatch))?;
            while !data.is_empty() {
                if pending.capacity() == 0 {
                    // A small upload never reserves a whole chunk.
                    let declared = usize::try_from(self.declared).unwrap_or(usize::MAX);
                    pending.reserve_exact(self.chunk_bytes.min(declared));
                }
                let take = (self.chunk_bytes - pending.len()).min(data.len());
                pending.extend_from_slice(&data.split_to(take));
                if pending.len() == self.chunk_bytes {
                    self.deliver(std::mem::take(&mut pending)).await?;
                }
            }
        }
        if total != self.declared {
            return Err(Stop::Failed(StreamError::LengthMismatch));
        }
        if !pending.is_empty() {
            self.deliver(pending).await?;
        }
        Ok(())
    }

    async fn deliver(&self, chunk: Vec<u8>) -> Result<(), Stop> {
        self.chunks
            .send(Ok(Bytes::from(chunk)))
            .await
            .map_err(|_| Stop::Closed)
    }
}

/// Why forwarding stopped early.
enum Stop {
    /// The receiving call finished or was dropped: nobody is left to tell.
    Closed,
    Failed(StreamError),
}

#[cfg(test)]
mod tests;
