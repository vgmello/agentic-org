use super::*;
use http_body_util::Full;
use hyper::body::Frame;
use std::collections::VecDeque;
use std::io;

fn run(test: impl std::future::Future<Output = ()>) {
    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .unwrap()
        .block_on(async {
            tokio::time::pause();
            test.await;
        });
}

/// A body that yields its frames in order, without any stream crate.
struct Frames(VecDeque<Result<Frame<Bytes>, io::Error>>);
impl Body for Frames {
    type Data = Bytes;
    type Error = io::Error;
    fn poll_frame(
        mut self: std::pin::Pin<&mut Self>,
        _: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Result<Frame<Bytes>, io::Error>>> {
        std::task::Poll::Ready(self.0.pop_front())
    }
}

fn frames(parts: &[&[u8]]) -> Frames {
    Frames(
        parts
            .iter()
            .map(|part| Ok(Frame::data(Bytes::copy_from_slice(part))))
            .collect(),
    )
}

/// Drives `feeder` and drains `body` together, as the service does.
async fn drain<B>(
    mut body: StreamedBody,
    feeder: BodyFeeder,
    source: B,
) -> Vec<Result<Bytes, StreamError>>
where
    B: Body<Data = Bytes> + Send + 'static,
    B::Error: Into<Box<dyn std::error::Error + Send + Sync>> + Send,
{
    let pump = tokio::spawn(feeder.pump(source));
    let mut out = Vec::new();
    while let Some(next) = body.next_chunk().await {
        let failed = next.is_err();
        out.push(next);
        if failed {
            break;
        }
    }
    pump.await.unwrap();
    out
}

#[test]
fn bodies_are_recut_into_bounded_chunks_in_order_and_end_at_the_declared_length() {
    run(async {
        let deadline = Instant::now() + STREAMED_BODY_TIMEOUT;
        let (body, feeder) = channel(10, 4, deadline);
        let out = drain(body, feeder, frames(&[b"ab", b"cdefg", b"", b"hij"])).await;
        let chunks: Vec<_> = out.into_iter().map(Result::unwrap).collect();
        assert_eq!(
            chunks,
            [&b"abcd"[..], &b"efgh"[..], &b"ij"[..]].map(Bytes::from_static)
        );
        // One frame larger than several chunks is split without loss.
        let (body, feeder) = channel(9, 4, deadline);
        let out = drain(body, feeder, Full::new(Bytes::from_static(b"012345678"))).await;
        let lengths: Vec<_> = out.iter().map(|c| c.as_ref().unwrap().len()).collect();
        assert_eq!(lengths, [4, 4, 1]);
    });
}

#[test]
fn a_body_longer_or_shorter_than_declared_is_refused_and_never_ends_cleanly() {
    run(async {
        let deadline = Instant::now() + STREAMED_BODY_TIMEOUT;
        let (body, feeder) = channel(3, 4, deadline);
        let out = drain(body, feeder, frames(&[b"ab", b"cd"])).await;
        assert_eq!(out.last(), Some(&Err(StreamError::LengthMismatch)));
        let (body, feeder) = channel(5, 4, deadline);
        let out = drain(body, feeder, frames(&[b"ab"])).await;
        assert_eq!(out, [Err(StreamError::LengthMismatch)]);
    });
}

#[test]
fn a_failing_connection_is_malformed_without_its_error_text() {
    run(async {
        let deadline = Instant::now() + STREAMED_BODY_TIMEOUT;
        let (body, feeder) = channel(4, 4, deadline);
        let source = Frames(VecDeque::from([
            Ok(Frame::data(Bytes::from_static(b"ab"))),
            Err(io::Error::other("PRIVATE_STREAM_ERROR")),
        ]));
        let out = drain(body, feeder, source).await;
        assert_eq!(out, [Err(StreamError::Malformed)]);
        assert!(!format!("{out:?}").contains("PRIVATE"));
    });
}

#[test]
fn a_feeder_dropped_early_is_an_error_not_an_end() {
    run(async {
        let (mut body, feeder) = channel(4, 4, Instant::now() + STREAMED_BODY_TIMEOUT);
        drop(feeder);
        assert_eq!(body.next_chunk().await, Some(Err(StreamError::Malformed)));
        assert_eq!(body.next_chunk().await, None);
    });
}

/// A body that never sends its second byte: the total deadline ends the transfer.
struct Stalled(bool);
impl Body for Stalled {
    type Data = Bytes;
    type Error = io::Error;
    fn poll_frame(
        mut self: std::pin::Pin<&mut Self>,
        _: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Result<Frame<Bytes>, io::Error>>> {
        if self.0 {
            return std::task::Poll::Pending;
        }
        self.0 = true;
        std::task::Poll::Ready(Some(Ok(Frame::data(Bytes::from_static(b"a")))))
    }
}

#[test]
fn the_total_deadline_ends_a_stalled_upload() {
    run(async {
        let start = Instant::now();
        let (body, feeder) = channel(2, 1, start + STREAMED_BODY_TIMEOUT);
        let out = drain(body, feeder, Stalled(false)).await;
        assert_eq!(
            out,
            [Ok(Bytes::from_static(b"a")), Err(StreamError::TimedOut)]
        );
        assert!(Instant::now() >= start + STREAMED_BODY_TIMEOUT);
    });
}

#[test]
fn the_feeder_stops_once_the_consumer_stops_listening() {
    run(async {
        let (body, feeder) = channel(12, 4, Instant::now() + STREAMED_BODY_TIMEOUT);
        drop(body);
        // Returns instead of waiting on a channel nobody drains.
        feeder.pump(frames(&[b"0123456789ab"])).await;
    });
}

/// `TASK-98.5`: a feeder that fails after filling the one slot cannot queue its error, and
/// the chunk it queued already adds up to the declared length. The receiver still reports
/// the failure, never a clean end of a body that broke its declared length or framing.
#[test]
fn a_failure_behind_an_undrained_full_slot_is_reported_never_a_clean_end() {
    run(async {
        let deadline = Instant::now() + STREAMED_BODY_TIMEOUT;
        // One more byte than declared, after a first frame of exactly the declared length.
        let (mut body, feeder) = channel(4, 4, deadline);
        feeder.pump(frames(&[b"abcd", b"e"])).await;
        assert_eq!(
            body.next_chunk().await,
            Some(Err(StreamError::LengthMismatch))
        );
        assert_eq!(body.next_chunk().await, None);
        // The connection fails after the declared bytes arrived.
        let (mut body, feeder) = channel(4, 4, deadline);
        let source = Frames(VecDeque::from([
            Ok(Frame::data(Bytes::from_static(b"abcd"))),
            Err(io::Error::other("reset")),
        ]));
        feeder.pump(source).await;
        assert_eq!(body.next_chunk().await, Some(Err(StreamError::Malformed)));
        assert_eq!(body.next_chunk().await, None);
    });
}
