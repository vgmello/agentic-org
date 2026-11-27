//! `agentic-org backup` and `agentic-org restore` as a person runs them (`TASK-104`): the real
//! binary against a registered instance under a temporary application base
//! (`ORG_DEV_ACCOUNT_BASE`, which only a debug build reads), whose office is a real SQLite HQ
//! made by `org-office`'s test kit. Nothing touches a user's organization workspace.

#![cfg(debug_assertions)]

use std::fs;
use std::io::Write;
use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

use org_host_setup::fixture::registered_account;
use org_office::{Office, TestOffice};
use serde_json::Value;

const PASSPHRASE: &[u8] = b"correct horse battery staple\n";

struct Instance {
    _dir: tempfile::TempDir,
    root: PathBuf,
    base: PathBuf,
    folder: PathBuf,
    durable: PathBuf,
    org_id: String,
    pass: PathBuf,
}

impl Instance {
    fn new() -> Self {
        // Short and disk-backed: the instance's socket paths must fit their limit.
        let dir = tempfile::Builder::new()
            .prefix("bk")
            .tempdir_in(org_platform::dirs::testing::disk_backed_temp().unwrap())
            .unwrap();
        let root = dir.path().canonicalize().unwrap();
        let base = root.join("a");
        fs::DirBuilder::new().mode(0o700).create(&base).unwrap();
        let port = std::net::TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
        let account = registered_account(&base, None, port).unwrap();
        let (office, roots, _, _) = Office::for_test_with_real_ca_in(
            &root.join("t"),
            TestOffice::sqlite(org_office_sqlite::sqlite_backend()),
        );
        let org_id = office.info().org_id.clone();
        let office_id = office.info().office_id.clone();
        let made = office.folder().to_path_buf();
        office.close().unwrap();
        // Where the host keeps its offices: the office records its own data roots.
        let folder = account.office_home.join("offices").join(&office_id);
        fs::rename(&made, &folder).unwrap();
        let pass = root.join("pass");
        fs::write(&pass, PASSPHRASE).unwrap();
        fs::set_permissions(&pass, fs::Permissions::from_mode(0o600)).unwrap();
        Self {
            _dir: dir,
            root,
            base,
            folder,
            durable: roots.durable,
            org_id,
            pass,
        }
    }

    fn run(&self, args: &[&str], stdin: Option<&[u8]>) -> Output {
        let mut child = Command::new(env!("CARGO_BIN_EXE_agentic-org"))
            .args(args)
            .env("ORG_DEV_ACCOUNT_BASE", &self.base)
            .env_remove("AGENTIC_ORG_INSTANCE")
            .env_remove("AGENTIC_ORG_OFFICE")
            .current_dir(&self.root)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let mut input = child.stdin.take().unwrap();
        if let Some(bytes) = stdin {
            input.write_all(bytes).unwrap();
        }
        drop(input);
        child.wait_with_output().unwrap()
    }

    /// [`Self::run`] with nothing on standard input, killed and failed if it has not ended
    /// within `limit`: a command that waits for a terminal answer must not hang a test run.
    fn run_within(&self, args: &[&str], limit: std::time::Duration) -> Output {
        let mut child = Command::new(env!("CARGO_BIN_EXE_agentic-org"))
            .args(args)
            .env("ORG_DEV_ACCOUNT_BASE", &self.base)
            .env_remove("AGENTIC_ORG_INSTANCE")
            .env_remove("AGENTIC_ORG_OFFICE")
            .current_dir(&self.root)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let started = std::time::Instant::now();
        while child.try_wait().unwrap().is_none() {
            if started.elapsed() > limit {
                let _ = child.kill();
                let _ = child.wait();
                panic!(
                    "`agentic-org {}` did not end within {limit:?}",
                    args.join(" ")
                );
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        child.wait_with_output().unwrap()
    }

    fn path(&self, name: &str) -> String {
        self.root.join(name).to_str().unwrap().to_owned()
    }
}

fn json(output: &Output) -> Value {
    assert!(
        output.status.success(),
        "{:?}\nstdout: {}\nstderr: {}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}

fn refused(output: &Output, code: i32, needle: &str) {
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(output.status.code(), Some(code), "{stderr}");
    assert!(stderr.contains(needle), "{needle:?} not in {stderr}");
}

fn mode(path: &Path) -> u32 {
    fs::metadata(path).unwrap().permissions().mode() & 0o777
}

#[test]
fn backup_verify_and_restore_round_trip_through_the_command_line() {
    let at = Instance::new();
    let archive = at.path("acme.backup");
    let pass = at.pass.to_str().unwrap().to_owned();
    let made = json(&at.run(
        &[
            "backup",
            &at.org_id,
            "--out",
            &archive,
            "--encrypt",
            "--passphrase-file",
            &pass,
            "--json",
        ],
        None,
    ));
    assert_eq!(made["encrypted"], true);
    assert_eq!(made["org_id"], at.org_id.as_str());
    assert!(made["head_ordinal"].as_u64().unwrap() >= 1);
    assert_eq!(mode(Path::new(&archive)), 0o600);

    let to = at.path("checked");
    let verified = json(&at.run(
        &[
            "restore",
            "--verify",
            &archive,
            "--to",
            &to,
            "--passphrase-fd",
            "0",
            "--json",
        ],
        Some(PASSPHRASE),
    ));
    assert_eq!(verified["verified"], true);
    assert_eq!(verified["rebuilt"]["offices"], 1);
    assert_eq!(verified["rebuilt"]["head_ordinal"], made["head_ordinal"]);
    assert!(Path::new(&to).join("manifest.json").is_file());
    assert!(Path::new(&to).join("durable/state").is_dir());

    // An in-place restore asks first; where no one can be asked it needs --yes.
    let restore = [
        "restore",
        &archive,
        "--org",
        &at.org_id,
        "--passphrase-file",
        &pass,
        "--json",
    ];
    refused(&at.run(&restore, None), 2, "--yes");
    let mut confirmed = restore.to_vec();
    confirmed.push("--yes");
    let restored = json(&at.run(&confirmed, None));
    assert_eq!(restored["restored"], true);
    assert_eq!(restored["live_journal"]["state"], "head");
    assert!(restored["scope_notice"]
        .as_str()
        .unwrap()
        .contains("Keychain"));
    let previous = restored["previous_durable_root"].as_str().unwrap();
    assert!(Path::new(previous).join("state").is_dir());
    assert!(at.durable.join("state").is_dir());
}

#[test]
fn the_command_line_refuses_a_running_office_a_wrong_passphrase_and_unsafe_sources() {
    let at = Instance::new();
    let archive = at.path("acme.backup");
    let pass = at.pass.to_str().unwrap().to_owned();

    let lock = fs::OpenOptions::new()
        .write(true)
        .open(at.folder.join("office.lock"))
        .unwrap();
    lock.try_lock().unwrap();
    let held = at.run(&["backup", &at.org_id, "--out", &archive, "--json"], None);
    refused(&held, 1, "backup::office_running");
    assert!(!Path::new(&archive).exists());
    lock.unlock().unwrap();

    json(&at.run(
        &[
            "backup",
            &at.org_id,
            "--out",
            &archive,
            "--encrypt",
            "--passphrase-file",
            &pass,
            "--json",
        ],
        None,
    ));
    let wrong = at.run(
        &[
            "restore",
            "--verify",
            &archive,
            "--passphrase-fd",
            "0",
            "--json",
        ],
        Some(b"incorrect horse battery staple\n"),
    );
    refused(&wrong, 1, "backup::authentication");
    let none = at.run(
        &["restore", "--verify", &archive, "--no-input", "--json"],
        None,
    );
    refused(&none, 2, "--passphrase-file");

    fs::set_permissions(&at.pass, fs::Permissions::from_mode(0o644)).unwrap();
    let open = at.run(
        &[
            "backup",
            &at.org_id,
            "--out",
            &at.path("b2"),
            "--encrypt",
            "--passphrase-file",
            &pass,
        ],
        None,
    );
    refused(&open, 2, "chmod 600");
    let plain = at.run(
        &[
            "backup",
            &at.org_id,
            "--out",
            &at.path("b3"),
            "--passphrase-fd",
            "0",
        ],
        None,
    );
    refused(&plain, 2, "need --encrypt");
    let slug = at.run(&["backup", "acme", "--out", &at.path("b4")], None);
    refused(&slug, 2, "organization ID");
}

#[test]
fn machine_mode_never_asks_for_a_passphrase_and_keeps_standard_error_empty() {
    let at = Instance::new();
    let archive = at.path("acme.backup");
    let pass = at.pass.to_str().unwrap().to_owned();
    let limit = std::time::Duration::from_secs(60);
    // `--json` (or `-q`) without a passphrase source fails at once (exit 2), never waiting on
    // the terminal, and writes no archive.
    for mode in ["--json", "-q"] {
        let asked = at.run_within(
            &["backup", &at.org_id, "--out", &archive, "--encrypt", mode],
            limit,
        );
        // Refused before the terminal is touched (not "could not be read" from `/dev/tty`).
        refused(&asked, 2, "no one can be asked here");
        refused(&asked, 2, "--passphrase-file");
        assert!(!Path::new(&archive).exists());
    }
    json(&at.run(
        &[
            "backup",
            &at.org_id,
            "--out",
            &archive,
            "--encrypt",
            "--passphrase-file",
            &pass,
            "--json",
        ],
        None,
    ));
    let verify = at.run_within(&["restore", "--verify", &archive, "--json"], limit);
    refused(&verify, 2, "no one can be asked here");

    // A human verification shows the rebuilt projections on the card, not only in --json.
    let shown = at.run(
        &["restore", "--verify", &archive, "--passphrase-file", &pass],
        None,
    );
    let stdout = String::from_utf8_lossy(&shown.stdout);
    assert!(shown.status.success(), "{stdout}");
    assert!(stdout.contains("rebuilt\t"), "{stdout}");
    assert!(stdout.contains("offices 1"), "{stdout}");
    assert!(stdout.contains("stopped_cleanly true"), "{stdout}");

    // Over a live journal that does not verify, a --json restore reports it as a field and
    // writes nothing to standard error.
    let acceptance = at
        .durable
        .join("recovery/offices")
        .read_dir()
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path()
        .join("journals/acceptance");
    let incarnation = acceptance
        .read_dir()
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    fs::write(incarnation.join("anchor.json"), b"{not an anchor").unwrap();
    let restored = at.run(
        &[
            "restore",
            &archive,
            "--org",
            &at.org_id,
            "--passphrase-file",
            &pass,
            "--yes",
            "--json",
        ],
        None,
    );
    let answer = json(&restored);
    assert_eq!(answer["live_journal"]["state"], "unreadable", "{answer}");
    assert!(
        restored.stderr.is_empty(),
        "{}",
        String::from_utf8_lossy(&restored.stderr)
    );
    let previous = answer["previous_durable_root"].as_str().unwrap();
    assert!(
        Path::new(previous)
            .parent()
            .unwrap()
            .ends_with("restore-backups"),
        "{previous}"
    );
}

#[test]
fn protected_restore_enforces_0700_0600_discards_protected_dbs_and_fails_closed_on_tampering() {
    let at = Instance::new();
    let archive = at.path("protected.backup");
    let pass = at.pass.to_str().unwrap().to_owned();

    // Locate the office's runtime folder (sibling of durable/) and seed stale protected DB files
    // representing protected/messaging.db, protected/accounting.db, protected/budgets.db.
    let office_id = at.folder.file_name().unwrap().to_str().unwrap().to_owned();
    let runtime_root = at.durable.parent().unwrap().join("runtime");
    let protected_dir = runtime_root.join(&office_id).join("protected");
    fs::create_dir_all(&protected_dir).unwrap();
    fs::set_permissions(&protected_dir, fs::Permissions::from_mode(0o700)).unwrap();
    for db in ["messaging.db", "accounting.db", "budgets.db"] {
        let p = protected_dir.join(db);
        fs::write(&p, b"STALE_PRE_RESTORE_PROJECTION").unwrap();
        fs::set_permissions(&p, fs::Permissions::from_mode(0o600)).unwrap();
    }

    // 1. Create encrypted backup (0600).
    let made = json(&at.run(
        &[
            "backup",
            &at.org_id,
            "--out",
            &archive,
            "--encrypt",
            "--passphrase-file",
            &pass,
            "--json",
        ],
        None,
    ));
    assert_eq!(made["encrypted"], true);
    assert_eq!(mode(Path::new(&archive)), 0o600);

    // 2. Tamper one byte of a copy of the archive -> restore MUST fail closed without changing
    // live durable files or deleting existing protected projections!
    let tampered = at.path("tampered.backup");
    let mut raw = fs::read(&archive).unwrap();
    let flip_at = raw.len() - 8;
    raw[flip_at] ^= 0x55;
    fs::write(&tampered, &raw).unwrap();
    fs::set_permissions(&tampered, fs::Permissions::from_mode(0o600)).unwrap();

    let marker_file = at.durable.join("state/.live-sentinel");
    fs::write(&marker_file, b"LIVE_BEFORE_TAMPERED_RESTORE").unwrap();
    let bad_restore = at.run(
        &[
            "restore",
            &tampered,
            "--org",
            &at.org_id,
            "--passphrase-file",
            &pass,
            "--yes",
            "--json",
        ],
        None,
    );
    assert!(
        !bad_restore.status.success(),
        "tampered archive must be refused"
    );
    assert_eq!(
        fs::read(&marker_file).unwrap(),
        b"LIVE_BEFORE_TAMPERED_RESTORE",
        "failed tampered restore must not alter live durable state"
    );
    for db in ["messaging.db", "accounting.db", "budgets.db"] {
        assert!(
            protected_dir.join(db).is_file(),
            "failed tampered restore must not discard live protected/{db}"
        );
    }

    // 3. Valid restore atomically swaps durable root, preserves previous root under 0700
    // restore-backups/before-<suffix>, enforces 0700/0600 permissions, and discards stale
    // protected companion DBs so the office rebuilds them cleanly from the restored durable root.
    let restored = json(&at.run(
        &[
            "restore",
            &archive,
            "--org",
            &at.org_id,
            "--passphrase-file",
            &pass,
            "--yes",
            "--json",
        ],
        None,
    ));
    assert_eq!(restored["restored"], true);
    let previous = PathBuf::from(restored["previous_durable_root"].as_str().unwrap());
    assert!(
        previous.join("state/.live-sentinel").is_file(),
        "atomic swap must preserve the displaced live root"
    );
    assert!(
        !at.durable.join("state/.live-sentinel").exists(),
        "restored durable root must come strictly from the verified archive"
    );
    assert_eq!(mode(&at.durable), 0o700);
    assert_eq!(mode(previous.parent().unwrap()), 0o700);
    for db in ["messaging.db", "accounting.db", "budgets.db"] {
        assert!(
            !protected_dir.join(db).exists(),
            "restore must discard stale protected/{db} for clean rebuild"
        );
    }

    // Walk restored durable tree: every directory is 0700 and every regular file is 0600.
    fn assert_owner_only_tree(dir: &Path) {
        assert_eq!(mode(dir), 0o700, "dir {}", dir.display());
        for entry in fs::read_dir(dir).unwrap() {
            let entry = entry.unwrap();
            let path = entry.path();
            let meta = fs::symlink_metadata(&path).unwrap();
            assert!(
                !meta.file_type().is_symlink(),
                "no symlinks in restored tree"
            );
            if meta.is_dir() {
                assert_owner_only_tree(&path);
            } else {
                assert_eq!(
                    meta.permissions().mode() & 0o777,
                    0o600,
                    "file {}",
                    path.display()
                );
            }
        }
    }
    assert_owner_only_tree(&at.durable);
}
