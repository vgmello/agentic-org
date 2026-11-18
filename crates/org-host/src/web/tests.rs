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

/// A supervisor log sink for tests that do not read the log.
fn quiet(_: &str) {}

fn wait_for(what: &str, mut done: impl FnMut() -> bool) {
    let until = Instant::now() + Duration::from_secs(5);
    while !done() {
        assert!(Instant::now() < until, "timed out waiting for {what}");
        thread::sleep(Duration::from_millis(5));
    }
}

/// A child of `program` that a debug build's check admitted and that has not been let run:
/// it stays alive (on macOS suspended) without running anything, until it is dropped.
fn held_child(program: &str, args: &[&str]) -> org_platform_host::admission::Started {
    let args: Vec<&OsStr> = args.iter().map(OsStr::new).collect();
    let held = ServerCheck::Skip
        .admission(Path::new(program))
        .unwrap()
        .start(&args, &ChildEnv::new(), &SPAWN_LOCK)
        .unwrap();
    org_platform_host::testing::admission::unreleased(held)
}

#[test]
fn refused_web_start_never_becomes_ready() {
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
    // The log names the real cause and that it is the web process's check.
    let message = error.to_string();
    assert!(message.contains("web process"), "{message}");
    assert!(message.contains("no app team id"), "{message}");
    let observed = *lock(&state);
    assert_ne!(observed, GatewayState::Running);
    assert!(!crate::service::web_serving(true, true, observed));
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
    assert!(
        message.contains("web process admission refused"),
        "{message}"
    );
    assert!(message.contains("no app team id"), "{message}");
}

/// A started web process: `live` is the child/control state, `reported` its `web.serving`.
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
    fn ended(&mut self) -> String {
        "the fake exited".to_owned()
    }
}
impl Drop for Fake {
    fn drop(&mut self) {
        self.dropped.store(true, Ordering::SeqCst);
    }
}

#[test]
fn running_is_published_only_after_the_web_reports_serving() {
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
            supervise(
                &stopping,
                &state,
                &|| true,
                Timing::PRODUCTION,
                &quiet,
                || {
                    Ok(Fake {
                        live: Arc::clone(&live),
                        reported: Arc::clone(&reported),
                        dropped: Arc::new(AtomicBool::new(false)),
                    })
                },
            )
        })
    };
    wait_for("starting", || *lock(&state) == GatewayState::Starting);
    thread::sleep(Duration::from_millis(200));
    // Started and admitted, but HTTP initialization is not reported yet.
    assert_eq!(*lock(&state), GatewayState::Starting);
    assert!(!crate::service::web_serving(true, true, *lock(&state)));
    reported.store(true, Ordering::SeqCst);
    wait_for("running", || *lock(&state) == GatewayState::Running);
    assert!(crate::service::web_serving(true, true, *lock(&state)));
    stopping.store(true, Ordering::SeqCst);
    supervisor.thread().unpark();
    supervisor.join().unwrap();
}

#[test]
fn a_web_that_never_reports_serving_fails_after_the_deadline_and_is_stopped() {
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
                &quiet,
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
    assert!(!crate::service::web_serving(true, true, *lock(&state)));
    // The unreported child is dropped: control closed, then killed and reaped.
    wait_for("child stopped", || dropped.load(Ordering::SeqCst));
    stopping.store(true, Ordering::SeqCst);
    supervisor.thread().unpark();
    supervisor.join().unwrap();
    assert_eq!(*lock(&state), GatewayState::Failed);
}

#[test]
fn readiness_turns_false_when_the_web_exits_or_its_control_reaches_eof() {
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
            supervise(
                &stopping,
                &state,
                &|| true,
                Timing::PRODUCTION,
                &quiet,
                || {
                    starts.fetch_add(1, Ordering::SeqCst);
                    Ok(Fake::reporting(&live))
                },
            )
        })
    };
    wait_for("running", || *lock(&state) == GatewayState::Running);
    assert!(crate::service::web_serving(true, true, *lock(&state)));
    live.store(false, Ordering::SeqCst); // child exit or control EOF
    wait_for("failed", || *lock(&state) == GatewayState::Failed);
    assert!(!crate::service::web_serving(true, true, *lock(&state)));
    stopping.store(true, Ordering::SeqCst);
    supervisor.thread().unpark();
    supervisor.join().unwrap();
    assert_eq!(starts.load(Ordering::SeqCst), 1);
    assert_eq!(*lock(&state), GatewayState::Failed);
}

/// HQ leaving Active or authentication going away while the web process runs stops it
/// with a logged reason. It starts again once eligible: no failure is counted and no
/// backoff is waited (the 30-second backoff would exceed `wait_for`'s deadline).
#[test]
fn a_running_web_stops_when_eligibility_ends_and_restarts_once_eligible() {
    let stopping = Arc::new(AtomicBool::new(false));
    let state = Arc::new(Mutex::new(GatewayState::NotConfigured));
    let eligible = Arc::new(AtomicBool::new(true));
    let live = Arc::new(AtomicBool::new(true));
    let dropped = Arc::new(AtomicBool::new(false));
    let starts = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let logged = Arc::new(Mutex::new(Vec::<String>::new()));
    let supervisor = {
        let (stopping, state, eligible, live, dropped, starts, logged) = (
            Arc::clone(&stopping),
            Arc::clone(&state),
            Arc::clone(&eligible),
            Arc::clone(&live),
            Arc::clone(&dropped),
            Arc::clone(&starts),
            Arc::clone(&logged),
        );
        thread::spawn(move || {
            supervise(
                &stopping,
                &state,
                &|| eligible.load(Ordering::SeqCst),
                Timing {
                    backoff: Duration::from_secs(30),
                    ..Timing::PRODUCTION
                },
                &|line| lock(&logged).push(line.to_owned()),
                || {
                    starts.fetch_add(1, Ordering::SeqCst);
                    Ok(Fake {
                        live: Arc::clone(&live),
                        reported: Arc::new(AtomicBool::new(true)),
                        dropped: Arc::clone(&dropped),
                    })
                },
            )
        })
    };
    wait_for("running", || *lock(&state) == GatewayState::Running);
    eligible.store(false, Ordering::SeqCst);
    wait_for("child stopped", || dropped.load(Ordering::SeqCst));
    wait_for("not serving", || *lock(&state) == GatewayState::Starting);
    assert!(!crate::service::web_serving(true, true, *lock(&state)));
    thread::sleep(Duration::from_millis(150));
    assert_eq!(
        starts.load(Ordering::SeqCst),
        1,
        "restarted while ineligible"
    );
    assert_eq!(*lock(&state), GatewayState::Starting);
    eligible.store(true, Ordering::SeqCst);
    wait_for("running again", || *lock(&state) == GatewayState::Running);
    assert_eq!(starts.load(Ordering::SeqCst), 2);
    stopping.store(true, Ordering::SeqCst);
    supervisor.thread().unpark();
    supervisor.join().unwrap();
    let logged = lock(&logged).clone();
    assert_eq!(
        logged,
        ["authenticated browser stopped because the host is no longer eligible; it starts again once eligible"],
        "no failure or backoff was logged"
    );
}

#[test]
fn an_ineligible_host_never_starts_the_web() {
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
                &quiet,
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
fn transfer_end_handed_to_the_web_cannot_send() {
    let (transfer, child) = transfer_pair().unwrap();
    let error = child.send(b"x").unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::BrokenPipe, "{error}");
    transfer.send(b"record").unwrap();
    let mut bytes = [0u8; 16];
    assert_eq!(child.recv(&mut bytes).unwrap(), 6);
}

#[test]
fn handoff_sends_exactly_the_transfer_end_then_the_listener() {
    let (mut host, web) = UnixStream::pair().unwrap();
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

    let mut receiver = org_socket::rights::StreamReceiver::new(web);
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

    // First: the web process's receive-only transfer end, paired with the host's end.
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
    fn ended(&mut self) -> String {
        "the brief run ended".to_owned()
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
            supervise(&stopping, &state, &|| true, timing, &quiet, || {
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
    supervise(&stopping, &state, &|| true, timing, &quiet, || {
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
fn a_stable_run_ended_by_ineligibility_also_resets_failures() {
    const STOPPED: &str = "authenticated browser stopped because the host is no longer eligible; it starts again once eligible";
    let stopping = AtomicBool::new(false);
    let state = Mutex::new(GatewayState::NotConfigured);
    let starts = std::sync::atomic::AtomicUsize::new(0);
    let eligible = AtomicBool::new(true);
    let (lines, record) = recording();
    let timing = Timing {
        serving: SERVING_DEADLINE,
        stable: Duration::from_millis(100),
        backoff: Duration::from_millis(1),
    };
    // MAX_STARTS - 1 failed starts, one run that stays Running past `stable`
    // and is then stopped because the host is no longer eligible, then
    // MAX_STARTS - 1 more failed starts (still restarting), then the limit.
    // The run never ends on its own, so only the eligibility stop can reset.
    thread::scope(|scope| {
        scope.spawn(|| {
            let wait = |what: &str, done: &dyn Fn() -> bool| {
                let until = Instant::now() + Duration::from_secs(20);
                while !done() {
                    assert!(Instant::now() < until, "timed out waiting for {what}");
                    thread::sleep(Duration::from_millis(5));
                }
            };
            wait("the stable run", &|| *lock(&state) == GatewayState::Running);
            thread::sleep(Duration::from_millis(300)); // past `stable`
            eligible.store(false, Ordering::SeqCst);
            wait("the eligibility stop", &|| {
                *lock(&state) == GatewayState::Starting
            });
            eligible.store(true, Ordering::SeqCst);
        });
        supervise(
            &stopping,
            &state,
            &|| eligible.load(Ordering::SeqCst),
            timing,
            &record,
            || {
                let n = starts.fetch_add(1, Ordering::SeqCst) + 1;
                assert!(n <= 2 * MAX_STARTS, "restarted past the limit");
                if n == MAX_STARTS {
                    Ok(Brief {
                        until: Instant::now() + Duration::from_secs(3600),
                    })
                } else {
                    Err(io::Error::other("scripted failed start"))
                }
            },
        );
    });
    assert_eq!(starts.load(Ordering::SeqCst), 2 * MAX_STARTS);
    assert_eq!(*lock(&state), GatewayState::Failed);
    let lines = lock(&lines);
    assert_eq!(
        lines.iter().filter(|l| l.as_str() == STOPPED).count(),
        1,
        "{lines:?}"
    );
    assert!(
        !lines
            .iter()
            .any(|l| l.contains("running web process stopped")),
        "{lines:?}"
    );
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
    supervise(&stopping, &state, &|| true, timing, &quiet, || {
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

/// A child script that reads the first line, connects to the rendezvous and
/// sends `ready` as its ready line, then waits.
#[cfg(target_os = "macos")]
fn connecting_script(ready: &str) -> String {
    format!(
        "#!/bin/zsh -f\n\
         IFS= read -r first || exit 1\n\
         p=${{first#*\\\"rendezvous\\\":\\\"}}\n\
         p=${{p%%\\\"*}}\n\
         zmodload zsh/net/socket || exit 1\n\
         zsocket \"$p\" || exit 1\n\
         print -r -u $REPLY '{ready}'\n\
         read -r -t 20 -u $REPLY _\n"
    )
}

/// The ready line a well-behaved web process sends.
#[cfg(target_os = "macos")]
const READY: &str = r#"{"v":1,"event":"ready_for_secrets"}"#;

/// Starts a web process whose child runs `script`. The scripted admission answers
/// `answer(step, nth, pid)`, with `nth` counting calls of that step (1-based).
#[cfg(target_os = "macos")]
fn start_scripted(
    script: &str,
    answer: impl Fn(crate::server_check::Step, usize, u32) -> Result<Option<crate::peer::AuditToken>, String>
        + Send
        + Sync
        + 'static,
) -> io::Error {
    let (_dir, config) = scripted_config(script, answer);
    start_error(&config)
}

#[cfg(target_os = "macos")]
/// Starts `config`, which must be refused, and returns the refusal.
fn start_error(config: &Configuration) -> io::Error {
    match Running::start(config, Arc::new(NoOps)) {
        Err(error) => error,
        Ok(_) => panic!("this start must be refused"),
    }
}

#[cfg(target_os = "macos")]
/// A loopback listen configuration on `port`.
fn loopback_on(port: u16) -> ListenConfig {
    serde_json::from_str(&format!(
        r#"{{"address":"127.0.0.1","port":{port},"scheme":"http_loopback","origin":"http://127.0.0.1:{port}","hsts":"off"}}"#
    ))
    .unwrap()
}

/// The configuration [`start_scripted`] starts, on a free loopback port, with its
/// temporary directory (program and runtime) kept alive by the caller.
#[cfg(target_os = "macos")]
fn scripted_config(
    script: &str,
    answer: impl Fn(crate::server_check::Step, usize, u32) -> Result<Option<crate::peer::AuditToken>, String>
        + Send
        + Sync
        + 'static,
) -> (tempfile::TempDir, Configuration) {
    use crate::server_check::{Scripted, Step};
    use std::os::unix::fs::PermissionsExt;
    use std::sync::atomic::AtomicUsize;
    let dir = tempfile::tempdir().unwrap();
    let program = dir.path().join("agentic-org-web");
    fs::write(&program, script).unwrap();
    fs::set_permissions(&program, fs::Permissions::from_mode(0o700)).unwrap();
    let port = std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let listen = loopback_on(port);
    let calls = [AtomicUsize::new(0), AtomicUsize::new(0)];
    let check = ServerCheck::Scripted(Scripted(Arc::new(move |step: Step, pid: u32| {
        let n = calls[usize::from(step == Step::Still)].fetch_add(1, Ordering::SeqCst) + 1;
        answer(step, n, pid)
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
    (dir, config)
}

/// Starts a scripted web process whose admission answers the child's real token when
/// `tokens` (no token otherwise, so the child passes the peer-pid check and
/// reaches the post-ready re-admission), except that the `nth` call (1-based)
/// of `refuse` fails with `cause`.
#[cfg(target_os = "macos")]
fn start_refusing(
    refuse: crate::server_check::Step,
    nth: usize,
    tokens: bool,
    cause: &'static str,
) -> io::Error {
    start_scripted(&connecting_script(READY), move |step, n, pid| {
        if step == refuse && n == nth {
            return Err(cause.to_owned());
        }
        if !tokens {
            return Ok(None);
        }
        org_platform_host::testing::admission::child_peer(pid)
            .map(|peer| Some(peer.macos_audit_token))
            .map_err(|e| e.to_string())
    })
}

#[cfg(target_os = "macos")]
#[test]
fn a_changed_identity_at_re_admission_names_that_cause() {
    use crate::server_check::Step;
    // Admitted with no token; re-admission after the ready line returns one.
    let error = start_scripted(&connecting_script(READY), |step, n, pid| {
        if step == Step::Admit && n == 2 {
            org_platform_host::testing::admission::child_peer(pid)
                .map(|peer| Some(peer.macos_audit_token))
                .map_err(|e| e.to_string())
        } else {
            Ok(None)
        }
    });
    let message = error.to_string();
    assert!(
        message.contains("web process admission refused at re-admission: the identity changed"),
        "{message}"
    );
}

#[cfg(target_os = "macos")]
#[test]
fn a_mismatched_ready_line_names_that_cause() {
    let error = start_scripted(
        &connecting_script(r#"{"v":1,"event":"other"}"#),
        |_, _, _| Ok(None),
    );
    let message = error.to_string();
    assert!(
        message.contains("the ready line does not match ready_for_secrets"),
        "{message}"
    );
}

#[cfg(target_os = "macos")]
#[test]
fn a_ready_line_reporting_landlock_on_macos_names_that_cause() {
    let error = start_scripted(
        &connecting_script(
            r#"{"v":1,"event":"ready_for_secrets","landlock":{"abi":4,"enforcement":"fully_enforced"}}"#,
        ),
        |_, _, _| Ok(None),
    );
    let message = error.to_string();
    assert!(
        message.contains("web process start failed: the ready line reports landlock on macOS"),
        "{message}"
    );
}

#[test]
fn only_the_exact_handoff_ack_is_accepted() {
    check_ack(br#"{"v":1,"event":"fds.received","of":"handoff"}"#).unwrap();
    for malformed in [
        &b"not json"[..],
        br#"{"v":1,"event":"fds.received"}"#,
        br#"{"v":1,"event":"fds.received","of":"handoff","extra":1}"#,
        br#"{"v":"1","event":"fds.received","of":"handoff"}"#,
    ] {
        let message = check_ack(malformed).unwrap_err().to_string();
        assert_eq!(
            message, "web process start failed: the handoff ACK is malformed",
            "{malformed:?}"
        );
    }
    for wrong in [
        &br#"{"v":2,"event":"fds.received","of":"handoff"}"#[..],
        br#"{"v":1,"event":"ready_for_secrets","of":"handoff"}"#,
        br#"{"v":1,"event":"fds.received","of":"hello"}"#,
    ] {
        let message = check_ack(wrong).unwrap_err().to_string();
        assert_eq!(
            message, "web process start failed: the handoff ACK does not acknowledge the handoff",
            "{wrong:?}"
        );
    }
}

#[test]
fn a_fatal_receive_logs_its_step_and_error_kind_escaped() {
    let line = fatal_receive("handoff ACK", io::ErrorKind::InvalidData);
    assert_eq!(
        line,
        "web process start failed fatally while receiving the handoff ACK (InvalidData); the host exits"
    );
    // A step label is fixed text, but the line is escaped all the same.
    let forged = fatal_receive("ready line\nagentic-org: forged", io::ErrorKind::Other);
    assert!(!forged.contains('\n'), "{forged}");
}

#[cfg(target_os = "macos")]
#[test]
fn a_child_that_exits_before_connecting_names_its_status() {
    let error = start_scripted("#!/bin/zsh -f\nIFS= read -r first\nexit 3\n", |_, _, _| {
        Ok(None)
    });
    let message = error.to_string();
    assert!(
        message.contains("the child exited before connecting (exit code 3)"),
        "{message}"
    );
}

/// Collects supervisor log lines.
fn recording() -> (Arc<Mutex<Vec<String>>>, impl Fn(&str) + Send + 'static) {
    let lines = Arc::new(Mutex::new(Vec::new()));
    let sink = Arc::clone(&lines);
    (lines, move |line: &str| lock(&sink).push(line.to_owned()))
}

#[test]
fn the_exit_of_a_running_web_is_logged_with_its_reason_and_restart() {
    let stopping = Arc::new(AtomicBool::new(false));
    let state = Arc::new(Mutex::new(GatewayState::NotConfigured));
    let live = Arc::new(AtomicBool::new(true));
    let (lines, log) = recording();
    let supervisor = {
        let (stopping, state, live) =
            (Arc::clone(&stopping), Arc::clone(&state), Arc::clone(&live));
        thread::spawn(move || {
            let mut first = true;
            supervise(
                &stopping,
                &state,
                &|| true,
                Timing::PRODUCTION,
                &log,
                || {
                    if std::mem::take(&mut first) {
                        Ok(Fake::reporting(&live))
                    } else {
                        Err(io::Error::other("second start"))
                    }
                },
            )
        })
    };
    wait_for("running", || *lock(&state) == GatewayState::Running);
    live.store(false, Ordering::SeqCst);
    wait_for("restart logged", || !lock(&lines).is_empty());
    stopping.store(true, Ordering::SeqCst);
    supervisor.thread().unpark();
    supervisor.join().unwrap();
    let logged = lock(&lines).clone();
    assert_eq!(
        logged,
        ["authenticated browser restarting in 1s because the running web process stopped: the fake exited"],
    );
}

#[test]
fn every_failed_start_is_logged_escaped_with_its_restart_until_the_limit() {
    let stopping = AtomicBool::new(false);
    let state = Mutex::new(GatewayState::NotConfigured);
    let (lines, log) = recording();
    let timing = Timing {
        serving: Duration::from_millis(50),
        stable: Duration::from_secs(60),
        backoff: Duration::from_millis(1),
    };
    let mut n = 0;
    supervise(&stopping, &state, &|| true, timing, &log, || {
        n += 1;
        match n {
            // Never reports serving: the deadline.
            1 => Ok(Fake {
                live: Arc::new(AtomicBool::new(true)),
                reported: Arc::new(AtomicBool::new(false)),
                dropped: Arc::new(AtomicBool::new(false)),
            }),
            // Exits before reporting serving.
            2 => Ok(Fake {
                live: Arc::new(AtomicBool::new(false)),
                reported: Arc::new(AtomicBool::new(false)),
                dropped: Arc::new(AtomicBool::new(false)),
            }),
            _ => Err(io::Error::other("refused\nforged line")),
        }
    });
    let logged = lock(&lines).clone();
    assert_eq!(logged.len(), MAX_STARTS, "{logged:?}");
    assert!(logged[0].ends_with("because it did not report web.serving before the deadline"));
    assert!(logged[1].ends_with("because it stopped before serving: the fake exited"));
    assert!(
        logged[2].ends_with("because refused\\nforged line"),
        "{logged:?}"
    );
    assert!(logged.iter().all(|line| !line.contains('\n')));
    assert_eq!(
        logged[MAX_STARTS - 1],
        format!("authenticated browser restart limit reached after {MAX_STARTS} failed starts; last: refused\\nforged line")
    );
}

#[cfg(target_os = "macos")]
#[test]
fn every_admission_refusal_inside_start_keeps_its_cause() {
    use crate::server_check::Step;
    // With real tokens the zsh child itself is the rendezvous peer, so the
    // second `still` call is the post-ready re-check after the peer check.
    for (step, nth, tokens, cause) in [
        (Step::Admit, 1, true, "admission refused by the test"),
        (
            Step::Still,
            1,
            true,
            "pre-resume re-check refused by the test",
        ),
        (
            Step::Still,
            2,
            true,
            "post-ready re-check refused by the test",
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
            message.contains("web process admission refused"),
            "{step:?} {nth}: {message}"
        );
        assert!(message.contains(cause), "{step:?} {nth}: {message}");
        if (step, nth) == (Step::Still, 2) {
            assert!(
                message.contains("the post-ready re-check"),
                "{step:?} {nth}: {message}"
            );
        }
    }
}

#[test]
fn a_running_web_names_why_it_ended() {
    let exit = || Some("exit status: 3".to_owned());
    assert_eq!(
        ended_cause(true, None, false),
        "the host refused its web.serving as a protocol error"
    );
    // The refusal closes the connection; its EOF and exit must not hide it.
    assert_eq!(
        ended_cause(true, exit(), false),
        "the host refused its web.serving as a protocol error"
    );
    assert_eq!(
        ended_cause(false, exit(), false),
        "the child exited (exit status: 3)"
    );
    assert_eq!(
        ended_cause(false, None, false),
        "its control connection reached EOF"
    );
    assert_eq!(ended_cause(false, None, true), "it stopped serving");
}

#[cfg(target_os = "macos")]
/// A free loopback port nothing listens on.
fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

#[cfg(target_os = "macos")]
#[test]
fn the_supervisor_refuses_a_listener_that_is_not_http_loopback() {
    let dir = tempfile::tempdir().unwrap();
    let state = Arc::new(Mutex::new(GatewayState::NotConfigured));
    let https = serde_json::from_str(
        r#"{"address":"127.0.0.1","port":48124,"scheme":"https","origin":"https://127.0.0.1:48124","hsts":"off"}"#,
    )
    .unwrap();
    // A check that would admit anything: only the listener gate can refuse.
    let started = Supervisor::start(
        Configuration {
            program: dir.path().join("agentic-org-web"),
            instance: "instance".into(),
            runtime: dir.path().to_owned(),
            listen: https,
            check: ServerCheck::gateway_for_build(true, None).unwrap(),
            critical_paths: vec![],
            release: None,
        },
        Arc::new(NoOps),
        Arc::clone(&state),
        Arc::new(|| true),
    );
    let Err(error) = started else {
        panic!("an HTTPS listener never starts the web process")
    };
    assert_eq!(
        error.to_string(),
        "the web process requires a verified HTTP loopback listener"
    );
    assert_eq!(*lock(&state), GatewayState::NotConfigured);
}

#[cfg(not(target_os = "macos"))]
#[test]
fn the_supervisor_refuses_a_platform_without_a_confinement_adapter() {
    let dir = tempfile::tempdir().unwrap();
    let state = Arc::new(Mutex::new(GatewayState::NotConfigured));
    let started = Supervisor::start(
        Configuration {
            program: dir.path().join("agentic-org-web"),
            instance: "instance".into(),
            runtime: dir.path().to_owned(),
            listen: loopback(),
            check: ServerCheck::gateway_for_build(true, None).unwrap(),
            critical_paths: vec![],
            release: None,
        },
        Arc::new(NoOps),
        Arc::clone(&state),
        Arc::new(|| true),
    );
    let Err(error) = started else {
        panic!("no web process without a confinement adapter")
    };
    assert!(
        error
            .to_string()
            .contains("has no adapter on this platform"),
        "{error}"
    );
}

/// A receiver over one end of a fresh stream pair, and the other (sending) end.
fn control_pair() -> (org_socket::rights::StreamReceiver, UnixStream) {
    let (ours, theirs) = UnixStream::pair().unwrap();
    (org_socket::rights::StreamReceiver::new(ours), theirs)
}

#[test]
fn a_control_line_is_read_through_its_newline_up_to_1023_bytes() {
    let (mut receiver, mut peer) = control_pair();
    let full = vec![b'a'; 1023];
    peer.write_all(&full).unwrap();
    peer.write_all(b"\nnext").unwrap();
    let until = Instant::now() + Duration::from_secs(5);
    assert_eq!(line(&mut receiver, until, "ready line").unwrap(), full);
}

#[test]
fn a_control_line_longer_than_1023_bytes_is_refused() {
    let (mut receiver, mut peer) = control_pair();
    peer.write_all(&[b'a'; 1024]).unwrap();
    let until = Instant::now() + Duration::from_secs(5);
    let error = line(&mut receiver, until, "ready line").unwrap_err();
    assert_eq!(
        error.to_string(),
        "web process start failed: the ready line exceeds 1023 bytes"
    );
}

#[test]
fn a_control_line_that_does_not_arrive_before_the_deadline_is_refused() {
    let (mut receiver, mut peer) = control_pair();
    peer.write_all(b"partial").unwrap();
    let started = Instant::now();
    let until = started + Duration::from_millis(100);
    let error = line(&mut receiver, until, "handoff ACK").unwrap_err();
    assert_eq!(
        error.to_string(),
        "web process start failed: the handoff ACK did not arrive before the deadline"
    );
    assert!(started.elapsed() < Duration::from_secs(4));
    drop(peer);
}

#[test]
fn control_eof_before_a_line_is_refused() {
    let (mut receiver, mut peer) = control_pair();
    peer.write_all(b"partial").unwrap();
    drop(peer);
    let until = Instant::now() + Duration::from_secs(5);
    let error = line(&mut receiver, until, "ready line").unwrap_err();
    assert_eq!(
        error.to_string(),
        "web process start failed: control EOF before the ready line"
    );
}

#[test]
fn a_rendezvous_folder_that_is_a_symlink_is_refused_before_binding() {
    let dir = tempfile::tempdir().unwrap();
    let elsewhere = dir.path().join("elsewhere");
    fs::DirBuilder::new()
        .mode(0o700)
        .create(&elsewhere)
        .unwrap();
    std::os::unix::fs::symlink(&elsewhere, dir.path().join("r")).unwrap();
    let Err(error) = Rendezvous::bind(dir.path()) else {
        panic!("a symlinked rendezvous folder was accepted");
    };
    assert_eq!(error.kind(), io::ErrorKind::Other);
    assert_eq!(
        error.to_string(),
        "authenticated browser service unavailable"
    );
    // Nothing was bound through the symlink, and the symlink target is unchanged.
    assert_eq!(fs::read_dir(&elsewhere).unwrap().count(), 0);
    assert_eq!(
        fs::metadata(&elsewhere).unwrap().permissions().mode() & 0o777,
        0o700
    );
}

#[cfg(target_os = "macos")]
#[test]
fn a_child_that_does_not_connect_before_the_deadline_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let rendezvous = Rendezvous::bind(dir.path()).unwrap();
    let mut spawned = held_child("/bin/sleep", &["30"]);
    // The deadline has passed: no connection is accepted, and the child still runs.
    let error = rendezvous
        .accept(&mut spawned.child, Instant::now())
        .unwrap_err();
    assert_eq!(
        error.to_string(),
        "web process start failed: the child did not connect before the deadline"
    );
}

#[cfg(target_os = "macos")]
#[test]
fn a_rendezvous_peer_other_than_the_admitted_child_is_refused() {
    // A forked subshell, not the spawned child, connects and sends the ready line.
    let script = connecting_script(READY)
        .replacen("zsocket", "( zsocket", 1)
        .replace(
            "read -r -t 20 -u $REPLY _\n",
            // A trailing command keeps zsh from exec-ing the subshell in place.
            "read -r -t 20 -u $REPLY _ )\nexit 0\n",
        );
    assert!(
        script.contains("( zsocket") && script.contains("_ )"),
        "{script}"
    );
    let error = start_scripted(&script, |_, _, _| Ok(None));
    let message = error.to_string();
    assert!(
        message.contains("web process start failed: the rendezvous peer is not the admitted child"),
        "{message}"
    );
}

#[cfg(target_os = "macos")]
#[test]
fn an_unconfined_child_fails_the_sandbox_verification() {
    // Admitted with its real token; every check passes until the sandbox check. A
    // non-empty critical path, so the refusal is the child's missing confinement,
    // not the empty path list.
    let (dir, mut config) = scripted_config(&connecting_script(READY), |_, _, pid| {
        org_platform_host::testing::admission::child_peer(pid)
            .map(|peer| Some(peer.macos_audit_token))
            .map_err(|e| e.to_string())
    });
    config.critical_paths = vec![fs::canonicalize(dir.path()).unwrap()];
    let error = start_error(&config);
    let message = error.to_string();
    assert!(
        message.contains("web process start failed: verify the web process sandbox"),
        "{message}"
    );
}

#[cfg(target_os = "macos")]
#[test]
fn each_failed_start_step_before_the_child_connects_names_its_cause() {
    let script = connecting_script(READY);
    // The browser port is taken.
    let (_dir, mut config) = scripted_config(&script, |_, _, _| Ok(None));
    let taken = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    config.listen = loopback_on(taken.local_addr().unwrap().port());
    let message = start_error(&config).to_string();
    assert!(
        message.starts_with("web process start failed: bind the browser listener ("),
        "{message}"
    );
    // The runtime directory is a file, so its rendezvous folder cannot be created.
    let (dir, mut config) = scripted_config(&script, |_, _, _| Ok(None));
    config.listen = loopback_on(free_port());
    let file = dir.path().join("not-a-directory");
    fs::write(&file, b"").unwrap();
    config.runtime = file;
    assert_eq!(
        start_error(&config).to_string(),
        "web process start failed: bind the rendezvous (NotADirectory)"
    );
    // The rendezvous folder already exists as a symlink, so the owner-only check
    // refuses it (mkdir alone would accept the existing name and bind through it).
    let (dir, mut config) = scripted_config(&script, |_, _, _| Ok(None));
    config.listen = loopback_on(free_port());
    let runtime = dir.path().join("runtime");
    let elsewhere = dir.path().join("elsewhere");
    fs::DirBuilder::new().mode(0o700).create(&runtime).unwrap();
    fs::DirBuilder::new()
        .mode(0o700)
        .create(&elsewhere)
        .unwrap();
    std::os::unix::fs::symlink(&elsewhere, runtime.join("r")).unwrap();
    config.runtime = runtime;
    assert_eq!(
        start_error(&config).to_string(),
        "web process start failed: bind the rendezvous (Other)"
    );
    assert_eq!(fs::read_dir(&elsewhere).unwrap().count(), 0);
    // The program is missing.
    let (dir, mut config) = scripted_config(&script, |_, _, _| Ok(None));
    config.program = dir.path().join("missing");
    let message = start_error(&config).to_string();
    assert!(
        message.starts_with("web process start failed: spawn the web process suspended ("),
        "{message}"
    );
    // The first line would not fit in 1023 bytes.
    let (_dir, mut config) = scripted_config(&script, |_, _, _| Ok(None));
    config.instance = "i".repeat(1024);
    assert_eq!(
        start_error(&config).to_string(),
        "web process start failed: the first line exceeds 1023 bytes"
    );
}

/// The supervisor's exit cause for an admitted web process comes from the live `Running`
/// inputs: the control connection's `web.serving` refusal, the reaped child's
/// status and the control EOF, each read at the `Running::ended` call site.
#[test]
fn running_ended_reads_the_refusal_the_reaped_child_and_the_control_eof() {
    use org_host_control::ops::WebServing;
    use org_proto::frame;
    let home = tempfile::tempdir().unwrap();
    let (handler, offices) = crate::service::tests::web_handler(home.path());
    let running = |program: &str, resume: bool| {
        let (host, peer) = UnixStream::pair().unwrap();
        peer.set_read_timeout(Some(Duration::from_secs(10)))
            .unwrap();
        let (transfer, _child) = UnixDatagram::pair().unwrap();
        let control = WebConnection::start(host, transfer, loopback(), handler.clone()).unwrap();
        let spawned = if resume {
            ServerCheck::Skip
                .admission(Path::new(program))
                .unwrap()
                .spawn(&[], &ChildEnv::new(), &SPAWN_LOCK)
                .unwrap()
        } else {
            held_child(program, &[])
        };
        let running = Running {
            control,
            process: spawned.child,
            _stdin: spawned.stdin,
        };
        (running, io::BufReader::new(peer))
    };
    let call = |peer: &mut io::BufReader<UnixStream>, request: org_proto::Request| {
        frame::write_message(peer.get_mut(), &request).unwrap();
        frame::read_message::<org_proto::Response>(peer)
            .ok()
            .flatten()
    };

    // A refused web.serving wins over the EOF it causes; the child still runs.
    let (mut refused, mut peer) = running("/bin/sleep", false);
    let hello = org_proto::Request::new::<org_host_control::Hello>(1, &Default::default());
    call(&mut peer, hello.unwrap()).expect("hello answered");
    let early = org_proto::Request::new::<WebServing>(2, &Default::default()).unwrap();
    call(&mut peer, early);
    wait_for("refusal closes control", || !refused.serving());
    assert!(matches!(refused.process.try_wait(), Ok(false)));
    assert_eq!(
        refused.ended(),
        "the host refused its web.serving as a protocol error"
    );

    // A reaped child's status, while the control connection is still open.
    let (mut exited, _peer) = running("/usr/bin/false", true);
    wait_for("the child exits", || !exited.serving());
    assert!(exited.control.live());
    assert_eq!(exited.ended(), "the child exited (exit code 1)");

    // Control EOF while the (suspended) child still runs.
    let (mut closed, peer) = running("/bin/sleep", false);
    assert!(closed.serving());
    drop(peer);
    wait_for("control EOF", || !closed.serving());
    assert!(matches!(closed.process.try_wait(), Ok(false)));
    assert_eq!(closed.ended(), "its control connection reached EOF");
    drop((refused, exited, closed));
    offices.shutdown();
}

fn binding() -> org_host_control::gateway::WebRelease {
    org_host_control::gateway::WebRelease {
        release_id: "release".into(),
        manifest_sha256: "ab".repeat(32),
        sequence: 7,
    }
}

/// The pin a selected release names for its web process: both of the manifest's identities.
fn pin(cdhash: [u8; 20]) -> crate::server_check::Pin {
    crate::server_check::Pin {
        sha256: [9; 32],
        macos_cdhash: Some(cdhash),
    }
}

#[test]
fn a_release_build_web_admission_pins_the_selected_image_and_binds_its_release() {
    const TEAM: &str = "AB12CD34EF";
    let hash = pin([7u8; 20]);
    let web = ServerCheck::gateway_for_build(false, Some(TEAM)).unwrap();
    assert!(matches!(web, ServerCheck::Gateway(_)));
    // A selected release pins its exact web process image and binds its identity.
    let (check, release) = admission::<()>(false, Some(TEAM), Some((Ok(hash), binding())));
    assert_eq!(check, web.clone().pinned(hash));
    assert_eq!(release, Some(binding()));
    // No release: the Developer ID web process check alone, no binding.
    let (check, release) = admission::<()>(false, Some(TEAM), None);
    assert_eq!(check, web);
    assert_eq!(release, None);
    assert!(check.may_spawn().is_ok());
}

#[test]
fn a_release_build_web_admission_refuses_a_missing_image_hash_a_malformed_or_missing_team() {
    const TEAM: &str = "AB12CD34EF";
    // A selected release without a web process image hash refuses every start.
    let (check, release) = admission(false, Some(TEAM), Some((Err(()), binding())));
    assert_eq!(
        check,
        ServerCheck::WebRefused("the selected release has no agentic-org-web image hash")
    );
    assert_eq!(release, Some(binding()));
    assert!(check.may_spawn().is_err());
    // Even in a debug build: the missing hash is never a skipped check.
    let (check, _) = admission(true, None, Some((Err(()), binding())));
    assert!(check.may_spawn().is_err(), "{check:?}");
    // A malformed team id refuses, with or without a release, never a weaker check.
    for release in [None, Some((Ok(pin([7u8; 20])), binding()))] {
        let (check, _) = admission::<()>(false, Some("bad"), release);
        assert!(check.may_spawn().is_err(), "{check:?}");
        assert!(
            check.describe().contains("malformed app team id"),
            "{}",
            check.describe()
        );
    }
    // No team id in a release build refuses.
    let (check, release) = admission::<()>(false, None, None);
    assert!(matches!(check, ServerCheck::WebRefused(_)), "{check:?}");
    assert!(check.may_spawn().is_err());
    assert_eq!(release, None);
}

#[test]
fn a_debug_build_web_admission_skips_the_signature_check_but_still_pins_a_release() {
    let (check, release) = admission::<()>(true, None, None);
    assert_eq!(check, ServerCheck::Skip);
    assert_eq!(release, None);
    let (check, release) =
        admission::<()>(true, Some("bad"), Some((Ok(pin([7u8; 20])), binding())));
    assert_eq!(check, ServerCheck::Skip.pinned(pin([7u8; 20])));
    assert_eq!(release, Some(binding()));
    // A pinned debug check never admits an unchecked (token-less) child: the image it names
    // is a development image with a pin, and the platform never takes an unidentified child
    // for a pinned image (`org_platform_host::admission`, the pin decision).
    let expected = check
        .expected(Path::new("/nonexistent/agentic-org-web"))
        .unwrap();
    assert!(expected.development && expected.macos_requirement.is_none());
    assert_eq!(expected.macos_cdhash, Some([7u8; 20]));
    assert_eq!(expected.sha256, Some([9; 32]));
    assert_eq!(
        check.why(&ImageRefusal::Unidentified),
        "selected release requires platform-audited image"
    );
}

/// A child confined by `sandbox-exec` with a profile that denies reading and writing
/// one critical folder, so it passes the real `verify_gateway`; after its ready line
/// it runs `after_ready` on the control connection (`$REPLY`).
#[cfg(target_os = "macos")]
fn confined_start(after_ready: &str) -> io::Error {
    let (_dir, config) = confined_config(after_ready);
    start_error(&config)
}

/// The configuration [`confined_start`] starts, with its temporary directory.
#[cfg(target_os = "macos")]
fn confined_config(after_ready: &str) -> (tempfile::TempDir, Configuration) {
    let (dir, mut config) = scripted_config("", |_, _, _| Ok(None));
    let root = fs::canonicalize(dir.path()).unwrap();
    let critical = root.join("critical");
    fs::create_dir(&critical).unwrap();
    let profile = root.join("profile.sb");
    fs::write(
        &profile,
        format!(
            "(version 1)\n(allow default)\n(deny file-read-data file-write-data (subpath \"{}\"))\n",
            critical.display()
        ),
    )
    .unwrap();
    let inner = root.join("inner.zsh");
    let script = connecting_script(READY).replace("read -r -t 20 -u $REPLY _\n", after_ready);
    fs::write(&inner, script).unwrap();
    fs::write(
        &config.program,
        format!(
            "#!/bin/zsh -f\nexec /usr/bin/sandbox-exec -f '{}' /bin/zsh -f '{}'\n",
            profile.display(),
            inner.display()
        ),
    )
    .unwrap();
    config.critical_paths = vec![critical];
    (dir, config)
}

#[cfg(target_os = "macos")]
#[test]
fn a_confined_child_with_a_wrong_handoff_ack_is_refused() {
    let message = confined_start(
        "print -r -u $REPLY '{\"v\":1,\"event\":\"fds.received\",\"of\":\"hello\"}'\n\
         read -r -t 20 -u $REPLY _\n",
    )
    .to_string();
    assert_eq!(
        message, "web process start failed: the handoff ACK does not acknowledge the handoff",
        "the child passed the sandbox verification and reached the ACK check"
    );
}

#[cfg(target_os = "macos")]
#[test]
fn a_confined_child_that_closes_control_before_its_handoff_ack_is_refused() {
    // Wait for the handoff to arrive, then close the control connection and stay alive.
    let message =
        confined_start("read -r -t 20 -k 1 -u $REPLY _\nsleep 0.5\nexec {REPLY}>&-\nsleep 20\n")
            .to_string();
    assert_eq!(
        message,
        "web process start failed: control EOF before the handoff ACK"
    );
}

#[cfg(target_os = "macos")]
#[test]
fn the_rendezvous_is_owner_only_when_the_child_connects() {
    // The child refuses (exit 7) unless the rendezvous socket is 0600 in a 0700
    // folder, then sends a mismatched ready line so the start stops right after.
    let script = connecting_script(r#"{"v":1,"event":"other"}"#).replace(
        "zmodload zsh/net/socket || exit 1\n",
        "[[ $(/usr/bin/stat -f %Lp \"$p\") == 600 ]] || exit 7\n\
         [[ $(/usr/bin/stat -f %Lp \"${p:h}\") == 700 ]] || exit 8\n\
         zmodload zsh/net/socket || exit 1\n",
    );
    let message = start_scripted(&script, |_, _, _| Ok(None)).to_string();
    assert!(
        message.contains("the ready line does not match ready_for_secrets"),
        "{message}"
    );
}

#[cfg(target_os = "macos")]
#[test]
fn a_started_web_serves_but_is_not_reported_before_web_serving() {
    // Confined, acknowledged: the start succeeds, but nothing reported serving yet.
    let (_dir, config) = confined_config(
        "print -r -u $REPLY '{\"v\":1,\"event\":\"fds.received\",\"of\":\"handoff\"}'\nsleep 20\n",
    );
    let mut running = Running::start(&config, Arc::new(NoOps)).unwrap();
    assert!(running.serving());
    assert!(
        !running.reported(),
        "running is reported only after web.serving"
    );
}

#[cfg(target_os = "macos")]
#[test]
fn a_child_that_exec_ed_after_connecting_is_refused_at_the_ready_line() {
    // Admitted with its real token; it connects, then execs (a new image, same pid)
    // before its ready line. The scripted re-checks answer the admitted token, so
    // only the ready-line peer check can see the change.
    let script = connecting_script(READY).replace(
        "print -r -u $REPLY",
        "exec 3<&$REPLY\nsleep 0.5\nready='{\"v\":1,\"event\":\"ready_for_secrets\"}'\n\
         exec /bin/zsh -f -c 'print -r -u 3 -- \"$0\"; sleep 20' \"$ready\"\nprint -r -u $REPLY",
    );
    let admitted = Arc::new(Mutex::new(None));
    let error = start_scripted(&script, move |step, n, pid| {
        let mut admitted = lock(&admitted);
        if step == crate::server_check::Step::Admit && n == 1 {
            *admitted = Some(
                org_platform_host::testing::admission::child_peer(pid)
                    .map_err(|e| e.to_string())?
                    .macos_audit_token,
            );
        }
        Ok(*admitted)
    });
    assert_eq!(
        error.to_string(),
        "web process start failed: the ready-line peer is not the admitted child"
    );
}

#[test]
fn runs_that_end_before_they_are_stable_count_as_failed_starts() {
    let stopping = Arc::new(AtomicBool::new(false));
    let state = Arc::new(Mutex::new(GatewayState::NotConfigured));
    let starts = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let timing = Timing {
        serving: SERVING_DEADLINE,
        stable: Duration::from_secs(3600),
        backoff: Duration::from_millis(1),
    };
    let supervisor = {
        let (stopping, state, starts) = (
            Arc::clone(&stopping),
            Arc::clone(&state),
            Arc::clone(&starts),
        );
        thread::spawn(move || {
            // Each run is published Running, then ends long before it is stable.
            supervise(&stopping, &state, &|| true, timing, &quiet, || {
                starts.fetch_add(1, Ordering::SeqCst);
                Ok(Brief {
                    until: Instant::now() + Duration::from_millis(30),
                })
            })
        })
    };
    let until = Instant::now() + Duration::from_secs(10);
    while !supervisor.is_finished() && Instant::now() < until {
        thread::sleep(Duration::from_millis(10));
    }
    let gave_up = supervisor.is_finished();
    stopping.store(true, Ordering::SeqCst);
    supervisor.thread().unpark();
    supervisor.join().unwrap();
    assert!(
        gave_up,
        "kept restarting after {} short runs",
        starts.load(Ordering::SeqCst)
    );
    assert_eq!(starts.load(Ordering::SeqCst), MAX_STARTS);
    assert_eq!(*lock(&state), GatewayState::Failed);
}
