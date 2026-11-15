//! Bounded business-request bodies, before decoding or native admission.
//!
//! Credential routes need B1-06's zeroed ingress buffers and direct secret-field
//! decoding; this business-data collector must not be used for those routes.

use std::fmt;
use std::time::Duration;

use http_body_util::{BodyExt, LengthLimitError, Limited};
use hyper::body::{Body, Bytes};

pub const MAX_BUSINESS_BODY_BYTES: usize = 1024 * 1024;
pub const BODY_READ_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BodyError {
    TooLarge,
    TimedOut,
    Malformed,
    InvalidLimits,
}

/// No diagnostic or serialization path exposes the request payload.
pub struct BusinessBody(Bytes);

impl BusinessBody {
    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }
}

impl fmt::Debug for BusinessBody {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("BusinessBody")
            .field("length", &self.0.len())
            .finish_non_exhaustive()
    }
}

/// Size applies to all DATA frames, including chunked requests. The deadline is
/// absolute for the whole body, not restarted for each arriving chunk. All
/// failures precede native admission and contain no codec error text or payload.
pub async fn read_business_body<B>(
    body: B,
    max_bytes: usize,
    deadline: Duration,
) -> Result<BusinessBody, BodyError>
where
    B: Body<Data = Bytes>,
    B::Error: Into<Box<dyn std::error::Error + Send + Sync>>,
{
    if max_bytes == 0
        || deadline.is_zero()
        || tokio::time::Instant::now().checked_add(deadline).is_none()
    {
        return Err(BodyError::InvalidLimits);
    }
    if body.size_hint().lower() > max_bytes as u64 {
        return Err(BodyError::TooLarge);
    }
    let collected = tokio::time::timeout(deadline, Limited::new(body, max_bytes).collect())
        .await
        .map_err(|_| BodyError::TimedOut)?
        .map_err(|error| {
            if error.is::<LengthLimitError>() {
                BodyError::TooLarge
            } else {
                BodyError::Malformed
            }
        })?;
    Ok(BusinessBody(collected.to_bytes()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use http_body_util::Full;
    use hyper::body::{Frame, SizeHint};
    use std::collections::VecDeque;
    use std::future::Future;
    use std::io;
    use std::pin::Pin;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::sync::Arc;
    use std::task::{Context, Poll};
    use tokio::time::{self, Sleep};

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

    struct Chunks {
        chunks: VecDeque<Result<Bytes, io::Error>>,
        polls: Arc<AtomicUsize>,
        declared: Option<u64>,
    }

    impl Body for Chunks {
        type Data = Bytes;
        type Error = io::Error;
        fn poll_frame(
            mut self: Pin<&mut Self>,
            _: &mut Context<'_>,
        ) -> Poll<Option<Result<Frame<Bytes>, io::Error>>> {
            self.polls.fetch_add(1, Ordering::AcqRel);
            Poll::Ready(self.chunks.pop_front().map(|chunk| chunk.map(Frame::data)))
        }
        fn size_hint(&self) -> SizeHint {
            let mut hint = SizeHint::new();
            if let Some(size) = self.declared {
                hint.set_exact(size);
            }
            hint
        }
    }

    #[test]
    fn exact_limit_and_empty_bodies_succeed_without_payload_diagnostics() {
        run(async {
            let body = read_business_body(
                Full::new(Bytes::from_static(b"PRIVATE_BODY")),
                12,
                BODY_READ_TIMEOUT,
            )
            .await
            .unwrap();
            assert_eq!(body.as_bytes(), b"PRIVATE_BODY");
            assert!(!format!("{body:?}").contains("PRIVATE_BODY"));
            let empty = read_business_body(Full::new(Bytes::new()), 1, BODY_READ_TIMEOUT)
                .await
                .unwrap();
            assert!(empty.as_bytes().is_empty());
            assert_eq!(MAX_BUSINESS_BODY_BYTES, 1_048_576);
        });
    }

    #[test]
    fn oversized_declared_bodies_are_refused_without_polling() {
        run(async {
            let polls = Arc::new(AtomicUsize::new(0));
            let body = Chunks {
                chunks: VecDeque::new(),
                polls: Arc::clone(&polls),
                declared: Some(6),
            };
            assert!(matches!(
                read_business_body(body, 5, BODY_READ_TIMEOUT).await,
                Err(BodyError::TooLarge)
            ));
            assert_eq!(polls.load(Ordering::Acquire), 0);
        });
    }

    #[test]
    fn unknown_length_chunks_are_bounded_and_stream_errors_are_sanitized() {
        run(async {
            let chunks = VecDeque::from([
                Ok(Bytes::from_static(b"123")),
                Ok(Bytes::from_static(b"456")),
            ]);
            let body = Chunks {
                chunks,
                polls: Arc::new(AtomicUsize::new(0)),
                declared: None,
            };
            assert!(matches!(
                read_business_body(body, 5, BODY_READ_TIMEOUT).await,
                Err(BodyError::TooLarge)
            ));
            let body = Chunks {
                chunks: VecDeque::from([Err(io::Error::other("PRIVATE_STREAM_ERROR"))]),
                polls: Arc::new(AtomicUsize::new(0)),
                declared: None,
            };
            let error = read_business_body(body, 5, BODY_READ_TIMEOUT)
                .await
                .unwrap_err();
            assert_eq!(error, BodyError::Malformed);
            assert!(!format!("{error:?}").contains("PRIVATE_STREAM_ERROR"));
        });
    }

    struct Drip {
        remaining: usize,
        delay: Pin<Box<Sleep>>,
        dropped: Arc<AtomicBool>,
    }
    impl Body for Drip {
        type Data = Bytes;
        type Error = io::Error;
        fn poll_frame(
            mut self: Pin<&mut Self>,
            cx: &mut Context<'_>,
        ) -> Poll<Option<Result<Frame<Bytes>, io::Error>>> {
            if self.remaining == 0 {
                return Poll::Ready(None);
            }
            if self.delay.as_mut().poll(cx).is_pending() {
                return Poll::Pending;
            }
            self.remaining -= 1;
            self.delay
                .as_mut()
                .reset(time::Instant::now() + Duration::from_secs(6));
            Poll::Ready(Some(Ok(Frame::data(Bytes::from_static(b"x")))))
        }
    }
    impl Drop for Drip {
        fn drop(&mut self) {
            self.dropped.store(true, Ordering::Release);
        }
    }

    #[test]
    fn the_total_read_deadline_is_not_extended_by_chunk_progress() {
        run(async {
            let dropped = Arc::new(AtomicBool::new(false));
            let body = Drip {
                remaining: 2,
                delay: Box::pin(time::sleep(Duration::from_secs(6))),
                dropped: Arc::clone(&dropped),
            };
            assert!(matches!(
                read_business_body(body, 5, BODY_READ_TIMEOUT).await,
                Err(BodyError::TimedOut)
            ));
            assert!(dropped.load(Ordering::Acquire));
        });
    }
}
