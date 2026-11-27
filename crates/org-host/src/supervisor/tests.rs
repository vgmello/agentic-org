use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::{mpsc, Arc};
use std::time::{Duration, Instant};
use std::{fs, io, thread};

use child::Mode;
use org_office_launch::{OfficeFile, ServerEvent};
use org_platform_host::admission::ImageIdentity;
use org_proto::{OfficeSecretsKey, OfficeStatus, OfficeStatusReason};

use crate::host_log;
use crate::keys::memory::{Fault, MemoryKeys, Op};
use crate::keys::OfficeSecretsKeys;
use crate::peer::AuditToken;
use crate::server_check::ServerCheck;
use crate::server_check::{NoAdmission, Pin, Scripted, Step};
use crate::sync::lock;
use crate::test_programs::{image_identity, recorded_starts, recorded_stdin, recording_worker};
use crate::timings::Timings;

use super::create::CreateOutcome;
use super::*;

const OFFICE: &str = "01900000-0000-7000-8000-0000000000aa";
const B: &str = "01900000-0000-7000-8000-0000000000bb";
const C: &str = "01900000-0000-7000-8000-0000000000cc";
/// An office this host holds no writer custody for.
const NO_CUSTODY: &str = "01900000-0000-7000-8000-0000000000dd";

fn supervisor(dir: &Path, program: PathBuf, keys: &Arc<MemoryKeys>) -> Supervisor {
    checked_supervisor(dir, program, keys, ServerCheck::Skip)
}

/// Real writer custody for `OFFICE`, `B` and `C` (never `NO_CUSTODY`), under `dir`.
fn custody(dir: &Path) -> crate::writer::TestCustody {
    crate::writer::TestCustody::reserve(dir, INSTANCE, &[OFFICE, B, C])
}

fn checked_supervisor(
    dir: &Path,
    program: PathBuf,
    keys: &Arc<MemoryKeys>,
    check: ServerCheck,
) -> Supervisor {
    uncustodied_supervisor(dir, program, keys, check).with_writers(custody(dir).writers)
}

/// A supervisor of a host with no writer custody at all (no protected registry).
fn uncustodied_supervisor(
    dir: &Path,
    program: PathBuf,
    keys: &Arc<MemoryKeys>,
    check: ServerCheck,
) -> Supervisor {
    let timings = Timings {
        shutdown_grace: Duration::from_millis(200),
        discard_timeout: Duration::from_secs(10),
        ..Timings::default()
    };
    // The launch rendezvous is bound in `<home>/run`.
    fs::create_dir_all(dir.join("run")).unwrap();
    Supervisor::new(
        SupervisorConfig {
            program,
            home: dir.to_path_buf(),
            runtime_dir: None,
            logs_dir: dir.join("logs"),
            log_max_bytes: 1024,
            timings,
            keys: Arc::clone(keys) as Arc<dyn OfficeSecretsKeys>,
            server_check: Arc::new(check),
            sweep_orphans: false,
            federation: None,
        },
        INSTANCE,
    )
}

/// A temporary directory under `/tmp`, so the launch rendezvous path fits a socket address.
fn short_tempdir() -> tempfile::TempDir {
    tempfile::Builder::new()
        .prefix("sv")
        .tempdir_in("/tmp")
        .unwrap()
}

/// An executable `sh` script in `dir`.
fn script(dir: &Path, name: &str, body: &str) -> PathBuf {
    let path = dir.join(name);
    fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
    path
}

#[test]
fn no_office_is_started_once_shutdown_began() {
    let dir = short_tempdir();
    let keys = Arc::new(MemoryKeys::default());
    let supervisor = supervisor(dir.path(), dir.path().join("no-such-server"), &keys);
    supervisor.shutdown();
    assert!(supervisor.start_office(OFFICE).is_err());
    assert_eq!(supervisor.state(OFFICE), None);
}

/// Polls the office's state until `want` holds (or panics after 10 s).
fn wait_for_state(supervisor: &Supervisor, office_id: &str, want: impl Fn(&OfficeState) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !supervisor.state(office_id).is_some_and(|s| want(&s)) {
        assert!(
            Instant::now() < deadline,
            "{:?}",
            supervisor.state(office_id)
        );
        thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn a_start_without_a_usable_key_still_spawns_its_server() {
    let dir = short_tempdir();
    let ran = dir.path().join("ran");
    let program = script(
        dir.path(),
        "server",
        &format!("echo \"$@\" >> '{}'", ran.display()),
    );
    let keys = Arc::new(MemoryKeys::default());
    let supervisor = checked_supervisor(dir.path(), program, &keys, ServerCheck::ByPath);
    let spawned = |id: &str| {
        fs::read_to_string(&ran)
            .unwrap_or_default()
            .lines()
            .any(|line| line.split_whitespace().any(|arg| arg == id))
    };
    let wait_for_spawn = |id: &str| {
        let deadline = Instant::now() + Duration::from_secs(10);
        while !spawned(id) {
            assert!(Instant::now() < deadline, "office {id} wasn't started");
            thread::sleep(Duration::from_millis(10));
        }
    };
    // No item: the server starts anyway (its handoff would carry `null`).
    supervisor.start_office(OFFICE).unwrap();
    wait_for_spawn(OFFICE);
    // An item that isn't a key: the same.
    keys.create(B, &OfficeSecretsKey::from_bytes([1; 32]))
        .unwrap();
    keys.fail(Op::Load, Fault::Corrupt);
    supervisor.start_office(B).unwrap();
    wait_for_spawn(B);
    // A store that fails otherwise: `storage_error`, retried with backoff.
    keys.fail(Op::Load, Fault::Io);
    supervisor.start_office(C).unwrap();
    wait_for_state(&supervisor, C, |s| {
        s.status == OfficeStatus::Offline
            && s.reason == Some(OfficeStatusReason::StorageError)
            && s.next_restart_at.is_some()
    });
    supervisor.shutdown();
    assert!(!spawned(C), "a server started over an unreadable key store");
    // Nothing was deleted, and the item that isn't a key is still there.
    assert!(!keys.calls().iter().any(|(op, _)| *op == Op::Delete));
    assert!(keys.has(B));
}

/// H1: a server that isn't the signed one (this unsigned script, against a release
/// requirement) gets no line: no key. Where the loaded image is checked it is spawned
/// suspended, checked, killed and reaped before it runs anything; where the verified
/// bytes are executed, a release check without a selected release has no admission, so
/// nothing is spawned at all. With no
/// team id, nothing is even spawned. A start ends as `storage_error`, retried; a discard
/// fails and keeps the key.
#[test]
fn a_server_that_isnt_the_signed_one_gets_nothing() {
    let dir = short_tempdir();
    let got = dir.path().join("got");
    let program = script(dir.path(), "server", &format!("cat >> '{}'", got.display()));
    let keys = Arc::new(MemoryKeys::default());
    for id in [OFFICE, B] {
        keys.create(id, &OfficeSecretsKey::from_bytes([3; 32]))
            .unwrap();
    }
    let requirement = crate::server_check::server_requirement("AB12CD34EF").unwrap();
    let checked = checked_supervisor(
        dir.path(),
        program.clone(),
        &keys,
        ServerCheck::Requirement(requirement),
    );
    assert!(checked.discard(OFFICE).is_err());
    checked.start_office(B).unwrap();
    wait_for_state(&checked, B, |s| {
        s.status == OfficeStatus::Offline && s.reason == Some(OfficeStatusReason::StorageError)
    });
    checked.shutdown();
    // They got no line: nothing reached `got` (it isn't even there: they were stopped while
    // still suspended, or never started; that suspension holds is the admission adapter's
    // own test).
    assert!(!got.exists(), "a server that isn't the signed one ran");
    assert!(keys.has(OFFICE) && keys.has(B));

    // No team id: nothing is spawned at all.
    let dir = short_tempdir();
    let ran = dir.path().join("ran");
    let program = script(
        dir.path(),
        "server",
        &format!("echo run >> '{}'", ran.display()),
    );
    let refusing = checked_supervisor(dir.path(), program, &keys, ServerCheck::Refuse);
    assert!(refusing.discard(OFFICE).is_err());
    refusing.start_office(B).unwrap();
    wait_for_state(&refusing, B, |s| {
        s.status == OfficeStatus::Offline && s.reason == Some(OfficeStatusReason::StorageError)
    });
    refusing.shutdown();
    assert!(!ran.exists(), "a server was started without a team id");
}

/// A check scripted by the test: `admit` and `still` answer as `script` says.
fn scripted(
    script: impl Fn(Step, u32) -> Result<Option<AuditToken>, String> + Send + Sync + 'static,
) -> ServerCheck {
    ServerCheck::Scripted(Scripted(Arc::new(script)))
}

/// L2: the check is asked once, when the child is admitted, and a child it refuses gets
/// nothing. What else holds is the one outcome of this system's kind of admission
/// (`org_platform_host::admission::image_identity`, the platform's fact):
///
/// - A code signature is checked on the started child: the check runs while the child is
///   still suspended. At the admission the child is stopped (`ps` state `T`, the
///   kernel's `SSTOP`), so not one instruction of its image has run, and once refused it
///   never does: the worker never records a start.
/// - The executed bytes are checked: there is no suspended child. The image was checked
///   before the child existed (the admission names its digest), so the child may have
///   started, and still receives nothing.
#[test]
fn the_check_runs_before_the_child_gets_anything() {
    let dir = short_tempdir();
    let program = recording_worker();
    let keys = Arc::new(MemoryKeys::default());
    let states = Arc::new(std::sync::Mutex::new(Vec::new()));
    let seen = Arc::clone(&states);
    let check = scripted(move |step, pid| {
        assert_eq!(step, Step::Admit);
        let state = std::process::Command::new("/bin/ps")
            .args(["-o", "state=", "-p", &pid.to_string()])
            .output()
            .map(|out| String::from_utf8_lossy(&out.stdout).trim().to_owned())
            .unwrap_or_default();
        lock(&seen).push(state);
        Err("refused by the test".into())
    });
    let verified_digest = check.admission(&program).unwrap().image_sha256();
    let supervisor = checked_supervisor(dir.path(), program, &keys, check);
    let err = supervisor.discard(OFFICE).unwrap_err();
    assert!(err.to_string().contains("refused by the test"), "{err}");
    let states = lock(&states).clone();
    assert_eq!(states.len(), 1, "{states:?}");
    thread::sleep(Duration::from_millis(200));
    match image_identity() {
        ImageIdentity::CodeSignature => {
            assert!(
                states[0].starts_with('T'),
                "not stopped at the check: {states:?}"
            );
            assert_eq!(recorded_starts(dir.path()), 0, "the child ran");
            assert_eq!(verified_digest, None);
        }
        ImageIdentity::VerifiedBytes => {
            assert!(
                verified_digest.is_some(),
                "the image was not verified before the child existed"
            );
        }
    }
    assert_eq!(recorded_stdin(dir.path()), b"", "it got a line");
}

/// L3a: a child whose token changed between the admission and its first line (an exec) is
/// refused at the re-check: it gets no line.
#[test]
fn a_child_whose_token_changed_before_its_line_gets_nothing() {
    let dir = short_tempdir();
    let keys = Arc::new(MemoryKeys::default());
    keys.create(OFFICE, &OfficeSecretsKey::from_bytes([6; 32]))
        .unwrap();
    let check = scripted(|step, _| match step {
        Step::Admit => Ok(Some([1; 32])),
        Step::Still => Ok(Some([2; 32])),
    });
    let supervisor = checked_supervisor(dir.path(), recording_worker(), &keys, check);
    let err = supervisor.discard(OFFICE).unwrap_err();
    assert!(err.to_string().contains("changed"), "{err}");
    thread::sleep(Duration::from_millis(200));
    assert_eq!(recorded_stdin(dir.path()), b"", "it got a line");
    assert!(keys.has(OFFICE));
}

/// A unit of what a supervisor's spawner does for one `--discard`: the outcome, and what
/// the host logged on this thread.
fn spawn_discard(supervisor: &Supervisor) -> (Result<(), child::SpawnError>, Vec<String>) {
    let (sender, _signals) = mpsc::channel();
    let log = supervisor.log_for(OFFICE);
    host_log::capture(|| {
        supervisor
            .spawner()
            .unwrap()
            .spawn(Mode::Discard(OFFICE), 0, sender, log)
            .map(drop)
    })
}

/// The host builds its admission once, with its supervisor, and every start goes through
/// that one: a discard and an office start do not build another.
#[test]
fn one_admission_serves_every_start_of_a_host() {
    let dir = short_tempdir();
    let keys = Arc::new(MemoryKeys::default());
    keys.create(OFFICE, &OfficeSecretsKey::from_bytes([4; 32]))
        .unwrap();
    let supervisor = supervisor(dir.path(), recording_worker(), &keys);
    let images = Arc::clone(&supervisor.spawner().unwrap().images);
    let built = images.admission().expect("built with the supervisor");
    // The recording worker speaks no launch protocol: each run reads its first line and
    // exits, which ends the run as a crash. What matters here is which admission ran it.
    assert!(supervisor.discard(OFFICE).is_err());
    supervisor.start_office(OFFICE).unwrap();
    wait_for_state(&supervisor, OFFICE, |s| s.status == OfficeStatus::Offline);
    supervisor.shutdown();
    assert!(recorded_starts(dir.path()) >= 2, "the worker never ran");
    assert!(Arc::ptr_eq(&built, &images.admission().unwrap()));
    // Every office's runner holds the same one.
    assert!(Arc::ptr_eq(
        &images,
        &supervisor.spawner().unwrap().clone().images
    ));
}

/// A host whose admission cannot be built yet (its program is not there) starts no
/// server, says why, and asks again at the next start; once built, the admission is kept.
/// Where nothing of the image is read until a child is started, the admission exists at
/// once and the missing program fails each start instead.
#[test]
fn an_admission_that_could_not_be_built_is_asked_for_again() {
    let dir = short_tempdir();
    let program = dir.path().join("worker");
    let keys = Arc::new(MemoryKeys::default());
    keys.create(OFFICE, &OfficeSecretsKey::from_bytes([4; 32]))
        .unwrap();
    let supervisor = supervisor(dir.path(), program.clone(), &keys);
    let images = Arc::clone(&supervisor.spawner().unwrap().images);
    let before = match (image_identity(), images.admission()) {
        // Nothing is read until a child is started: the admission exists at once.
        (ImageIdentity::CodeSignature, Ok(admission)) => Some(admission),
        // The image is read when the admission is built: there is none yet.
        (ImageIdentity::VerifiedBytes, Err(NoAdmission::Unavailable(error))) => {
            assert_eq!(error.kind(), io::ErrorKind::NotFound, "{error:?}");
            None
        }
        (identity, other) => panic!("{identity:?}: {:?}", other.map(drop)),
    };
    // No server starts, whichever step notices: the host keeps serving and reports it.
    assert!(supervisor.discard(OFFICE).is_err());
    assert_eq!(recorded_starts(dir.path()), 0);
    // Stage and rename atomically so the background `office-images` thread from
    // `Supervisor::new` never reads a half-written or pre-chmod binary under load
    // and permanently caches a truncated `memfd` admission.
    let staged = dir.path().join(".worker.staged");
    fs::copy(recording_worker(), &staged).unwrap();
    fs::set_permissions(&staged, fs::Permissions::from_mode(0o700)).unwrap();
    fs::rename(&staged, &program).unwrap();
    let built = images.admission().expect("the program is there now");
    if let Some(before) = before {
        assert!(Arc::ptr_eq(&before, &built), "an admission was built twice");
    }
    assert!(Arc::ptr_eq(&built, &images.admission().unwrap()));
    // The next start runs it (and fails later: the worker speaks no launch protocol).
    let err = supervisor.discard(OFFICE).unwrap_err();
    assert_eq!(
        err.kind(),
        io::ErrorKind::UnexpectedEof,
        "expected recording_worker to run and exit without speaking rendezvous: {err:?}"
    );
    assert_eq!(recorded_starts(dir.path()), 1);
    supervisor.shutdown();
}

/// A check that may start no server at all is a refusal, in the check's words, not an
/// I/O failure: nothing is spawned and nothing is made for a child.
#[test]
fn a_check_that_may_not_start_a_server_is_a_refusal() {
    for check in [
        ServerCheck::Refuse,
        ServerCheck::WebRefused("refused by the test"),
        ServerCheck::Refuse.pinned(Pin {
            sha256: [7; 32],
            macos_cdhash: Some([7; 20]),
        }),
    ] {
        let dir = short_tempdir();
        let keys = Arc::new(MemoryKeys::default());
        let words = check.may_spawn().unwrap_err();
        let supervisor = checked_supervisor(dir.path(), recording_worker(), &keys, check);
        let (outcome, logged) = spawn_discard(&supervisor);
        match outcome {
            Err(child::SpawnError::Untrusted(why)) => assert_eq!(why, words),
            Err(other) => panic!("not a refusal: {other}"),
            Ok(()) => panic!("a server was started"),
        }
        assert!(
            logged.iter().any(|line| line.contains(&words)),
            "{logged:?}"
        );
        assert_eq!(recorded_starts(dir.path()), 0);
        assert!(!dir.path().join("run/r").exists(), "a rendezvous was made");
    }
}

/// LNX-1 refusal: a worker image that is not the image the selected release names (the
/// release's digest is of a file that differs from this program in one byte) receives no
/// first line and no secret, and the host log names the refusal. Where the verified bytes
/// are executed the digest differs when the host builds its admission, so no child ever
/// exists; where the loaded image is checked, a development image under a pin cannot be
/// identified and is killed before it runs. Both are a refusal in the check's words. A
/// program that is the pinned image is started and gets its first line.
#[test]
fn a_worker_image_that_differs_from_the_pinned_one_gets_nothing() {
    use sha2::{Digest, Sha256};
    let dir = short_tempdir();
    let keys = Arc::new(MemoryKeys::default());
    keys.create(OFFICE, &OfficeSecretsKey::from_bytes([5; 32]))
        .unwrap();
    let program = recording_worker();
    let image = fs::read(&program).unwrap();
    // What the release names: this program with its last byte changed.
    let mut released = image.clone();
    *released.last_mut().unwrap() ^= 1;
    let check = ServerCheck::Skip.pinned(Pin {
        sha256: Sha256::digest(&released).into(),
        macos_cdhash: Some([7; 20]),
    });
    // The one refusal of this system's kind of admission.
    let refusal = check.why(&match image_identity() {
        ImageIdentity::VerifiedBytes => org_platform_host::admission::ImageRefusal::DigestMismatch,
        ImageIdentity::CodeSignature => org_platform_host::admission::ImageRefusal::Unidentified,
    });
    let supervisor = checked_supervisor(dir.path(), program.clone(), &keys, check);
    let (outcome, logged) = spawn_discard(&supervisor);
    let why = match outcome {
        Err(child::SpawnError::Untrusted(why)) => why,
        Err(other) => panic!("not a refusal: {other}"),
        Ok(()) => panic!("an image that is not the pinned one was started"),
    };
    assert_eq!(why, refusal);
    assert!(
        logged
            .iter()
            .any(|line| line.contains(&why) && line.contains("office server")),
        "{logged:?}"
    );
    // An office start ends the same way: a storage error, retried, and nothing received.
    supervisor.start_office(OFFICE).unwrap();
    wait_for_state(&supervisor, OFFICE, |s| {
        s.status == OfficeStatus::Offline && s.reason == Some(OfficeStatusReason::StorageError)
    });
    supervisor.shutdown();
    thread::sleep(Duration::from_millis(200));
    assert_eq!(recorded_stdin(dir.path()), b"", "the image got a line");
    assert!(keys.has(OFFICE));

    // The control: the same program under the digest of its own bytes is started and
    // receives the first line.
    let exact = ServerCheck::Skip.pinned(Pin {
        sha256: Sha256::digest(&image).into(),
        macos_cdhash: None,
    });
    let supervisor = checked_supervisor(dir.path(), program, &keys, exact);
    let _ = supervisor.discard(OFFICE);
    let line = String::from_utf8(recorded_stdin(dir.path())).unwrap();
    assert!(line.contains(OFFICE) && line.ends_with('\n'), "{line:?}");
}

/// LNX-1 refusal: a server whose environment would carry a variable that changes how the
/// loader runs its image is never started, on any system: no child exists, so nothing
/// receives a first line or a secret, and the host log names the variable (never its
/// value). An office start ends as a storage error, as every refused server does.
#[test]
fn a_server_environment_with_a_loader_variable_starts_nothing() {
    for name in [
        "LD_PRELOAD",
        "LD_LIBRARY_PATH",
        "DYLD_INSERT_LIBRARIES",
        "GLIBC_TUNABLES",
        "GCONV_PATH",
    ] {
        let dir = short_tempdir();
        let keys = Arc::new(MemoryKeys::default());
        keys.create(OFFICE, &OfficeSecretsKey::from_bytes([9; 32]))
            .unwrap();
        let mut supervisor = supervisor(dir.path(), recording_worker(), &keys);
        supervisor
            .spawner
            .as_mut()
            .unwrap()
            .extra_env
            .push((name.into(), "/tmp/injected-value.so".into()));
        let (outcome, logged) = spawn_discard(&supervisor);
        match outcome {
            Err(child::SpawnError::Untrusted(why)) => {
                assert!(
                    why.contains("loader variable") && why.contains(name),
                    "{why}"
                );
            }
            Err(other) => panic!("{name}: not a refusal: {other}"),
            Ok(()) => panic!("{name}: a server was started"),
        }
        assert!(
            logged
                .iter()
                .any(|line| line.contains("loader variable") && line.contains(name)),
            "{name}: {logged:?}"
        );
        assert!(
            !logged.iter().any(|line| line.contains("injected-value")),
            "{name}: the value was logged: {logged:?}"
        );
        supervisor.start_office(OFFICE).unwrap();
        wait_for_state(&supervisor, OFFICE, |s| {
            s.status == OfficeStatus::Offline && s.reason == Some(OfficeStatusReason::StorageError)
        });
        supervisor.shutdown();
        assert_eq!(recorded_starts(dir.path()), 0, "{name}: a server ran");
        assert_eq!(recorded_stdin(dir.path()), b"", "{name}");
        assert!(keys.has(OFFICE));
    }
    // The same worker without such a variable is started and gets its first line.
    let dir = short_tempdir();
    let keys = Arc::new(MemoryKeys::default());
    let supervisor = supervisor(dir.path(), recording_worker(), &keys);
    let _ = supervisor.discard(OFFICE);
    assert_eq!(recorded_starts(dir.path()), 1);
    assert!(!recorded_stdin(dir.path()).is_empty());
}

/// Names the file where this test binary, run again as a launch `--discard` server by
/// [`a_discard_hands_off_no_key_and_deletes_it_only_after_discarded`], records its
/// handoff's op (`discard`).
const DISCARD_SEEN: &str = "ORG_HOST_TEST_DISCARD_SEEN";
const INSTANCE: &str = "0190a5d2-7c3e-7a01-8b2c-3d4e5f6071c0";

/// Not a test: the launch `--discard` server the supervisor test runs, as the real worker
/// speaks it (first line on stdin, the rendezvous connect, `ready_for_secrets`, the handoff
/// header with no descriptor, then `discarded` on the stream). Without its variable it does
/// nothing.
#[test]
#[ignore = "run only as the launch discard server of a supervisor test"]
fn discard_server() {
    use org_office_launch::HostFirstLine;
    use std::os::unix::net::UnixStream;
    let Some(seen) = std::env::var_os(DISCARD_SEEN) else {
        return;
    };
    let mut stdin = std::io::BufReader::new(std::io::stdin());
    let Ok(Some(HostFirstLine::Discard(office))) =
        org_office_launch::read_first_line(&mut stdin, org_office_launch::FirstLine::Discard)
    else {
        std::process::exit(2);
    };
    if office.office_id != OFFICE || office.instance_id != INSTANCE {
        std::process::exit(3);
    }
    let mut stream = UnixStream::connect(&office.rendezvous).unwrap();
    let org_office_launch::Handoff::Discard { fds } =
        channels::tests::discard_worker_reads_handoff(&mut stream)
    else {
        std::process::exit(4);
    };
    assert!(fds.is_empty());
    let note = "discard\n";
    fs::write(seen, note).unwrap();
    org_office_launch::write_rendezvous_event(
        &mut stream,
        &org_office_launch::RendezvousEvent::Server(ServerEvent::Discarded),
    )
    .unwrap();
    std::process::exit(0);
}

/// The whole discard path: the spawner binds a rendezvous, writes the discard line, the
/// broker hands off a header with no secret although the key store holds the
/// office's key, the worker's `discarded` arrives on the stream, and only then is the key
/// deleted. The server is a `#!` script that runs this test binary, so it is started by
/// its path (the test admission): a system that executes verified bytes starts no script.
#[test]
fn a_discard_hands_off_no_key_and_deletes_it_only_after_discarded() {
    // Under /tmp: the rendezvous path must fit a socket address.
    let dir = tempfile::Builder::new()
        .prefix("sv")
        .tempdir_in("/tmp")
        .unwrap();
    fs::create_dir(dir.path().join("run")).unwrap();
    let keys = Arc::new(MemoryKeys::default());
    let key = OfficeSecretsKey::from_bytes([8; 32]);
    keys.create(OFFICE, &key).unwrap();
    let seen = dir.path().join("seen");
    // The host closes the server's stdout over the rendezvous, so libtest's output goes nowhere.
    let program = script(
        dir.path(),
        "server",
        &format!(
            "{DISCARD_SEEN}='{}' exec '{}' --exact supervisor::tests::discard_server \
                 --ignored --test-threads=1 >/dev/null",
            seen.display(),
            std::env::current_exe().unwrap().display()
        ),
    );
    let supervisor = checked_supervisor(dir.path(), program, &keys, ServerCheck::ByPath);
    assert_eq!(
        supervisor.discard(OFFICE).unwrap(),
        DiscardOutcome::Discarded
    );
    assert!(!keys.has(OFFICE), "the key outlived `discarded`");
    assert_eq!(
        fs::read_to_string(&seen).unwrap(),
        "discard\n",
        "the server didn't read a discard handoff"
    );
    assert!(keys
        .calls()
        .iter()
        .any(|call| *call == (Op::Delete, OFFICE.to_owned())));
}

/// Names the file where this test binary, run again as a launch HQ `--create` server by
/// [`an_hq_create_reported_after_shutdown_began_is_stopped_not_adopted`], records its pid.
const CREATE_PID: &str = "ORG_HOST_TEST_CREATE_PID";
/// Set for a create server that takes its handoff and then never reports and never
/// exits by itself (its closed stdin does not end it).
const CREATE_HANG: &str = "ORG_HOST_TEST_CREATE_HANG";

/// Writes `path`: a program that runs this test binary as the launch HQ `--create` server
/// ([`hq_create_server`]), which records its pid in `pid_file`. With `hang` it takes its
/// handoff and then never reports `created` and ignores its closed stdin.
pub(crate) fn write_create_server(path: &Path, pid_file: &Path, hang: bool) {
    // The host closes the server's stdout over the rendezvous, so libtest's output goes nowhere.
    let hang = if hang {
        format!("{CREATE_HANG}=1 ")
    } else {
        String::new()
    };
    let body = format!(
        "{CREATE_PID}='{}' {hang}exec '{}' --exact supervisor::tests::hq_create_server \
             --ignored --test-threads=1 >/dev/null",
        pid_file.display(),
        std::env::current_exe().unwrap().display()
    );
    fs::write(path, format!("#!/bin/sh\n{body}\n")).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o700)).unwrap();
}

/// The selected HQ create of `office`, for the organization `custody` holds it for.
fn hq_create_line(
    custody: &crate::writer::TestCustody,
    office: &str,
) -> org_office_launch::CreateLine {
    use org_proto::selected_bootstrap::{CreatorV1, PlanV1, PreparedV1};
    let prepared = PreparedV1 {
        plan: PlanV1 {
            version: 1,
            name: "Acme".into(),
            slug: "acme".into(),
            repository_remote_url: "https://example.test/org.git".into(),
            state_branch: "org/state".into(),
        },
        organization_id: custody.organization.clone(),
        office_id: office.into(),
        repository_id: crate::ids::uuid_v7().unwrap(),
        home_team_id: crate::ids::uuid_v7().unwrap(),
        hr_employee_id: crate::ids::uuid_v7().unwrap(),
        creator: CreatorV1::from_login(custody.member.clone(), "Ana"),
    };
    org_office_launch::CreateLine {
        office: org_office_launch::OfficeLine {
            instance_id: INSTANCE.into(),
            office_id: office.into(),
            rendezvous: String::new(),
        },
        org_id: custody.organization.clone(),
        org_name: "Acme".into(),
        office_name: "HQ".into(),
        role: org_proto::OfficeRole::Hq,
        owner_member_id: custody.member.clone(),
        hq_creation: org_office_launch::HqCreationV1 {
            version: org_office_launch::HqCreationV1::VERSION,
            operation_id: crate::ids::uuid_v7().unwrap(),
            escrow_profile: 1,
            bootstrap: Box::new(prepared),
            import_source: None,
        },
    }
}

/// A supervisor over real custody for `OFFICE`, `B` and `C`, with that custody.
fn custodied_supervisor(
    dir: &Path,
    program: PathBuf,
    keys: &Arc<MemoryKeys>,
) -> (Supervisor, crate::writer::TestCustody) {
    let custody = custody(dir);
    let supervisor = uncustodied_supervisor(dir, program, keys, ServerCheck::ByPath)
        .with_writers(Arc::clone(&custody.writers));
    (supervisor, custody)
}

/// Not a test: a launch HQ `--create` server, as the real worker speaks it (first line on
/// stdin, the rendezvous connect, `ready_for_secrets`, the handoff header, the transfer
/// socket after its marker, the receipt, then the writer control on the transfer socket
/// and its receipt), that reports `created` at once and keeps running. Without its
/// variable it does nothing.
#[test]
#[ignore = "run only as the launch HQ create server of a supervisor test"]
fn hq_create_server() {
    use org_office_launch::HostFirstLine;
    use std::os::unix::net::UnixStream;
    let Some(pid_file) = std::env::var_os(CREATE_PID) else {
        return;
    };
    fs::write(&pid_file, std::process::id().to_string()).unwrap();
    let mut stdin = std::io::BufReader::new(std::io::stdin());
    let Ok(Some(HostFirstLine::Create(create))) =
        org_office_launch::read_first_line(&mut stdin, org_office_launch::FirstLine::Create)
    else {
        std::process::exit(2);
    };
    let mut stream = UnixStream::connect(&create.office.rendezvous).unwrap();
    org_proto::frame::write_message(&mut stream, &org_office_launch::ReadyForSecrets).unwrap();
    let fence = std::sync::Mutex::new(());
    let mut receiver = org_socket::rights::StreamReceiver::new(stream.try_clone().unwrap());
    let mut receive = |count: usize, marker: bool| {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            let got = if marker {
                receiver
                    .receive_marker(count, org_office_launch::FD_MARKER, &fence)
                    .map(|fds| (org_office_launch::FD_MARKER, fds))
            } else {
                receiver.receive_byte(count, &fence)
            };
            match got {
                Ok(value) => return value,
                Err(org_socket::rights::ReceiveError::WouldBlock) if Instant::now() < deadline => {
                    std::thread::yield_now()
                }
                Err(_) => std::process::exit(4),
            }
        }
    };
    let mut header = Vec::new();
    loop {
        let (b, _) = receive(0, false);
        if b == b'\n' {
            break;
        }
        header.push(b);
    }
    let Ok(org_office_launch::Handoff::Create { fds, .. }) =
        org_office_launch::decode_handoff(&header)
    else {
        std::process::exit(5);
    };
    crate::secrets::zero(&mut header);
    let (_, mut transfer) = receive(fds.len(), true);
    drop(receiver);
    stream.set_nonblocking(false).unwrap();
    org_office_launch::write_rendezvous_event(
        &mut stream,
        &org_office_launch::RendezvousEvent::FdsReceived {
            of: "handoff".into(),
        },
    )
    .unwrap();
    // Every create receives its writer control before it creates anything.
    let Some(transfer) = transfer.pop() else {
        std::process::exit(6);
    };
    let mut control =
        org_socket::rights::DatagramReceiver::new(std::os::unix::net::UnixDatagram::from(transfer));
    let mut payload = [0; org_office_launch::TRANSFER_HEADER_MAX_BYTES];
    let deadline = Instant::now() + Duration::from_secs(5);
    let received = loop {
        match control.receive_up_to(&mut payload, 1, &fence) {
            Ok(received) => break received,
            Err(org_socket::rights::ReceiveError::WouldBlock) if Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(5))
            }
            Err(_) => std::process::exit(7),
        }
    };
    let Ok(org_office_launch::TransferRecord::WriterControl {
        office_id,
        channel_id,
    }) = org_office_launch::decode_transfer_record(&payload[..received.bytes])
    else {
        std::process::exit(8);
    };
    if office_id != create.office.office_id || received.descriptors.len() != 1 {
        std::process::exit(9);
    }
    // The host's admission comes first on the custody channel: it must be valid, and
    // what it names is recorded beside the pid file for the test to check. The channel
    // stays open for the server's life.
    let Some(writer) = received.descriptors.into_iter().next() else {
        std::process::exit(9);
    };
    let writer = UnixStream::from(writer);
    writer.set_nonblocking(false).unwrap();
    writer
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    let mut writer = std::io::BufReader::new(writer);
    let Ok(Some(admission)) =
        org_office_launch::writer::read::<org_office_launch::writer::Admission>(&mut writer)
    else {
        std::process::exit(11);
    };
    if !admission.check()
        || admission.identity.office_id != create.office.office_id
        || admission.identity.organization_id != create.org_id
    {
        std::process::exit(12);
    }
    let mut seen = std::path::PathBuf::from(&pid_file).into_os_string();
    seen.push(".admission");
    fs::write(seen, serde_json::to_vec(&admission).unwrap()).unwrap();
    org_office_launch::write_rendezvous_event(
        &mut stream,
        &org_office_launch::RendezvousEvent::FdsReceived { of: channel_id },
    )
    .unwrap();
    if std::env::var_os(CREATE_HANG).is_some() {
        // Never reports, and ignores its closed stdin: only a SIGKILL ends it.
        std::thread::sleep(Duration::from_secs(600));
        std::process::exit(0);
    }
    let Ok(office) = OfficeFile::selected(
        &create.office.office_id,
        &create.office_name,
        create.role,
        &create.org_id,
        &create.org_name,
        "2026-11-08T00:00:00.000Z",
    ) else {
        std::process::exit(10);
    };
    org_office_launch::write_rendezvous_event(
        &mut stream,
        &org_office_launch::RendezvousEvent::Server(ServerEvent::Created { office }),
    )
    .unwrap();
    std::thread::sleep(Duration::from_secs(30));
    std::process::exit(0);
}

/// An HQ create whose server reports `created` after shutdown began is stopped, not
/// adopted: the supervisor never starts supervising it, its server is gone, and its key
/// (the HQ's original key, installed before the create) stays. Its server is a `#!`
/// script too, started by its path.
#[test]
fn an_hq_create_reported_after_shutdown_began_is_stopped_not_adopted() {
    let dir = short_tempdir();
    let keys = Arc::new(MemoryKeys::default());
    let key = OfficeSecretsKey::from_bytes([2; 32]);
    keys.create(OFFICE, &key).unwrap();
    let pid_file = dir.path().join("pid");
    let program = dir.path().join("server");
    write_create_server(&program, &pid_file, false);
    let (supervisor, custody) = custodied_supervisor(dir.path(), program, &keys);
    let first = hq_create_line(&custody, OFFICE);
    let recovery = org_proto::RecoveryCode::new("ABCD-EFGH-IJKL".into()).unwrap();
    let pending = supervisor.begin_hq_create(first, recovery).unwrap();
    let CreateOutcome::Created(adoptable) = pending.wait(Duration::from_secs(10)) else {
        panic!("the server didn't report created");
    };
    let pid: libc::pid_t = fs::read_to_string(&pid_file).unwrap().parse().unwrap();
    // The shutdown begins between `created` and the adoption.
    supervisor.shutdown();
    assert_eq!(supervisor.adopt(*adoptable).status, OfficeStatus::Offline);
    assert_eq!(supervisor.state(OFFICE), None, "adopted after shutdown");
    // The stop reaps the server: signal 0 then finds no such process.
    // Signal 0 only probes whether the stopped child PID exists.
    assert!(
        !org_platform::posix::process_exists(pid),
        "the server outlived the shutdown"
    );
    // The HQ's original key stays: it was installed before the create, not by it.
    assert!(keys.holds(OFFICE, &key));
}

/// A shutdown returns only after a create in flight is over: its server, which ignores
/// its closed stdin, is killed at the end of the grace period and reaped, and no process
/// of it is left the moment `shutdown` returns (the create's own thread has not been
/// joined yet).
#[test]
fn a_shutdown_returns_only_after_a_create_in_flight_has_no_server_left() {
    let dir = short_tempdir();
    let keys = Arc::new(MemoryKeys::default());
    keys.create(OFFICE, &OfficeSecretsKey::from_bytes([2; 32]))
        .unwrap();
    let pid_file = dir.path().join("pid");
    let program = dir.path().join("server");
    write_create_server(&program, &pid_file, true);
    let (supervisor, custody) = custodied_supervisor(dir.path(), program, &keys);
    let recovery = org_proto::RecoveryCode::new("ABCD-EFGH-IJKL".into()).unwrap();
    let pending = supervisor
        .begin_hq_create(hq_create_line(&custody, OFFICE), recovery)
        .unwrap();
    let pid: libc::pid_t = fs::read_to_string(&pid_file).unwrap().parse().unwrap();
    thread::scope(|scope| {
        let waiting = scope.spawn(move || pending.wait(Duration::from_secs(30)));
        let began = Instant::now();
        supervisor.shutdown();
        // Signal 0 only checks the pid.
        let left = org_platform::posix::process_exists(pid);
        assert!(!left, "the create's server outlived the shutdown");
        assert!(
            began.elapsed() >= Duration::from_millis(200),
            "it ignores its stdin, so only the kill after the grace period ends it"
        );
        assert!(matches!(waiting.join().unwrap(), CreateOutcome::Stopped));
    });
    assert!(supervisor
        .begin_hq_create(
            hq_create_line(&custody, OFFICE),
            org_proto::RecoveryCode::new("ABCD-EFGH-IJKL".into()).unwrap()
        )
        .is_err());
}

/// X5 review V2: the wait for short runs is bounded from the deadline the stopping host
/// fixed, not from when the supervisor's turn came: a run whose owner never takes its
/// stop signal is waited for until [`RUN_END_MARGIN`] after that deadline, and then left
/// to its owner, which kills its server when it drops it.
#[test]
fn a_shutdown_waits_for_a_run_until_its_margin_after_the_hosts_deadline() {
    let dir = short_tempdir();
    let keys = Arc::new(MemoryKeys::default());
    keys.create(OFFICE, &OfficeSecretsKey::from_bytes([2; 32]))
        .unwrap();
    let pid_file = dir.path().join("pid");
    let program = dir.path().join("server");
    write_create_server(&program, &pid_file, true);
    let (supervisor, custody) = custodied_supervisor(dir.path(), program, &keys);
    let recovery = org_proto::RecoveryCode::new("ABCD-EFGH-IJKL".into()).unwrap();
    // Its owner (this test) never waits on it, so never takes the stop.
    let pending = supervisor
        .begin_hq_create(hq_create_line(&custody, OFFICE), recovery)
        .unwrap();
    let pid: libc::pid_t = fs::read_to_string(&pid_file).unwrap().parse().unwrap();
    // The host fixed its deadline a grace period ago.
    let deadline = Instant::now();
    supervisor.shutdown_by(deadline);
    let took = deadline.elapsed();
    assert!(took >= RUN_END_MARGIN, "{took:?}");
    assert!(
        took < RUN_END_MARGIN + Duration::from_millis(400),
        "{took:?}"
    );
    drop(pending);
    // Signal 0 only probes whether the stopped child PID exists.
    assert!(
        !org_platform::posix::process_exists(pid),
        "the run's server outlived its owner"
    );
}

/// X5 review V2: a create whose worker has not connected to its launch rendezvous yet
/// does not hold the host's stop for the handshake's own 5 s: the shutdown ends the
/// handshake at once, the create fails, and its worker is killed before its owner returns.
#[test]
fn a_shutdown_ends_a_launch_handshake_at_once_and_kills_its_worker() {
    let dir = short_tempdir();
    let keys = Arc::new(MemoryKeys::default());
    keys.create(OFFICE, &OfficeSecretsKey::from_bytes([2; 32]))
        .unwrap();
    let pid_file = dir.path().join("pid");
    // It never connects to the rendezvous.
    let program = script(
        dir.path(),
        "server",
        &format!("echo $$ > '{}'; exec sleep 60", pid_file.display()),
    );
    let (supervisor, custody) = custodied_supervisor(dir.path(), program, &keys);
    thread::scope(|scope| {
        let creating = scope.spawn(|| {
            let recovery = org_proto::RecoveryCode::new("ABCD-EFGH-IJKL".into()).unwrap();
            let begun = supervisor.begin_hq_create(hq_create_line(&custody, OFFICE), recovery);
            (begun.is_err(), Instant::now())
        });
        let until = Instant::now() + Duration::from_secs(10);
        let pid: libc::pid_t = loop {
            if let Some(pid) = fs::read_to_string(&pid_file)
                .ok()
                .and_then(|text| text.trim().parse().ok())
            {
                break pid;
            }
            assert!(Instant::now() < until, "the worker never started");
            thread::sleep(Duration::from_millis(5));
        };
        let asked = Instant::now();
        supervisor.shutdown_by(asked + Duration::from_millis(200));
        let (failed, returned) = creating.join().unwrap();
        assert!(failed, "the create did not fail");
        let took = returned.duration_since(asked);
        assert!(took < Duration::from_secs(1), "{took:?}");
        // Signal 0 only probes whether the stopped worker PID exists.
        assert!(
            !org_platform::posix::process_exists(pid),
            "the worker outlived its handshake"
        );
    });
}

#[test]
fn a_discard_whose_key_cant_be_read_starts_nothing_and_keeps_everything() {
    let dir = short_tempdir();
    let keys = Arc::new(MemoryKeys::default());
    keys.create(OFFICE, &OfficeSecretsKey::from_bytes([7; 32]))
        .unwrap();
    let seen = dir.path().join("seen");
    let program = script(
        dir.path(),
        "server",
        &format!("echo run >> '{}'", seen.display()),
    );
    let supervisor = supervisor(dir.path(), program, &keys);
    for fault in [Fault::Corrupt, Fault::Io, Fault::Os(-25308)] {
        keys.fail(Op::Load, fault);
        assert_eq!(
            supervisor.discard(OFFICE).unwrap(),
            DiscardOutcome::KeyUnreadable,
            "{fault:?}"
        );
    }
    assert!(!seen.exists(), "a server started");
    assert!(keys.has(OFFICE));
    assert!(!keys.calls().iter().any(|(op, _)| *op == Op::Delete));
}

/// M8: every office needs writer custody. An office without a valid record is never
/// spawned and its key is never read; it is `unavailable` (`storage_error`) at once, not
/// retried with backoff, and `office.restart` tries once more without a loop. A host with
/// no writer custody at all starts no office either.
#[test]
fn an_office_without_writer_custody_never_starts_and_is_not_restarted_in_a_loop() {
    let unavailable = |s: &OfficeState| {
        s.status == OfficeStatus::Unavailable
            && s.reason == Some(OfficeStatusReason::StorageError)
            && s.next_restart_at.is_none()
    };
    let dir = short_tempdir();
    let keys = Arc::new(MemoryKeys::default());
    for id in [OFFICE, NO_CUSTODY] {
        keys.create(id, &OfficeSecretsKey::from_bytes([8; 32]))
            .unwrap();
    }
    let supervisor = supervisor(dir.path(), recording_worker(), &keys);
    supervisor.start_office(NO_CUSTODY).unwrap();
    wait_for_state(&supervisor, NO_CUSTODY, unavailable);
    // Longer than the first backoff: still unavailable, still never spawned.
    thread::sleep(Duration::from_millis(500));
    assert!(unavailable(&supervisor.state(NO_CUSTODY).unwrap()));
    assert_eq!(
        recorded_starts(dir.path()),
        0,
        "a server started without custody"
    );
    assert_eq!(supervisor.restart(NO_CUSTODY), RestartOutcome::Restarting);
    thread::sleep(Duration::from_millis(300));
    wait_for_state(&supervisor, NO_CUSTODY, unavailable);
    assert_eq!(recorded_starts(dir.path()), 0);
    assert!(!keys
        .calls()
        .iter()
        .any(|(op, id)| *op == Op::Load && id == NO_CUSTODY));
    // The control: an office with custody is started.
    supervisor.start_office(OFFICE).unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    while recorded_starts(dir.path()) == 0 {
        assert!(
            Instant::now() < deadline,
            "the office with custody never ran"
        );
        thread::sleep(Duration::from_millis(10));
    }
    supervisor.shutdown();

    // A host with no writer custody at all.
    let dir = short_tempdir();
    let bare = uncustodied_supervisor(dir.path(), recording_worker(), &keys, ServerCheck::Skip);
    bare.start_office(OFFICE).unwrap();
    wait_for_state(&bare, OFFICE, unavailable);
    thread::sleep(Duration::from_millis(300));
    assert!(unavailable(&bare.state(OFFICE).unwrap()));
    bare.shutdown();
    assert_eq!(recorded_starts(dir.path()), 0);
    assert!(keys.has(OFFICE) && keys.has(NO_CUSTODY));
}

/// Custody that can't be read now (the custody store fails) is not a refusal: the
/// office is `offline` (`storage_error`) with a restart scheduled, nothing is spawned
/// meanwhile, and once custody reads again the office starts.
#[test]
fn unreadable_custody_is_retried_and_the_office_starts_once_it_reads() {
    let dir = short_tempdir();
    let keys = Arc::new(MemoryKeys::default());
    let custody = custody(dir.path());
    let supervisor =
        uncustodied_supervisor(dir.path(), recording_worker(), &keys, ServerCheck::Skip)
            .with_writers(Arc::clone(&custody.writers));
    custody
        .unreadable
        .store(true, std::sync::atomic::Ordering::SeqCst);
    supervisor.start_office(OFFICE).unwrap();
    wait_for_state(&supervisor, OFFICE, |s| {
        s.status == OfficeStatus::Offline
            && s.reason == Some(OfficeStatusReason::StorageError)
            && s.next_restart_at.is_some()
    });
    assert_eq!(
        recorded_starts(dir.path()),
        0,
        "started over unreadable custody"
    );
    custody
        .unreadable
        .store(false, std::sync::atomic::Ordering::SeqCst);
    let deadline = Instant::now() + Duration::from_secs(15);
    while recorded_starts(dir.path()) == 0 {
        assert!(
            Instant::now() < deadline,
            "never started once custody read: {:?}",
            supervisor.state(OFFICE)
        );
        thread::sleep(Duration::from_millis(20));
    }
    supervisor.shutdown();
}

/// A create needs the custody provisioning reserved for that office, organization and
/// owner member: otherwise nothing is spawned and the key is not read.
#[test]
fn a_create_without_its_custody_starts_nothing() {
    let dir = short_tempdir();
    let keys = Arc::new(MemoryKeys::default());
    for id in [OFFICE, NO_CUSTODY] {
        keys.create(id, &OfficeSecretsKey::from_bytes([2; 32]))
            .unwrap();
    }
    let (supervisor, custody) = custodied_supervisor(dir.path(), recording_worker(), &keys);
    let recovery = || org_proto::RecoveryCode::new("ABCD-EFGH-IJKL".into()).unwrap();
    // No record for the office: refused, and the host log says why.
    let (refused, logged) = host_log::capture(|| {
        supervisor
            .begin_hq_create(hq_create_line(&custody, NO_CUSTODY), recovery())
            .is_err()
    });
    assert!(refused);
    assert!(
        logged
            .iter()
            .any(|line| line.contains(NO_CUSTODY) && line.contains("no valid writer custody")),
        "{logged:?}"
    );
    // A record, but for another organization.
    let mut other = hq_create_line(&custody, OFFICE);
    let organization = crate::ids::uuid_v7().unwrap();
    other.org_id = organization.clone();
    other.hq_creation.bootstrap.organization_id = organization;
    assert!(supervisor.begin_hq_create(other, recovery()).is_err());
    // A record for this organization, but another owner member.
    let mut other = hq_create_line(&custody, OFFICE);
    let member = crate::ids::uuid_v7().unwrap();
    other.owner_member_id = member.clone();
    other.hq_creation.bootstrap.creator.employee_id = member;
    assert!(supervisor.begin_hq_create(other, recovery()).is_err());
    assert_eq!(recorded_starts(dir.path()), 0, "a create server ran");
    assert!(!keys.calls().iter().any(|(op, _)| *op == Op::Load));
    supervisor.shutdown();
}

/// G1: a host with no registered instance spawns nothing: no discard, no office start.
#[test]
fn an_unregistered_supervisor_spawns_nothing() {
    let dir = short_tempdir();
    let ran = dir.path().join("ran");
    let program = script(
        dir.path(),
        "server",
        &format!("echo run >> '{}'", ran.display()),
    );
    let keys = Arc::new(MemoryKeys::default());
    keys.create(OFFICE, &OfficeSecretsKey::from_bytes([3; 32]))
        .unwrap();
    let supervisor = Supervisor::unregistered(SupervisorConfig {
        program,
        home: dir.path().to_path_buf(),
        runtime_dir: None,
        logs_dir: dir.path().join("logs"),
        log_max_bytes: 1024,
        timings: Timings::default(),
        keys: Arc::clone(&keys) as Arc<dyn OfficeSecretsKeys>,
        server_check: Arc::new(ServerCheck::Skip),
        sweep_orphans: false,
        federation: None,
    });
    let err = supervisor.discard(OFFICE).unwrap_err();
    assert!(err.to_string().contains("setup must run first"), "{err}");
    let err = supervisor.start_office(OFFICE).unwrap_err();
    assert!(err.to_string().contains("setup must run first"), "{err}");
    supervisor.shutdown();
    assert!(!ran.exists(), "an unregistered host started a server");
    assert!(keys.has(OFFICE));
    assert!(!keys.calls().iter().any(|(op, _)| *op == Op::Delete));
}
