//! ENC-1 and ENC-5 through real processes (ADR-0016 decisions 3 and 5; A1 plan §3): the
//! real registered host supervises copies of the real built `agentic-org-office` with
//! debug development key custody under a disposable `/private/tmp` root (never the
//! Keychain, never a real organization root). Debug `ServerCheck::Skip` admits the
//! worker; this proves no release-signing evidence.
//!
//! It proves: a provisioned HQ reaches `active`; after the worker is killed the
//! supervisor restarts it to `active` again; after the office's key file is deleted
//! from the development store a restart still reaches `active` with no status reason
//! (the start proceeds with no key) and the data is plain SQLite that reads with no
//! key. Its dev key file holds the office secrets key as lowercase hex (its stored and
//! handoff form); once that file is deleted, no remaining file holds the key as raw bytes
//! or lowercase/uppercase hex. The wrapped CA record is found raw in the office's folder
//! and, in any of those forms, in no file outside it.
use crate::web_process_tests::{provision_hq, start_signed_in, Started};
use org_proto::{OfficeInfo, OfficeStatus};
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
    thread,
    time::{Duration, Instant},
};

const WAIT: Duration = Duration::from_secs(60);

fn office(host: &crate::Host, id: &str) -> Option<OfficeInfo> {
    host.offices.offices().into_iter().find(|o| o.id == id)
}

fn wait_active(host: &crate::Host, id: &str, what: &str) {
    let until = Instant::now() + WAIT;
    loop {
        let info = office(host, id);
        if let Some(info) = &info {
            if info.status == OfficeStatus::Active {
                assert_eq!(info.status_reason, None, "{what}");
                return;
            }
        }
        assert!(
            Instant::now() < until,
            "{what}: never active; last {info:?}"
        );
        thread::sleep(Duration::from_millis(20));
    }
}

/// The live processes executing exactly `program` (a unique temp path).
fn pids(program: &Path) -> Vec<i32> {
    let out = Command::new("/usr/bin/pgrep")
        .arg("-f")
        .arg(program)
        .output()
        .unwrap();
    String::from_utf8(out.stdout)
        .unwrap()
        .split_whitespace()
        .map(|p| p.parse().unwrap())
        .collect()
}

/// Kills the single running worker and waits until the supervisor runs a new one.
fn kill_worker_and_wait_for_another(worker: &Path) -> i32 {
    let before = pids(worker);
    assert_eq!(before.len(), 1, "exactly one worker: {before:?}");
    // SAFETY: plain signal to a child process this test's host started.
    assert_eq!(unsafe { libc::kill(before[0], libc::SIGKILL) }, 0);
    let until = Instant::now() + WAIT;
    loop {
        let now = pids(worker);
        if now.len() == 1 && now[0] != before[0] {
            return now[0];
        }
        assert!(Instant::now() < until, "no restarted worker; {now:?}");
        thread::sleep(Duration::from_millis(20));
    }
}

fn files(root: &Path, out: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(root).unwrap() {
        let path = entry.unwrap().path();
        let kind = fs::symlink_metadata(&path).unwrap().file_type();
        if kind.is_dir() {
            files(&path, out);
        } else if kind.is_file() {
            out.push(path);
        }
    }
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack.windows(needle.len()).any(|w| w == needle)
}

/// The forms a secret can take in a file: raw bytes and lowercase/uppercase hex. Keys
/// travel as hex (handoff JSON, `keys/dev.rs`); no key or CA channel uses base64.
fn encodings(bytes: &[u8]) -> [Vec<u8>; 3] {
    let lower: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
    let upper = lower.to_ascii_uppercase();
    [bytes.to_vec(), lower.into_bytes(), upper.into_bytes()]
}

fn unhex(text: &str) -> Vec<u8> {
    let text = text.trim();
    (0..text.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&text[i..i + 2], 16).unwrap())
        .collect()
}

#[test]
#[ignore = "root runs with ORG_TEST_OFFICE_BIN and ORG_TEST_WEB_BIN; actual web/worker listeners require permitted local bind"]
fn actual_host_restarts_a_killed_worker_and_starts_it_without_its_key_on_plain_sqlite() {
    // `_temp` is bound first so it drops last, after the host and its children.
    let Started {
        temp: _temp,
        program,
        host,
        mut wire,
        session,
        ..
    } = start_signed_in();
    let base = _temp.path().canonicalize().unwrap();
    let worker = program.with_file_name("agentic-org-office");
    let mut id = 3;
    let (_, provisioned) = provision_hq(&mut wire, &mut id, session);
    let office_id = provisioned.office_id;
    wait_active(&host, &office_id, "provisioned");

    // The dev store holds exactly one key file for this office, outside the office home.
    let mut all = Vec::new();
    files(&base.join("keys"), &mut all);
    let key_files: Vec<_> = all
        .iter()
        .filter(|p| p.file_name().unwrap() == format!("{office_id}.key").as_str())
        .cloned()
        .collect();
    assert_eq!(key_files.len(), 1, "{all:?}");
    let key_text = fs::read_to_string(&key_files[0]).unwrap();
    let key = unhex(&key_text);
    assert_eq!(key.len(), 32);
    // Positive control: the key file holds the key in the lowercase hex form scanned below.
    let [_, key_lower_hex, _] = encodings(&key);
    assert!(
        contains(key_text.as_bytes(), &key_lower_hex),
        "key file is not lowercase hex"
    );

    // A killed worker is restarted and reaches `active` again.
    kill_worker_and_wait_for_another(&worker);
    wait_active(&host, &office_id, "restart after kill");

    // Without its key file the next start proceeds with no key and still serves.
    fs::remove_file(&key_files[0]).unwrap();
    kill_worker_and_wait_for_another(&worker);
    wait_active(&host, &office_id, "restart without a key");

    // ENC-1: both office databases are plain SQLite that read with no key, beside the
    // live office.
    let mut all = Vec::new();
    files(&base, &mut all);
    let folder = all
        .iter()
        .find(|p| {
            p.file_name().unwrap() == "office.db"
                && p.parent().unwrap().file_name().unwrap() == office_id.as_str()
        })
        .expect("the office's office.db")
        .parent()
        .unwrap()
        .to_path_buf();
    for name in ["office.db", "org.db"] {
        let bytes = fs::read(folder.join(name)).unwrap();
        assert!(bytes.starts_with(b"SQLite format 3\0"), "{name}");
    }
    let read_only = rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY;
    let db = rusqlite::Connection::open_with_flags(folder.join("office.db"), read_only).unwrap();
    let wrapped: Vec<u8> = db
        .query_row(
            "SELECT wrapped_ca_key FROM local_hq_creation WHERE singleton=1",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert!(wrapped.len() > 32);
    let org = rusqlite::Connection::open_with_flags(folder.join("org.db"), read_only).unwrap();
    let tables: i64 = org
        .query_row("SELECT count(*) FROM sqlite_schema", [], |r| r.get(0))
        .unwrap();
    assert!(tables > 0);
    // The office answers the host's status checks on the same data with no key.
    wait_active(&host, &office_id, "serving without a key");

    // ENC-5: with its dev key file deleted, the office secrets key, raw or hex, appears
    // in no remaining file, so neither the office nor the host stored it elsewhere; the
    // wrapped CA record, raw or hex, appears only in the office's own folder, so no
    // rendezvous event or handoff carried it to anything the host wrote (registry, logs,
    // runtime files).
    let key_forms = encodings(&key);
    let wrapped_forms = encodings(&wrapped);
    assert!(
        !all.contains(&key_files[0]),
        "the deleted key file is not scanned"
    );
    let mut scanned = 0;
    let mut wrapped_in_folder = false;
    for path in &all {
        if path.starts_with(base.join("bin")) {
            continue;
        }
        let Ok(bytes) = fs::read(path) else { continue };
        scanned += 1;
        for form in &key_forms {
            assert!(!contains(&bytes, form), "office secrets key in {path:?}");
        }
        if path.starts_with(&folder) {
            wrapped_in_folder |= contains(&bytes, &wrapped);
        } else {
            for form in &wrapped_forms {
                assert!(!contains(&bytes, form), "wrapped CA record in {path:?}");
            }
        }
    }
    assert!(scanned > 4, "{scanned}");
    // Positive control: the scan reads the files that do hold the wrapped CA record.
    assert!(
        wrapped_in_folder,
        "no file in {folder:?} holds the wrapped CA record"
    );
    drop(wire);
    host.shutdown();
}
