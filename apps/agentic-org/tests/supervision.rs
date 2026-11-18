//! Supervision, startup and shutdown, against the fake office server.

// Every host here keeps its offices' keys in a development key folder, which only debug builds
// have (a release host uses the Keychain, which tests never touch): debug builds only.
#![cfg(debug_assertions)]
// Only macOS can spawn the office servers the other tests here start (BUG-187); off macOS their
// helpers are unused.
#![cfg_attr(not(target_os = "macos"), allow(unused_imports, dead_code))]

mod support;

use std::fs::{self, OpenOptions};
use std::os::unix::fs::PermissionsExt;
use std::thread;
use std::time::{Duration, Instant};

use org_host::{Host, HostError};
use org_office_launch::OfficeFile;
use org_proto::{
    Empty, Hello, HelloParams, OfficeList, OfficeRole, OfficeStatus, OrganizationList,
};
use serde_json::json;
use support::{alive, eventually, hq, Client, TestHome};

const WAIT: Duration = Duration::from_secs(10);

const A: &str = "01AAAAAAAAAAAAAAAAAAAAAAAA";
const B: &str = "01BBBBBBBBBBBBBBBBBBBBBBBB";
const C: &str = "01CCCCCCCCCCCCCCCCCCCCCCCC";

fn status(host: &Host, office_id: &str) -> OfficeStatus {
    Client::for_host(host)
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

#[cfg(target_os = "macos")]
#[test]
fn a_crashed_server_is_restarted_with_growing_backoff() {
    let home = TestHome::registered();
    home.write_office(&hq(A, "01ORGA", "Acme", "2026-11-08T00:00:00.000Z"));
    home.script(json!({ A: [{"do": "crash"}, {"do": "crash"}, {"do": "ready"}] }));
    let host = home.start(home.config()).unwrap(); // backoff 100 ms, then 200 ms
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

#[cfg(target_os = "macos")]
#[test]
fn in_use_and_unknown_failures_are_retried() {
    let home = TestHome::registered();
    home.write_office(&hq(A, "01ORGA", "Acme", "2026-11-08T00:00:00.000Z"));
    home.script(json!({ A: [
        {"do": "fail", "code": "in_use"},
        {"do": "fail", "code": "a_code_from_the_future"},
        {"do": "ready"}
    ]}));
    let host = home.start(home.config()).unwrap();
    wait_for(&host, A, OfficeStatus::Active);
    assert_eq!(home.runs(A), 3);
    host.shutdown();
}

#[cfg(target_os = "macos")]
#[test]
fn a_fatal_failure_makes_the_office_unavailable_without_restarts() {
    let home = TestHome::registered();
    home.write_office(&hq(A, "01ORGA", "Acme", "2026-11-08T00:00:00.000Z"));
    home.write_office(&hq(B, "01ORGB", "Beta", "2026-11-08T00:00:00.000Z"));
    home.script(json!({
        A: {"do": "fail", "code": "key_missing"},
        B: {"do": "fail", "code": "unsupported_version"}
    }));
    let host = home.start(home.config()).unwrap();
    wait_for(&host, A, OfficeStatus::Unavailable);
    wait_for(&host, B, OfficeStatus::Unavailable);
    // Several backoff periods (100 + 200 + 400 ms) pass without a restart.
    thread::sleep(Duration::from_millis(800));
    assert_eq!(home.runs(A), 1);
    assert_eq!(home.runs(B), 1);
    assert_eq!(status(&host, A), OfficeStatus::Unavailable);
    host.shutdown();
}

#[cfg(target_os = "macos")]
#[test]
fn a_server_that_is_never_ready_is_killed_and_restarted() {
    let home = TestHome::registered();
    home.write_office(&hq(A, "01ORGA", "Acme", "2026-11-08T00:00:00.000Z"));
    home.script(json!({ A: [{"do": "hang", "ignore_eof": true}, {"do": "ready"}] }));
    let mut config = home.config();
    config.timings.ready_timeout = Duration::from_millis(1500);
    let host = home.start(config).unwrap();
    assert_eq!(status(&host, A), OfficeStatus::Starting);
    wait_for(&host, A, OfficeStatus::Active);
    let pids = home.pids(A);
    assert_eq!(pids.len(), 2);
    assert!(!alive(pids[0]), "the hung server is still running");
    host.shutdown();
}

#[cfg(target_os = "macos")]
#[test]
fn startup_serves_stored_offices_cleans_unfinished_ones_and_skips_bad_ones() {
    let home = TestHome::registered();
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
    let host = home.start(config).unwrap();
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

    let orgs = Client::for_host(&host)
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
    let offices = Client::for_host(&host)
        .call::<OfficeList>(&Empty {})
        .unwrap()
        .offices;
    assert_eq!(offices.len(), 3);
    host.shutdown();
}

#[test]
fn a_second_host_on_the_same_home_fails_fast() {
    let home = TestHome::new();
    let first = home.start(home.config()).unwrap();
    let started = Instant::now();
    let second = home.start(home.config());
    assert!(matches!(second, Err(HostError::AlreadyRunning { .. })));
    assert!(started.elapsed() < Duration::from_secs(1));
    assert!(second
        .err()
        .unwrap()
        .to_string()
        .contains("already running"));
    // The first host is untouched.
    Client::for_host(&first)
        .call::<OfficeList>(&Empty {})
        .unwrap();
    first.shutdown();
}

#[cfg(target_os = "macos")]
#[test]
fn shutdown_closes_stdins_so_servers_exit_cleanly_and_releases_the_home() {
    let home = TestHome::registered();
    home.write_office(&hq(A, "01ORGA", "Acme", "2026-11-08T00:00:00.000Z"));
    home.write_office(&hq(B, "01ORGB", "Beta", "2026-11-08T00:00:00.000Z"));
    let host = home.start(home.config()).unwrap();
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
    support::restart_host(&home, &home.config()).shutdown();
}

#[cfg(target_os = "macos")]
#[test]
fn a_server_that_ignores_eof_is_killed_after_the_grace_period() {
    let home = TestHome::registered();
    home.write_office(&hq(A, "01ORGA", "Acme", "2026-11-08T00:00:00.000Z"));
    home.script(json!({ A: {"do": "ready", "ignore_eof": true} }));
    let mut config = home.config();
    config.timings.shutdown_grace = Duration::from_millis(300);
    let host = home.start(config).unwrap();
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
    // run/<12 chars>.sock adds 22 bytes: make that exceed this system's limit.
    let limit = org_platform::dirs::socket_path_limit().unwrap();
    let base = home.path();
    let pad = limit - 22 - base.as_os_str().len();
    let long = base.join("x".repeat(pad));
    let mut config = org_host::Config::new(&long, support::fake_server());
    config.dev_key_dir = Some(home.keys_dir());
    match home.start(config) {
        Err(e @ HostError::SocketPathTooLong { .. }) => {
            assert!(
                e.to_string()
                    .contains(&format!("this system allows at most {limit}")),
                "{e}"
            );
        }
        Err(other) => panic!("{other}"),
        Ok(_) => panic!(
            "started with a {}-byte socket path",
            long.as_os_str().len() + 22
        ),
    }
    assert!(!long.exists(), "it touched the home before refusing");
}

#[cfg(target_os = "macos")]
#[test]
fn server_stderr_goes_to_an_owner_only_log_rotated_at_the_cap() {
    let home = TestHome::registered();
    home.write_office(&hq(A, "01ORGA", "Acme", "2026-11-08T00:00:00.000Z"));
    home.script(json!({ A: {"do": "ready", "stderr_bytes": 10_000} }));
    let mut config = home.config();
    config.log_max_bytes = 4096;
    let host = home.start(config).unwrap();
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

#[cfg(target_os = "macos")]
#[test]
fn a_discard_refused_with_already_exists_or_storage_error_is_left_alone() {
    // The server finds an organization committed in the folder's databases (already_exists),
    // or databases it can't open or check (storage_error).
    for code in ["already_exists", "storage_error"] {
        let home = TestHome::registered();
        let unfinished = home.office_dir("01UNFINISHEDUNFINISHEDUNFI");
        fs::create_dir_all(&unfinished).unwrap();
        home.write_office(&hq(A, "01ORGA", "Acme", "2026-11-08T00:00:00.000Z"));
        home.script(json!({"discard": {"do": "fail", "code": code}}));
        let host = home.start(home.config()).unwrap();
        wait_for(&host, A, OfficeStatus::Active);
        eventually(WAIT, "the startup discard", || home.runs("discard") == 1);
        // Several backoff periods pass: no retry, and the folder is kept.
        thread::sleep(Duration::from_millis(800));
        assert_eq!(home.runs("discard"), 1, "{code}");
        assert!(unfinished.exists(), "{code}");
        host.shutdown();
        // Each start asks once, never in a loop.
        let host = support::restart_host(&home, &home.config());
        wait_for(&host, A, OfficeStatus::Active);
        eventually(WAIT, "the startup discard", || home.runs("discard") == 2);
        thread::sleep(Duration::from_millis(500));
        assert_eq!(home.runs("discard"), 2, "{code}");
        assert!(unfinished.exists(), "{code}");
        host.shutdown();
    }
}

#[cfg(target_os = "macos")]
#[test]
fn a_folder_holding_another_offices_file_is_moved_aside_never_started_or_discarded() {
    const MISPLACED: &str = "01MISPLACEDMISPLACEDMISPLA";
    let home = TestHome::registered();
    home.write_office(&hq(A, "01ORGA", "Acme", "2026-11-08T00:00:00.000Z"));
    // MISPLACED's folder holds an office.json for B (whose own folder doesn't exist).
    let file = hq(B, "01ORGB", "Beta", "2026-11-08T00:00:00.000Z")
        .to_vec()
        .unwrap();
    let misplaced = home.office_dir(MISPLACED);
    fs::create_dir_all(&misplaced).unwrap();
    fs::write(misplaced.join("office.json"), &file).unwrap();
    let moved = home.path().join("offices/.refused").join(MISPLACED);

    let host = home.start(home.config()).unwrap();
    wait_for(&host, A, OfficeStatus::Active);
    assert!(!misplaced.exists());
    assert_eq!(fs::read(moved.join("office.json")).unwrap(), file);
    let offices = Client::for_host(&host)
        .call::<OfficeList>(&Empty {})
        .unwrap()
        .offices;
    let ids: Vec<_> = offices.iter().map(|o| o.id.as_str()).collect();
    assert_eq!(ids, [A]);
    assert_eq!((home.runs(MISPLACED), home.runs(B)), (0, 0), "started");
    assert_eq!(home.runs("discard"), 0);
    host.shutdown();

    // The next start leaves .refused/ alone.
    let host = support::restart_host(&home, &home.config());
    wait_for(&host, A, OfficeStatus::Active);
    assert_eq!(fs::read(moved.join("office.json")).unwrap(), file);
    assert_eq!(home.runs("discard"), 0);
    assert_eq!((home.runs(MISPLACED), home.runs(B)), (0, 0), "started");
    host.shutdown();
}

#[cfg(target_os = "macos")]
#[test]
fn a_failed_discard_is_logged_not_retried_and_the_host_still_starts() {
    let home = TestHome::registered();
    let unfinished = home.office_dir("01UNFINISHEDUNFINISHEDUNFI");
    fs::create_dir_all(&unfinished).unwrap();
    home.write_office(&hq(A, "01ORGA", "Acme", "2026-11-08T00:00:00.000Z"));
    home.script(json!({"discard": {"do": "fail", "code": "storage_error"}}));
    let host = home.start(home.config()).unwrap();
    wait_for(&host, A, OfficeStatus::Active);
    eventually(WAIT, "the startup discard", || home.runs("discard") == 1);
    assert!(unfinished.exists());
    host.shutdown();
    // The next start tries again.
    home.script(json!({}));
    let host = support::restart_host(&home, &home.config());
    eventually(WAIT, "the unfinished folder to be discarded", || {
        !unfinished.exists()
    });
    assert_eq!(home.runs("discard"), 2);
    host.shutdown();
}

#[cfg(target_os = "macos")]
#[test]
fn a_server_that_exits_while_a_descendant_holds_its_stdout_is_restarted() {
    let home = TestHome::registered();
    home.write_office(&hq(A, "01ORGA", "Acme", "2026-11-08T00:00:00.000Z"));
    // It gets ready, leaves a `sleep` holding its stdout for 20 s, and exits.
    home.script(json!({ A: [
        {"do": "crash", "at": "ready", "orphan_stdout_ms": 20_000},
        {"do": "ready"}
    ]}));
    let host = home.start(home.config()).unwrap();
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

#[cfg(target_os = "macos")]
#[test]
fn a_late_close_from_a_killed_server_doesnt_end_its_replacement() {
    let home = TestHome::registered();
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
    let host = home.start(config).unwrap();
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

#[cfg(target_os = "macos")]
#[test]
fn a_fatal_failure_counts_even_if_the_server_then_outlives_its_ready_deadline() {
    let home = TestHome::registered();
    home.write_office(&hq(A, "01ORGA", "Acme", "2026-11-08T00:00:00.000Z"));
    // It reports a fatal failure, then neither exits nor gets ready.
    home.script(json!({ A: {"do": "fail", "code": "key_missing", "linger": true} }));
    let mut config = home.config();
    config.timings.ready_timeout = Duration::from_millis(700);
    let host = home.start(config).unwrap();
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

#[cfg(target_os = "macos")]
#[test]
fn the_host_answers_at_once_during_a_slow_startup_discard_which_then_finishes() {
    let home = TestHome::registered();
    home.write_office(&hq(A, "01ORGA", "Acme", "2026-11-08T00:00:00.000Z"));
    let unfinished = home.office_dir("01UNFINISHEDUNFINISHEDUNFI");
    fs::create_dir_all(&unfinished).unwrap();
    home.script(json!({"discard": {"do": "ready", "delay_ms": 2000}}));
    let started = Instant::now();
    let host = home.start(home.config()).unwrap();
    let mut client = Client::for_host(&host);
    Client::connect(host.socket_path())
        .call::<Hello>(&HelloParams::default())
        .unwrap();
    let offices = client.call::<OfficeList>(&Empty {}).unwrap().offices;
    assert_eq!(offices.len(), 1);
    assert!(
        started.elapsed() < Duration::from_secs(1),
        "the host waited for the discard before answering"
    );
    assert!(unfinished.exists());
    // The discard still finishes: the unfinished folder goes once the slow discard reports.
    eventually(Duration::from_secs(10), "the startup discard", || {
        !unfinished.exists()
    });
    assert!(started.elapsed() >= Duration::from_secs(2));
    host.shutdown();
}

/// No host descriptor reaches a server: every start, crash and restart while clients keep
/// connecting to `host.sock` (so the host keeps accepting while it spawns) reports `-` for the
/// descriptors it inherited above 2. Once the runs are over, the host keeps no office log open
/// that nothing writes.
#[cfg(target_os = "macos")]
#[test]
fn servers_inherit_no_host_fds_while_clients_connect() {
    use std::os::unix::net::UnixStream;
    use std::process::Command;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;

    const CRASHES: u64 = 12;
    let home = TestHome::registered();
    let offices = [A, B, C];
    let mut script = serde_json::Map::new();
    for (i, id) in offices.iter().enumerate() {
        home.write_office(&hq(
            id,
            &format!("01ORG{i}"),
            &format!("Org {i}"),
            "2026-11-08T00:00:00.000Z",
        ));
        let mut steps = vec![json!({"do": "crash"}); CRASHES as usize];
        steps.push(json!({"do": "ready"}));
        script.insert((*id).to_owned(), json!(steps));
    }
    home.script(serde_json::Value::Object(script));
    let mut config = home.config();
    config.timings.backoff_initial = Duration::from_millis(10);
    config.timings.backoff_max = Duration::from_millis(20);
    config.timings.give_up_after = 1000;
    let host = home.start(config).unwrap();

    // Clients connecting and leaving all the while.
    let socket = host.socket_path().to_path_buf();
    let stop = Arc::new(AtomicBool::new(false));
    let churn: Vec<_> = (0..4)
        .map(|_| {
            let (socket, stop) = (socket.clone(), Arc::clone(&stop));
            thread::spawn(move || {
                while !stop.load(Ordering::Relaxed) {
                    drop(UnixStream::connect(&socket));
                    // Paced so the listen backlog never fills and refuses the status calls.
                    thread::sleep(Duration::from_millis(1));
                }
            })
        })
        .collect();
    for id in offices {
        wait_for(&host, id, OfficeStatus::Active);
    }
    stop.store(true, Ordering::Relaxed);
    for thread in churn {
        thread.join().unwrap();
    }
    for id in offices {
        assert_eq!(home.runs(id), CRASHES + 1, "{id}");
    }

    // The host runs in this process: count the office logs of this home it holds open. Three
    // active offices may each keep theirs; 39 finished runs must not leave 39 open.
    let base = home.fixture_base();
    let pid = std::process::id().to_string();
    let out = Command::new("/usr/sbin/lsof")
        .args(["-n", "-P", "-Fn", "-p", &pid])
        .output()
        .expect("lsof runs");
    assert!(out.status.success(), "lsof failed: {out:?}");
    let listing = String::from_utf8_lossy(&out.stdout);
    assert!(
        listing.lines().any(|l| l == format!("p{pid}")),
        "lsof didn't list this process"
    );
    let open_logs = listing
        .lines()
        .filter(|l| {
            l.starts_with('n') && l.contains(base.to_str().unwrap()) && l.contains("/logs/")
        })
        .count();
    assert!(
        open_logs <= offices.len(),
        "the host holds {open_logs} office logs open"
    );
    host.shutdown();

    let log = fs::read_to_string(home.path().join("fake.log")).unwrap();
    let reports: Vec<&str> = log.lines().filter(|l| l.starts_with("fds ")).collect();
    assert_eq!(
        reports.len(),
        offices.len() * (CRASHES as usize + 1),
        "every server reports its fds"
    );
    let leaked: Vec<&&str> = reports.iter().filter(|l| !l.ends_with(" -")).collect();
    assert!(leaked.is_empty(), "servers inherited host fds: {leaked:?}");
}

/// A server's environment holds only the allowlisted names of this (host) process's own,
/// plus its office home and runtime folder. The test process has names outside the allowlist
/// (cargo's `CARGO_*` at least), so the check is not vacuous. The exact filter, with a canary
/// and the development key folder, is `org-host`'s `a_server_gets_only_the_allowed_environment`.
#[cfg(target_os = "macos")]
#[test]
fn servers_get_only_the_allowed_environment() {
    let allowed = [
        "HOME",
        "USER",
        "LOGNAME",
        "PATH",
        "TMPDIR",
        "LANG",
        "ORG_OFFICE_HOME",
        "ORG_OFFICE_RUNTIME_DIR",
    ];
    let outside: Vec<String> = std::env::vars_os()
        .map(|(name, _)| name.to_string_lossy().into_owned())
        .filter(|name| !allowed.contains(&name.as_str()))
        .collect();
    assert!(
        !outside.is_empty(),
        "the test environment has nothing to keep out"
    );

    let home = TestHome::registered();
    home.write_office(&hq(A, "01ORGA", "Acme", "2026-11-08T00:00:00.000Z"));
    home.script(json!({ A: [{"do": "crash"}, {"do": "ready"}] }));
    let host = home.start(home.config()).unwrap();
    wait_for(&host, A, OfficeStatus::Active);
    host.shutdown();

    let runs = home.env_names(A);
    assert_eq!(runs.len(), 2, "a start and a restart: {runs:?}");
    for names in runs {
        assert!(names.iter().any(|n| n == "ORG_OFFICE_HOME"), "{names:?}");
        for name in names.iter().filter(|n| !n.is_empty()) {
            assert!(allowed.contains(&name.as_str()), "{name} was passed on");
        }
        for name in &outside {
            assert!(!names.contains(name), "{name} was passed on");
        }
    }
}
