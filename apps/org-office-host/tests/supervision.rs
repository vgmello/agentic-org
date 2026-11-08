//! Supervision, startup and shutdown, against the fake office server.

mod support;

use std::fs::{self, OpenOptions};
use std::os::unix::fs::PermissionsExt;
use std::thread;
use std::time::{Duration, Instant};

use org_office_host::{Host, HostError};
use org_office_launch::OfficeFile;
use org_proto::{
    Empty, ErrorCode, Hello, OfficeList, OfficeRole, OfficeStatus, OrganizationCreate,
    OrganizationCreateParams, OrganizationList,
};
use serde_json::json;
use support::{alive, eventually, hq, Client, TestHome};

const WAIT: Duration = Duration::from_secs(10);

const A: &str = "01AAAAAAAAAAAAAAAAAAAAAAAA";
const B: &str = "01BBBBBBBBBBBBBBBBBBBBBBBB";
const C: &str = "01CCCCCCCCCCCCCCCCCCCCCCCC";

fn status(host: &Host, office_id: &str) -> OfficeStatus {
    Client::connect(host.socket_path())
        .call::<OfficeList>(&Empty {})
        .unwrap()
        .offices
        .into_iter()
        .find(|o| o.id == office_id)
        .map_or(OfficeStatus::Unknown, |o| o.status)
}

fn wait_for(host: &Host, office_id: &str, want: OfficeStatus) {
    eventually(WAIT, &format!("{office_id} to be {want:?}"), || {
        status(host, office_id) == want
    });
}

#[test]
fn a_crashed_server_is_restarted_with_growing_backoff() {
    let home = TestHome::new();
    home.write_office(&hq(A, "01ORGA", "Acme", "2026-11-08T00:00:00.000Z"));
    home.script(json!({ A: [{"do": "crash"}, {"do": "crash"}, {"do": "ready"}] }));
    let host = Host::start(home.config()).unwrap(); // backoff 100 ms, then 200 ms
    wait_for(&host, A, OfficeStatus::Active);
    assert_eq!(home.runs(A), 3);
    let t = home.start_times(A);
    assert!(t[1] - t[0] >= 100, "first restart after {} ms", t[1] - t[0]);
    assert!(
        t[2] - t[1] >= 200,
        "second restart after {} ms",
        t[2] - t[1]
    );
    host.shutdown();
}

#[test]
fn in_use_and_unknown_failures_are_retried() {
    let home = TestHome::new();
    home.write_office(&hq(A, "01ORGA", "Acme", "2026-11-08T00:00:00.000Z"));
    home.script(json!({ A: [
        {"do": "fail", "code": "in_use"},
        {"do": "fail", "code": "a_code_from_the_future"},
        {"do": "ready"}
    ]}));
    let host = Host::start(home.config()).unwrap();
    wait_for(&host, A, OfficeStatus::Active);
    assert_eq!(home.runs(A), 3);
    host.shutdown();
}

#[test]
fn a_fatal_failure_makes_the_office_unavailable_without_restarts() {
    let home = TestHome::new();
    home.write_office(&hq(A, "01ORGA", "Acme", "2026-11-08T00:00:00.000Z"));
    home.write_office(&hq(B, "01ORGB", "Beta", "2026-11-08T00:00:00.000Z"));
    home.script(json!({
        A: {"do": "fail", "code": "key_missing"},
        B: {"do": "fail", "code": "unsupported_version"}
    }));
    let host = Host::start(home.config()).unwrap();
    wait_for(&host, A, OfficeStatus::Unavailable);
    wait_for(&host, B, OfficeStatus::Unavailable);
    // Several backoff periods (100 + 200 + 400 ms) pass without a restart.
    thread::sleep(Duration::from_millis(800));
    assert_eq!(home.runs(A), 1);
    assert_eq!(home.runs(B), 1);
    assert_eq!(status(&host, A), OfficeStatus::Unavailable);
    host.shutdown();
}

#[test]
fn a_server_that_is_never_ready_is_killed_and_restarted() {
    let home = TestHome::new();
    home.write_office(&hq(A, "01ORGA", "Acme", "2026-11-08T00:00:00.000Z"));
    home.script(json!({ A: [{"do": "hang", "ignore_eof": true}, {"do": "ready"}] }));
    let mut config = home.config();
    config.timings.ready_timeout = Duration::from_millis(1500);
    let host = Host::start(config).unwrap();
    assert_eq!(status(&host, A), OfficeStatus::Starting);
    wait_for(&host, A, OfficeStatus::Active);
    let pids = home.pids(A);
    assert_eq!(pids.len(), 2);
    assert!(!alive(pids[0]), "the hung server is still running");
    host.shutdown();
}

#[test]
fn startup_serves_stored_offices_cleans_unfinished_ones_and_skips_bad_ones() {
    let home = TestHome::new();
    let sat = OfficeFile::new(
        C,
        "Lab",
        OfficeRole::Satellite,
        "01ORGB",
        "Beta",
        "2026-11-08T00:00:00.000Z",
    );
    home.write_office(&hq(A, "01ORGA", "Acme", "2026-11-08T05:00:00.000Z"));
    home.write_office(&hq(B, "01ORGB", "Beta", "2026-11-08T00:00:00.000Z"));
    home.write_office(&sat);
    // Not offices: bad JSON, an office.json for another folder (moved aside), an unfinished
    // create.
    let bad = home.office_dir("01BADBADBADBADBADBADBADBAD");
    fs::create_dir_all(&bad).unwrap();
    fs::write(bad.join("office.json"), b"{nope").unwrap();
    let wrong = home.office_dir("01WRONGWRONGWRONGWRONGWRON");
    fs::create_dir_all(&wrong).unwrap();
    fs::write(
        wrong.join("office.json"),
        hq(A, "X", "X", "2026-11-08T00:00:00.000Z")
            .to_vec()
            .unwrap(),
    )
    .unwrap();
    let unfinished = home.office_dir("01UNFINISHEDUNFINISHEDUNFI");
    fs::create_dir_all(&unfinished).unwrap();
    // An unfinished folder whose lock a (pretend) server still holds is kept.
    let held = home.office_dir("01HELDHELDHELDHELDHELDHELD");
    fs::create_dir_all(&held).unwrap();
    let lock = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(false)
        .open(held.join("office.lock"))
        .unwrap();
    lock.lock().unwrap();

    let mut config = home.config();
    // The held folder's discard waits for its lock: give up on it quickly.
    config.timings.discard_timeout = Duration::from_millis(1500);
    let started = Instant::now();
    let host = Host::start(config).unwrap();
    // The discards run in the background: starting doesn't wait for them.
    assert!(started.elapsed() < Duration::from_secs(1));
    // Only the two folders without office.json were discarded; only the free one went, and
    // the held one's discard gave up after its timeout.
    eventually(WAIT, "the free unfinished folder to be discarded", || {
        !unfinished.exists()
    });
    eventually(WAIT, "the held folder's discard to give up", || {
        home.runs("discard") == 2 && started.elapsed() > Duration::from_millis(1500)
    });
    assert!(held.exists());
    assert!(bad.join("office.json").exists());
    // The folder holding another office's office.json was moved to .refused/, not discarded.
    assert!(!wrong.exists());
    assert!(home
        .path()
        .join("offices/.refused/01WRONGWRONGWRONGWRONGWRON/office.json")
        .exists());

    let orgs = Client::connect(host.socket_path())
        .call::<OrganizationList>(&Empty {})
        .unwrap()
        .organizations;
    let ids: Vec<_> = orgs.iter().map(|o| o.id.as_str()).collect();
    assert_eq!(ids, ["01ORGB", "01ORGA"], "ordered by created_at");
    let beta: Vec<_> = orgs[0].offices.iter().map(|o| o.id.as_str()).collect();
    assert_eq!(beta, [B, C], "HQ first");
    for id in [A, B, C] {
        wait_for(&host, id, OfficeStatus::Active);
        assert_eq!(home.runs(id), 1);
    }
    let offices = Client::connect(host.socket_path())
        .call::<OfficeList>(&Empty {})
        .unwrap()
        .offices;
    assert_eq!(offices.len(), 3);
    host.shutdown();
}

#[test]
fn a_second_host_on_the_same_home_fails_fast() {
    let home = TestHome::new();
    let first = Host::start(home.config()).unwrap();
    let started = Instant::now();
    let second = Host::start(home.config());
    assert!(matches!(second, Err(HostError::AlreadyRunning { .. })));
    assert!(started.elapsed() < Duration::from_secs(1));
    assert!(second
        .err()
        .unwrap()
        .to_string()
        .contains("already running"));
    // The first host is untouched.
    Client::connect(first.socket_path())
        .call::<OfficeList>(&Empty {})
        .unwrap();
    first.shutdown();
}

#[test]
fn shutdown_closes_stdins_so_servers_exit_cleanly_and_releases_the_home() {
    let home = TestHome::new();
    home.write_office(&hq(A, "01ORGA", "Acme", "2026-11-08T00:00:00.000Z"));
    home.write_office(&hq(B, "01ORGB", "Beta", "2026-11-08T00:00:00.000Z"));
    let host = Host::start(home.config()).unwrap();
    wait_for(&host, A, OfficeStatus::Active);
    wait_for(&host, B, OfficeStatus::Active);
    let socket = host.socket_path().to_path_buf();
    host.shutdown();
    for id in [A, B] {
        let pids = home.pids(id);
        assert_eq!(home.eof_exits(id), pids, "{id} didn't exit on stdin EOF");
        assert!(!alive(pids[0]));
    }
    assert!(!socket.exists(), "host.sock is still there");
    // host.lock is free again.
    support::restart_host(&home.config()).shutdown();
}

#[test]
fn a_server_that_ignores_eof_is_killed_after_the_grace_period() {
    let home = TestHome::new();
    home.write_office(&hq(A, "01ORGA", "Acme", "2026-11-08T00:00:00.000Z"));
    home.script(json!({ A: {"do": "ready", "ignore_eof": true} }));
    let mut config = home.config();
    config.timings.shutdown_grace = Duration::from_millis(300);
    let host = Host::start(config).unwrap();
    wait_for(&host, A, OfficeStatus::Active);
    let started = Instant::now();
    host.shutdown();
    let took = started.elapsed();
    assert!(took >= Duration::from_millis(300), "{took:?}");
    assert!(took < Duration::from_secs(5), "{took:?}");
    let pid = home.pids(A)[0];
    assert_eq!(home.eof_ignored(A), [pid], "it never saw EOF");
    assert!(!alive(pid), "it survived shutdown");
}

#[test]
fn a_home_too_long_for_its_sockets_refuses_to_start() {
    let home = TestHome::new();
    // run/<12 chars>.sock adds 22 bytes: make that exceed 103.
    let base = home.path();
    let pad = 103 - 22 - base.as_os_str().len();
    let long = base.join("x".repeat(pad));
    let config = org_office_host::Config::new(&long, support::fake_server());
    match Host::start(config) {
        Err(e @ HostError::SocketPathTooLong { .. }) => {
            assert!(e.to_string().contains("macOS allows at most 103"), "{e}");
        }
        Err(other) => panic!("{other}"),
        Ok(_) => panic!(
            "started with a {}-byte socket path",
            long.as_os_str().len() + 22
        ),
    }
    assert!(!long.exists(), "it touched the home before refusing");
}

#[test]
fn server_stderr_goes_to_an_owner_only_log_rotated_at_the_cap() {
    let home = TestHome::new();
    home.write_office(&hq(A, "01ORGA", "Acme", "2026-11-08T00:00:00.000Z"));
    home.script(json!({ A: {"do": "ready", "stderr_bytes": 10_000} }));
    let mut config = home.config();
    config.log_max_bytes = 4096;
    let host = Host::start(config).unwrap();
    let logs = home.path().join("logs");
    let handle = &A[A.len() - 12..];
    let (current, old) = (
        logs.join(format!("{handle}.log")),
        logs.join(format!("{handle}.log.1")),
    );
    let len = |p: &std::path::Path| fs::metadata(p).map(|m| m.len()).unwrap_or(0);
    // 10 000 bytes at a 4096-byte cap: 4096 (dropped), 4096 (kept as .1), 1808 (current).
    eventually(WAIT, "the log to be written", || len(&current) == 1808);
    assert_eq!(len(&old), 4096);
    for path in [&current, &old] {
        let mode = fs::metadata(path).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600, "{}", path.display());
    }
    host.shutdown();
}

#[test]
fn a_discard_refused_with_already_exists_or_storage_error_is_left_alone() {
    // The server finds an organization committed in the folder's databases (already_exists),
    // or databases it can't open or check (storage_error).
    for code in ["already_exists", "storage_error"] {
        let home = TestHome::new();
        let unfinished = home.office_dir("01UNFINISHEDUNFINISHEDUNFI");
        fs::create_dir_all(&unfinished).unwrap();
        home.write_office(&hq(A, "01ORGA", "Acme", "2026-11-08T00:00:00.000Z"));
        home.script(json!({"discard": {"do": "fail", "code": code}}));
        let host = Host::start(home.config()).unwrap();
        wait_for(&host, A, OfficeStatus::Active);
        eventually(WAIT, "the startup discard", || home.runs("discard") == 1);
        // Several backoff periods pass: no retry, and the folder is kept.
        thread::sleep(Duration::from_millis(800));
        assert_eq!(home.runs("discard"), 1, "{code}");
        assert!(unfinished.exists(), "{code}");
        host.shutdown();
        // Each start asks once, never in a loop.
        let host = support::restart_host(&home.config());
        wait_for(&host, A, OfficeStatus::Active);
        eventually(WAIT, "the startup discard", || home.runs("discard") == 2);
        thread::sleep(Duration::from_millis(500));
        assert_eq!(home.runs("discard"), 2, "{code}");
        assert!(unfinished.exists(), "{code}");
        host.shutdown();
    }
}

#[test]
fn a_folder_holding_another_offices_file_is_moved_aside_never_started_or_discarded() {
    const MISPLACED: &str = "01MISPLACEDMISPLACEDMISPLA";
    let home = TestHome::new();
    home.write_office(&hq(A, "01ORGA", "Acme", "2026-11-08T00:00:00.000Z"));
    // MISPLACED's folder holds an office.json for B (whose own folder doesn't exist).
    let file = hq(B, "01ORGB", "Beta", "2026-11-08T00:00:00.000Z")
        .to_vec()
        .unwrap();
    let misplaced = home.office_dir(MISPLACED);
    fs::create_dir_all(&misplaced).unwrap();
    fs::write(misplaced.join("office.json"), &file).unwrap();
    let moved = home.path().join("offices/.refused").join(MISPLACED);

    let host = Host::start(home.config()).unwrap();
    wait_for(&host, A, OfficeStatus::Active);
    assert!(!misplaced.exists());
    assert_eq!(fs::read(moved.join("office.json")).unwrap(), file);
    let offices = Client::connect(host.socket_path())
        .call::<OfficeList>(&Empty {})
        .unwrap()
        .offices;
    let ids: Vec<_> = offices.iter().map(|o| o.id.as_str()).collect();
    assert_eq!(ids, [A]);
    assert_eq!((home.runs(MISPLACED), home.runs(B)), (0, 0), "started");
    assert_eq!(home.runs("discard"), 0);
    host.shutdown();

    // The next start leaves .refused/ alone.
    let host = support::restart_host(&home.config());
    wait_for(&host, A, OfficeStatus::Active);
    assert_eq!(fs::read(moved.join("office.json")).unwrap(), file);
    assert_eq!(home.runs("discard"), 0);
    assert_eq!((home.runs(MISPLACED), home.runs(B)), (0, 0), "started");
    host.shutdown();
}

#[test]
fn a_failed_discard_is_logged_not_retried_and_the_host_still_starts() {
    let home = TestHome::new();
    let unfinished = home.office_dir("01UNFINISHEDUNFINISHEDUNFI");
    fs::create_dir_all(&unfinished).unwrap();
    home.write_office(&hq(A, "01ORGA", "Acme", "2026-11-08T00:00:00.000Z"));
    home.script(json!({"discard": {"do": "fail", "code": "storage_error"}}));
    let host = Host::start(home.config()).unwrap();
    wait_for(&host, A, OfficeStatus::Active);
    eventually(WAIT, "the startup discard", || home.runs("discard") == 1);
    assert!(unfinished.exists());
    host.shutdown();
    // The next start tries again.
    home.script(json!({}));
    let host = support::restart_host(&home.config());
    eventually(WAIT, "the unfinished folder to be discarded", || {
        !unfinished.exists()
    });
    assert_eq!(home.runs("discard"), 2);
    host.shutdown();
}

#[test]
fn a_server_that_exits_while_a_descendant_holds_its_stdout_is_restarted() {
    let home = TestHome::new();
    home.write_office(&hq(A, "01ORGA", "Acme", "2026-11-08T00:00:00.000Z"));
    // It gets ready, leaves a `sleep` holding its stdout for 20 s, and exits.
    home.script(json!({ A: [
        {"do": "crash", "at": "ready", "orphan_stdout_ms": 20_000},
        {"do": "ready"}
    ]}));
    let host = Host::start(home.config()).unwrap();
    let started = Instant::now();
    eventually(WAIT, "a restart", || home.runs(A) == 2);
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "the exit was noticed only when the pipe closed"
    );
    wait_for(&host, A, OfficeStatus::Active);
    host.shutdown();
    home.kill_orphans();
}

#[test]
fn a_late_close_from_a_killed_server_doesnt_end_its_replacement() {
    let home = TestHome::new();
    home.write_office(&hq(A, "01ORGA", "Acme", "2026-11-08T00:00:00.000Z"));
    // The first server is never ready and leaves its stdout held for 1.5 s, so its pipe closes
    // well after its replacement (restarted at once) is up.
    home.script(json!({ A: [
        {"do": "hang", "ignore_eof": true, "orphan_stdout_ms": 1500},
        {"do": "ready"}
    ]}));
    let mut config = home.config();
    config.timings.ready_timeout = Duration::from_millis(700);
    config.timings.backoff_initial = Duration::from_millis(10);
    config.timings.backoff_max = Duration::from_millis(10);
    let host = Host::start(config).unwrap();
    wait_for(&host, A, OfficeStatus::Active);
    // Past the moment the first server's pipe closed.
    thread::sleep(Duration::from_millis(1500));
    assert_eq!(
        home.runs(A),
        2,
        "the replacement was ended by its predecessor's close"
    );
    assert_eq!(status(&host, A), OfficeStatus::Active);
    assert!(alive(home.pids(A)[1]));
    host.shutdown();
    home.kill_orphans();
}

#[test]
fn a_fatal_failure_counts_even_if_the_server_then_outlives_its_ready_deadline() {
    let home = TestHome::new();
    home.write_office(&hq(A, "01ORGA", "Acme", "2026-11-08T00:00:00.000Z"));
    // It reports a fatal failure, then neither exits nor gets ready.
    home.script(json!({ A: {"do": "fail", "code": "key_missing", "linger": true} }));
    let mut config = home.config();
    config.timings.ready_timeout = Duration::from_millis(700);
    let host = Host::start(config).unwrap();
    wait_for(&host, A, OfficeStatus::Unavailable);
    // Several backoff periods (100 + 200 + 400 ms) pass without a restart.
    thread::sleep(Duration::from_millis(800));
    assert_eq!(home.runs(A), 1);
    assert_eq!(status(&host, A), OfficeStatus::Unavailable);
    assert!(
        !alive(home.pids(A)[0]),
        "the lingering server is still running"
    );
    host.shutdown();
}

#[test]
fn the_host_answers_at_once_during_a_slow_startup_discard_and_a_create_waits_for_it() {
    let home = TestHome::new();
    home.write_office(&hq(A, "01ORGA", "Acme", "2026-11-08T00:00:00.000Z"));
    let unfinished = home.office_dir("01UNFINISHEDUNFINISHEDUNFI");
    fs::create_dir_all(&unfinished).unwrap();
    home.script(json!({"discard": {"do": "ready", "delay_ms": 2000}}));
    let started = Instant::now();
    let host = Host::start(home.config()).unwrap();
    let mut client = Client::connect(host.socket_path());
    client.call::<Hello>(&Empty {}).unwrap();
    let offices = client.call::<OfficeList>(&Empty {}).unwrap().offices;
    assert_eq!(offices.len(), 1);
    assert!(
        started.elapsed() < Duration::from_secs(1),
        "the host waited for the discard before answering"
    );
    assert!(unfinished.exists());
    // A create waits for the discard, then goes ahead.
    client
        .call::<OrganizationCreate>(&OrganizationCreateParams {
            name: "Beta".into(),
        })
        .unwrap();
    assert!(
        !unfinished.exists(),
        "the create didn't wait for the discard"
    );
    assert!(started.elapsed() >= Duration::from_secs(2));
    host.shutdown();
}

#[test]
fn a_create_during_a_too_slow_startup_gets_unavailable() {
    let home = TestHome::new();
    fs::create_dir_all(home.office_dir("01UNFINISHEDUNFINISHEDUNFI")).unwrap();
    home.script(json!({"discard": {"do": "ready", "delay_ms": 2000}}));
    let mut config = home.config();
    config.timings.create_wait = Duration::from_millis(500);
    let host = Host::start(config).unwrap();
    let started = Instant::now();
    let err = Client::connect(host.socket_path())
        .call::<OrganizationCreate>(&OrganizationCreateParams {
            name: "Acme".into(),
        })
        .unwrap_err();
    assert_eq!(err.code, ErrorCode::NotReady);
    assert_eq!(err.message, "host starting");
    assert!(started.elapsed() >= Duration::from_millis(500));
    assert_eq!(home.runs("create"), 0);
    host.shutdown();
}
