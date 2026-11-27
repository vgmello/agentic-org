//! Streamed request bodies: the one ingress path that bypasses the buffered 1 MiB business
//! body ([`super::read_business_body`]), used only by attachment uploads.
//!
//! A [`BodyFeeder`] cuts the request body into chunks of at most `chunk_bytes` and hands
//! them over a one-slot channel, so the web process holds at most a few chunks of one
//! upload at a time, never the whole file. The body must carry exactly the declared
//! `Content-Length`; one absolute deadline covers the whole transfer and is never
//! restarted by chunk progress. No failure keeps codec error text or payload bytes.

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

/// The receiving half: chunks in order, then `None` once exactly the declared bytes arrived.
pub struct StreamedBody {
    chunks: mpsc::Receiver<Result<Bytes, StreamError>>,
    declared: u64,
    received: u64,
    deadline: Instant,
    ended: bool,
}

/// The sending half, driven by the service alongside the call that consumes the chunks.
pub struct BodyFeeder {
    chunks: mpsc::Sender<Result<Bytes, StreamError>>,
    declared: u64,
    chunk_bytes: usize,
    deadline: Instant,
}

/// A streamed body of exactly `declared` bytes, in chunks of at most `chunk_bytes`, that
/// must arrive before `deadline`.
pub fn channel(declared: u64, chunk_bytes: usize, deadline: Instant) -> (StreamedBody, BodyFeeder) {
    let (sender, receiver) = mpsc::channel(1);
    (
        StreamedBody {
            chunks: receiver,
            declared,
            received: 0,
            deadline,
            ended: false,
        },
        BodyFeeder {
            chunks: sender,
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
    /// early (dropped, or out of time with a full channel) is an error here, never an end.
    pub async fn next_chunk(&mut self) -> Option<Result<Bytes, StreamError>> {
        if self.ended {
            return None;
        }
        let next = tokio::time::timeout_at(self.deadline, self.chunks.recv())
            .await
            .unwrap_or(Some(Err(StreamError::TimedOut)));
        let next = match next {
            Some(Ok(chunk)) => match self.received.checked_add(chunk.len() as u64) {
                Some(total) if total <= self.declared => {
                    self.received = total;
                    return Some(Ok(chunk));
                }
                _ => Err(StreamError::LengthMismatch),
            },
            Some(Err(error)) => Err(error),
            None if self.received == self.declared => {
                self.ended = true;
                return None;
            }
            None if Instant::now() >= self.deadline => Err(StreamError::TimedOut),
            None => Err(StreamError::Malformed),
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
            // Never wait past the deadline to report: a full channel or a receiver that
            // stopped listening sees the early end in `next_chunk` instead.
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
                    pending.reserve_exact(self.chunk_bytes);
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
