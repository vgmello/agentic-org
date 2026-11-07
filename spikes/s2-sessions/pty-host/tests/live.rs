//! Live tests against a real background session you own. Ignored by default:
//! PTY_HOST_TEST_SESSION=<id8> cargo test -- --ignored --test-threads=1
use std::sync::mpsc;
use std::time::Duration;

use pty_host::{open, DeliveryError, ExitReason, Frame, OpenOptions};

fn session() -> String {
    std::env::var("PTY_HOST_TEST_SESSION").expect("set PTY_HOST_TEST_SESSION=<id8> of a throwaway background session")
}

/// A failing callback (webview gone) makes the handle close its own attach client.
#[test]
#[ignore = "needs a live background session (PTY_HOST_TEST_SESSION)"]
fn live_delivery_failure_closes_attach_client() {
    let (tx, rx) = mpsc::channel();
    let mut first = true;
    let mut h = open(OpenOptions { session_id: session(), cols: 80, rows: 24, claude_bin: None }, move |f| match f {
        Frame::Exit { reason, .. } => {
            tx.send(reason).unwrap();
            Ok(())
        }
        Frame::Data(_) if first => {
            first = false;
            Err(DeliveryError("webview gone".into()))
        }
        Frame::Data(_) | Frame::Title(_) => panic!("no frames after a failed delivery"),
    })
    .unwrap();
    let reason = rx.recv_timeout(Duration::from_secs(5)).expect("exit frame");
    assert_eq!(reason, ExitReason::Closed);
    h.close();
    assert!(h.write(b"x").is_err());
}

#[test]
#[ignore = "needs a live background session (PTY_HOST_TEST_SESSION)"]
fn live_ctrl_z_detach_is_attach_exited_and_close_sends_nothing() {
    let (tx, rx) = mpsc::channel();
    let mut h = open(OpenOptions { session_id: session(), cols: 80, rows: 24, claude_bin: None }, move |f| {
        if let Frame::Exit { code, reason, .. } = f {
            tx.send((code, reason)).unwrap();
        }
        Ok(())
    })
    .unwrap();
    std::thread::sleep(Duration::from_secs(2));
    h.write(b"\x1a").unwrap();
    let (code, reason) = rx.recv_timeout(Duration::from_secs(5)).expect("exit frame");
    assert_eq!((code, reason), (0, ExitReason::AttachExited));
    h.close();
    assert_eq!(h.signals_sent(), 0, "close() after the client exited must not signal");
}

#[test]
#[ignore = "needs a live background session (PTY_HOST_TEST_SESSION)"]
fn live_close_is_closed_and_quick() {
    let (tx, rx) = mpsc::channel();
    let mut h = open(OpenOptions { session_id: session(), cols: 100, rows: 30, claude_bin: None }, move |f| {
        if let Frame::Exit { reason, .. } = f {
            tx.send(reason).unwrap();
        }
        Ok(())
    })
    .unwrap();
    std::thread::sleep(Duration::from_secs(2));
    let t = std::time::Instant::now();
    h.close();
    h.close();
    assert!(t.elapsed() < Duration::from_secs(1), "close took {:?}", t.elapsed());
    assert_eq!(rx.recv_timeout(Duration::from_secs(1)).unwrap(), ExitReason::Closed);
}
