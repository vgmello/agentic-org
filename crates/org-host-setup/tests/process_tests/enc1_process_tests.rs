//! ENC-1 and ENC-5 through real processes (ADR-0016 decisions 3 and 5; A1 plan §3): the
//! real registered host supervises copies of the real built `agentic-org-office` with
//! debug development key custody under a disposable root on disk (never the
//! Keychain, never a real organization root). Debug `ServerCheck::Skip` admits the
//! worker; this proves no release-signing evidence.
//!
//! It proves: a provisioned HQ reaches `active`; after the worker is killed the
//! supervisor restarts it to `active` again; after the office's key file is deleted
//! from the development store a restart still reaches `active` with no status reason
//! (the start proceeds with no key). The office's store keeps its projection as plain
//! SQLite at `<instance-root>/<organization-id>/runtime/<office-id>/office.db` (its file
//! header, read as bytes: this test does not open it), and the wrapped CA key in the
//! office's local record `<organization-id>/durable/local/offices/<office-id>/security/
//! ca-key.v1.json` (data contract document 16), read here from that file.
//!
//! The dev key file holds the office secrets key as lowercase hex (its stored and handoff
//! form); once that file is deleted, no remaining file holds the key as raw bytes or
//! lowercase/uppercase hex. The wrapped CA key, in any of those forms, is in the local
//! record and may be in the office's acceptance journal under `<organization-id>/durable/
//! recovery/offices/<office-id>/journals/acceptance/` (creation's journal entry carries
//! the record until its segment is released); it is in no other file: not the
//! projection, not `durable/state/` (the organization's Git content), not the host's
//! registry, logs or runtime files.
use crate::web_process_tests::{create_hq, start_signed_in, Started};
use org_host::test_support::process;
use org_host::Host;
use org_proto::{OfficeInfo, OfficeStatus};
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
    thread,
    time::{Duration, Instant},
};

const WAIT: Duration = Duration::from_secs(60);

fn office(host: &Host, id: &str) -> Option<OfficeInfo> {
    process::offices(host).into_iter().find(|o| o.id == id)
}

fn wait_active(host: &Host, id: &str, what: &str) {
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
    // A plain signal to a child process this test's host started.
    org_platform::posix::kill(before[0], libc::SIGKILL).unwrap();
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

pub(crate) fn files(root: &Path, out: &mut Vec<PathBuf>) {
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

pub(crate) fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack.windows(needle.len()).any(|w| w == needle)
}

/// The forms a secret can take in a file: raw bytes and lowercase/uppercase hex. Keys
/// travel as hex (handoff JSON, the development store of `keys/custody.rs`); no key or CA
/// channel uses base64.
pub(crate) fn encodings(bytes: &[u8]) -> [Vec<u8>; 3] {
    let lower: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
    let upper = lower.to_ascii_uppercase();
    [bytes.to_vec(), lower.into_bytes(), upper.into_bytes()]
}

pub(crate) fn unhex(text: &str) -> Vec<u8> {
    let text = text.trim();
    (0..text.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&text[i..i + 2], 16).unwrap())
        .collect()
}

/// Every file under `folder` holds none of `forms`; `what` names the secret.
fn holds_none(folder: &Path, forms: &[Vec<u8>], what: &str) -> usize {
    let mut all = Vec::new();
    files(folder, &mut all);
    for path in &all {
        // The live office may fold its log in between the listing and the read.
        let bytes = match fs::read(path) {
            Ok(bytes) => bytes,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(e) => panic!("can't read {path:?} for the leak scan: {e}"),
        };
        for form in forms {
            assert!(!contains(&bytes, form), "{what} in {path:?}");
        }
    }
    all.len()
}

#[test]
#[ignore = "root runs with ORG_TEST_OFFICE_BIN and ORG_TEST_WEB_BIN; actual web/worker listeners require permitted local bind"]
fn actual_host_restarts_a_killed_worker_and_starts_it_without_its_key_on_plain_sqlite() {
    // `_temp` is bound first so it drops last, after the host and its children.
    let Started {
        temp: _temp,
        address,
        program,
        host,
        wire,
        ..
    } = start_signed_in();
    let base = _temp.path().canonicalize().unwrap();
    let worker = program.with_file_name("agentic-org-office");
    let created = create_hq(&host, address);
    let (organization_id, office_id) = (created.organization_id, created.office_id);
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

    // The organization's data root, found by the local record's exact place under it.
    let record_tail = Path::new("durable/local/offices")
        .join(&office_id)
        .join("security/ca-key.v1.json");
    let mut all = Vec::new();
    files(&base, &mut all);
    let records: Vec<_> = all
        .iter()
        .filter(|p| p.ends_with(Path::new(&organization_id).join(&record_tail)))
        .collect();
    assert_eq!(records.len(), 1, "the HQ's CA key record: {records:?}");
    let record_path = records[0].clone();
    let organization_root = record_path
        .ancestors()
        .nth(record_tail.components().count())
        .unwrap()
        .to_path_buf();
    assert!(organization_root.ends_with(&organization_id));
    let durable = organization_root.join("durable");
    let projection = organization_root.join("runtime").join(&office_id);

    // The wrapped CA key, from the local record (document 16's one exact form).
    let record_text = fs::read_to_string(&record_path).unwrap();
    assert!(record_text.ends_with("}\n"), "{record_text}");
    let record: serde_json::Value = serde_json::from_str(&record_text).unwrap();
    assert_eq!(record["schema_version"], 1, "{record}");
    assert_eq!(record["organization_id"], organization_id.as_str());
    assert_eq!(record["office_id"], office_id.as_str());
    let wrapped_hex = record["wrapped_ca_key"].as_str().expect("wrapped_ca_key");
    assert_eq!(wrapped_hex, wrapped_hex.to_ascii_lowercase());
    let wrapped = unhex(wrapped_hex);
    assert!(wrapped.len() > 32, "{} bytes", wrapped.len());

    // ENC-1: the projection is plain SQLite, by its header bytes, beside the live office.
    let database = fs::read(projection.join("office.db")).unwrap();
    assert!(database.starts_with(b"SQLite format 3\0"), "office.db");
    // The office answers the host's status checks on the same data with no key.
    wait_active(&host, &office_id, "serving without a key");

    // The projection (the database and its log) holds neither the office secrets key
    // nor the wrapped CA key, in any form.
    let key_forms = encodings(&key);
    let wrapped_forms = encodings(&wrapped);
    let in_projection = holds_none(&projection, &key_forms, "office secrets key");
    holds_none(&projection, &wrapped_forms, "wrapped CA key");
    assert!(in_projection >= 1, "{projection:?}");
    // Nor does any file of the organization's Git content.
    holds_none(&durable.join("state"), &wrapped_forms, "wrapped CA key");

    // ENC-5: with its dev key file deleted, the office secrets key, raw or hex, appears
    // in no remaining file, so neither the office nor the host stored it elsewhere. The
    // wrapped CA key, raw or hex, is only in the local record and the acceptance journal,
    // so no rendezvous event or handoff carried it to anything the host wrote (registry,
    // logs, runtime files).
    // Only the office's acceptance journal may carry it, not the rest of `recovery/`.
    let journal = durable
        .join("recovery/offices")
        .join(&office_id)
        .join("journals/acceptance");
    assert!(journal.is_dir(), "{journal:?}");
    let mut all = Vec::new();
    files(&base, &mut all);
    assert!(
        !all.contains(&key_files[0]),
        "the deleted key file is not scanned"
    );
    let mut scanned = 0;
    let mut wrapped_in_record = false;
    for path in &all {
        if path.starts_with(base.join("bin")) {
            continue;
        }
        // A file removed since the listing (for example a WAL checkpointed away) is skipped;
        // any other read failure fails the test so an unreadable file can't hide a leak.
        let bytes = match fs::read(path) {
            Ok(bytes) => bytes,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(e) => panic!("can't read {path:?} for the leak scan: {e}"),
        };
        scanned += 1;
        for form in &key_forms {
            assert!(!contains(&bytes, form), "office secrets key in {path:?}");
        }
        if *path == record_path {
            wrapped_in_record = contains(&bytes, &wrapped_forms[1]);
        } else if !path.starts_with(&journal) {
            for form in &wrapped_forms {
                assert!(!contains(&bytes, form), "wrapped CA key in {path:?}");
            }
        }
    }
    assert!(scanned > 4, "{scanned}");
    // Positive control: the scan reads the file that does hold the wrapped CA key.
    assert!(
        wrapped_in_record,
        "{record_path:?} does not hold the wrapped CA key as lowercase hex"
    );
    drop(wire);
    host.shutdown();
}
