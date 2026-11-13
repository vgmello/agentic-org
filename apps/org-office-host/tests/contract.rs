//! The host's office contract (org-proto, docs/design/office-host-boundary.md §4): status reasons,
//! `next_restart_at`, `office.status_changed`, the give-up rule, and `office.restart`, against
//! the fake office server. App tokens are retired (launch v3).
//!
//! Retired wire calls are refused under every peer configuration. Native restart lifecycle
//! checks use the temporary in-process fixture; these gate tests do not establish
//! signed-peer authorization. Real app_auth wire coverage returns with verified sessions in2b.3.

// Every host here keeps its offices' keys in a development key folder, which only debug builds
// have (a release host uses the Keychain, which tests never touch): debug builds only. Many
// tests also need the development fallback, which only debug builds have.
#![cfg(debug_assertions)]
// Only macOS can spawn the office servers the other tests here start (BUG-187); off macOS their
// helpers are unused.
#![cfg_attr(not(target_os = "macos"), allow(unused_imports, dead_code))]

mod support;

use std::path::Path;
use std::thread;
use std::time::{Duration, Instant, SystemTime};

use org_office_host::{Config, Host};
use org_proto::{
    ApiError, ErrorCode, OfficeAppToken, OfficeAppTokenParams, OfficeRestart, OfficeRestartParams,
    OfficeStatus, OfficeStatusChanged, OfficeStatusReason,
};
use serde_json::{json, Value};
use support::{eventually, hq, office_info, rfc3339, Client, TestHome};

const WAIT: Duration = Duration::from_secs(10);
const A: &str = "01AAAAAAAAAAAAAAAAAAAAAAAA";
const B: &str = "01BBBBBBBBBBBBBBBBBBBBBBBB";
const T: &str = "2026-11-08T00:00:00.000Z";

/// A matching development peer policy; retired wire calls remain refused.
#[cfg(debug_assertions)]
fn app_config(home: &TestHome) -> Config {
    let mut config = home.config();
    config.dev_app_path = Some(std::env::current_exe().unwrap());
    config
}

/// A negotiated client used to test refusal of retired wire calls.
fn app_client(socket: &Path) -> Client {
    let mut client = Client::connect(socket);
    client
        .call::<org_proto::Hello>(&org_proto::HelloParams::default())
        .unwrap();
    client
}

fn wire_app_token(socket: &Path, office_id: &str) -> Result<org_proto::AppToken, ApiError> {
    app_client(socket)
        .call::<OfficeAppToken>(&OfficeAppTokenParams {
            office_id: office_id.into(),
        })
        .map(|result| result.app_token)
}

fn wire_restart(socket: &Path, office_id: &str) -> Result<(), ApiError> {
    app_client(socket)
        .call::<OfficeRestart>(&OfficeRestartParams {
            office_id: office_id.into(),
        })
        .map(drop)
}

fn restart(host: &Host, office_id: &str) -> Result<(), ApiError> {
    org_office_host::test_support::Fixture::new(host).restart(office_id)
}

fn wait_for_state(
    host: &Host,
    office_id: &str,
    status: OfficeStatus,
    reason: Option<OfficeStatusReason>,
) -> org_proto::OfficeInfo {
    let mut last = None;
    let deadline = Instant::now() + WAIT;
    loop {
        let info = office_info(host, office_id).expect("the office is listed");
        if info.status == status && info.status_reason == reason {
            return info;
        }
        assert!(
            Instant::now() < deadline,
            "{office_id} never became {status:?} ({reason:?}); last {last:?}"
        );
        last = Some(info);
        thread::sleep(Duration::from_millis(20));
    }
}

#[cfg(target_os = "macos")]
#[test]
fn retired_calls_are_refused_under_each_non_app_policy() {
    let home = TestHome::registered();
    home.write_office(&hq(A, "01ORGA", "Acme", T));
    // No development app and no team id: nobody. Debug builds also try another executable as the
    // allowed development app (this test process isn't it), and one that isn't built yet.
    #[allow(unused_mut)]
    let mut configs = vec![home.config()];
    #[cfg(debug_assertions)]
    for dev_app in ["/bin/sh", "/nonexistent/Org.app/Contents/MacOS/Org"] {
        let mut config = home.config();
        config.dev_app_path = Some(std::path::PathBuf::from(dev_app));
        configs.push(config);
    }
    for config in configs {
        // The same home, one host after another.
        let host = support::restart_host(&home, &config);
        wait_for_state(&host, A, OfficeStatus::Active, None);
        let err = wire_app_token(host.socket_path(), A).unwrap_err();
        assert_eq!(err.code, ErrorCode::NotAuthorized);
        let err = wire_restart(host.socket_path(), A).unwrap_err();
        assert_eq!(err.code, ErrorCode::NotAuthorized);
        // The check comes first: even an unknown office is `not_authorized`.
        let err = wire_app_token(host.socket_path(), "01NOSUCHOFFICENOSUCHOFFIC").unwrap_err();
        assert_eq!(err.code, ErrorCode::NotAuthorized);
        host.shutdown();
    }
}

// Needs the development fallback: a debug build.
// Needs the development fallback: a debug build.
#[cfg(debug_assertions)]
#[cfg(target_os = "macos")]
#[test]
fn native_restart_follows_the_runtime_contract() {
    let home = TestHome::registered();
    home.write_office(&hq(A, "01ORGA", "Acme", T));
    home.write_office(&hq(B, "01ORGB", "Beta", T));
    // A's key is missing at first, then found; B's server speaks another version.
    home.script(json!({
        A: [{"do": "fail", "code": "key_missing"}, {"do": "ready"}],
        B: {"do": "fail", "code": "unsupported_version"}
    }));
    let host = home.start(app_config(&home)).unwrap();
    wait_for_state(
        &host,
        A,
        OfficeStatus::Unavailable,
        Some(OfficeStatusReason::KeyMissing),
    );
    wait_for_state(
        &host,
        B,
        OfficeStatus::Unavailable,
        Some(OfficeStatusReason::UnsupportedVersion),
    );
    // Fatal: no restart on its own.
    thread::sleep(Duration::from_millis(500));
    assert_eq!(home.runs(A), 1);

    // `key_missing` is retried by a restart; this time the key is there.
    restart(&host, A).unwrap();
    wait_for_state(&host, A, OfficeStatus::Active, None);
    assert_eq!(home.runs(A), 2);
    // Running: `conflict`.
    let err = restart(&host, A).unwrap_err();
    assert_eq!(err.code, ErrorCode::Conflict);
    // A reason a restart can't fix: `unavailable`, saying why.
    let err = restart(&host, B).unwrap_err();
    assert_eq!(err.code, ErrorCode::Unavailable);
    assert!(err.message.contains("version"), "{}", err.message);
    assert_eq!(home.runs(B), 1);
    let err = restart(&host, "01NOSUCHOFFICENOSUCHOFFIC").unwrap_err();
    assert_eq!(err.code, ErrorCode::NotFound);
    host.shutdown();
}

#[cfg(target_os = "macos")]
#[test]
fn every_way_a_start_ends_has_its_reason() {
    use OfficeStatus::{Offline, Unavailable};
    use OfficeStatusReason as R;
    // The ready timeout is short only where the case needs it to pass (a server that hangs, or
    // lingers after a non-fatal `failed`). Elsewhere it is long, so a slow first launch of the
    // fake server under load can't turn into `start_timeout`.
    let ready_timeout = |step: &Value| {
        if step.get("do") == Some(&json!("hang")) {
            Duration::from_secs(1)
        } else if step.get("linger") == Some(&json!(true)) {
            Duration::from_secs(4)
        } else {
            WAIT
        }
    };
    let cases = [
        (json!({"do": "crash"}), Offline, R::Crashed),
        (
            json!({"do": "hang", "ignore_eof": true}),
            Offline,
            R::StartTimeout,
        ),
        (json!({"do": "fail", "code": "in_use"}), Offline, R::InUse),
        // A non-fatal `failed`, then nothing until the ready timeout: its own reason stands.
        (
            json!({"do": "fail", "code": "in_use", "linger": true}),
            Offline,
            R::InUse,
        ),
        (
            json!({"do": "fail", "code": "storage_error"}),
            Offline,
            R::StorageError,
        ),
        (
            json!({"do": "fail", "code": "a_code_from_the_future"}),
            Offline,
            R::StorageError,
        ),
        (
            json!({"do": "fail", "code": "key_missing"}),
            Unavailable,
            R::KeyMissing,
        ),
        (
            json!({"do": "fail", "code": "unsupported_version"}),
            Unavailable,
            R::UnsupportedVersion,
        ),
        (
            json!({"do": "fail", "code": "invalid_request"}),
            Unavailable,
            R::InvalidRequest,
        ),
    ];
    for (step, status, reason) in cases {
        let home = TestHome::registered();
        home.write_office(&hq(A, "01ORGA", "Acme", T));
        home.script(json!({ A: step.clone() }));
        let mut config = home.config();
        config.timings.ready_timeout = ready_timeout(&step);
        // Long enough to see the office offline, waiting for its restart.
        config.timings.backoff_initial = Duration::from_secs(5);
        let host = home.start(config).unwrap();
        let before = rfc3339(SystemTime::now());
        let info = wait_for_state(&host, A, status, Some(reason));
        let after = rfc3339(SystemTime::now() + Duration::from_secs(6));
        match status {
            // A restart is pending, within the backoff.
            Offline => {
                let at = info.next_restart_at.expect("next_restart_at while offline");
                assert!(before <= at && at <= after, "{before} <= {at} <= {after}");
            }
            _ => assert_eq!(info.next_restart_at, None, "{step}"),
        }
        host.shutdown();
    }
}

#[cfg(target_os = "macos")]
#[test]
fn status_changes_are_pushed_on_subscribe() {
    let home = TestHome::registered();
    home.write_office(&hq(A, "01ORGA", "Acme", T));
    // It gets ready after a moment (so this test subscribes first) and crashes at once, then
    // runs. Events are level-triggered (each carries the state as it is then; a state may be
    // skipped), so the test waits for `active` after an `offline`.
    home.script(json!({ A: [
        {"do": "crash", "at": "ready", "delay_ms": 800},
        {"do": "ready"}
    ]}));
    let mut config = home.config();
    config.timings.backoff_initial = Duration::from_millis(300);
    let host = home.start(config).unwrap();
    let mut events = Client::for_host(&host);
    events
        .call::<org_proto::Subscribe>(&org_proto::SubscribeParams::default())
        .unwrap();
    let mut seen = Vec::new();
    loop {
        let event = events
            .next_event_named("office.status_changed", WAIT)
            .expect("office.status_changed");
        let office = event.data::<OfficeStatusChanged>().unwrap().unwrap().office;
        assert_eq!(office.id, A);
        let done = office.status == OfficeStatus::Active
            && seen
                .iter()
                .any(|o: &org_proto::OfficeInfo| o.status == OfficeStatus::Offline);
        seen.push(office);
        if done {
            break;
        }
    }
    let offline = seen
        .iter()
        .find(|o| o.status == OfficeStatus::Offline)
        .expect("an offline event");
    assert_eq!(offline.status_reason, Some(OfficeStatusReason::Crashed));
    assert!(offline.next_restart_at.is_some());
    let last = seen.last().unwrap();
    assert_eq!((last.status_reason, &last.next_restart_at), (None, &None));
    host.shutdown();
}

// Needs the development fallback: a debug build.
#[cfg(debug_assertions)]
#[cfg(target_os = "macos")]
#[test]
fn the_give_up_rule_stops_restarts_until_office_restart() {
    let home = TestHome::registered();
    home.write_office(&hq(A, "01ORGA", "Acme", T));
    home.script(json!({ A: {"do": "crash"} }));
    let mut config = app_config(&home);
    config.timings.give_up_after = 3;
    config.timings.backoff_initial = Duration::from_millis(50);
    config.timings.backoff_max = Duration::from_millis(50);
    let host = home.start(config).unwrap();
    let info = wait_for_state(
        &host,
        A,
        OfficeStatus::Unavailable,
        Some(OfficeStatusReason::RestartsExhausted),
    );
    assert_eq!(info.next_restart_at, None);
    thread::sleep(Duration::from_millis(500));
    assert_eq!(home.runs(A), 3, "it kept restarting");

    // office.restart starts it again, with the count back at 0: three more tries.
    restart(&host, A).unwrap();
    eventually(WAIT, "three more runs", || home.runs(A) == 6);
    wait_for_state(
        &host,
        A,
        OfficeStatus::Unavailable,
        Some(OfficeStatusReason::RestartsExhausted),
    );
    host.shutdown();
}

#[cfg(target_os = "macos")]
#[test]
fn uptime_counts_from_ready_not_from_the_spawn() {
    let home = TestHome::registered();
    home.write_office(&hq(A, "01ORGA", "Acme", T));
    // Each run takes 1.2 s to get ready, then crashes at once: up for 1.2 s from its spawn,
    // over the 1 s that makes a fresh start, but up for no time after `ready`.
    home.script(json!({ A: {"do": "crash", "at": "ready", "delay_ms": 1200} }));
    let mut config = home.config();
    config.timings.give_up_after = 3;
    config.timings.backoff_reset_after = Duration::from_secs(1);
    config.timings.backoff_initial = Duration::from_millis(50);
    config.timings.backoff_max = Duration::from_millis(50);
    let host = home.start(config).unwrap();
    // Every run is a failed restart, so the give-up rule ends it after three.
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        let info = office_info(&host, A).unwrap();
        if info.status == OfficeStatus::Unavailable {
            assert_eq!(
                info.status_reason,
                Some(OfficeStatusReason::RestartsExhausted)
            );
            break;
        }
        assert!(Instant::now() < deadline, "never gave up: {info:?}");
        thread::sleep(Duration::from_millis(50));
    }
    assert_eq!(home.runs(A), 3);
    host.shutdown();
}

#[cfg(target_os = "macos")]
#[test]
fn a_status_change_publishes_the_whole_office_record() {
    // The event's data is the office's whole record, as `office.list` shows it.
    let home = TestHome::registered();
    home.write_office(&hq(A, "01ORGA", "Acme", T));
    home.script(json!({ A: {"do": "ready", "delay_ms": 600} }));
    let host = home.start(home.config()).unwrap();
    let mut events = Client::for_host(&host);
    events
        .call::<org_proto::Subscribe>(&org_proto::SubscribeParams::default())
        .unwrap();
    let event = events
        .next_event_named("office.status_changed", WAIT)
        .expect("office.status_changed");
    let data: Value = event.data.clone();
    let office = event.data::<OfficeStatusChanged>().unwrap().unwrap().office;
    assert_eq!(office.status, OfficeStatus::Active);
    assert_eq!(Some(office), office_info(&host, A));
    assert!(data["office"]["endpoint"].is_string(), "{data}");
    host.shutdown();
}

/// The retired token method remains refused when a second process writes an accepted socket.
/// This gate test does not establish app_auth's process-identity enforcement.
#[cfg(debug_assertions)]
#[cfg(target_os = "macos")]
#[test]
fn retired_calls_stay_refused_when_another_process_writes_the_connection() {
    use std::io::{BufRead, BufReader, Write};
    use std::os::fd::AsRawFd;
    use std::os::unix::process::CommandExt;

    let home = TestHome::registered();
    home.write_office(&hq(A, "01ORGA", "Acme", T));
    let host = home.start(app_config(&home)).unwrap();
    wait_for_state(&host, A, OfficeStatus::Active, None);

    let stream = std::os::unix::net::UnixStream::connect(host.socket_path()).unwrap();
    let _ = stream.set_read_timeout(Some(WAIT));
    let mut out = stream.try_clone().unwrap();
    let mut reader = BufReader::new(stream);
    let mut read_line = || {
        let mut line = String::new();
        assert!(reader.read_line(&mut line).unwrap() > 0, "the host hung up");
        serde_json::from_str::<org_proto::Response>(&line).unwrap()
    };
    // The host accepted this process's connection (and took its token) by the time it answers.
    out.write_all(b"{\"id\":1,\"method\":\"hello\",\"params\":{}}\n")
        .unwrap();
    assert_eq!(read_line().id, 1);

    // A child writes the call on the same connection (its fd 3).
    let fd = out.as_raw_fd();
    let request = format!(
        "{{\"id\":2,\"method\":\"office.app_token\",\"params\":{{\"office_id\":\"{A}\"}}}}"
    );
    let mut command = std::process::Command::new(std::env::current_exe().unwrap());
    command
        .args(["--exact", "write_on_fd3_when_asked", "--test-threads=1"])
        .env(WRITE_ON_FD3, request)
        .stdout(std::process::Stdio::null());
    // SAFETY: only async-signal-safe calls (dup2, fcntl) between fork and exec.
    unsafe {
        command.pre_exec(move || {
            let rc = if fd == 3 {
                libc::fcntl(3, libc::F_SETFD, 0)
            } else {
                libc::dup2(fd, 3)
            };
            if rc < 0 {
                Err(std::io::Error::last_os_error())
            } else {
                Ok(())
            }
        });
    }
    let mut child = command.spawn().unwrap();
    let answer = read_line();
    child.wait().unwrap();
    assert_eq!(answer.id, 2);
    match answer.outcome {
        org_proto::Outcome::Error(e) => assert_eq!(e.code, ErrorCode::NotAuthorized),
        org_proto::Outcome::Result(_) => panic!("the child got the app token"),
    }
    host.shutdown();
}

/// Set for the child of `retired_calls_stay_refused_when_another_process_writes_the_connection`.
const WRITE_ON_FD3: &str = "ORG_HOST_TEST_WRITE_ON_FD3";

/// Not a test on its own: in the child process of
/// `retired_calls_stay_refused_when_another_process_writes_the_connection`, writes the request
/// line it's given on fd 3 (the parent's connection), then lingers so the host sees it alive.
#[cfg(debug_assertions)]
#[test]
fn write_on_fd3_when_asked() {
    use std::io::Write;
    use std::os::fd::FromRawFd;
    let Ok(line) = std::env::var(WRITE_ON_FD3) else {
        return;
    };
    // SAFETY: fd 3 is the connection the parent handed down (dup2 before exec); it is never
    // closed here (ManuallyDrop), only written to.
    let mut connection = std::mem::ManuallyDrop::new(unsafe { std::fs::File::from_raw_fd(3) });
    connection
        .write_all(format!("{line}\n").as_bytes())
        .unwrap();
    thread::sleep(Duration::from_secs(1));
}

/// Retired token calls remain refused with any buffered suffix. Actual app_auth timing
/// enforcement is tested separately and needs session wire coverage again in2b.3.
#[cfg(debug_assertions)]
#[cfg(target_os = "macos")]
#[test]
fn retired_calls_stay_refused_with_a_buffered_suffix() {
    use std::io::{BufRead, BufReader, Write};

    let home = TestHome::registered();
    home.write_office(&hq(A, "01ORGA", "Acme", T));
    let host = home.start(app_config(&home)).unwrap();
    wait_for_state(&host, A, OfficeStatus::Active, None);
    let call = format!(
        "{{\"id\":2,\"method\":\"office.app_token\",\"params\":{{\"office_id\":\"{A}\"}}}}\n"
    );
    for after in ["{\"id\":3,\"method\":\"hello\",\"params\":{}}\n", "x"] {
        let stream = std::os::unix::net::UnixStream::connect(host.socket_path()).unwrap();
        let _ = stream.set_read_timeout(Some(WAIT));
        let mut out = stream.try_clone().unwrap();
        let mut reader = BufReader::new(stream);
        let mut answer = |id: u64| loop {
            let mut line = String::new();
            assert!(reader.read_line(&mut line).unwrap() > 0, "the host hung up");
            let response: org_proto::Response = serde_json::from_str(&line).unwrap();
            if response.id == id {
                return response;
            }
        };
        out.write_all(b"{\"id\":1,\"method\":\"hello\",\"params\":{}}\n")
            .unwrap();
        answer(1);
        out.write_all(format!("{call}{after}").as_bytes()).unwrap();
        match answer(2).outcome {
            org_proto::Outcome::Error(e) => {
                assert_eq!(e.code, ErrorCode::NotAuthorized, "{after:?}")
            }
            org_proto::Outcome::Result(_) => {
                panic!("the token was given with {after:?} after the call")
            }
        }
    }
    host.shutdown();
}
