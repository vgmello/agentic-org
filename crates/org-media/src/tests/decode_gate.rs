//! The process-wide image decode gate (`TASK-241` M-2): at most
//! [`MAX_CONCURRENT_IMAGE_DECODES`] decodes at once, a caller past the bound waits for a
//! returned permit, and a permit comes back on every path, a panic's unwind included.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::thread;
use std::time::Duration;

use crate::image::{model_derivative, DecodeGate, DECODES, MAX_CONCURRENT_IMAGE_DECODES};
use crate::testkit::png;
use crate::{validate, MediaType};

/// Long enough for a caller that could get a permit to get one.
const WAIT: Duration = Duration::from_millis(250);
/// The longest a test waits for a returned permit to reach its waiter.
const SETTLE: Duration = Duration::from_secs(30);

#[test]
fn a_caller_past_the_limit_waits_until_a_permit_is_returned() {
    assert_eq!(MAX_CONCURRENT_IMAGE_DECODES, 2);
    let gate = DecodeGate::new(MAX_CONCURRENT_IMAGE_DECODES);
    thread::scope(|scope| {
        let first = gate.acquire();
        let second = gate.acquire();
        assert_eq!(gate.held(), 2);
        let (entered, inside) = mpsc::channel();
        let (release, released) = mpsc::channel::<()>();
        let gate = &gate;
        scope.spawn(move || {
            let _permit = gate.acquire();
            entered.send(()).unwrap();
            released.recv().unwrap();
        });
        // Both permits are out: the third caller waits, however long.
        assert_eq!(inside.recv_timeout(WAIT), Err(RecvTimeoutError::Timeout));
        assert_eq!(gate.held(), 2);
        drop(first);
        inside.recv_timeout(SETTLE).expect("the returned permit");
        assert_eq!(gate.held(), 2);
        release.send(()).unwrap();
        drop(second);
    });
    assert_eq!(gate.held(), 0);
}

#[test]
fn many_callers_never_hold_more_than_the_limit_at_once() {
    let gate = DecodeGate::new(MAX_CONCURRENT_IMAGE_DECODES);
    let (inside, most) = (AtomicUsize::new(0), AtomicUsize::new(0));
    thread::scope(|scope| {
        for _ in 0..8 {
            scope.spawn(|| {
                for _ in 0..200 {
                    let _permit = gate.acquire();
                    let now = inside.fetch_add(1, Ordering::SeqCst) + 1;
                    most.fetch_max(now, Ordering::SeqCst);
                    thread::yield_now();
                    inside.fetch_sub(1, Ordering::SeqCst);
                }
            });
        }
    });
    let most = most.load(Ordering::SeqCst);
    assert!(
        (1..=MAX_CONCURRENT_IMAGE_DECODES).contains(&most),
        "{most} at once"
    );
    assert_eq!(gate.held(), 0);
}

#[test]
fn a_permit_is_returned_when_its_holder_panics() {
    let gate = DecodeGate::new(MAX_CONCURRENT_IMAGE_DECODES);
    thread::scope(|scope| {
        let holder = scope.spawn(|| {
            let _first = gate.acquire();
            let _second = gate.acquire();
            panic!("a decoder panics holding every permit");
        });
        assert!(holder.join().is_err());
    });
    assert_eq!(gate.held(), 0);
    // An early return drops its permit too.
    let early = |fail: bool| -> Result<(), ()> {
        let _permit = gate.acquire();
        if fail {
            return Err(());
        }
        Ok(())
    };
    assert!(early(true).is_err());
    assert!(early(false).is_ok());
    // Every permit is free again.
    let all: Vec<_> = (0..MAX_CONCURRENT_IMAGE_DECODES)
        .map(|_| gate.acquire())
        .collect();
    assert_eq!(gate.held(), MAX_CONCURRENT_IMAGE_DECODES);
    drop(all);
    assert_eq!(gate.held(), 0);
}

#[test]
fn sanitizing_and_model_derivatives_wait_at_the_process_gate() {
    let raw = png(8, 8);
    let kept = validate(MediaType::Png, raw.clone()).unwrap().bytes;
    thread::scope(|scope| {
        // Every permit of the process's gate, held here (other tests' decodes wait too).
        let held: Vec<_> = (0..MAX_CONCURRENT_IMAGE_DECODES)
            .map(|_| DECODES.acquire())
            .collect();
        let (done, finished) = mpsc::channel();
        let sanitized = done.clone();
        scope.spawn(move || {
            sanitized
                .send(validate(MediaType::Png, raw).is_ok())
                .unwrap()
        });
        scope.spawn(move || {
            done.send(model_derivative(MediaType::Png, &kept).is_ok())
                .unwrap()
        });
        assert_eq!(finished.recv_timeout(WAIT), Err(RecvTimeoutError::Timeout));
        drop(held);
        for _ in 0..2 {
            assert_eq!(finished.recv_timeout(SETTLE), Ok(true));
        }
    });
}
