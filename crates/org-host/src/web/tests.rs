//! Supervisor and handoff unit tests. A test that must start a child starts a compiled
//! one through the system's real admission: the fake web process of
//! `examples/fake_web.rs`, or the real `agentic-org-web` built beside these tests. The
//! tests of a whole host starting the real web process are in `web_process_tests.rs`.
use super::*;
use crate::service::listener::Connection;
use crate::test_programs::{fake_web, image_identity, sibling_program};
use org_platform_host::admission::ImageIdentity;
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

/// The stop flag of a start that the host never stops.
static GOING: AtomicBool = AtomicBool::new(false);

/// The rendezvous-through-ACK bound for tests that start a real child. Production
/// uses `WAIT` (5 s); a parallel workspace test run can delay a child's start past
/// that, so tests allow far longer. Tests of the deadline itself pass their own.
const START_WAIT: Duration = Duration::from_secs(60);

/// Starts the web process of `config` through an admission of its own, as the first start
/// of a host does.
fn start(config: &Configuration, handler: Arc<dyn Handler>) -> io::Result<Running> {
    Running::start(
        config,
        &Image::new(&config.check, &config.program),
        None,
        handler,
        START_WAIT,
        &GOING,
    )
}

/// This user's own copy of the fake web process (`examples/fake_web.rs`) in `dir`, named as
/// the web process is, which takes `steps` when it is started.
fn fake_program(dir: &Path, steps: &str) -> PathBuf {
    let program = dir.join("agentic-org-web");
    fs::copy(fake_web(), &program).unwrap();
    fs::set_permissions(&program, fs::Permissions::from_mode(0o700)).unwrap();
    fs::write(dir.join("agentic-org-web.steps"), steps).unwrap();
    program
}

/// A supervisor log sink for tests that do not read the log.
fn quiet(_: &str) {}

/// Waits for `done`; the bound is generous so a loaded test run cannot fail it.
fn wait_for(what: &str, mut done: impl FnMut() -> bool) {
    let until = Instant::now() + Duration::from_secs(30);
    while !done() {
        assert!(Instant::now() < until, "timed out waiting for {what}");
        thread::sleep(Duration::from_millis(5));
    }
}

/// A child that a debug build's check admitted and that has not been let run: the fake web
/// process, from a copy in `dir`. It stays alive until it is dropped: on macOS suspended,
/// without running anything; where a child runs from its start, its one step is to stay.
fn held_child(dir: &Path) -> org_platform_host::admission::Started {
    let held = ServerCheck::Skip
        .admission(&fake_program(dir, "stay\n"))
        .unwrap()
        .start(&[], &ChildEnv::new(), &SPAWN_LOCK)
        .unwrap();
    org_platform_host::testing::admission::unreleased(held)
}

/// A release build without a team id: exactly one outcome per system, the one the
/// platform's fact names.
#[test]
fn refused_web_start_never_becomes_ready() {
    let dir = tempfile::tempdir().unwrap();
    let state = Arc::new(Mutex::new(GatewayState::NotConfigured));
    let started = Supervisor::start(
        Configuration {
            // Never run: its first step would end it.
            program: fake_program(dir.path(), "exit 1\n"),
            instance: "instance".into(),
            runtime: dir.path().to_owned(),
            listen: loopback_on(free_port()),
            check: ServerCheck::gateway_for_build(false, None).unwrap(),
            critical_paths: vec![],
            release: None,
        },
        None,
        Arc::new(NoOps),
        Arc::clone(&state),
        Arc::new(|| true),
    );
    let _supervisor = match image_identity() {
        // No signature can be checked without a team: there is no supervisor at all.
        ImageIdentity::CodeSignature => {
            let Err(error) = started else {
                panic!("a release build without a team never spawns")
            };
            // The log names the real cause and that it is the web process's check.
            let message = error.to_string();
            assert!(message.contains("web process"), "{message}");
            assert!(message.contains("no app team id"), "{message}");
            None
        }
        // Where the bytes are checked no team names anything: the supervisor starts, and
        // each start is refused because no selected release names the image (the cause is
        // asserted in the next test, on the start itself).
        ImageIdentity::VerifiedBytes => {
            let supervisor = started.expect("the check may spawn; the image is refused");
            wait_for("the first refused start", || {
                *lock(&state) == GatewayState::Failed
            });
            Some(supervisor)
        }
    };
    let observed = *lock(&state);
    assert_ne!(observed, GatewayState::Running);
    assert!(!crate::service::web_serving(true, true, observed));
}

#[test]
fn an_admission_refusal_inside_start_keeps_its_cause() {
    let dir = tempfile::tempdir().unwrap();
    let config = Configuration {
        program: fake_program(dir.path(), "exit 1\n"),
        instance: "instance".into(),
        runtime: dir.path().to_owned(),
        listen: loopback_on(free_port()),
        check: ServerCheck::gateway_for_build(false, None).unwrap(),
        critical_paths: vec![],
        release: None,
    };
    let Err(error) = start(&config, Arc::new(NoOps)) else {
        panic!("a release build without a team or a selected release never spawns")
    };
    // The supervisor logs this text, so the refusal cause must survive.
    let message = error.to_string();
    match image_identity() {
        ImageIdentity::CodeSignature => {
            assert!(
                message.contains("web process admission refused"),
                "{message}"
            );
            assert!(message.contains("no app team id"), "{message}");
        }
        ImageIdentity::VerifiedBytes => assert_eq!(
            message,
            "web process admission refused at admission: this release build has no selected \
             release, so nothing names the image it may start (a release image is checked \
             against the digest in the signed release manifest)"
        ),
    }
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
    fn confined(&mut self) -> String {
        // A line break, as a reason from a web process's standard error could
        // never have, but the log must not depend on that.
        "confined by a fake\nforged line".to_owned()
    }
}

/// What the supervisor logs once for each start of a [`Fake`].
const FAKE_STARTED: &str =
    "authenticated browser web process started, confined by a fake\\nforged line";
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
/// backoff is waited (the 300-second backoff would exceed `wait_for`'s deadline).
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
                    backoff: Duration::from_secs(300),
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
    // Each of the two starts is logged once; no failure or backoff is.
    assert_eq!(
        logged,
        [
            FAKE_STARTED,
            "authenticated browser stopped because the host is no longer eligible; it starts again once eligible",
            FAKE_STARTED,
        ],
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
    // Nor to any other socket: a datagram socket can name a destination with each
    // send, and only the shut-down write side stops the web process's end from
    // reaching another Unix datagram socket of this user.
    let dir = tempfile::tempdir().unwrap();
    let other = UnixDatagram::bind(dir.path().join("other")).unwrap();
    other.set_nonblocking(true).unwrap();
    let error = child.send_to(b"x", dir.path().join("other")).unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::BrokenPipe, "{error}");
    assert_eq!(
        other.recv(&mut [0u8; 4]).unwrap_err().kind(),
        io::ErrorKind::WouldBlock
    );
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
    fn confined(&mut self) -> String {
        "confined by a brief run".to_owned()
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

/// The steps of a fake web process (`examples/fake_web.rs`) that reads the first line,
/// connects to the rendezvous and sends `ready` as its ready line, then waits.
fn connecting_steps(ready: &str) -> String {
    format!("first-line\nconnect\nsend {ready}\nstay\n")
}

/// The ready line a well-behaved web process sends where it reports no Landlock domain.
const READY: &str = r#"{"v":1,"event":"ready_for_secrets"}"#;

/// The identity a scripted admission answers for the child `pid`: the system's own for
/// that child, as far as a scripted check can carry it. Where the system has no audit
/// token this is the zero token: one fixed identity, which is all these tests compare.
fn child_token(pid: u32) -> Result<Option<crate::peer::AuditToken>, String> {
    org_platform_host::testing::admission::child_peer(pid)
        .map(|peer| Some(peer.macos_audit_token))
        .map_err(|e| e.to_string())
}

/// Starts a web process whose child is the fake web process taking `steps`. The scripted
/// admission answers `answer(step, nth, pid)`, with `nth` counting calls of that step
/// (1-based).
fn start_scripted(
    steps: &str,
    answer: impl Fn(crate::server_check::Step, usize, u32) -> Result<Option<crate::peer::AuditToken>, String>
        + Send
        + Sync
        + 'static,
) -> io::Error {
    let (_dir, config) = scripted_config(steps, answer);
    start_error(&config)
}

/// Starts `config`, which must be refused, and returns the refusal.
fn start_error(config: &Configuration) -> io::Error {
    match start(config, Arc::new(NoOps)) {
        Err(error) => error,
        Ok(_) => panic!("this start must be refused"),
    }
}

/// A loopback listen configuration on `port`.
fn loopback_on(port: u16) -> ListenConfig {
    serde_json::from_str(&format!(
        r#"{{"address":"127.0.0.1","port":{port},"scheme":"http_loopback","origin":"http://127.0.0.1:{port}","hsts":"off"}}"#
    ))
    .unwrap()
}

/// The configuration [`start_scripted`] starts, on a free loopback port, with its
/// temporary directory (program and runtime) kept alive by the caller. The program is the
/// fake web process, started through the system's real admission with a scripted image
/// check, and takes `steps`.
fn scripted_config(
    steps: &str,
    answer: impl Fn(crate::server_check::Step, usize, u32) -> Result<Option<crate::peer::AuditToken>, String>
        + Send
        + Sync
        + 'static,
) -> (tempfile::TempDir, Configuration) {
    use crate::server_check::{Scripted, Step};
    use std::sync::atomic::AtomicUsize;
    let dir = tempfile::tempdir().unwrap();
    let program = fake_program(dir.path(), steps);
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
fn start_refusing(
    refuse: crate::server_check::Step,
    nth: usize,
    tokens: bool,
    cause: &'static str,
) -> io::Error {
    start_scripted(&connecting_steps(READY), move |step, n, pid| {
        if step == refuse && n == nth {
            return Err(cause.to_owned());
        }
        if !tokens {
            return Ok(None);
        }
        child_token(pid)
    })
}

#[test]
fn a_changed_identity_at_re_admission_names_that_cause() {
    use crate::server_check::Step;
    // Admitted with no token; re-admission after the ready line returns one.
    let error = start_scripted(&connecting_steps(READY), |step, n, pid| {
        if step == Step::Admit && n == 2 {
            child_token(pid)
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

#[test]
fn a_mismatched_ready_line_names_that_cause() {
    let error = start_scripted(
        &connecting_steps(r#"{"v":1,"event":"other"}"#),
        |_, _, _| Ok(None),
    );
    let message = error.to_string();
    assert!(
        message.contains("the ready line does not match ready_for_secrets"),
        "{message}"
    );
}

/// A ready line that reports a Landlock domain, from a child that confined nothing. What
/// the system's confinement adapter says of that report decides the one outcome: an adapter
/// without Landlock refuses the report itself, and the start fails with its words; an
/// adapter with Landlock takes the report and then finds the child unconfined.
#[test]
fn a_ready_line_reporting_landlock_is_refused_by_the_report_or_by_the_missing_sandbox() {
    let error = start_scripted(
        &connecting_steps(
            r#"{"v":1,"event":"ready_for_secrets","landlock":{"abi":4,"enforcement":"fully_enforced"}}"#,
        ),
        |_, _, _| Ok(None),
    );
    let message = error.to_string();
    let reported = org_platform::confinement::Enforcement {
        landlock_abi: Some(4),
        seccomp: false,
    };
    match org_platform::confinement::verify_ready_report(&reported) {
        Err(refusal) => {
            // The one adapter without Landlock, in its own words.
            assert_eq!(
                refusal.to_string(),
                "the ready line reports landlock on macOS"
            );
            assert!(
                message
                    .contains("web process start failed: the ready line reports landlock on macOS"),
                "{message}"
            );
        }
        Ok(()) => assert!(
            message.starts_with("web process start failed: verify the web process sandbox: "),
            "{message}"
        ),
    }
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

#[test]
fn a_child_that_exits_before_connecting_names_its_status() {
    let error = start_scripted("first-line\nexit 3\n", |_, _, _| Ok(None));
    let message = error.to_string();
    assert!(
        message.contains("the child exited before connecting (exit code 3)"),
        "{message}"
    );
}

/// X5 review, third round, P2: a start given the listener setup bound serves on that one
/// socket and binds nothing. The port stays held throughout (here by a second handle on
/// the same socket, as setup holds it), so a start that bound again would be refused
/// ("bind the browser listener"); this one goes on to start its child. A listener on
/// another address or port is refused before anything is spawned.
#[test]
fn a_start_given_a_bound_listener_serves_on_it_and_binds_nothing() {
    let (_dir, config) = scripted_config("first-line\nexit 3\n", |_, _, _| Ok(None));
    let held =
        TcpListener::bind((config.listen.address(), config.listen.port())).expect("its port");
    let holding = held.try_clone().unwrap();
    let image = Image::new(&config.check, &config.program);
    let message = match Running::start(
        &config,
        &image,
        Some(held),
        Arc::new(NoOps),
        START_WAIT,
        &GOING,
    ) {
        Err(error) => error.to_string(),
        Ok(_) => panic!("this start must be refused"),
    };
    assert!(
        message.contains("the child exited before connecting (exit code 3)"),
        "{message}"
    );
    drop(holding);

    let elsewhere = TcpListener::bind("127.0.0.1:0").unwrap();
    let message = match Running::start(
        &config,
        &image,
        Some(elsewhere),
        Arc::new(NoOps),
        START_WAIT,
        &GOING,
    ) {
        Err(error) => error.to_string(),
        Ok(_) => panic!("a listener on another port must be refused"),
    };
    assert!(
        message.contains("the browser listener is not on its configured address and port"),
        "{message}"
    );
}

/// X5 review, third round, P1: when a start or a run of the web process ends (as at a
/// host's stop), the web process is killed and reaped at once, whatever a request of its
/// control connection is doing. A request still running (here held on a lock the test
/// holds, as a write lock of the registry would be held) is waited for only afterwards.
#[test]
fn a_running_request_never_keeps_the_web_process_alive() {
    struct Holds(Arc<Mutex<()>>, Arc<AtomicBool>);
    impl Handler for Holds {
        fn handle(&self, _: org_proto::Request, _: &Arc<Connection>, _: crate::peer::Received) {
            self.1.store(true, Ordering::SeqCst);
            drop(lock(&self.0));
        }
    }
    let dir = tempfile::tempdir().unwrap();
    let held = Arc::new(Mutex::new(()));
    let entered = Arc::new(AtomicBool::new(false));
    let holding = lock(&held);
    let (host, mut peer) = UnixStream::pair().unwrap();
    let (transfer, _child) = UnixDatagram::pair().unwrap();
    let handler = Arc::new(Holds(Arc::clone(&held), Arc::clone(&entered)));
    let control = WebConnection::start(host, transfer, loopback(), handler).unwrap();
    let spawned = held_child(dir.path());
    let pid = spawned.child.id();
    let running = Running {
        control,
        process: spawned.child,
        _stdin: spawned.stdin,
        layers: org_platform::confinement::Layers {
            description: "nothing: a test's own child".to_owned(),
            landlock_missing: false,
        },
        captured: Stderr::capture(spawned.stderr, "stderr-held-request-test").unwrap(),
    };
    let hello = org_proto::Request::new::<org_host_control::Hello>(1, &Default::default());
    org_proto::frame::write_message(&mut peer, &hello.unwrap()).unwrap();
    wait_for("the request to run", || entered.load(Ordering::SeqCst));

    let stopping = thread::spawn(move || drop(running));
    // Reaped: no process has the pid any more (a zombie still would).
    // SAFETY: signal 0 only asks whether the pid exists.
    let exists = || unsafe { libc::kill(pid as libc::pid_t, 0) } == 0;
    let until = Instant::now() + Duration::from_secs(5);
    while exists() && Instant::now() < until {
        thread::sleep(Duration::from_millis(5));
    }
    let reaped = !exists();
    drop(holding);
    stopping.join().unwrap();
    assert!(reaped, "the web process lived on while a request of it ran");
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
    wait_for("restart logged", || lock(&lines).len() >= 2);
    stopping.store(true, Ordering::SeqCst);
    supervisor.thread().unpark();
    supervisor.join().unwrap();
    let logged = lock(&lines).clone();
    // The start is logged once, with what the web process is confined by, escaped.
    assert_eq!(
        logged,
        [
            FAKE_STARTED,
            "authenticated browser restarting in 1s because the running web process stopped: the fake exited"
        ],
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
    // The two starts that succeeded are each logged once, before their failure; a
    // failed start logs no such line.
    let (started, logged): (Vec<String>, Vec<String>) = lock(&lines)
        .clone()
        .into_iter()
        .partition(|line| line.contains("web process started"));
    assert_eq!(started, [FAKE_STARTED, FAKE_STARTED]);
    assert_eq!(lock(&lines)[0], FAKE_STARTED);
    assert_eq!(lock(&lines)[2], FAKE_STARTED);
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

#[test]
fn every_admission_refusal_inside_start_keeps_its_cause() {
    use crate::server_check::Step;
    // With real tokens the child itself is the rendezvous peer, so the
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

/// A free loopback port nothing listens on.
fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

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
        None,
        Arc::new(NoOps),
        Arc::clone(&state),
        Arc::new(|| true),
    );
    let Err(error) = started else {
        panic!("an HTTPS listener never starts the web process")
    };
    // The confinement question comes first: where it refuses, the listener is not
    // looked at, and that refusal is the next test's.
    if org_platform::confinement::web_confinement_supported().is_ok() {
        assert_eq!(
            error.to_string(),
            "the web process requires a plain HTTP listener (http_loopback, or http_published \
             in the foreground mode): HTTPS has no verified certificate yet"
        );
    }
    assert_eq!(*lock(&state), GatewayState::NotConfigured);
}

/// The listener gate admits the two plain HTTP modes: a published listener of a foreground
/// instance passes it as a loopback one does, and the supervisor starts.
#[test]
fn the_supervisor_admits_a_published_plain_http_listener() {
    let dir = tempfile::tempdir().unwrap();
    let state = Arc::new(Mutex::new(GatewayState::NotConfigured));
    let published: ListenConfig = serde_json::from_str(
        r#"{"address":"0.0.0.0","port":48125,"scheme":"http_published","origin":"http://127.0.0.1:48125","hsts":"off"}"#,
    )
    .unwrap();
    let started = Supervisor::start(
        Configuration {
            listen: published,
            ..startable(dir.path())
        },
        None,
        Arc::new(NoOps),
        Arc::clone(&state),
        // Never eligible: the gate is what this asks about, and nothing is started.
        Arc::new(|| false),
    );
    match started {
        Ok(supervisor) => drop(supervisor),
        // Only a system that cannot confine the web process refuses, and for that reason.
        Err(error) => {
            assert!(org_platform::confinement::web_confinement_supported().is_err());
            assert!(
                !error.to_string().contains("plain HTTP listener"),
                "{error}"
            );
        }
    }
}

/// A configuration that every check but confinement would let start: a debug
/// build's check, a loopback listener, and a program that is never reached.
fn startable(dir: &Path) -> Configuration {
    Configuration {
        program: dir.join("agentic-org-web"),
        instance: "instance".into(),
        runtime: dir.to_owned(),
        listen: loopback_on(free_port()),
        check: ServerCheck::gateway_for_build(true, None).unwrap(),
        critical_paths: vec![],
        release: None,
    }
}

#[test]
fn the_supervisor_starts_only_where_the_web_process_can_be_confined() {
    let dir = tempfile::tempdir().unwrap();
    let state = Arc::new(Mutex::new(GatewayState::NotConfigured));
    let started = Supervisor::start(
        startable(dir.path()),
        None,
        Arc::new(NoOps),
        Arc::clone(&state),
        Arc::new(|| false),
    );
    match org_platform::confinement::web_confinement_supported() {
        // A supervisor that waits to become eligible: nothing was bound or started.
        Ok(()) => drop(started.unwrap()),
        // No bind, no child, and the adapter's reason after the fixed wording.
        Err(reason) => {
            let Err(error) = started else {
                panic!("no web process where it cannot be confined")
            };
            assert_eq!(
                error.to_string(),
                format!(
                    "the web process requires process confinement, which is unavailable \
                     here: {reason}"
                )
            );
            assert_eq!(*lock(&state), GatewayState::NotConfigured);
            assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 0);
        }
    }
}

#[test]
fn a_confinement_refusal_keeps_the_adapters_reason_bounded_and_printable() {
    let adapter = "gateway confinement unavailable: seccomp failed with EPERM: this kernel or \
                   its profile has no seccomp filter that kills the process";
    assert_eq!(
        unconfinable(io::Error::other(adapter)).to_string(),
        format!(
            "the web process requires process confinement, which is unavailable here: {adapter}"
        )
    );
    // Whatever an adapter says, the log gets printable ASCII of a bounded length.
    let odd = format!("a\nb\u{1b}[0m\u{202e}é{}", "x".repeat(1000));
    let text = unconfinable(io::Error::other(odd)).to_string();
    let reason = text.split("here: ").nth(1).unwrap();
    assert_eq!(reason.len(), REASON_BYTES);
    assert!(reason.starts_with("a?b?[0m??xxx"), "{reason}");
    assert!(reason.bytes().all(|byte| (0x20..=0x7e).contains(&byte)));
}

/// The environment variable that makes this test executable the host of the next
/// tests: a process under a profile that refuses one of the confinement calls.
const UNDER_PROFILE: &str = "ORG_HOST_WEB_TEST_UNDER_PROFILE";

/// Runs the test `name` again in a process under a profile that refuses `call`, as
/// a container runtime's profile does, and returns what it printed. `None` where
/// this system's adapter does not confine with such calls: nothing to refuse.
fn under_profile(
    name: &str,
    call: org_platform::confinement::testing::RefusedCall,
) -> Option<String> {
    let mut command = std::process::Command::new(std::env::current_exe().unwrap());
    command
        .args(["--exact", name, "--nocapture", "--test-threads=1"])
        .args(["--include-ignored"])
        .env(UNDER_PROFILE, format!("{call:?}"));
    if let Err(error) = org_platform::confinement::testing::refuse_call(&mut command, call) {
        assert_eq!(error.kind(), io::ErrorKind::Unsupported, "{error}");
        return None;
    }
    let output = command.output().unwrap();
    let printed = String::from_utf8_lossy(&output.stdout).into_owned();
    assert!(
        output.status.success(),
        "{printed}\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    Some(printed)
}

/// ADR-0017 LNX-3 at the host: under a profile that refuses `seccomp`, the layer
/// that must hold, the host starts no web process and says which call failed with
/// which error. A profile that refuses Landlock is no such reason: the host starts
/// its supervisor as anywhere else.
#[test]
fn lnx_3_a_host_under_a_profile_starts_nothing_only_where_the_filter_is_refused() {
    use org_platform::confinement::testing::RefusedCall;
    const NAME: &str =
        "web::tests::lnx_3_a_host_under_a_profile_starts_nothing_only_where_the_filter_is_refused";
    if std::env::var_os(UNDER_PROFILE).is_some() {
        // The host under the profile. It is never eligible, so a supervisor that
        // does start waits and binds nothing.
        let dir = tempfile::tempdir().unwrap();
        let state = Arc::new(Mutex::new(GatewayState::NotConfigured));
        let started = Supervisor::start(
            startable(dir.path()),
            None,
            Arc::new(NoOps),
            Arc::clone(&state),
            Arc::new(|| false),
        );
        assert_eq!(
            fs::read_dir(dir.path()).unwrap().count(),
            0,
            "nothing bound"
        );
        match started {
            Ok(supervisor) => {
                drop(supervisor);
                println!("\nstarted");
            }
            Err(error) => {
                assert_eq!(*lock(&state), GatewayState::NotConfigured);
                println!("\nrefused: {error}");
            }
        }
        return;
    }
    for call in [
        RefusedCall::LandlockCreateRuleset,
        RefusedCall::LandlockRuleset,
        RefusedCall::LandlockRestrictSelf,
        // The installation that fails is the web process's to report: the host's
        // question passes.
        RefusedCall::SeccompInstall,
    ] {
        let Some(printed) = under_profile(NAME, call) else {
            return;
        };
        assert!(
            printed.lines().any(|line| line == "started"),
            "{call:?}: {printed}"
        );
        assert!(!printed.contains("refused: "), "{call:?}: {printed}");
    }
    let printed = under_profile(NAME, RefusedCall::Seccomp).unwrap();
    assert!(
        printed.lines().any(|line| line
            == "refused: the web process requires process confinement, which is unavailable \
                here: gateway confinement unavailable: seccomp failed with EPERM: this kernel \
                or its profile has no seccomp filter that kills the process"),
        "{printed}"
    );
    assert!(!printed.lines().any(|line| line == "started"), "{printed}");
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
    assert_eq!(
        line(&mut receiver, until, &GOING, "ready line").unwrap(),
        full
    );
}

#[test]
fn a_control_line_longer_than_1023_bytes_is_refused() {
    let (mut receiver, mut peer) = control_pair();
    peer.write_all(&[b'a'; 1024]).unwrap();
    let until = Instant::now() + Duration::from_secs(5);
    let error = line(&mut receiver, until, &GOING, "ready line").unwrap_err();
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
    let error = line(&mut receiver, until, &GOING, "handoff ACK").unwrap_err();
    assert_eq!(
        error.to_string(),
        "web process start failed: the handoff ACK did not arrive before the deadline"
    );
    assert!(started.elapsed() < Duration::from_secs(4));
    drop(peer);
}

/// A child that closes its end, or dies, with bytes of the host's it has not read. Linux
/// reports that to the host as a reset of the connection, macOS as its end. Either way it is
/// a child that went away: the start is refused, and the host does not treat it as broken
/// descriptor framing, which would end the host itself.
#[test]
fn a_peer_that_closes_with_unread_bytes_is_a_control_eof() {
    let (ours, mut theirs) = UnixStream::pair().unwrap();
    // The handoff the child never reads, and the start of an acknowledgement it sent.
    (&ours)
        .write_all(b"{\"v\":1,\"event\":\"handoff\"}\n")
        .unwrap();
    theirs.write_all(b"partial").unwrap();
    drop(theirs);
    let mut receiver = org_socket::rights::StreamReceiver::new(ours);
    let until = Instant::now() + Duration::from_secs(5);
    let error = line(&mut receiver, until, &GOING, "handoff ACK").unwrap_err();
    assert_eq!(
        error.to_string(),
        "web process start failed: control EOF before the handoff ACK"
    );
    assert!(error
        .get_ref()
        .is_some_and(|inner| inner.is::<ControlEof>()));
}

#[test]
fn control_eof_before_a_line_is_refused() {
    let (mut receiver, mut peer) = control_pair();
    peer.write_all(b"partial").unwrap();
    drop(peer);
    let until = Instant::now() + Duration::from_secs(5);
    let error = line(&mut receiver, until, &GOING, "ready line").unwrap_err();
    assert_eq!(
        error.to_string(),
        "web process start failed: control EOF before the ready line"
    );
    // The start can tell a child that went away from one that is late.
    let went_away = |error: &io::Error| {
        error
            .get_ref()
            .is_some_and(|inner| inner.is::<ControlEof>())
    };
    assert!(went_away(&error));
    let (mut receiver, _peer) = control_pair();
    let late = line(&mut receiver, Instant::now(), &GOING, "ready line").unwrap_err();
    assert!(!went_away(&late), "{late}");
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

/// X5 review V2: a host that stops while its web process's start waits for the child, to
/// connect or then to send its ready line, does not wait for the start's deadline (60 s
/// here): the supervision ends at once, as after a failed start, with nothing logged of it,
/// the state `failed` and the child killed (its standard error ends).
#[test]
fn a_stop_ends_a_start_that_waits_for_the_web_process_at_once() {
    for (n, steps) in [
        "first-line\nstderr waiting\nstay\n",
        "first-line\nconnect\nstderr waiting\nstay\n",
    ]
    .into_iter()
    .enumerate()
    {
        let (_dir, mut config) = scripted_config(steps, |_, _, _| Ok(None));
        let instance = format!("stalled-start-{}-{n}", std::process::id());
        config.instance = instance.clone();
        let stop = Arc::new(AtomicBool::new(false));
        let state = Arc::new(Mutex::new(GatewayState::NotConfigured));
        let (lines, log) = recording();
        let thread = {
            let (stop, state) = (Arc::clone(&stop), Arc::clone(&state));
            thread::spawn(move || {
                let image = Image::new(&config.check, &config.program);
                supervise(&stop, &state, &|| true, Timing::PRODUCTION, &log, || {
                    Running::start(&config, &image, None, Arc::new(NoOps), START_WAIT, &stop)
                })
            })
        };
        let supervisor = Supervisor {
            stop,
            state: Arc::clone(&state),
            thread: Some(thread),
        };
        wait_for("the web process to wait for the host", || {
            lock(&CAPTURED_STDERR)
                .get(&instance)
                .is_some_and(|captured| captured.bytes.ends_with(b"waiting\n"))
        });
        let asked = Instant::now();
        assert!(supervisor.join_by(asked + START_WAIT), "{steps}");
        let took = asked.elapsed();
        assert!(took < Duration::from_secs(1), "{steps}: {took:?}");
        assert_eq!(*lock(&state), GatewayState::Failed);
        assert!(lock(&lines).is_empty(), "{:?}", lock(&lines));
        // Its standard error ended: the child is gone.
        assert_eq!(captured_stderr(&instance).unwrap(), b"waiting\n");
    }
}

#[test]
fn a_child_that_does_not_connect_before_the_deadline_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let rendezvous = Rendezvous::bind(dir.path()).unwrap();
    let mut spawned = held_child(dir.path());
    // The deadline has passed: no connection is accepted, and the child still runs.
    let error = rendezvous
        .accept(&mut spawned.child, Instant::now(), &GOING)
        .unwrap_err();
    assert_eq!(
        error.to_string(),
        "web process start failed: the child did not connect before the deadline"
    );
}

#[test]
fn a_rendezvous_peer_other_than_the_admitted_child_is_refused() {
    // A forked process, not the spawned child, connects and sends the ready line; the
    // spawned child waits for it and so is still alive.
    let steps = format!("first-line\nfork\nconnect\nsend {READY}\nstay\n");
    let error = start_scripted(&steps, |_, _, _| Ok(None));
    let message = error.to_string();
    assert!(
        message.contains("web process start failed: the rendezvous peer is not the admitted child"),
        "{message}"
    );
}

#[test]
fn an_unconfined_child_fails_the_sandbox_verification() {
    // Admitted with its real token; every check passes until the sandbox check. A
    // non-empty critical path, so the refusal is the child's missing confinement,
    // not the empty path list.
    let (dir, mut config) = scripted_config(&connecting_steps(READY), |_, _, pid| child_token(pid));
    config.critical_paths = vec![fs::canonicalize(dir.path()).unwrap()];
    let error = start_error(&config);
    let message = error.to_string();
    assert!(
        message.contains("web process start failed: verify the web process sandbox"),
        "{message}"
    );
}

#[test]
fn each_failed_start_step_before_the_child_starts_names_its_cause() {
    // None of these starts reaches the program, so its steps do not matter.
    let script = "exit 1\n";
    // The browser port is taken.
    let (_dir, mut config) = scripted_config(script, |_, _, _| Ok(None));
    let taken = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    config.listen = loopback_on(taken.local_addr().unwrap().port());
    let message = start_error(&config).to_string();
    assert!(
        message.starts_with("web process start failed: bind the browser listener ("),
        "{message}"
    );
    // The runtime directory is a file, so its rendezvous folder cannot be created.
    let (dir, mut config) = scripted_config(script, |_, _, _| Ok(None));
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
    let (dir, mut config) = scripted_config(script, |_, _, _| Ok(None));
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
    let (dir, mut config) = scripted_config(script, |_, _, _| Ok(None));
    config.program = dir.path().join("missing");
    let message = start_error(&config).to_string();
    assert!(
        message.starts_with("web process start failed: spawn the web process suspended ("),
        "{message}"
    );
}

#[test]
fn a_first_line_that_does_not_fit_names_its_cause() {
    // The first line would not fit in 1023 bytes. The child is started first, so
    // this needs a program the system starts.
    let (_dir, mut config) = scripted_config(&connecting_steps(READY), |_, _, _| Ok(None));
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
    // Each child is the fake web process in a folder of its own: one that stays (held, not
    // let run, where the system starts a child suspended), or one that is let run and
    // takes `steps`.
    let children = tempfile::tempdir().unwrap();
    let running = |steps: Option<&str>| {
        let dir = tempfile::tempdir_in(children.path()).unwrap().keep();
        let (host, peer) = UnixStream::pair().unwrap();
        peer.set_read_timeout(Some(Duration::from_secs(10)))
            .unwrap();
        let (transfer, _child) = UnixDatagram::pair().unwrap();
        let control = WebConnection::start(host, transfer, loopback(), handler.clone()).unwrap();
        let spawned = match steps {
            Some(steps) => ServerCheck::Skip
                .admission(&fake_program(&dir, steps))
                .unwrap()
                .spawn(&[], &ChildEnv::new(), &SPAWN_LOCK)
                .unwrap(),
            None => held_child(&dir),
        };
        let running = Running {
            control,
            process: spawned.child,
            _stdin: spawned.stdin,
            layers: org_platform::confinement::Layers {
                description: "nothing: a test's own child".to_owned(),
                landlock_missing: false,
            },
            captured: Stderr::capture(spawned.stderr, "stderr-unit-test").unwrap(),
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
    let (mut refused, mut peer) = running(None);
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
    let (mut exited, _peer) = running(Some("exit 1\n"));
    wait_for("the child exits", || !exited.serving());
    assert!(exited.control.live());
    assert_eq!(exited.ended(), "the child exited (exit code 1)");

    // Control EOF while the child still runs (on macOS it is suspended, elsewhere
    // it stays).
    let (mut closed, peer) = running(None);
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
    match image_identity() {
        ImageIdentity::CodeSignature => assert!(matches!(web, ServerCheck::Gateway(_))),
        // The team names nothing where the bytes are checked.
        ImageIdentity::VerifiedBytes => assert_eq!(web, ServerCheck::Published),
    }
    // A selected release pins its exact web process image and binds its identity.
    let (check, release) = admission::<()>(false, Some(TEAM), Some((Ok(hash), binding())));
    assert_eq!(check, web.clone().pinned(hash));
    assert_eq!(release, Some(binding()));
    // No release: this system's web process check alone, no binding.
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
    // No team id in a release build: refused where a code signature is checked; where the
    // bytes are checked, the image a selected release pins and nothing else (without a
    // release the platform refuses the image, as the admission refusal test shows).
    let (check, release) = admission::<()>(false, None, None);
    match image_identity() {
        ImageIdentity::CodeSignature => {
            assert!(matches!(check, ServerCheck::WebRefused(_)), "{check:?}");
            assert!(check.may_spawn().is_err());
        }
        ImageIdentity::VerifiedBytes => {
            assert_eq!(check, ServerCheck::Published);
            assert!(check.may_spawn().is_ok());
            let (pinned, _) = admission::<()>(false, None, Some((Ok(pin([7u8; 20])), binding())));
            assert_eq!(pinned, ServerCheck::Published.pinned(pin([7u8; 20])));
        }
    }
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

/// A configuration that starts `program` unchecked, as a debug build's web process is.
fn unchecked(dir: &Path, program: PathBuf) -> Configuration {
    Configuration {
        program,
        instance: "instance".into(),
        runtime: dir.to_owned(),
        listen: loopback_on(free_port()),
        check: ServerCheck::Skip,
        critical_paths: vec![],
        release: None,
    }
}

/// The host builds the admission of its web process once: every start goes through that
/// one, and what the program's path names later is never read.
#[test]
fn one_web_admission_serves_every_start_and_is_never_built_again() {
    const EXITED: &str =
        "web process start failed: the child exited before connecting (exit code 3)";
    let dir = tempfile::tempdir().unwrap();
    let config = unchecked(dir.path(), fake_program(dir.path(), "first-line\nexit 3\n"));
    let image = Image::new(&config.check, &config.program);
    let built = image.admission().unwrap();
    let failed_start =
        || match Running::start(&config, &image, None, Arc::new(NoOps), START_WAIT, &GOING) {
            Err(error) => error.to_string(),
            Ok(_) => panic!("the fake web process exits before it connects"),
        };
    for _ in 0..2 {
        assert_eq!(failed_start(), EXITED);
        assert!(Arc::ptr_eq(&built, &image.admission().unwrap()));
    }
    // Where the image's bytes were read when the admission was built (it then knows their
    // digest), a start runs those bytes whatever the path names by now: the program is
    // replaced by something that is no program, and the next start is the same child.
    if let Some(digest) = built.image_sha256() {
        fs::remove_file(&config.program).unwrap();
        fs::write(&config.program, b"no program").unwrap();
        assert_eq!(failed_start(), EXITED);
        let still = image.admission().unwrap();
        assert!(Arc::ptr_eq(&built, &still));
        assert_eq!(still.image_sha256(), Some(digest));
    }
    // A build asked for at a host's start finds it built, and logs nothing.
    let ((), logged) = crate::host_log::capture(|| image.build_at_start());
    assert_eq!(logged, Vec::<String>::new());
}

/// A web admission that cannot be built yet (the program is not there) says why in the host
/// log, fails that start, and is asked for again by the next start; once built it is kept.
/// Where nothing of the image is read until a child is started, the admission exists at
/// once and the missing program fails each start instead.
#[test]
fn a_web_admission_that_could_not_be_built_is_logged_and_asked_for_again() {
    use crate::server_check::NoAdmission;
    let dir = tempfile::tempdir().unwrap();
    let config = unchecked(dir.path(), dir.path().join("agentic-org-web"));
    let image = Image::new(&config.check, &config.program);
    let ((), logged) = crate::host_log::capture(|| image.build_at_start());
    let before = match (image_identity(), image.admission()) {
        // Nothing is read until a child is started: the admission exists at once.
        (ImageIdentity::CodeSignature, Ok(admission)) => {
            assert_eq!(logged, Vec::<String>::new());
            Some(admission)
        }
        // The image is read when the admission is built: there is none yet, and the host
        // log says why.
        (ImageIdentity::VerifiedBytes, Err(NoAdmission::Unavailable(error))) => {
            assert_eq!(error.kind(), io::ErrorKind::NotFound, "{error:?}");
            let [line] = &logged[..] else {
                panic!("one line for the failed build: {logged:?}")
            };
            assert!(
                line.starts_with("the web process cannot be started: ")
                    && line.ends_with("; each start asks again"),
                "{line}"
            );
            None
        }
        (identity, other) => panic!("{identity:?}: {:?}", other.map(drop)),
    };
    // No web process starts, whichever step notices.
    let message = match Running::start(&config, &image, None, Arc::new(NoOps), START_WAIT, &GOING) {
        Err(error) => error.to_string(),
        Ok(_) => panic!("a missing program was started"),
    };
    assert!(
        message.starts_with("web process start failed: spawn the web process suspended ("),
        "{message}"
    );
    // The program appears: the next start builds the admission, or finds the one it has.
    fake_program(dir.path(), "first-line\nexit 3\n");
    let message = match Running::start(&config, &image, None, Arc::new(NoOps), START_WAIT, &GOING) {
        Err(error) => error.to_string(),
        Ok(_) => panic!("the fake web process exits before it connects"),
    };
    assert_eq!(
        message,
        "web process start failed: the child exited before connecting (exit code 3)"
    );
    let built = image.admission().expect("the program is there now");
    if let Some(before) = before {
        assert!(Arc::ptr_eq(&before, &built), "an admission was built twice");
    }
    assert!(Arc::ptr_eq(&built, &image.admission().unwrap()));
}

/// Starts that come together wait for the one build: every one of them gets the same
/// admission.
#[test]
fn web_starts_that_come_together_share_the_one_build() {
    let dir = tempfile::tempdir().unwrap();
    let program = fake_program(dir.path(), "exit 1\n");
    let image = Image::new(&ServerCheck::Skip, &program);
    let go = std::sync::Barrier::new(8);
    let built: Vec<_> = thread::scope(|scope| {
        let asking: Vec<_> = (0..8)
            .map(|_| {
                scope.spawn(|| {
                    go.wait();
                    image.admission().unwrap()
                })
            })
            .collect();
        asking
            .into_iter()
            .map(|asked| asked.join().unwrap())
            .collect()
    });
    assert!(built.iter().all(|one| Arc::ptr_eq(one, &built[0])));
}

/// A child that confines itself as the real web process does (the fake web process's
/// `confine` step is the platform's own `confine_web`), so it passes the host's real
/// sandbox verification; after its ready line it takes `after_ready`.
fn confined_start(after_ready: &str) -> io::Error {
    let (_dir, config) = confined_config(after_ready);
    start_error(&config)
}

/// The configuration [`confined_start`] starts, with its temporary directory. One critical
/// folder is named, for a system whose host verifies the sandbox by asking about paths.
fn confined_config(after_ready: &str) -> (tempfile::TempDir, Configuration) {
    let steps = format!("first-line\nconfine\nready\n{after_ready}");
    let (dir, mut config) = scripted_config(&steps, |_, _, _| Ok(None));
    let root = fs::canonicalize(dir.path()).unwrap();
    // The real confinement names the rendezvous by its resolved path, as the kernel sees
    // it: a temporary directory reached through a link (macOS: `/var` is a link to
    // `/private/var`) must be given resolved, as every production runtime folder is.
    config.runtime = root.clone();
    let critical = root.join("critical");
    fs::create_dir(&critical).unwrap();
    config.critical_paths = vec![critical];
    (dir, config)
}

#[test]
fn a_confined_child_with_a_wrong_handoff_ack_is_refused() {
    let message =
        confined_start("send {\"v\":1,\"event\":\"fds.received\",\"of\":\"hello\"}\nstay\n")
            .to_string();
    assert_eq!(
        message, "web process start failed: the handoff ACK does not acknowledge the handoff",
        "the child passed the sandbox verification and reached the ACK check"
    );
}

#[test]
fn a_confined_child_that_closes_control_before_its_handoff_ack_is_refused() {
    // Wait for the handoff to arrive, then close the control connection and stay alive.
    let message = confined_start("await-handoff\npause 500\nclose\nstay\n").to_string();
    assert_eq!(
        message,
        "web process start failed: control EOF before the handoff ACK"
    );
}

#[test]
fn the_rendezvous_is_owner_only_when_the_child_connects() {
    // The child refuses (exit 7) unless the rendezvous socket is 0600 in a 0700
    // folder (exit 8), then sends a mismatched ready line so the start stops right after.
    let steps = "first-line\nowner-only\nconnect\nsend {\"v\":1,\"event\":\"other\"}\nstay\n";
    let message = start_scripted(steps, |_, _, _| Ok(None)).to_string();
    assert!(
        message.contains("the ready line does not match ready_for_secrets"),
        "{message}"
    );
}

#[test]
fn a_started_web_serves_but_is_not_reported_before_web_serving() {
    // Confined, acknowledged: the start succeeds, but nothing reported serving yet.
    let (_dir, config) = confined_config("ack\nstay\n");
    let mut running = start(&config, Arc::new(NoOps)).unwrap();
    assert!(running.serving());
    assert!(
        !running.reported(),
        "running is reported only after web.serving"
    );
    // It stays: confined, the child waits without anything that would end it.
    thread::sleep(Duration::from_millis(300));
    assert!(running.serving());
}

/// A child that replaces its image after it connected (a new image, the same process id)
/// and before its ready line. The scripted re-checks answer the admitted identity, so only
/// the system's own check of the ready line's peer can see the change. What identifies an
/// image here decides the one outcome.
#[test]
fn a_child_that_exec_ed_after_connecting_is_refused_at_the_ready_line_or_as_unconfined() {
    let steps = format!("first-line\nconnect\npause 500\nexec\nsend {READY}\nstay\n");
    let admitted = Arc::new(Mutex::new(None));
    let error = start_scripted(&steps, move |step, n, pid| {
        let mut admitted = lock(&admitted);
        if step == crate::server_check::Step::Admit && n == 1 {
            *admitted = child_token(pid)?;
        }
        Ok(*admitted)
    });
    let message = error.to_string();
    match image_identity() {
        // The peer is identified by its image: the new image is not the admitted child.
        ImageIdentity::CodeSignature => assert_eq!(
            message,
            "web process start failed: the ready-line peer is not the admitted child"
        ),
        // The peer is identified by its user and process id, which an `exec` keeps (the
        // admission's own limit: it rests on the verified image never calling `exec`).
        // The new image is refused all the same, because it confined nothing; the real
        // web process cannot `exec` once it is confined.
        ImageIdentity::VerifiedBytes => assert!(
            message.starts_with("web process start failed: verify the web process sandbox: "),
            "{message}"
        ),
    }
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

#[test]
fn a_refusal_line_is_found_only_whole_printable_and_bounded() {
    let reason = "gateway confinement unavailable: seccomp failed with EPERM (Operation not \
                  permitted (os error 1))";
    let line = format!("{REFUSAL_PREFIX}{reason}\n");
    assert_eq!(refusal_reason(line.as_bytes()), Some(reason));
    // After other lines, as a debug build's preload notice, and before more.
    let noisy = format!("agentic-org-web: explicit debug unpinned fixture preload\n{line}more\n");
    assert_eq!(refusal_reason(noisy.as_bytes()), Some(reason));
    // Only the first refusal line counts.
    let twice = format!("{line}{REFUSAL_PREFIX}another\n");
    assert_eq!(refusal_reason(twice.as_bytes()), Some(reason));
    let longest = "r".repeat(REASON_BYTES);
    assert_eq!(
        refusal_reason(format!("{REFUSAL_PREFIX}{longest}\n").as_bytes()),
        Some(longest.as_str())
    );
    for not_one in [
        String::new(),
        "agentic-org-web: authenticated startup unavailable\n".to_owned(),
        // Cut off: no line feed was kept.
        format!("{REFUSAL_PREFIX}{reason}"),
        // The prefix must start the line.
        format!("x{REFUSAL_PREFIX}{reason}\n"),
        format!("{REFUSAL_PREFIX}\n"),
        format!("{REFUSAL_PREFIX}{longest}r\n"),
        format!("{REFUSAL_PREFIX}a\tb\n"),
        format!("{REFUSAL_PREFIX}a\u{1b}[31mb\n"),
        format!("{REFUSAL_PREFIX}caf\u{e9}\n"),
        format!("{REFUSAL_PREFIX}a\rb\n"),
        // A malformed first refusal line is not skipped for a later one.
        format!("{REFUSAL_PREFIX}\n{line}"),
    ] {
        assert_eq!(refusal_reason(not_one.as_bytes()), None, "{not_one:?}");
    }
    assert_eq!(
        REFUSAL_PREFIX,
        "agentic-org-web: startup refused: confinement: "
    );
}

#[test]
fn only_the_start_of_a_childs_standard_error_is_kept() {
    let reason = "kept";
    let early = format!("{REFUSAL_PREFIX}{reason}\n{}", "x".repeat(100_000));
    let captured =
        Stderr::capture(io::Cursor::new(early.into_bytes()), "stderr-unit-test").unwrap();
    assert!(matches!(captured.ending(WAIT), Ending::Refused(found) if found == reason));
    assert_eq!(lock(&captured.kept).0.len(), STDERR_KEPT);
    // A refusal line after the kept bytes is not seen, and nothing whole was kept.
    let late = format!("{}\n{REFUSAL_PREFIX}{reason}\n", "x".repeat(STDERR_KEPT));
    let captured = Stderr::capture(io::Cursor::new(late.into_bytes()), "stderr-unit-test").unwrap();
    assert!(matches!(captured.ending(WAIT), Ending::Silent));
    assert!(lock(&captured.kept).1, "read to its end");
}

/// X5 review, fourth round (V-c): a child that ended before it connected without a refusal
/// line is reported with the last whole line it wrote (after a notice before it), as
/// printable ASCII and bounded; a line cut off at the end is not taken.
#[test]
fn a_child_that_ended_otherwise_is_reported_with_its_last_whole_line() {
    let said = |text: &str| {
        let captured = Stderr::capture(
            io::Cursor::new(text.as_bytes().to_vec()),
            "stderr-unit-test",
        )
        .unwrap();
        match captured.ending(WAIT) {
            Ending::Said(line) => Some(line),
            Ending::Silent => None,
            Ending::Refused(reason) => panic!("a refusal: {reason}"),
        }
    };
    assert_eq!(
        said(
            "agentic-org-web: explicit debug unpinned fixture preload; no production release \
             admission\nagentic-org-web: startup refused: ui build: InvalidKey\n"
        )
        .as_deref(),
        Some("agentic-org-web: startup refused: ui build: InvalidKey")
    );
    assert_eq!(
        said("agentic-org-web: authenticated startup unavailable\n\n").as_deref(),
        Some("agentic-org-web: authenticated startup unavailable")
    );
    assert_eq!(
        said("one\nforged\u{1b}[2J\u{202e}line\n").as_deref(),
        Some("forged?[2J?line")
    );
    let long = said(&format!("{}\n", "y".repeat(900))).unwrap();
    assert_eq!(long.len(), REASON_BYTES);
    assert_eq!(said("whole\ncut off"), Some("whole".to_owned()));
    assert_eq!(said("cut off"), None);
    assert_eq!(said(""), None);
}

#[test]
fn a_landlock_line_is_found_only_whole_printable_and_bounded() {
    let reason =
        "landlock_create_ruleset failed with EPERM: the Landlock version query was refused";
    let line = format!("{LANDLOCK_NOT_APPLIED_PREFIX}{reason}\n");
    fn found(text: &str) -> Option<&str> {
        line_reason(text.as_bytes(), LANDLOCK_NOT_APPLIED_PREFIX)
    }
    assert_eq!(found(&line), Some(reason));
    let noisy = format!("agentic-org-web: explicit debug unpinned fixture preload\n{line}more\n");
    assert_eq!(found(&noisy), Some(reason));
    for not_one in [
        String::new(),
        format!("{LANDLOCK_NOT_APPLIED_PREFIX}{reason}"),
        format!("x{line}"),
        format!("{LANDLOCK_NOT_APPLIED_PREFIX}\n"),
        format!(
            "{LANDLOCK_NOT_APPLIED_PREFIX}{}\n",
            "r".repeat(REASON_BYTES + 1)
        ),
        format!("{LANDLOCK_NOT_APPLIED_PREFIX}a\u{1b}[31mb\n"),
        // A refusal line is not this line, and this line is no refusal.
        format!("{REFUSAL_PREFIX}{reason}\n"),
    ] {
        assert_eq!(found(&not_one), None, "{not_one:?}");
    }
    assert_eq!(refusal_reason(line.as_bytes()), None);
    assert_eq!(
        LANDLOCK_NOT_APPLIED_PREFIX,
        "agentic-org-web: confinement: landlock not applied: "
    );
}

#[test]
fn the_reason_for_a_missing_landlock_layer_is_read_from_a_running_childs_standard_error() {
    let line = format!("{LANDLOCK_NOT_APPLIED_PREFIX}landlock_restrict_self failed with EPERM");
    // The child stays alive, as a web process that serves does: the line is found
    // as soon as it is read, long before the wait ends.
    let (mut child, captured, _stdin) = stepping(&format!(
        "stderr agentic-org-web: some other notice\nstderr {line}\nstay\n"
    ));
    let asked = Instant::now();
    assert_eq!(
        captured.landlock_not_applied(WAIT).as_deref(),
        Some("landlock_restrict_self failed with EPERM")
    );
    assert!(asked.elapsed() < WAIT);
    assert!(matches!(child.try_wait(), Ok(false)));
    // A child that says nothing: the wait passes, and no longer.
    let (mut silent, captured, _stdin) = stepping("stay\n");
    let asked = Instant::now();
    assert_eq!(captured.landlock_not_applied(STDERR_GRACE), None);
    assert!(asked.elapsed() >= STDERR_GRACE);
    assert!(asked.elapsed() < WAIT);
    assert!(matches!(silent.try_wait(), Ok(false)));
    // Ended, or with its kept bytes full: nothing more can come, and nothing is
    // waited for.
    let ended = Stderr::capture(
        io::Cursor::new(b"something else\n".to_vec()),
        "stderr-unit-test",
    )
    .unwrap();
    let full = Stderr::capture(
        io::Cursor::new(vec![b'x'; 10 * STDERR_KEPT]),
        "stderr-unit-test",
    )
    .unwrap();
    let late = format!("{}\n{line}\n", "x".repeat(STDERR_KEPT));
    let late = Stderr::capture(io::Cursor::new(late.into_bytes()), "stderr-unit-test").unwrap();
    let asked = Instant::now();
    for captured in [ended, full, late] {
        assert_eq!(captured.landlock_not_applied(WAIT), None);
    }
    assert!(asked.elapsed() < WAIT);
}

#[test]
fn the_start_line_names_the_layers_and_why_landlock_is_missing() {
    use org_platform::confinement::Layers;
    let both = Layers {
        description: "the seccomp filter and Landlock ABI 7".to_owned(),
        landlock_missing: false,
    };
    // No reason is asked for where no layer is missing.
    assert_eq!(
        confined_by(&both, || panic!("no reason is read")),
        "confined by the seccomp filter and Landlock ABI 7"
    );
    let alone = Layers {
        description: "the seccomp filter alone".to_owned(),
        landlock_missing: true,
    };
    assert_eq!(
        confined_by(&alone, || Some(
            "landlock_create_ruleset failed with ENOSYS: this kernel has no Landlock".to_owned()
        )),
        "confined by the seccomp filter alone: Landlock was not applied: \
         landlock_create_ruleset failed with ENOSYS: this kernel has no Landlock"
    );
    assert_eq!(
        confined_by(&alone, || None),
        "confined by the seccomp filter alone: Landlock was not applied, and the web process \
         gave no reason"
    );
}

/// A real child, the fake web process taking `steps`, started and let run as the web
/// process is, with its captured standard error. The last value keeps its folder.
fn stepping(steps: &str) -> (Child, Arc<Stderr>, (fs::File, tempfile::TempDir)) {
    let dir = tempfile::tempdir().unwrap();
    let spawned = ServerCheck::Skip
        .admission(&fake_program(dir.path(), steps))
        .unwrap()
        .spawn(&[OsStr::new("--serve")], &web_env(), &SPAWN_LOCK)
        .unwrap();
    let captured = Stderr::capture(spawned.stderr, "stderr-unit-test").unwrap();
    (spawned.child, captured, (spawned.stdin, dir))
}

#[test]
fn a_child_that_exited_with_a_refusal_line_gives_the_failed_start_its_reason() {
    let line = format!("{REFUSAL_PREFIX}seccomp failed with ENOMEM");
    let (mut child, captured, _stdin) = stepping(&format!(
        "stderr agentic-org-web: some other notice\nstderr {line}\nexit 1\n"
    ));
    wait_for("the child exits", || matches!(child.try_wait(), Ok(true)));
    let error = with_refusal(
        failed("the child exited before connecting (exit code 1)"),
        &captured,
        &mut child,
    );
    assert_eq!(
        error.to_string(),
        "web process start failed: the child exited before connecting (exit code 1); it \
         reported that it cannot confine itself: seccomp failed with ENOMEM"
    );
}

/// A child that exited without a refusal line is no refusal: the cause stays, followed by
/// the last whole line it wrote, if any, shown as printable ASCII (X5 review, fourth
/// round, V-c).
#[test]
fn a_child_that_exited_without_a_refusal_line_keeps_the_cause_and_says_its_last_line() {
    for (script, expected) in [
        ("exit 1\n", "web process start failed: the cause"),
        (
            "stderr agentic-org-web: authenticated startup unavailable\nexit 1\n",
            "web process start failed: the cause; it wrote: agentic-org-web: authenticated \
             startup unavailable",
        ),
        (
            "stderr agentic-org-web: startup refused: ui build: InvalidKey\nexit 1\n",
            "web process start failed: the cause; it wrote: agentic-org-web: startup refused: \
             ui build: InvalidKey",
        ),
        // Not printable: a tab in the reason. No refusal, and the tab is not shown raw.
        (
            "stderr agentic-org-web: startup refused: confinement: a\tb\nexit 1\n",
            "web process start failed: the cause; it wrote: agentic-org-web: startup refused: \
             confinement: a?b",
        ),
        // No line feed: nothing whole was written.
        (
            "stderr-raw agentic-org-web: startup refused: confinement: cut\nexit 1\n",
            "web process start failed: the cause",
        ),
    ] {
        let (mut child, captured, _stdin) = stepping(script);
        wait_for("the child exits", || matches!(child.try_wait(), Ok(true)));
        let error = with_refusal(failed("the cause"), &captured, &mut child);
        assert_eq!(error.to_string(), expected, "{script}");
    }
}

#[test]
fn a_child_whose_control_stream_ended_gets_a_moment_to_exit_and_any_other_none() {
    let eof = || io::Error::other(ControlEof("web process start failed: control EOF".into()));
    // It closes its stream, writes its line and exits a moment later.
    let (mut child, captured, _stdin) = stepping(&format!(
        "pause 100\nstderr {REFUSAL_PREFIX}seccomp failed with EPERM\nexit 1\n"
    ));
    let error = with_refusal(eof(), &captured, &mut child);
    assert_eq!(
        error.to_string(),
        "web process start failed: control EOF; it reported that it cannot confine itself: \
         seccomp failed with EPERM"
    );
    // It stays alive: the cause is unchanged after the grace, and the child still
    // runs for its owner to kill.
    let (mut child, captured, _stdin) = stepping(&format!(
        "stderr {REFUSAL_PREFIX}written by a child that stays\nstay\n"
    ));
    let asked = Instant::now();
    let error = with_refusal(eof(), &captured, &mut child);
    assert!(asked.elapsed() >= EXIT_GRACE);
    assert_eq!(error.to_string(), "web process start failed: control EOF");
    assert!(matches!(child.try_wait(), Ok(false)));
    // Any other failure does not wait for a running child at all.
    let asked = Instant::now();
    let error = with_refusal(failed("the cause"), &captured, &mut child);
    assert!(asked.elapsed() < EXIT_GRACE);
    assert_eq!(error.to_string(), "web process start failed: the cause");
    assert!(matches!(child.try_wait(), Ok(false)));
}

#[test]
fn a_child_that_exited_after_its_sandbox_was_verified_is_refused() {
    // Still running: the check passes and reaps nothing.
    let dir = tempfile::tempdir().unwrap();
    let mut sleeping = held_child(dir.path());
    still_running(&mut sleeping.child).unwrap();
    still_running(&mut sleeping.child).unwrap();
    // Exited: the status that was read may be another process's by now.
    let (mut child, _captured, _stdin) = stepping("exit 4\n");
    wait_for("the child exits", || matches!(child.try_wait(), Ok(true)));
    assert_eq!(
        still_running(&mut child).unwrap_err().to_string(),
        "web process start failed: the child exited after its sandbox was verified (exit code 4)"
    );
}

#[test]
fn the_web_process_gets_nothing_of_the_hosts_environment() {
    assert!(web_env().pairs().is_empty());
    // A real child started with it sees no variable, whatever this process has
    // (this test process has at least a PATH).
    assert!(std::env::var_os("PATH").is_some());
    let dir = tempfile::tempdir().unwrap();
    let mut spawned = ServerCheck::Skip
        .admission(&fake_program(dir.path(), "env\n"))
        .unwrap()
        .spawn(&[OsStr::new("--serve")], &web_env(), &SPAWN_LOCK)
        .unwrap();
    let mut printed = String::new();
    spawned.stdout.read_to_string(&mut printed).unwrap();
    // It ran, and printed its whole environment: nothing.
    assert_eq!(printed, "environment:\n");
    wait_for("the child exits", || {
        matches!(spawned.child.try_wait(), Ok(true))
    });
}

/// The real web process for the two tests below: the `agentic-org-web` cargo built beside
/// this crate's tests (`cargo test --workspace` builds it before any test runs), copied
/// beside a one-file `ui` build, with a handler that answers its control calls, on a free
/// loopback port.
struct RealWeb {
    config: Configuration,
    handler: Arc<crate::service::Service>,
    offices: Arc<crate::offices::Offices>,
    _dir: tempfile::TempDir,
}

fn real_web() -> RealWeb {
    use org_proto::frame;
    use std::os::unix::fs::PermissionsExt;
    let built = sibling_program("agentic-org-web");
    let dir = tempfile::tempdir().unwrap();
    let root = fs::canonicalize(dir.path()).unwrap();
    for name in ["bin", "bin/ui", "runtime", "critical", "home"] {
        fs::create_dir(root.join(name)).unwrap();
        fs::set_permissions(root.join(name), fs::Permissions::from_mode(0o700)).unwrap();
    }
    let program = root.join("bin/agentic-org-web");
    fs::copy(built, &program).unwrap();
    // An admission refuses a program that its group or others can write.
    fs::set_permissions(&program, fs::Permissions::from_mode(0o700)).unwrap();
    // Startup validation hashes the entry's inline bootstrap into the CSP.
    fs::write(
        root.join("bin/ui/index.html"),
        b"<html><body>fixture<script>/* fixture bootstrap */</script></body></html>",
    )
    .unwrap();
    let (handler, offices) = crate::service::tests::web_handler(&root.join("home"));
    // The instance the handler answers `hello` for, asked as a web process asks.
    let (host, peer) = UnixStream::pair().unwrap();
    peer.set_read_timeout(Some(WAIT)).unwrap();
    let (transfer, _end) = UnixDatagram::pair().unwrap();
    let asking = WebConnection::start(host, transfer, loopback(), handler.clone()).unwrap();
    let mut peer = io::BufReader::new(peer);
    let hello = org_proto::Request::new::<org_host_control::Hello>(1, &Default::default());
    frame::write_message(peer.get_mut(), &hello.unwrap()).unwrap();
    let answer: serde_json::Value = frame::read_message(&mut peer).unwrap().unwrap();
    let instance = answer["result"]["instance_id"].as_str().unwrap().to_owned();
    drop((asking, peer));
    RealWeb {
        config: Configuration {
            program,
            instance,
            runtime: root.join("runtime"),
            listen: loopback_on(free_port()),
            check: ServerCheck::gateway_for_build(true, None).unwrap(),
            critical_paths: vec![root.join("critical")],
            release: None,
        },
        handler,
        offices,
        _dir: dir,
    }
}

/// The production path as far as this module reaches: the host's own start of the
/// real web process. It is treated as started only once it is confined, and then
/// serves its listener. The same holds under a profile that refuses Landlock at each
/// of its three calls: the web process is confined by its filter alone, serves the
/// same way, and the host's log line for the start says so with the reason.
#[test]
fn the_real_web_process_is_started_only_confined_and_then_serves() {
    use org_platform::confinement::testing::RefusedCall;
    const NAME: &str = "web::tests::the_real_web_process_is_started_only_confined_and_then_serves";
    const STARTED: &str = "authenticated browser web process started, ";
    let web = real_web();
    let mut running = start(&web.config, web.handler.clone()).unwrap();
    // What the host verified it is confined by, as its log line says it.
    let confined = running.confined();
    assert!(confined.starts_with("confined by "), "{confined}");
    assert!(!confined.contains("gave no reason"), "{confined}");
    println!("\nconfined: {confined}");
    wait_for("web.serving", || running.reported());
    assert!(running.serving());
    let address = (web.config.listen.address(), web.config.listen.port());
    let mut browser = std::net::TcpStream::connect(address).unwrap();
    browser.set_read_timeout(Some(WAIT)).unwrap();
    write!(
        browser,
        "GET /api/v1/version HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nConnection: close\r\n\r\n",
        address.1
    )
    .unwrap();
    let mut reply = String::new();
    browser.read_to_string(&mut reply).unwrap();
    assert!(reply.starts_with("HTTP/1.1 200 "), "{reply}");
    // What a client can make it do without signing in: refuse, time nothing out,
    // and stay confined and alive. Each of these ends in a refusal or a close,
    // never in the filter killing the process.
    let host = format!("Host: 127.0.0.1:{}\r\n", address.1);
    let sends: [(&str, Vec<u8>); 5] = [
        (
            "a request head far beyond the limit",
            format!(
                "GET / HTTP/1.1\r\n{host}X-Fill: {}\r\n\r\n",
                "h".repeat(600_000)
            )
            .into_bytes(),
        ),
        (
            "a megabyte posted to the one route",
            format!(
                "POST /api/v1/version HTTP/1.1\r\n{host}Origin: http://127.0.0.1:{}\r\n\
                 Content-Type: application/json\r\nConnection: close\r\nContent-Length: \
                 1048576\r\n\r\n{}",
                address.1,
                "b".repeat(1 << 20)
            )
            .into_bytes(),
        ),
        (
            "a megabyte posted to a missing route",
            format!(
                "POST /api/v1/none HTTP/1.1\r\n{host}Origin: http://127.0.0.1:{}\r\n\
                 Connection: close\r\nTransfer-Encoding: chunked\r\n\r\n100000\r\n{}\r\n0\r\n\r\n",
                address.1,
                "c".repeat(1 << 20)
            )
            .into_bytes(),
        ),
        ("not HTTP", vec![0x16, 0x03, 0x01, 0xff, 0xff, 0, 0, 0]),
        ("half a request head", b"GET / HTTP/1.1\r\nHost: ".to_vec()),
    ];
    for (what, bytes) in &sends {
        let mut client = std::net::TcpStream::connect(address).unwrap();
        client
            .set_read_timeout(Some(Duration::from_millis(500)))
            .unwrap();
        client.set_write_timeout(Some(WAIT)).unwrap();
        // The server may answer and close before it has read everything.
        let _ = client.write_all(bytes);
        let _ = client.read_to_end(&mut Vec::new());
        drop(client);
        assert!(running.serving(), "after {what}");
    }
    let mut browser = std::net::TcpStream::connect(address).unwrap();
    browser.set_read_timeout(Some(WAIT)).unwrap();
    write!(
        browser,
        "GET /api/v1/version HTTP/1.1\r\n{host}Connection: close\r\n\r\n"
    )
    .unwrap();
    let mut reply = String::new();
    browser.read_to_string(&mut reply).unwrap();
    assert!(reply.starts_with("HTTP/1.1 200 "), "{reply}");
    assert!(running.serving());
    // The host goes away: its end of the child's standard input closes. The web
    // process stops serving and exits by itself, normally, not by a signal.
    let Running {
        control,
        mut process,
        _stdin,
        ..
    } = running;
    drop(_stdin);
    wait_for("the web process exits", || {
        matches!(process.try_wait(), Ok(true))
    });
    assert_eq!(process.ended().as_deref(), Some("exit code 0"));
    drop(control);
    // The supervisor's own start of it: logged once, with the same words, and it
    // serves.
    let stopping = AtomicBool::new(false);
    let state = Mutex::new(GatewayState::NotConfigured);
    let (lines, log) = recording();
    let image = Image::new(&web.config.check, &web.config.program);
    thread::scope(|scope| {
        let supervisor = scope.spawn(|| {
            supervise(
                &stopping,
                &state,
                &|| true,
                Timing::PRODUCTION,
                &log,
                || {
                    Running::start(
                        &web.config,
                        &image,
                        None,
                        web.handler.clone(),
                        START_WAIT,
                        &stopping,
                    )
                },
            )
        });
        wait_for("the supervised web process serves", || {
            *lock(&state) == GatewayState::Running
        });
        let mut browser = std::net::TcpStream::connect(address).unwrap();
        browser.set_read_timeout(Some(WAIT)).unwrap();
        write!(
            browser,
            "GET /api/v1/version HTTP/1.1\r\n{host}Connection: close\r\n\r\n"
        )
        .unwrap();
        let mut reply = String::new();
        browser.read_to_string(&mut reply).unwrap();
        assert!(reply.starts_with("HTTP/1.1 200 "), "{reply}");
        stopping.store(true, Ordering::SeqCst);
        supervisor.thread().unpark();
    });
    let logged = lock(&lines).clone();
    assert_eq!(logged, [format!("{STARTED}{confined}")]);
    println!("log: {}", logged[0]);
    web.offices.shutdown();
    if std::env::var_os(UNDER_PROFILE).is_some() {
        return;
    }
    // All of the above again in a host under a profile that refuses Landlock.
    let not_permitted = "EPERM (Operation not permitted (os error 1))";
    for (call, reason) in [
        (
            RefusedCall::LandlockCreateRuleset,
            "landlock_create_ruleset failed with EPERM: the Landlock version query was refused"
                .to_owned(),
        ),
        (
            RefusedCall::LandlockRuleset,
            format!("landlock_create_ruleset failed with {not_permitted}"),
        ),
        (
            RefusedCall::LandlockRestrictSelf,
            format!("landlock_restrict_self failed with {not_permitted}"),
        ),
    ] {
        let Some(printed) = under_profile(NAME, call) else {
            return;
        };
        const ALONE: &str = "confined by the seccomp filter alone: Landlock was not applied: ";
        // A kernel without Landlock gives its own reason before a profile can.
        let said = |line: &str, prefix: &str| {
            line.strip_prefix(prefix)
                .and_then(|rest| rest.strip_prefix(ALONE))
                .is_some_and(|given| {
                    given == reason
                        || given.contains("this kernel has no Landlock")
                        || given.contains("is not an enabled security module")
                })
        };
        assert!(
            printed.lines().any(|line| said(line, "confined: ")),
            "{call:?}: {printed}"
        );
        assert!(
            printed
                .lines()
                .any(|line| said(line, &format!("log: {STARTED}"))),
            "{call:?}: {printed}"
        );
        // For whoever reads this test's output: what that host logged.
        for line in printed.lines().filter(|line| line.starts_with("log: ")) {
            println!("under a profile that refuses {call:?}: {line}");
        }
    }
}

/// ADR-0017 LNX-3 for the web process's own refusal: where the host's question
/// passes and the installation of the filter then fails, the real web process exits
/// before it reports ready, and every failed start is logged with the call and the
/// error it named. Nothing is handed to it.
#[test]
fn lnx_3_the_real_web_process_that_cannot_confine_itself_is_logged_with_its_reason() {
    use org_platform::confinement::testing::RefusedCall;
    const NAME: &str =
        "web::tests::lnx_3_the_real_web_process_that_cannot_confine_itself_is_logged_with_its_reason";
    // The child connects, fails to install its filter, drops its stream and exits.
    // The host meets one of the two, whichever it reaches first, and the same reason.
    const ENDED: [&str; 2] = [
        "web process start failed: control EOF before the ready line",
        "web process start failed: the child exited before its ready line (exit code 1)",
    ];
    const REASON: &str = "; it reported that it cannot confine itself: gateway confinement \
                          unavailable: seccomp failed with ENOMEM (Cannot allocate memory (os \
                          error 12))";
    let is_cause = |text: &str| ENDED.iter().any(|ended| text == format!("{ended}{REASON}"));
    if std::env::var_os(UNDER_PROFILE).is_some() {
        // The host under the profile.
        let web = real_web();
        org_platform::confinement::web_confinement_supported().unwrap();
        let error = start_error(&web.config);
        println!("\nrefused: {error}");
        // The supervisor: each failed start is logged with that cause, up to the
        // limit, and the web service ends as failed.
        let state = Mutex::new(GatewayState::NotConfigured);
        let (lines, log) = recording();
        let timing = Timing {
            serving: Duration::from_secs(5),
            stable: Duration::from_secs(60),
            backoff: Duration::from_millis(1),
        };
        let image = Image::new(&web.config.check, &web.config.program);
        supervise(
            &AtomicBool::new(false),
            &state,
            &|| true,
            timing,
            &log,
            || {
                Running::start(
                    &web.config,
                    &image,
                    None,
                    web.handler.clone(),
                    START_WAIT,
                    &GOING,
                )
            },
        );
        assert_eq!(*lock(&state), GatewayState::Failed);
        for line in lock(&lines).iter() {
            println!("log: {line}");
        }
        web.offices.shutdown();
        return;
    }
    let Some(printed) = under_profile(NAME, RefusedCall::SeccompInstall) else {
        return;
    };
    let lines: Vec<&str> = printed.lines().collect();
    let refused: Vec<&str> = lines
        .iter()
        .filter_map(|line| line.strip_prefix("refused: "))
        .collect();
    assert!(
        matches!(refused[..], [cause] if is_cause(cause)),
        "{printed}"
    );
    let logged: Vec<&str> = lines
        .iter()
        .filter_map(|line| line.strip_prefix("log: "))
        .collect();
    assert_eq!(logged.len(), MAX_STARTS, "{printed}");
    for line in &logged[..MAX_STARTS - 1] {
        let cause = line
            .strip_prefix("authenticated browser restarting in ")
            .and_then(|rest| rest.split_once(" because "))
            .map(|(_, cause)| cause);
        assert!(cause.is_some_and(is_cause), "{line}");
    }
    let last = logged[MAX_STARTS - 1].strip_prefix(&format!(
        "authenticated browser restart limit reached after {MAX_STARTS} failed starts; last: "
    ));
    assert!(last.is_some_and(is_cause), "{printed}");
}
