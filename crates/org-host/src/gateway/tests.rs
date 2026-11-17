//! Supervisor and handoff unit tests. The real-process test (the host spawning the
//! real `agentic-org-web`) needs the built web binary and is added separately.
use super::*;
use crate::service::listener::Connection;
use std::os::fd::OwnedFd;

struct NoOps;
impl Handler for NoOps {
    fn handle(&self, _: org_proto::Request, _: &Arc<Connection>, _: crate::peer::Received) {}
}

fn loopback() -> ListenConfig {
    serde_json::from_str(
        r#"{"address":"127.0.0.1","port":48123,"scheme":"http_loopback","origin":"http://127.0.0.1:48123","hsts":"off"}"#,
    )
    .unwrap()
}

fn wait_for(what: &str, mut done: impl FnMut() -> bool) {
    let until = Instant::now() + Duration::from_secs(5);
    while !done() {
        assert!(Instant::now() < until, "timed out waiting for {what}");
        thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn refused_gateway_start_never_becomes_ready() {
    let dir = tempfile::tempdir().unwrap();
    let state = Arc::new(Mutex::new(GatewayState::NotConfigured));
    let started = Supervisor::start(
        Configuration {
            program: dir.path().join("agentic-org-web"),
            instance: "instance".into(),
            runtime: dir.path().to_owned(),
            listen: loopback(),
            check: ServerCheck::gateway_for_build(false, None).unwrap(),
            critical_paths: vec![],
            release: None,
        },
        Arc::new(NoOps),
        Arc::clone(&state),
        Arc::new(|| true),
    );
    let Err(error) = started else {
        panic!("a release build without a team never spawns")
    };
    // The log names the real cause and that it is the gateway's check.
    let message = error.to_string();
    assert!(message.contains("browser gateway"), "{message}");
    assert!(message.contains("no app team id"), "{message}");
    let observed = *lock(&state);
    assert_ne!(observed, GatewayState::Running);
    assert!(!crate::service::gateway_serving(true, true, observed));
}

#[test]
fn an_admission_refusal_inside_start_keeps_its_cause() {
    let dir = tempfile::tempdir().unwrap();
    let config = Configuration {
        program: dir.path().join("agentic-org-web"),
        instance: "instance".into(),
        runtime: dir.path().to_owned(),
        listen: loopback(),
        check: ServerCheck::gateway_for_build(false, None).unwrap(),
        critical_paths: vec![],
        release: None,
    };
    let Err(error) = Running::start(&config, Arc::new(NoOps)) else {
        panic!("a release build without a team never spawns")
    };
    // The supervisor logs this text, so the refusal cause must survive.
    let message = error.to_string();
    assert!(message.contains("gateway admission refused"), "{message}");
    assert!(message.contains("no app team id"), "{message}");
}

/// A started gateway: `live` is the child/control state, `reported` its `gateway.serving`.
struct Fake {
    live: Arc<AtomicBool>,
    reported: Arc<AtomicBool>,
    dropped: Arc<AtomicBool>,
}
impl Fake {
    fn reporting(live: &Arc<AtomicBool>) -> Self {
        Self {
            live: Arc::clone(live),
            reported: Arc::new(AtomicBool::new(true)),
            dropped: Arc::new(AtomicBool::new(false)),
        }
    }
}
impl Serving for Fake {
    fn serving(&mut self) -> bool {
        self.live.load(Ordering::SeqCst)
    }
    fn reported(&mut self) -> bool {
        self.reported.load(Ordering::SeqCst)
    }
}
impl Drop for Fake {
    fn drop(&mut self) {
        self.dropped.store(true, Ordering::SeqCst);
    }
}

#[test]
fn running_is_published_only_after_the_gateway_reports_serving() {
    let stopping = Arc::new(AtomicBool::new(false));
    let state = Arc::new(Mutex::new(GatewayState::NotConfigured));
    let live = Arc::new(AtomicBool::new(true));
    let reported = Arc::new(AtomicBool::new(false));
    let supervisor = {
        let (stopping, state, live, reported) = (
            Arc::clone(&stopping),
            Arc::clone(&state),
            Arc::clone(&live),
            Arc::clone(&reported),
        );
        thread::spawn(move || {
            supervise(&stopping, &state, &|| true, Timing::PRODUCTION, || {
                Ok(Fake {
                    live: Arc::clone(&live),
                    reported: Arc::clone(&reported),
                    dropped: Arc::new(AtomicBool::new(false)),
                })
            })
        })
    };
    wait_for("starting", || *lock(&state) == GatewayState::Starting);
    thread::sleep(Duration::from_millis(200));
    // Started and admitted, but HTTP initialization is not reported yet.
    assert_eq!(*lock(&state), GatewayState::Starting);
    assert!(!crate::service::gateway_serving(true, true, *lock(&state)));
    reported.store(true, Ordering::SeqCst);
    wait_for("running", || *lock(&state) == GatewayState::Running);
    assert!(crate::service::gateway_serving(true, true, *lock(&state)));
    stopping.store(true, Ordering::SeqCst);
    supervisor.thread().unpark();
    supervisor.join().unwrap();
}

#[test]
fn a_gateway_that_never_reports_serving_fails_after_the_deadline_and_is_stopped() {
    let stopping = Arc::new(AtomicBool::new(false));
    let state = Arc::new(Mutex::new(GatewayState::NotConfigured));
    let live = Arc::new(AtomicBool::new(true));
    let dropped = Arc::new(AtomicBool::new(false));
    let observed = Arc::new(Mutex::new(Vec::new()));
    let supervisor = {
        let (stopping, state, live, dropped) = (
            Arc::clone(&stopping),
            Arc::clone(&state),
            Arc::clone(&live),
            Arc::clone(&dropped),
        );
        thread::spawn(move || {
            supervise(
                &stopping,
                &state,
                &|| true,
                Timing {
                    serving: Duration::from_millis(300),
                    ..Timing::PRODUCTION
                },
                || {
                    Ok(Fake {
                        live: Arc::clone(&live),
                        reported: Arc::new(AtomicBool::new(false)),
                        dropped: Arc::clone(&dropped),
                    })
                },
            )
        })
    };
    let started = Instant::now();
    wait_for("failed", || {
        let now = *lock(&state);
        lock(&observed).push(now);
        now == GatewayState::Failed
    });
    assert!(started.elapsed() >= Duration::from_millis(250));
    assert!(!lock(&observed).contains(&GatewayState::Running));
    assert!(!crate::service::gateway_serving(true, true, *lock(&state)));
    // The unreported child is dropped: control closed, then killed and reaped.
    wait_for("child stopped", || dropped.load(Ordering::SeqCst));
    stopping.store(true, Ordering::SeqCst);
    supervisor.thread().unpark();
    supervisor.join().unwrap();
    assert_eq!(*lock(&state), GatewayState::Failed);
}

#[test]
fn readiness_turns_false_when_the_gateway_exits_or_its_control_reaches_eof() {
    let stopping = Arc::new(AtomicBool::new(false));
    let state = Arc::new(Mutex::new(GatewayState::NotConfigured));
    let live = Arc::new(AtomicBool::new(true));
    let starts = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let supervisor = {
        let (stopping, state, live, starts) = (
            Arc::clone(&stopping),
            Arc::clone(&state),
            Arc::clone(&live),
            Arc::clone(&starts),
        );
        thread::spawn(move || {
            supervise(&stopping, &state, &|| true, Timing::PRODUCTION, || {
                starts.fetch_add(1, Ordering::SeqCst);
                Ok(Fake::reporting(&live))
            })
        })
    };
    wait_for("running", || *lock(&state) == GatewayState::Running);
    assert!(crate::service::gateway_serving(true, true, *lock(&state)));
    live.store(false, Ordering::SeqCst); // child exit or control EOF
    wait_for("failed", || *lock(&state) == GatewayState::Failed);
    assert!(!crate::service::gateway_serving(true, true, *lock(&state)));
    stopping.store(true, Ordering::SeqCst);
    supervisor.thread().unpark();
    supervisor.join().unwrap();
    assert_eq!(starts.load(Ordering::SeqCst), 1);
    assert_eq!(*lock(&state), GatewayState::Failed);
}

#[test]
fn an_ineligible_host_never_starts_the_gateway() {
    let stopping = Arc::new(AtomicBool::new(false));
    let state = Arc::new(Mutex::new(GatewayState::NotConfigured));
    let supervisor = {
        let (stopping, state) = (Arc::clone(&stopping), Arc::clone(&state));
        thread::spawn(move || {
            supervise(
                &stopping,
                &state,
                &|| false,
                Timing::PRODUCTION,
                || -> io::Result<Fake> { panic!("started without HQ/authentication") },
            )
        })
    };
    wait_for("starting", || *lock(&state) == GatewayState::Starting);
    thread::sleep(Duration::from_millis(120));
    assert_eq!(*lock(&state), GatewayState::Starting);
    stopping.store(true, Ordering::SeqCst);
    supervisor.thread().unpark();
    supervisor.join().unwrap();
}

#[test]
fn transfer_end_handed_to_the_gateway_cannot_send() {
    let (transfer, child) = transfer_pair().unwrap();
    let error = child.send(b"x").unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::BrokenPipe, "{error}");
    transfer.send(b"record").unwrap();
    let mut bytes = [0u8; 16];
    assert_eq!(child.recv(&mut bytes).unwrap(), 6);
}

#[test]
fn handoff_sends_exactly_the_transfer_end_then_the_listener() {
    let (mut host, gateway) = UnixStream::pair().unwrap();
    let (transfer, child) = transfer_pair().unwrap();
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let port = listener.local_addr().unwrap().port();
    send_handoff(
        &mut host,
        &child,
        &listener,
        Instant::now() + Duration::from_secs(5),
    )
    .unwrap();
    drop(child);
    drop(listener);

    let mut receiver = org_socket::rights::StreamReceiver::new(gateway);
    let mut header = Vec::new();
    loop {
        // Every header byte carries no descriptor (an early descriptor is refused).
        let (byte, fds) = receiver.receive_byte(0, &SPAWN_LOCK).unwrap();
        assert!(fds.is_empty());
        if byte == b'\n' {
            break;
        }
        header.push(byte);
    }
    let named: GatewayHandoff = serde_json::from_slice(&header).unwrap();
    assert_eq!(named, GatewayHandoff);
    let header: serde_json::Value = serde_json::from_slice(&header).unwrap();
    assert_eq!(header["fds"], serde_json::json!(["transfer", "listener"]));

    // Exactly two descriptors ride the marker; any other count is refused.
    let (marker, fds) = receiver
        .receive_byte(GATEWAY_HANDOFF_FDS.len(), &SPAWN_LOCK)
        .unwrap();
    assert_eq!(marker, org_host_control::ops::HOST_SOCK_MARKER);
    let [first, second]: [OwnedFd; 2] = fds.try_into().unwrap();

    // First: the gateway's receive-only transfer end, paired with the host's end.
    let received = UnixDatagram::from(first);
    assert_eq!(
        received.send(b"x").unwrap_err().kind(),
        io::ErrorKind::BrokenPipe
    );
    transfer.send(b"record").unwrap();
    let mut bytes = [0u8; 16];
    assert_eq!(received.recv(&mut bytes).unwrap(), 6);

    // Second: the bound browser listener.
    let listening = TcpListener::from(second);
    assert_eq!(listening.local_addr().unwrap().port(), port);
}

/// Serves (reported) until `lifetime` after its start, then exits.
struct Brief {
    until: Instant,
}
impl Serving for Brief {
    fn serving(&mut self) -> bool {
        Instant::now() < self.until
    }
    fn reported(&mut self) -> bool {
        true
    }
}

#[test]
fn only_consecutive_failed_starts_count_and_a_stable_run_resets_them() {
    let stopping = Arc::new(AtomicBool::new(false));
    let state = Arc::new(Mutex::new(GatewayState::NotConfigured));
    let starts = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let timing = Timing {
        serving: SERVING_DEADLINE,
        stable: Duration::from_millis(100),
        backoff: Duration::from_millis(1),
    };
    let supervisor = {
        let (stopping, state, starts) = (
            Arc::clone(&stopping),
            Arc::clone(&state),
            Arc::clone(&starts),
        );
        thread::spawn(move || {
            supervise(&stopping, &state, &|| true, timing, || {
                starts.fetch_add(1, Ordering::SeqCst);
                // Each run stays Running past `stable`, then dies.
                Ok(Brief {
                    until: Instant::now() + Duration::from_millis(250),
                })
            })
        })
    };
    // Healthy runs that later die are not consecutive failed starts: the
    // supervisor keeps restarting well past the failed-start limit.
    let until = Instant::now() + Duration::from_secs(20);
    while starts.load(Ordering::SeqCst) <= MAX_STARTS + 1 {
        assert!(
            !supervisor.is_finished(),
            "gave up after {} stable runs",
            starts.load(Ordering::SeqCst)
        );
        assert!(Instant::now() < until, "timed out waiting for restarts");
        thread::sleep(Duration::from_millis(10));
    }
    stopping.store(true, Ordering::SeqCst);
    supervisor.thread().unpark();
    supervisor.join().unwrap();
    assert_eq!(*lock(&state), GatewayState::Failed);
}

#[test]
fn a_stable_run_resets_failures_accumulated_before_it() {
    let stopping = AtomicBool::new(false);
    let state = Mutex::new(GatewayState::NotConfigured);
    let starts = std::sync::atomic::AtomicUsize::new(0);
    let timing = Timing {
        serving: SERVING_DEADLINE,
        stable: Duration::from_millis(100),
        backoff: Duration::from_millis(1),
    };
    // MAX_STARTS - 1 failed starts, one stable run, MAX_STARTS - 1 more failed
    // starts (still restarting), then one more failed start: the limit.
    supervise(&stopping, &state, &|| true, timing, || {
        let n = starts.fetch_add(1, Ordering::SeqCst) + 1;
        assert!(n <= 2 * MAX_STARTS, "restarted past the limit");
        if n == MAX_STARTS {
            Ok(Brief {
                until: Instant::now() + Duration::from_millis(250),
            })
        } else {
            Err(io::Error::other("scripted failed start"))
        }
    });
    assert_eq!(starts.load(Ordering::SeqCst), 2 * MAX_STARTS);
    assert_eq!(*lock(&state), GatewayState::Failed);
}

#[test]
fn consecutive_failed_starts_stop_at_the_limit_without_a_final_backoff() {
    let stopping = AtomicBool::new(false);
    let state = Mutex::new(GatewayState::NotConfigured);
    let starts = std::sync::atomic::AtomicUsize::new(0);
    let last = Mutex::new(Instant::now());
    let timing = Timing {
        serving: SERVING_DEADLINE,
        stable: Duration::from_millis(100),
        backoff: Duration::from_millis(40),
    };
    supervise(&stopping, &state, &|| true, timing, || {
        starts.fetch_add(1, Ordering::SeqCst);
        *lock(&last) = Instant::now();
        // Exits before it ever reports serving.
        Ok(Brief {
            until: Instant::now(),
        })
    });
    assert_eq!(starts.load(Ordering::SeqCst), MAX_STARTS);
    assert_eq!(*lock(&state), GatewayState::Failed);
    // The final backoff would be 40 ms << 4 = 640 ms; none follows the last start.
    let tail = lock(&last).elapsed();
    assert!(
        tail < Duration::from_millis(320),
        "slept {tail:?} after the final start"
    );
}

/// Starts a gateway whose child is a zsh script that reads the first line,
/// connects to the rendezvous and reports ready. The scripted admission
/// answers the child's real token when `tokens` (no token otherwise, so the
/// child passes the peer-pid check and reaches the post-ready re-admission),
/// except that the `nth` call (1-based) of `refuse` fails with `cause`.
#[cfg(target_os = "macos")]
fn start_refusing(
    refuse: crate::server_check::Step,
    nth: usize,
    tokens: bool,
    cause: &'static str,
) -> io::Error {
    use crate::server_check::{Scripted, Step};
    use std::os::unix::fs::PermissionsExt;
    use std::sync::atomic::AtomicUsize;
    let dir = tempfile::tempdir().unwrap();
    let program = dir.path().join("agentic-org-web");
    fs::write(
        &program,
        "#!/bin/zsh -f\n\
         IFS= read -r first || exit 1\n\
         p=${first#*\\\"rendezvous\\\":\\\"}\n\
         p=${p%%\\\"*}\n\
         zmodload zsh/net/socket || exit 1\n\
         zsocket \"$p\" || exit 1\n\
         print -r -u $REPLY '{\"v\":1,\"event\":\"ready_for_secrets\"}'\n\
         read -r -t 20 -u $REPLY _\n",
    )
    .unwrap();
    fs::set_permissions(&program, fs::Permissions::from_mode(0o700)).unwrap();
    let port = std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let listen = serde_json::from_str(&format!(
        r#"{{"address":"127.0.0.1","port":{port},"scheme":"http_loopback","origin":"http://127.0.0.1:{port}","hsts":"off"}}"#
    ))
    .unwrap();
    let calls = [AtomicUsize::new(0), AtomicUsize::new(0)];
    let check = ServerCheck::Scripted(Scripted(Arc::new(move |step: Step, pid: u32| {
        let n = calls[usize::from(step == Step::Still)].fetch_add(1, Ordering::SeqCst) + 1;
        if step == refuse && n == nth {
            return Err(cause.to_owned());
        }
        if !tokens {
            return Ok(None);
        }
        crate::spawn::audit_token_of_child(pid)
            .map(Some)
            .map_err(|e| e.to_string())
    })));
    let config = Configuration {
        program,
        instance: "instance".into(),
        runtime: dir.path().to_owned(),
        listen,
        check,
        critical_paths: vec![],
        release: None,
    };
    match Running::start(&config, Arc::new(NoOps)) {
        Err(error) => error,
        Ok(_) => panic!("the scripted check refuses this start"),
    }
}

#[cfg(target_os = "macos")]
#[test]
fn every_admission_refusal_inside_start_keeps_its_cause() {
    use crate::server_check::Step;
    // The post-ready `still` re-check is mapped the same way; reaching it
    // needs a token-matching peer, which the real-process test provides.
    for (step, nth, tokens, cause) in [
        (Step::Admit, 1, true, "admission refused by the test"),
        (
            Step::Still,
            1,
            true,
            "pre-resume re-check refused by the test",
        ),
        (
            Step::Admit,
            2,
            false,
            "post-ready re-admission refused by the test",
        ),
    ] {
        let message = start_refusing(step, nth, tokens, cause).to_string();
        assert!(
            message.contains("gateway admission refused"),
            "{step:?} {nth}: {message}"
        );
        assert!(message.contains(cause), "{step:?} {nth}: {message}");
    }
}
