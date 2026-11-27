//! Unit and adversarial security tests for `features::diagnostics`.

use std::fs;
use std::os::unix::fs::{symlink, PermissionsExt};
use std::path::{Path, PathBuf};

use org_office::{EngineChoice, Office, TestOffice};
use org_platform::service::{Registration, ServiceError, ServiceManager, ServiceStatus, Unit};
use serde_json::json;

use super::doctor::{CheckStatus, DoctorCheck};
use super::doctor_apparmor::check_linux_apparmor_userns_at;
use super::doctor_host::{
    check_permissions, check_release_pin_at, check_sockets, validate_marker_content,
};
use super::logs::{matches_office_filter, validate_office_filter};
use super::org_integrity::{walk_state_tree_bounded, StateIndex};
use super::org_integrity_checks::build_sqlite_parity_check;
use super::org_integrity_checks::check_org_state;
use super::paths::{
    normalize_lstat_path, probe_lock_held, read_regular_bounded, validate_loaded_abs_path,
    write_atomic_owner_only, InstancePaths, LockProbe,
};
use super::redact::{redact_json, redact_lines, redact_text, REDACTED};
use super::sqlite_read::{decode_record, parse_task_columns, read_projected_tasks};
use super::sqlite_wal::{load_wal_index_with_limit, wal_checksum};
use super::uninstall::{acquire_office_locks, unregister_service_with};
use super::uninstall_guard::{verify_dir_within_anchor, verify_instance_temp_dir};
use super::yaml_front::{parse_markdown_front_matter, parse_yaml_mapping};

#[test]
fn redacts_all_required_secret_shapes_and_preserves_uuids() {
    let uuid = "0193a5c0-0000-7000-8000-000000000001";
    // Assemble high-entropy synthetic fixtures dynamically so secret scanners never flag tests.
    let fake_jwt = [
        "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9",
        "eyJzdWIiOiIxMjM0NTY3ODkwIn0",
        "SflKxwRJSMeKKF2QT4fwpMeJf36POk6yJV_adQssw5c",
    ]
    .join(".");
    let fake_hex_val = ["cafe", "babe", "01234567", "89abcdef", "01234567"].join("");
    let fake_ghp_1 = ["ghp_", "aBcDeFgHiJkLmNoPqRsTuVwXyZ0123456789"].join("");
    let fake_ghp_2 = ["ghp_", "secondSecret888"].join("");
    let fake_gh_pat = ["github_pat_", "11AAAAAA0123456789_secretvalue"].join("");
    let fake_slack = ["xoxb-", "123456789012-", "abcdefghij123456"].join("");
    let fake_ant = ["sk-ant-", "api03-abcdefgh1234567890"].join("");
    let fake_oai = ["sk-", "1234567890abcdef1234567890"].join("");
    let fake_aws = ["AKIA", "IOSFODNN7EXAMPLE"].join("");
    let fake_ant_mb = ["sk-ant-", "multibyte1234567890"].join("");
    let samples: Vec<(String, String)> = vec![
        (format!("Authorization: Bearer {fake_jwt}"), "eyJhbGci".into()),
        (
            "authorization: bEaReR super-secret-bearer-token-999".into(),
            "super-secret-bearer-token-999".into(),
        ),
        (
            "X-Api-Key: custom-header-key-value-1234".into(),
            "custom-header-key-value-1234".into(),
        ),
        (
            "Cookie: sid=sessioncookievalue987654321".into(),
            "sessioncookievalue987654321".into(),
        ),
        (format!("provider key {fake_ant} failed"), fake_ant),
        (format!("openai {fake_oai} failed"), fake_oai),
        (fake_ghp_1.clone(), fake_ghp_1),
        (fake_gh_pat.clone(), fake_gh_pat),
        (format!("slack {fake_slack}"), fake_slack),
        (format!("aws {fake_aws} leaked"), fake_aws),
        (
            "AGENT_OS_SESSION_TOKEN=tok_live_secret_value_abc123".into(),
            "tok_live_secret_value_abc123".into(),
        ),
        (
            "env AGENT_OS_SESSION_TOKEN=\"quoted_secret_val_999\" ok".into(),
            "quoted_secret_val_999".into(),
        ),
        (
            "https://alice:hunter2secret@git.example.com/org.git".into(),
            "hunter2secret".into(),
        ),
        (
            "https://host.example/register#AbCdEfGhIjKlMnOpQrStUvWxYz012345".into(),
            "AbCdEfGhIjKlMnOpQrStUvWxYz012345".into(),
        ),
        (
            "https://host.example/set-password#ResetTokenSecret9876543210".into(),
            "ResetTokenSecret9876543210".into(),
        ),
        (
            "https://host.example/invite/InviteSecretToken12345678".into(),
            "InviteSecretToken12345678".into(),
        ),
        (
            "https://host.example/cb?token=QueryTok998877&sig=SigHex998877&ok=1".into(),
            "QueryTok998877".into(),
        ),
        (
            "recovery code: ZXCV-BNMQ-WERT-YUIO".into(),
            "ZXCV-BNMQ-WERT-YUIO".into(),
        ),
        (
            "standalone ABCD-EFGH-JKLM-NPQR code".into(),
            "ABCD-EFGH-JKLM-NPQR".into(),
        ),
        (
            "{\"secret_b64\": \"YWJjZGVmZ2hpamtsbW5vcHFyc3R1dnd4eXo=\", \"key_hex\": \"deadbeef0123456789abcdef\"}".into(),
            "deadbeef0123456789abcdef".into(),
        ),
        (format!("secret_hex: {fake_hex_val}"), fake_hex_val),
        (format!("日本語Bearer {fake_ant_mb}émoji🔑"), fake_ant_mb),
    ];
    for (input, secret) in &samples {
        let res = redact_text(input);
        assert!(
            !res.text.contains(secret.as_str()),
            "secret `{secret}` leaked in `{}` (from `{input}`)",
            res.text
        );
        assert!(res.count > 0, "expected redaction count > 0 for `{input}`");
    }

    // Multiple secrets on one line are ALL scrubbed.
    let multi = format!(
        "Bearer firstSecret999 then {fake_ghp_2} and AGENT_OS_SESSION_TOKEN=thirdSecret777"
    );
    let multi_res = redact_text(&multi);
    for secret in ["firstSecret999", fake_ghp_2.as_str(), "thirdSecret777"] {
        assert!(!multi_res.text.contains(secret), "{}", multi_res.text);
    }
    assert!(multi_res.count >= 3, "count={}", multi_res.count);

    // Canonical UUIDv7 and benign prose must NOT be falsely redacted.
    let benign = format!("office {uuid} has basic understanding of task-123 status");
    let benign_res = redact_text(&benign);
    assert_eq!(benign_res.text, benign);
    assert_eq!(benign_res.count, 0);
}

#[test]
fn redacts_single_line_multiline_and_truncated_pem_private_keys() {
    let pem_begin = format!("{}BEGIN PRIVATE KEY{}", "-----", "-----");
    let pem_end = format!("{}END PRIVATE KEY{}", "-----", "-----");
    let rsa_begin = format!("{}BEGIN RSA PRIVATE KEY{}", "-----", "-----");
    let ec_end = format!("{}END EC PRIVATE KEY{}", "-----", "-----");
    let pem_lines = vec![
        "before line".to_owned(),
        pem_begin,
        "MIIEvgIBADANBgkqhkiG9w0BAQEFAASCBKgwggSkAgEAAoIBAQC7".to_owned(),
        "secretBase64LineInMiddleOfKey1234567890ABCDEF==".to_owned(),
        format!("{pem_end} trailing ok"),
        "after line".to_owned(),
    ];
    let (cleaned, count) = redact_lines(&pem_lines);
    let joined = cleaned.join("\n");
    assert!(!joined.contains("MIIEvgIB"), "{joined}");
    assert!(!joined.contains("secretBase64Line"), "{joined}");
    assert!(joined.contains("before line"), "{joined}");
    assert!(joined.contains("trailing ok"), "{joined}");
    assert!(joined.contains("after line"), "{joined}");
    assert!(count >= 4);

    // Unclosed PEM fails closed through the end of the text.
    let unclosed = format!("prefix {rsa_begin}\nMIIE_SENSITIVE_BYTES_HERE");
    let unclosed_res = redact_text(&unclosed);
    assert!(
        !unclosed_res.text.contains("MIIE_SENSITIVE"),
        "{}",
        unclosed_res.text
    );
    assert!(unclosed_res.text.contains("prefix "));

    // Orphan tail of a rotated log ending in END PRIVATE KEY scrubs preceding base64 lines.
    let orphan = vec![
        "normal log line".to_owned(),
        "MIIEvgIBADANBgkqhkiG9w0BAQEFAASCBKgwggSkAgEAAoIBAQC7".to_owned(),
        ec_end,
    ];
    let (orphan_clean, orphan_count) = redact_lines(&orphan);
    assert_eq!(orphan_clean[0], "normal log line");
    assert!(!orphan_clean[1].contains("MIIEvgIB"));
    assert!(orphan_count >= 2);
}

#[test]
fn redact_json_scrubs_sensitive_keys_and_nested_strings() {
    let val = json!({
        "instance": "default",
        "token": "top-secret-val",
        "nested": {
            "recovery_code": "AAAA-BBBB-CCCC-DDDD",
            "note": "saw sk-ant-nested9876543210 in stderr"
        }
    });
    let (clean, count) = redact_json(&val);
    assert_eq!(clean["token"], REDACTED);
    assert_eq!(clean["nested"]["recovery_code"], REDACTED);
    assert!(!clean["nested"]["note"]
        .as_str()
        .unwrap()
        .contains("sk-ant-nested"));
    assert_eq!(clean["instance"], "default");
    assert!(count >= 3);
}

#[test]
fn yaml_front_matter_parses_valid_and_rejects_malformed() {
    let valid = "---\nschema_version: 1\nid: task-1\noffice_code: A\nnumber: 1\nproject_id: proj-1\ndepends_on:\n  - T-A2\nlinks:\n  - id: link-1\n    kind: depends_on\n    target_kind: task\n    target_id: task-2\n---\n# Title\n";
    let doc = parse_markdown_front_matter(valid).unwrap();
    let fm = doc.front_matter.unwrap();
    assert_eq!(fm["id"], "task-1");
    assert_eq!(fm["number"], 1);
    assert_eq!(fm["depends_on"][0], "T-A2");
    assert_eq!(fm["links"][0]["target_id"], "task-2");

    for bad in [
        "---\nid: unclosed\n",
        "---\n\tid: tabbed\n---\n",
        "---\nid: a\nid: b\n---\n",
        "---\ntitle: \"unclosed\n---\n",
        "---\ndepends_on: [a, b\n---\n",
        "---\n<<: *anchor\n---\n",
        "---\n- list-at-root\n---\n",
    ] {
        assert!(
            parse_markdown_front_matter(bad).is_err(),
            "expected error for malformed front matter: {bad:?}"
        );
    }
    assert!(parse_yaml_mapping("id: p1\nnumber: 1\n").is_ok());
}

fn write_file(path: &Path, content: &str) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    fs::write(path, content).unwrap();
}

#[test]
fn org_integrity_accepts_valid_state_and_detects_syntax_key_link_and_symlink_faults() {
    let dir = tempfile::tempdir().unwrap();
    let state = dir.path().join("durable/state");
    write_file(
        &state.join("projects/proj-1/project.yaml"),
        "id: proj-1\nnumber: 1\nslug: core\n",
    );
    write_file(
        &state.join("features/feat-1/feature.yaml"),
        "id: feat-1\nnumber: 1\n",
    );
    write_file(
        &state.join("tasks/task-1/task.md"),
        "---\nid: task-1\nkey: T-A1\nproject_id: proj-1\nfeature_id: feat-1\n---\n# Task 1\n",
    );
    write_file(
        &state.join("tasks/task-2/task.md"),
        "---\nid: task-2\noffice_code: A\nnumber: 2\nproject_id: proj-1\ndepends_on: [T-A1]\n---\n# Task 2\n",
    );
    write_file(
        &state.join("docs/arch.md"),
        "---\nid: doc-1\nkey: DOC-1\nrelates_to: [T-A1, proj-1]\n---\n# Arch\n",
    );
    write_file(
        &state.join("decisions/adr-001.md"),
        "---\nid: dec-1\nkey: ADR-1\nrelates_to: [DOC-1]\n---\n# ADR 1\n",
    );

    let checks = check_org_state(dir.path(), None, "org.integrity");
    assert!(
        checks.iter().all(|c| c.status == CheckStatus::Pass),
        "valid state failed: {checks:#?}"
    );

    // Inject: duplicate display key T-A1, broken depends_on, broken project_id, broken relates_to,
    // invalid front matter, malformed JSON, and a symlink.
    write_file(
        &state.join("tasks/task-3/task.md"),
        "---\nid: task-3\nkey: T-A1\nproject_id: proj-missing\ndepends_on: [T-NOPE]\n---\n",
    );
    write_file(
        &state.join("docs/broken.md"),
        "---\nid: doc-bad\nrelates_to: [MISSING-ENTITY]\n---\n",
    );
    write_file(&state.join("decisions/unclosed.md"), "---\nid: dec-bad\n");
    write_file(&state.join("tasks/task-1/bad.json"), "{not valid json");
    symlink(dir.path(), state.join("docs/evil-link.md")).unwrap();

    let bad_checks = check_org_state(dir.path(), None, "org.integrity");
    let by_id = |id: &str| bad_checks.iter().find(|c| c.id == id).unwrap();
    assert_eq!(by_id("org.integrity.syntax").status, CheckStatus::Fail);
    assert!(by_id("org.integrity.syntax")
        .message
        .contains("unclosed.md"));
    assert!(by_id("org.integrity.syntax").message.contains("bad.json"));
    assert!(by_id("org.integrity.syntax").message.contains("symlink"));
    assert_eq!(
        by_id("org.integrity.display_keys").status,
        CheckStatus::Fail
    );
    assert!(by_id("org.integrity.display_keys").message.contains("T-A1"));
    assert_eq!(by_id("org.integrity.cross_links").status, CheckStatus::Fail);
    assert!(by_id("org.integrity.cross_links")
        .message
        .contains("T-NOPE"));
    assert!(by_id("org.integrity.cross_links")
        .message
        .contains("proj-missing"));
    assert!(by_id("org.integrity.cross_links")
        .message
        .contains("MISSING-ENTITY"));
}

#[test]
fn sqlite_parity_matches_real_org_office_db_and_catches_drift_without_mutating() {
    org_office_sqlite::install();
    let dir = tempfile::Builder::new()
        .prefix("sq")
        .tempdir_in(org_platform::dirs::testing::disk_backed_temp().unwrap())
        .unwrap();
    let (office, roots, _, _) = Office::for_test_with_real_ca_in(
        dir.path(),
        TestOffice {
            engine: EngineChoice::Sqlite,
        },
    );
    let office_id = office.info().office_id.clone();
    let owner = office.owner_member_id().to_owned();
    let root_team = office.org_snapshot().unwrap().teams[0].id.clone();
    let created = office
        .task_create(
            &owner,
            org_proto::tasks::TasksCreateParams {
                command_id: "0193a5c0-0000-7000-8000-000000000010".into(),
                task_id: "0193a5c0-0000-7000-8000-000000000011".into(),
                team_id: Some(root_team),
                project_id: None,
                assigned_team_id: None,
                assignee_id: None,
                task_type: Some("task".into()),
                work_type: None,
                title: "Verify SQLite parity".into(),
                body_markdown: "Real task projected into office.db.".into(),
                priority: None,
                effort: None,
                severity: None,
                rank_order: None,
                labels: vec![],
                reference_files: vec![],
                modified_files: vec![],
                depends_on: vec![],
                details: None,
                dod: None,
                attachment_ids: vec![],
            },
        )
        .unwrap()
        .unwrap();
    let created_key = created.task.key.clone();
    office.close().unwrap();

    let db_path = roots.runtime.join(&office_id).join("office.db");
    assert!(
        db_path.is_file(),
        "expected real SQLite office.db at {}",
        db_path.display()
    );
    let before_bytes = fs::read(&db_path).unwrap();

    // Populated SQLite projection matches durable task.md cleanly (exercising B-tree/WAL + VIRTUAL cols).
    let projected = read_projected_tasks(&db_path).unwrap();
    assert_eq!(projected.len(), 1, "{projected:?}");
    assert_eq!(
        projected[0].id.as_deref(),
        Some("0193a5c0-0000-7000-8000-000000000011")
    );
    assert_eq!(projected[0].key.as_deref(), Some(created_key.as_str()));
    let ok_checks = check_org_state(&roots.durable, Some(&db_path), "org.integrity");
    let parity = ok_checks
        .iter()
        .find(|c| c.id == "org.integrity.sqlite_parity")
        .unwrap();
    assert_eq!(parity.status, CheckStatus::Pass, "{parity:?}");
    assert_eq!(
        fs::read(&db_path).unwrap(),
        before_bytes,
        "office.db must never be mutated"
    );

    // Add a second durable task file without updating office.db -> parity drift detected!
    write_file(
        &roots
            .durable
            .join("state/tasks/0193a5c0-0000-7000-8000-000000000099/task.md"),
        "---\nid: 0193a5c0-0000-7000-8000-000000000099\noffice_code: HQ\nnumber: 99\n---\n# Drift\n",
    );
    let drift_checks = check_org_state(&roots.durable, Some(&db_path), "org.integrity");
    let drift = drift_checks
        .iter()
        .find(|c| c.id == "org.integrity.sqlite_parity")
        .unwrap();
    assert_eq!(drift.status, CheckStatus::Fail, "{drift:?}");
    assert!(drift.message.contains("T-HQ99"), "{}", drift.message);
    assert!(
        drift.message.contains("durable=2 sqlite=1"),
        "{}",
        drift.message
    );
    assert_eq!(
        fs::read(&db_path).unwrap(),
        before_bytes,
        "office.db must never be mutated"
    );
}

#[test]
fn safe_file_helpers_refuse_symlinks_fifos_oversized_and_enforce_0600() {
    let dir = tempfile::tempdir().unwrap();
    let real = dir.path().join("real.txt");
    fs::write(&real, b"0123456789abcdef").unwrap();
    assert!(read_regular_bounded(&real, 16).is_ok());
    assert!(
        read_regular_bounded(&real, 8).is_err(),
        "oversized file must fail"
    );

    let link = dir.path().join("link.txt");
    symlink(&real, &link).unwrap();
    assert!(
        read_regular_bounded(&link, 64).is_err(),
        "symlink must be refused"
    );
    assert_eq!(
        normalize_lstat_path(Path::new("/tmp/foo/")),
        Path::new("/tmp/foo")
    );
    assert_eq!(
        normalize_lstat_path(Path::new("/tmp/foo/.")),
        Path::new("/tmp/foo")
    );

    // FIFO must never block open_nofollow_ro.
    let fifo = dir.path().join("pipe.fifo");
    let c_fifo = std::ffi::CString::new(fifo.as_os_str().as_encoded_bytes()).unwrap();
    org_platform::posix::make_fifo(&c_fifo, 0o600).unwrap();
    assert!(
        read_regular_bounded(&fifo, 64).is_err(),
        "FIFO must be refused"
    );
    assert!(matches!(probe_lock_held(&fifo), LockProbe::Unsafe(_)));

    // Atomic 0600 write succeeds on new file and refuses symlink leaf without overwriting target.
    let out = dir.path().join("bundle.json");
    write_atomic_owner_only(&out, b"{\"ok\":true}\n").unwrap();
    let mode = fs::symlink_metadata(&out).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o600);

    let err = write_atomic_owner_only(&link, b"overwritten").unwrap_err();
    assert!(!err.is_empty());
    assert_eq!(fs::read(&real).unwrap(), b"0123456789abcdef");
    assert!(write_atomic_owner_only(&dir.path().join("../escape.json"), b"x").is_err());
}

// --- Pass-2 review regression tests (P0, P1, P2) ---

fn encode_varint(mut v: u64) -> Vec<u8> {
    let mut parts = vec![(v & 0x7f) as u8];
    v >>= 7;
    while v > 0 {
        parts.push(((v & 0x7f) as u8) | 0x80);
        v >>= 7;
    }
    parts.reverse();
    parts
}

fn encode_text_record(fields: &[&str]) -> Vec<u8> {
    let mut hdr_body = Vec::new();
    for f in fields {
        let serial = 13 + (f.len() as u64) * 2;
        hdr_body.extend_from_slice(&encode_varint(serial));
    }
    let total_hdr = 1 + hdr_body.len();
    let mut rec = encode_varint(total_hdr as u64);
    rec.extend_from_slice(&hdr_body);
    for f in fields {
        rec.extend_from_slice(f.as_bytes());
    }
    rec
}

fn encode_schema_tasks_record(root_page: u8, ddl: &str) -> Vec<u8> {
    let mut hdr_body = Vec::new();
    hdr_body.extend_from_slice(&encode_varint(13 + 5 * 2)); // "table"
    hdr_body.extend_from_slice(&encode_varint(13 + 5 * 2)); // "tasks"
    hdr_body.extend_from_slice(&encode_varint(13 + 5 * 2)); // "tasks"
    hdr_body.push(1); // 1-byte signed int root_page
    hdr_body.extend_from_slice(&encode_varint(13 + (ddl.len() as u64) * 2));
    let hdr_len = encode_varint((hdr_body.len() + 1) as u64);
    let mut rec = if hdr_len.len() == 1 {
        hdr_len
    } else {
        encode_varint((hdr_body.len() + hdr_len.len()) as u64)
    };
    rec.extend_from_slice(&hdr_body);
    rec.extend_from_slice(b"tabletaskstasks");
    rec.push(root_page);
    rec.extend_from_slice(ddl.as_bytes());
    rec
}

fn build_leaf_cell(rowid: u64, payload: &[u8]) -> Vec<u8> {
    let mut cell = encode_varint(payload.len() as u64);
    cell.extend_from_slice(&encode_varint(rowid));
    cell.extend_from_slice(payload);
    cell
}

fn make_sqlite_header(page_size: u16) -> [u8; 100] {
    let mut hdr = [0u8; 100];
    hdr[..16].copy_from_slice(b"SQLite format 3\0");
    hdr[16..18].copy_from_slice(&page_size.to_be_bytes());
    hdr[18] = 1;
    hdr[19] = 1;
    hdr[21] = 64;
    hdr[22] = 32;
    hdr[23] = 32;
    hdr
}

fn make_two_page_db_with_tasks(ddl: &str, page2: &[u8; 4096]) -> Vec<u8> {
    let mut db = vec![0u8; 8192];
    db[..100].copy_from_slice(&make_sqlite_header(4096));
    let schema_rec = encode_schema_tasks_record(2, ddl);
    let cell = build_leaf_cell(1, &schema_rec);
    let cell_off = 4096 - cell.len();
    db[100] = 0x0d; // leaf table
    db[103..105].copy_from_slice(&1u16.to_be_bytes());
    db[105..107].copy_from_slice(&(cell_off as u16).to_be_bytes());
    db[108..110].copy_from_slice(&(cell_off as u16).to_be_bytes());
    db[cell_off..4096].copy_from_slice(&cell);
    db[4096..8192].copy_from_slice(page2);
    db
}

#[test]
fn sqlite_read_handles_corrupt_cell_pointers_oom_truncation_wal_and_comments() {
    let dir = tempfile::tempdir().unwrap();
    let ddl = "CREATE TABLE tasks (\n-- primary id, with comma\nid TEXT PRIMARY KEY,\nkey TEXT -- inline, comment\n)";
    let cols = parse_task_columns(ddl);
    assert_eq!(cols.id, Some(0));
    assert_eq!(cols.key, Some(1));

    // P0 #1: Out-of-bounds cell pointer (0xffff) and tail-edge pointer (4095) must Err without panic.
    for bad_ptr in [0xffffu16, 4095u16] {
        let mut p2 = [0u8; 4096];
        p2[0] = 0x0d;
        p2[3..5].copy_from_slice(&1u16.to_be_bytes());
        p2[8..10].copy_from_slice(&bad_ptr.to_be_bytes());
        let path = dir.path().join(format!("corrupt_ptr_{bad_ptr}.db"));
        fs::write(&path, make_two_page_db_with_tasks(ddl, &p2)).unwrap();
        assert!(read_projected_tasks(&path).is_err(), "ptr={bad_ptr}");
    }

    // P1 #4a: Single-cell OOM payload (> 1 MiB) and aliased-cell cumulative OOM bomb (> 32 MiB).
    let mut oom_p2 = [0u8; 4096];
    oom_p2[0] = 0x0d;
    oom_p2[3..5].copy_from_slice(&1u16.to_be_bytes());
    let mut big_cell = encode_varint(2 * 1024 * 1024);
    big_cell.push(1); // rowid
    let big_off = 3000usize;
    oom_p2[8..10].copy_from_slice(&(big_off as u16).to_be_bytes());
    oom_p2[big_off..big_off + big_cell.len()].copy_from_slice(&big_cell);
    let oom_path = dir.path().join("oom_single.db");
    fs::write(&oom_path, make_two_page_db_with_tasks(ddl, &oom_p2)).unwrap();
    assert!(read_projected_tasks(&oom_path).is_err());

    // 70 aliased cells each claiming 500_000 bytes (> 32 MiB cumulative budget) pointing to self-loop overflow page 3.
    let mut alias_p2 = [0u8; 4096];
    alias_p2[0] = 0x0d;
    alias_p2[3..5].copy_from_slice(&70u16.to_be_bytes());
    let alias_off = 2000usize;
    for i in 0..70usize {
        alias_p2[8 + i * 2..10 + i * 2].copy_from_slice(&(alias_off as u16).to_be_bytes());
    }
    let mut alias_cell = encode_varint(500_000);
    alias_cell.push(1); // rowid
    let start_after_hdr = alias_off + alias_cell.len();
    alias_p2[alias_off..start_after_hdr].copy_from_slice(&alias_cell);
    // Valid SQLite record header: 4-byte header + 499_996-byte blob (serial = 12 + 2 * 499_996).
    let blob_serial = encode_varint(12 + 2 * 499_996);
    assert_eq!(blob_serial.len(), 3);
    alias_p2[start_after_hdr] = 4;
    alias_p2[start_after_hdr + 1..start_after_hdr + 4].copy_from_slice(&blob_serial);
    // For total_len=500_000, usable=4096: min_local=489, k=489+((500000-489)%4092)=776 <= 4061 -> local=776.
    let ovf_ptr_off = start_after_hdr + 776;
    alias_p2[ovf_ptr_off..ovf_ptr_off + 4].copy_from_slice(&3u32.to_be_bytes());
    let mut alias_db = make_two_page_db_with_tasks(ddl, &alias_p2);
    let mut p3 = vec![0u8; 4096];
    p3[0..4].copy_from_slice(&3u32.to_be_bytes());
    alias_db.extend_from_slice(&p3);
    let alias_path = dir.path().join("oom_aliased.db");
    fs::write(&alias_path, alias_db).unwrap();
    let alias_err = read_projected_tasks(&alias_path).unwrap_err();
    assert!(alias_err.contains("budget"), "{alias_err}");

    // P1 #4b: Truncated overflow payload (next == 0 while out.len() < total_len) must Err.
    let mut trunc_p2 = [0u8; 4096];
    trunc_p2[0] = 0x0d;
    trunc_p2[3..5].copy_from_slice(&1u16.to_be_bytes());
    let trunc_off = 200usize;
    trunc_p2[8..10].copy_from_slice(&(trunc_off as u16).to_be_bytes());
    let mut trunc_cell = encode_varint(5000);
    trunc_cell.push(1);
    trunc_p2[trunc_off..trunc_off + trunc_cell.len()].copy_from_slice(&trunc_cell);
    // Overflow next page pointer remains 0 -> truncated!
    let trunc_path = dir.path().join("trunc_overflow.db");
    fs::write(&trunc_path, make_two_page_db_with_tasks(ddl, &trunc_p2)).unwrap();
    let trunc_err = read_projected_tasks(&trunc_path).unwrap_err();
    assert!(trunc_err.contains("truncated"), "{trunc_err}");

    // P1 #4c: decode_record rejects hdr_len == 0 and truncated 8-byte float.
    assert!(decode_record(&[0x00, 0x01]).is_err());
    assert!(decode_record(&[0x02, 0x07, 1, 2, 3, 4]).is_err());

    // P2 #9: Missing `tasks` table in sqlite_schema fails closed.
    let mut no_tasks = vec![0u8; 4096];
    no_tasks[..100].copy_from_slice(&make_sqlite_header(4096));
    no_tasks[100] = 0x0d;
    let no_tasks_path = dir.path().join("no_tasks.db");
    fs::write(&no_tasks_path, no_tasks).unwrap();
    assert!(read_projected_tasks(&no_tasks_path).is_err());
    let empty_idx = StateIndex::default();
    let missing_tbl_check = build_sqlite_parity_check(&empty_idx, &no_tasks_path, "org.integrity");
    assert_eq!(missing_tbl_check.status, CheckStatus::Fail);

    // P1 #4d: Corrupt WAL, symlinked WAL, and WAL exceeding frame cap fail closed.
    let mut valid_p2 = [0u8; 4096];
    valid_p2[0] = 0x0d;
    valid_p2[3..5].copy_from_slice(&1u16.to_be_bytes());
    let task_rec = encode_text_record(&["task-1", "T-A1"]);
    let task_cell = build_leaf_cell(1, &task_rec);
    let task_off = 4096 - task_cell.len();
    valid_p2[8..10].copy_from_slice(&(task_off as u16).to_be_bytes());
    valid_p2[task_off..4096].copy_from_slice(&task_cell);
    let good_db = dir.path().join("wal_test.db");
    fs::write(&good_db, make_two_page_db_with_tasks(ddl, &valid_p2)).unwrap();
    let tasks = read_projected_tasks(&good_db).unwrap();
    assert_eq!(tasks.len(), 1);
    assert_eq!(tasks[0].id.as_deref(), Some("task-1"));
    assert_eq!(tasks[0].key.as_deref(), Some("T-A1"));

    // Corrupt WAL header -> fails closed instead of falling back.
    let wal_path = dir.path().join("wal_test.db-wal");
    fs::write(&wal_path, b"corrupt wal bytes that are not valid").unwrap();
    assert!(read_projected_tasks(&good_db).is_err());

    // Symlinked WAL -> fails closed.
    fs::remove_file(&wal_path).unwrap();
    symlink(&good_db, &wal_path).unwrap();
    assert!(read_projected_tasks(&good_db).is_err());
    fs::remove_file(&wal_path).unwrap();

    // WAL exceeding max_frames -> fails closed.
    let mut wal_bytes = Vec::new();
    let mut whdr = [0u8; 32];
    whdr[0..4].copy_from_slice(&0x377f_0682u32.to_be_bytes());
    whdr[8..12].copy_from_slice(&4096u32.to_be_bytes());
    whdr[16..24].copy_from_slice(b"SALT1234");
    let (mut s0, mut s1) = wal_checksum(&whdr[..24], 0, 0, false);
    whdr[24..28].copy_from_slice(&s0.to_be_bytes());
    whdr[28..32].copy_from_slice(&s1.to_be_bytes());
    wal_bytes.extend_from_slice(&whdr);
    for frame_idx in 1..=3u32 {
        let mut fhdr = [0u8; 24];
        fhdr[0..4].copy_from_slice(&frame_idx.to_be_bytes());
        fhdr[4..8].copy_from_slice(&2u32.to_be_bytes());
        fhdr[8..16].copy_from_slice(b"SALT1234");
        let fpage = vec![0u8; 4096];
        let (n0, n1) = wal_checksum(&fhdr[..8], s0, s1, false);
        let (n0, n1) = wal_checksum(&fpage, n0, n1, false);
        s0 = n0;
        s1 = n1;
        fhdr[16..20].copy_from_slice(&s0.to_be_bytes());
        fhdr[20..24].copy_from_slice(&s1.to_be_bytes());
        wal_bytes.extend_from_slice(&fhdr);
        wal_bytes.extend_from_slice(&fpage);
    }
    fs::write(&wal_path, &wal_bytes).unwrap();
    assert!(load_wal_index_with_limit(&wal_path, 4096, 3).is_ok());
    assert!(load_wal_index_with_limit(&wal_path, 4096, 2).is_err());

    // P2 #9: task_id drift detected even when task_key and count match!
    fs::remove_file(&wal_path).unwrap();
    let mut drift_idx = StateIndex {
        task_count: 1,
        ..StateIndex::default()
    };
    drift_idx.task_keys.insert(
        "T-A1".to_owned(),
        vec!["tasks/task-other/task.md".to_owned()],
    );
    drift_idx.task_ids.insert(
        "task-other".to_owned(),
        vec!["tasks/task-other/task.md".to_owned()],
    );
    let id_drift = build_sqlite_parity_check(&drift_idx, &good_db, "org.integrity");
    assert_eq!(id_drift.status, CheckStatus::Fail);
    assert!(
        id_drift.message.contains("task-other"),
        "{}",
        id_drift.message
    );
}

#[test]
fn multibyte_office_filter_never_panics() {
    assert!(!matches_office_filter("host.log", Some("éaaaaaaaaaaa")));
    assert!(!matches_office_filter(
        "aaaaaaaaaaa.log",
        Some("éaaaaaaaaaaa")
    ));
    assert!(validate_office_filter("éaaaaaaaaaaa").is_err());
    assert!(validate_office_filter("0193a5c0-0000-7000-8000-000000000001").is_ok());
    assert!(matches_office_filter(
        "000000000001.log",
        Some("0193a5c0-0000-7000-8000-000000000001")
    ));
}

#[test]
fn yaml_flow_nesting_and_state_tree_depth_and_entry_caps_fail_closed() {
    // Deep flow sequence & flow mapping rejected cleanly without stack overflow.
    let deep_seq = format!("---\na: {}{}\n---\n", "[".repeat(64), "]".repeat(64));
    assert!(parse_markdown_front_matter(&deep_seq).is_err());
    let deep_map = format!("---\na: {}1{}\n---\n", "{k: ".repeat(32), "}".repeat(32));
    assert!(parse_markdown_front_matter(&deep_map).is_err());

    // Deep empty-directory chain hits MAX_SCAN_DEPTH.
    let dir = tempfile::tempdir().unwrap();
    let mut deep = dir.path().to_path_buf();
    for i in 0..40 {
        deep.push(format!("d{i}"));
    }
    fs::create_dir_all(&deep).unwrap();
    let checks = check_org_state(dir.path(), None, "org.integrity");
    let syntax = checks
        .iter()
        .find(|c| c.id == "org.integrity.syntax")
        .unwrap();
    assert_eq!(syntax.status, CheckStatus::Fail);
    assert!(
        syntax.message.contains("maximum depth"),
        "{}",
        syntax.message
    );

    // Wide empty-directory tree counts directories toward MAX_SCAN_ENTRIES cap.
    let wide = tempfile::tempdir().unwrap();
    for i in 0..30 {
        fs::create_dir(wide.path().join(format!("empty_{i:02}"))).unwrap();
    }
    let mut idx = StateIndex::default();
    walk_state_tree_bounded(wide.path(), wide.path(), 0, 32, 15, &mut idx);
    assert!(idx.cap_hit);
    assert!(idx
        .syntax_errors
        .iter()
        .any(|e| e.contains("exceeded scan cap")));
}

#[test]
fn redaction_fixes_cover_short_pem_orphan_tail_same_line_aws_and_escaped_quotes() {
    let pem_begin = format!("{}BEGIN PRIVATE KEY{}", "-----", "-----");
    let pem_end = format!("{}END PRIVATE KEY{}", "-----", "-----");
    let rsa_begin = format!("{}BEGIN RSA PRIVATE KEY{}", "-----", "-----");

    // 1. Short PEM final line (AgMBAAE=, xQ==) before END scrubs both final line and full body above.
    for short_tail in ["AgMBAAE=", "xQ=="] {
        let lines = vec![
            "normal log line".to_owned(),
            "MIIEvgIBADANBgkqhkiG9w0BAQEFAASCBKgwggSkAgEAAoIBAQC7".to_owned(),
            short_tail.to_owned(),
            pem_end.clone(),
        ];
        let (clean, count) = redact_lines(&lines);
        assert_eq!(clean[0], "normal log line");
        assert!(!clean[1].contains("MIIEvgIB"), "{clean:?}");
        assert!(!clean[2].contains(short_tail), "{clean:?}");
        assert!(count >= 3);
    }
    // Single plain prose word with no base64 above is not falsely redacted.
    let prose = vec!["Error".to_owned(), pem_end.clone()];
    let (prose_clean, _) = redact_lines(&prose);
    assert_eq!(prose_clean[0], "Error");

    // 2. Orphan PEM tail inside redact_text / redact_json is scrubbed.
    let orphan_text = format!(
        "normal\nMIIEvgIBADANBgkqhkiG9w0BAQEFAASCBKgwggSkAgEAAoIBAQC7\nAgMBAAE=\n{pem_end}\nafter"
    );
    let orphan_red = redact_text(&orphan_text);
    assert!(!orphan_red.text.contains("MIIEvgIB"), "{}", orphan_red.text);
    assert!(!orphan_red.text.contains("AgMBAAE="), "{}", orphan_red.text);
    assert!(orphan_red.text.contains("normal"), "{}", orphan_red.text);
    assert!(orphan_red.text.contains("after"), "{}", orphan_red.text);

    let (jclean, _) = redact_json(&json!({ "msg": orphan_text }));
    assert!(!jclean["msg"].as_str().unwrap().contains("AgMBAAE="));

    // 3. Same-line END + unclosed BEGIN scrubs preceding body AND trailing open block.
    let same_line = vec![
        "before".to_owned(),
        "MIIEvgIBADANBgkqhkiG9w0BAQEFAASCBKgwggSkAgEAAoIBAQC7".to_owned(),
        "AgMBAAE=".to_owned(),
        format!("{pem_end} {rsa_begin}"),
        "SECOND_KEY_SENSITIVE_BODY_LINE_1234567890".to_owned(),
    ];
    let (same_clean, _) = redact_lines(&same_line);
    let joined = same_clean.join("\n");
    assert!(joined.contains("before"), "{joined}");
    assert!(!joined.contains("MIIEvgIB"), "{joined}");
    assert!(!joined.contains("AgMBAAE="), "{joined}");
    assert!(!joined.contains("SECOND_KEY_SENSITIVE"), "{joined}");

    // Also same-line END + unclosed BEGIN when already inside an open PEM block.
    let chained = vec![
        pem_begin,
        "FIRST_KEY_BODY_1234567890".to_owned(),
        format!("{pem_end} {rsa_begin}"),
        "SECOND_KEY_BODY_9876543210".to_owned(),
    ];
    let (chained_clean, _) = redact_lines(&chained);
    let chained_joined = chained_clean.join("\n");
    assert!(
        !chained_joined.contains("FIRST_KEY_BODY"),
        "{chained_joined}"
    );
    assert!(
        !chained_joined.contains("SECOND_KEY_BODY"),
        "{chained_joined}"
    );

    // 4. AWS_SECRET_ACCESS_KEY across env, JSON, and YAML.
    let aws_secret = ["wJalrXUtnFEMI/", "K7MDENG/bPxRfiCY", "EXAMPLEKEY"].join("");
    for sample in [
        format!("AWS_SECRET_ACCESS_KEY={aws_secret}"),
        format!("{{\"aws_secret_access_key\": \"{aws_secret}\"}}"),
        format!("aws_secret_access_key: {aws_secret}"),
    ] {
        let r = redact_text(&sample);
        assert!(!r.text.contains(&aws_secret), "leaked in `{}`", r.text);
        assert!(r.count >= 1);
    }

    // 5. Odd backslash run (3 backslashes `\\\"`) closes escaped-quote value cleanly.
    let escaped = r#"\"token\": \"secret_val_99\\\", \"safe\": \"keep_me\""#;
    let esc_red = redact_text(escaped);
    assert!(!esc_red.text.contains("secret_val_99"), "{}", esc_red.text);
    assert!(esc_red.text.contains("keep_me"), "{}", esc_red.text);
}

#[test]
fn doctor_release_pin_marker_permissions_sockets_and_path_redaction() {
    let dir = tempfile::tempdir().unwrap();

    // Release store with real ReleaseStore pin format passes without false warning.
    let store = dir.path().join("releases");
    let pins = store.join("pins");
    fs::create_dir_all(&pins).unwrap();
    let pin1 = json!({
        "version": 1,
        "ordinal": 1,
        "previous": null,
        "highest_sequence": 1,
        "pin": {
            "release_id": "v1.0.0",
            "manifest_sha256": "0".repeat(64),
            "sequence": 1
        }
    });
    let pin2 = json!({
        "version": 1,
        "ordinal": 2,
        "previous": "1".repeat(64),
        "highest_sequence": 2,
        "pin": {
            "release_id": "v1.0.1",
            "manifest_sha256": "a".repeat(64),
            "sequence": 2
        }
    });
    fs::write(pins.join("00000000000000000001.json"), pin1.to_string()).unwrap();
    fs::write(pins.join("00000000000000000002.json"), pin2.to_string()).unwrap();
    let mut checks = Vec::new();
    check_release_pin_at(&store, &mut checks);
    assert_eq!(checks.len(), 1);
    assert_eq!(checks[0].status, CheckStatus::Pass, "{:?}", checks[0]);
    assert!(
        checks[0].message.contains("v1.0.1"),
        "{}",
        checks[0].message
    );

    // Symlinked release store fails closed.
    let store_link = dir.path().join("releases-link");
    symlink(&store, &store_link).unwrap();
    let mut sym_checks = Vec::new();
    check_release_pin_at(&store_link, &mut sym_checks);
    assert_eq!(sym_checks[0].status, CheckStatus::Fail);

    // Exact byte equality on host-registry.created (trailing newline must fail).
    let base = dir.path().join("inst-base");
    let inst = base.join("instances/demo");
    let office_home = inst.join("office-state");
    fs::create_dir_all(&office_home).unwrap();
    let marker = inst.join("host-registry.created");
    let id = "0193a5c0-0000-7000-8000-0000000000aa";
    fs::write(&marker, format!("{id}\n")).unwrap();
    let run_dir = office_home.join("run");
    fs::create_dir_all(&run_dir).unwrap();
    let paths = InstancePaths {
        name: "demo".to_owned(),
        account_base: base.clone(),
        user_temp: Some(base.join("temp")),
        setup_dir: base.join("setup/demo"),
        setup_record: base.join("setup/demo/setup.v1.json"),
        locator_dir: base.join("locator"),
        locator_file: base.join("locator/instances.json"),
        locator_lock: base.join("locator/registry.lock"),
        instance_root: inst.clone(),
        instance_config: inst.join("instance.json"),
        registry_created: marker.clone(),
        office_home: office_home.clone(),
        host_lock: office_home.join("host.lock"),
        host_registry_db: office_home.join("host-registry.db"),
        logs_dir: office_home.join("logs"),
        offices_dir: office_home.join("offices"),
        host_socket: run_dir.join("host.sock"),
        run_dir: run_dir.clone(),
        instance_id: Some(id.to_owned()),
        service_mode: Some("foreground".to_owned()),
        credentials_dir: None,
    };
    assert!(
        validate_marker_content(&paths).is_some(),
        "trailing newline in marker must fail"
    );
    fs::write(&marker, id).unwrap();
    assert!(validate_marker_content(&paths).is_none());

    // Symlinked run_dir/o fails runtime.office_sockets.
    let target_dir = dir.path().join("sock-target");
    fs::create_dir(&target_dir).unwrap();
    symlink(&target_dir, run_dir.join("o")).unwrap();
    let mut sock_checks = Vec::new();
    check_sockets(&paths, &mut sock_checks);
    assert!(sock_checks
        .iter()
        .any(|c| c.id == "runtime.office_sockets" && c.status == CheckStatus::Fail));

    // PermissionDenied during check_permissions reports Fail instead of silently passing.
    if org_platform::posix::effective_uid() != 0 {
        let locked_parent = dir.path().join("no-exec-parent");
        let locked_child = locked_parent.join("office-state");
        fs::create_dir_all(&locked_child).unwrap();
        fs::set_permissions(&locked_parent, fs::Permissions::from_mode(0o000)).unwrap();
        let mut perm_paths = paths.clone();
        perm_paths.office_home = locked_child;
        let mut perm_checks = Vec::new();
        check_permissions(&perm_paths, &mut perm_checks);
        fs::set_permissions(&locked_parent, fs::Permissions::from_mode(0o700)).unwrap();
        assert_eq!(perm_checks[0].status, CheckStatus::Fail, "{perm_checks:?}");
    }

    // DoctorCheck constructors redact secrets in `path`.
    let secret = ["sk-ant-", "pathsecret987654321"].join("");
    let c = DoctorCheck::pass("t", "cat", "ok", Some(format!("/tmp/{secret}/db")));
    assert!(!c.path.as_deref().unwrap().contains(&secret));
}

struct StubServiceManager {
    status_result: Result<ServiceStatus, fn() -> ServiceError>,
}

impl ServiceManager for StubServiceManager {
    fn register(&self, _: &Unit) -> Result<Registration, ServiceError> {
        Ok(Registration::Unchanged)
    }
    fn start(&self, _: &str) -> Result<(), ServiceError> {
        Ok(())
    }
    fn stop(&self, _: &str) -> Result<(), ServiceError> {
        Ok(())
    }
    fn status(&self, _: &str) -> Result<ServiceStatus, ServiceError> {
        self.status_result.map_err(|f| f())
    }
    fn unregister(&self, _: &str) -> Result<(), ServiceError> {
        Ok(())
    }
}

#[test]
fn uninstall_guard_and_service_and_office_locks_fail_closed() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let temp = root.join("temp");
    let ao = temp.join("ao");
    fs::create_dir_all(&ao).unwrap();
    let run_ok = ao.join("h1");
    fs::create_dir(&run_ok).unwrap();

    // validate_loaded_abs_path rejects relative, `..`, and `.` paths.
    assert!(validate_loaded_abs_path(Path::new("rel/path"), "test").is_err());
    assert!(validate_loaded_abs_path(&ao.join("../../victim"), "test").is_err());
    assert!(validate_loaded_abs_path(Path::new("/tmp/./foo"), "test").is_err());

    let make_paths = |run_dir: PathBuf, user_temp: Option<PathBuf>| InstancePaths {
        name: "t".to_owned(),
        account_base: root.clone(),
        user_temp,
        setup_dir: root.join("setup/t"),
        setup_record: root.join("setup/t/setup.v1.json"),
        locator_dir: root.join("locator"),
        locator_file: root.join("locator/instances.json"),
        locator_lock: root.join("locator/registry.lock"),
        instance_root: root.join("instances/t"),
        instance_config: root.join("instances/t/instance.json"),
        registry_created: root.join("instances/t/host-registry.created"),
        office_home: root.join("instances/t/office-state"),
        host_lock: root.join("instances/t/office-state/host.lock"),
        host_registry_db: root.join("instances/t/office-state/host-registry.db"),
        logs_dir: root.join("instances/t/office-state/logs"),
        offices_dir: root.join("instances/t/office-state/offices"),
        host_socket: run_dir.join("host.sock"),
        run_dir,
        instance_id: Some("0193a5c0-0000-7000-8000-000000000001".to_owned()),
        service_mode: Some("registered".to_owned()),
        credentials_dir: None,
    };

    assert!(
        verify_instance_temp_dir(&make_paths(run_ok.clone(), Some(temp.clone())))
            .unwrap()
            .is_some()
    );
    // Reject `..` traversal, multi-level nested child, and symlinked run_dir.
    assert!(
        verify_instance_temp_dir(&make_paths(ao.join("../../victim"), Some(temp.clone()))).is_err()
    );
    let nested = run_ok.join("deep");
    fs::create_dir(&nested).unwrap();
    assert!(verify_instance_temp_dir(&make_paths(nested, Some(temp.clone()))).is_err());
    let victim = root.join("victim");
    fs::create_dir(&victim).unwrap();
    let run_link = ao.join("h_link");
    symlink(&victim, &run_link).unwrap();
    assert!(verify_instance_temp_dir(&make_paths(run_link, Some(temp.clone()))).is_err());

    // Symlinked intermediate directory inside anchor is refused.
    let anchor = root.join("anchor");
    fs::create_dir(&anchor).unwrap();
    symlink(&victim, anchor.join("office-state")).unwrap();
    assert!(verify_dir_within_anchor(&anchor, &anchor.join("office-state/logs")).is_err());

    // acquire_office_locks refuses symlinked offices_dir and symlinked office subdir.
    let sym_offices = root.join("sym-offices");
    symlink(&victim, &sym_offices).unwrap();
    assert!(acquire_office_locks(&sym_offices).is_err());
    let real_offices = root.join("real-offices");
    fs::create_dir(&real_offices).unwrap();
    symlink(&victim, real_offices.join("office-link")).unwrap();
    assert!(acquire_office_locks(&real_offices).is_err());
    assert!(acquire_office_locks(&root.join("missing-offices"))
        .unwrap()
        .is_empty());
    // Same-line unclosed BEGIN PRIVATE KEY followed by BEGIN CERTIFICATE stays open.
    assert!(super::redact_pem::has_unclosed_pem_begin(
        "-----BEGIN PRIVATE KEY----- -----BEGIN CERTIFICATE-----"
    ));

    // unregister_service_with fails closed on Running / Unavailable / System / Conflict.
    let id = "0193a5c0-0000-7000-8000-000000000001";
    let err_cases: [Result<ServiceStatus, fn() -> ServiceError>; 4] = [
        Ok(ServiceStatus::Running { pid: 9999 }),
        Err(|| ServiceError::Unavailable("no systemd".into())),
        Err(|| ServiceError::System("systemctl failed".into())),
        Err(|| ServiceError::Conflict("foreign unit".into())),
    ];
    for case in err_cases {
        let mgr = StubServiceManager {
            status_result: case,
        };
        assert!(unregister_service_with("t", id, &mgr).is_err());
    }
    let stopped = StubServiceManager {
        status_result: Ok(ServiceStatus::Stopped),
    };
    assert!(unregister_service_with("t", id, &stopped).unwrap());
    let absent = StubServiceManager {
        status_result: Ok(ServiceStatus::NotRegistered),
    };
    assert!(!unregister_service_with("t", id, &absent).unwrap());
}

const VALID_SCOPED_BWRAP_PROFILE: &str = "\
abi <abi/4.0>,\n\
include <tunables/global>\n\
\n\
profile agentic-org-bwrap /{usr/,}bin/bwrap flags=(unconfined) {\n\
  userns,\n\
\n\
  include if exists <local/agentic-org-bwrap>\n\
}\n";

const UBUNTU_RESTRICT_PROFILE: &str = "\
abi <abi/5.0>,\n\
include <tunables/global>\n\
\n\
profile bwrap /usr/bin/bwrap flags=(attach_disconnected,mediate_deleted) {\n\
  allow userns,\n\
  allow pix /** -> &bwrap//&unpriv_bwrap,\n\
}\n\
profile unpriv_bwrap flags=(attach_disconnected,mediate_deleted) {\n\
  audit deny capability,\n\
}\n";

fn assert_never_advises_global_sysctl_disable(check: &DoctorCheck) {
    let combined = format!(
        "{}\n{}",
        check.message,
        check.remediation.as_deref().unwrap_or("")
    );
    let lower = combined.to_ascii_lowercase();
    assert!(
        !lower.contains("= 0")
            && !lower.contains("=0")
            && !lower.contains("relax kernel.apparmor_restrict_unprivileged_userns"),
        "doctor output must never advise disabling apparmor_restrict_unprivileged_userns globally: {combined}"
    );
}

#[test]
fn doctor_apparmor_userns_handles_missing_zero_unreadable_and_scoped_profile_states() {
    let dir = tempfile::tempdir().unwrap();
    let empty_aa = dir.path().join("aa-empty");
    fs::create_dir_all(&empty_aa).unwrap();

    // 1. Missing sysctl (e.g., macOS or non-AppArmor Linux) returns None.
    let missing = dir.path().join("missing-sysctl");
    assert!(check_linux_apparmor_userns_at(&missing, &empty_aa, None).is_none());

    // 2. sysctl=0 returns Pass.
    let sysctl_zero = dir.path().join("sysctl-0");
    fs::write(&sysctl_zero, "0\n").unwrap();
    let pass_zero = check_linux_apparmor_userns_at(&sysctl_zero, &empty_aa, None).unwrap();
    assert_eq!(pass_zero.status, CheckStatus::Pass, "{pass_zero:?}");
    assert!(pass_zero
        .message
        .contains("unprivileged user namespaces permitted"));

    // 3. sysctl=1 without any scoped profile returns Warn and NEVER suggests disabling sysctl globally.
    let sysctl_one = dir.path().join("sysctl-1");
    fs::write(&sysctl_one, "1\n").unwrap();
    let warn_missing = check_linux_apparmor_userns_at(&sysctl_one, &empty_aa, None).unwrap();
    assert_eq!(warn_missing.status, CheckStatus::Warn, "{warn_missing:?}");
    let hint = warn_missing.remediation.as_deref().unwrap();
    assert!(hint.contains("/etc/apparmor.d/agentic-org-bwrap"), "{hint}");
    assert!(
        hint.contains("scripts/install.sh --install-bwrap-apparmor"),
        "{hint}"
    );
    assert_never_advises_global_sysctl_disable(&warn_missing);

    // 4. sysctl=1 with valid scoped agentic-org-bwrap profile returns Pass (with None or unreadable profiles status).
    let scoped_aa = dir.path().join("aa-scoped");
    fs::create_dir_all(&scoped_aa).unwrap();
    let scoped_profile = scoped_aa.join("agentic-org-bwrap");
    fs::write(&scoped_profile, VALID_SCOPED_BWRAP_PROFILE).unwrap();
    let pass_present = check_linux_apparmor_userns_at(&sysctl_one, &scoped_aa, None).unwrap();
    assert_eq!(pass_present.status, CheckStatus::Pass, "{pass_present:?}");
    assert!(
        pass_present
            .message
            .contains("scoped bwrap AppArmor profile present at"),
        "{}",
        pass_present.message
    );
    assert!(
        pass_present
            .message
            .contains("(apparmor_restrict_unprivileged_userns=1)"),
        "{}",
        pass_present.message
    );

    // Unreadable /sys/kernel/security/apparmor/profiles (0000 -> PermissionDenied) falls back cleanly to Pass.
    if org_platform::posix::effective_uid() != 0 {
        let unreadable_profiles = dir.path().join("profiles-denied");
        fs::write(&unreadable_profiles, "agentic-org-bwrap (unconfined)\n").unwrap();
        fs::set_permissions(&unreadable_profiles, fs::Permissions::from_mode(0o000)).unwrap();
        let pass_perm_denied =
            check_linux_apparmor_userns_at(&sysctl_one, &scoped_aa, Some(&unreadable_profiles))
                .unwrap();
        fs::set_permissions(&unreadable_profiles, fs::Permissions::from_mode(0o600)).unwrap();
        assert_eq!(
            pass_perm_denied.status,
            CheckStatus::Pass,
            "{pass_perm_denied:?}"
        );
        assert!(pass_perm_denied
            .message
            .contains("scoped bwrap AppArmor profile present at"));
    }

    // Readable profiles status confirming agentic-org-bwrap (unconfined) is loaded returns Pass.
    let loaded_profiles = dir.path().join("profiles-loaded");
    fs::write(
        &loaded_profiles,
        "unprivileged_userns (enforce)\nagentic-org-bwrap (unconfined)\n",
    )
    .unwrap();
    let pass_loaded =
        check_linux_apparmor_userns_at(&sysctl_one, &scoped_aa, Some(&loaded_profiles)).unwrap();
    assert_eq!(pass_loaded.status, CheckStatus::Pass, "{pass_loaded:?}");
    assert!(pass_loaded.message.contains("loaded (unconfined)"));

    // Readable profiles status WITHOUT agentic-org-bwrap warns that the on-disk profile is not loaded.
    let unloaded_profiles = dir.path().join("profiles-unloaded");
    fs::write(&unloaded_profiles, "unprivileged_userns (enforce)\n").unwrap();
    let warn_unloaded =
        check_linux_apparmor_userns_at(&sysctl_one, &scoped_aa, Some(&unloaded_profiles)).unwrap();
    assert_eq!(warn_unloaded.status, CheckStatus::Warn, "{warn_unloaded:?}");
    assert!(warn_unloaded.message.contains("not loaded"));
    assert_never_advises_global_sysctl_disable(&warn_unloaded);

    // 5. sysctl=1 with disabled profile symlink returns Warn.
    let disabled_aa = dir.path().join("aa-disabled");
    fs::create_dir_all(disabled_aa.join("disable")).unwrap();
    let disabled_profile = disabled_aa.join("agentic-org-bwrap");
    fs::write(&disabled_profile, VALID_SCOPED_BWRAP_PROFILE).unwrap();
    symlink(
        &disabled_profile,
        disabled_aa.join("disable/agentic-org-bwrap"),
    )
    .unwrap();
    let warn_disabled = check_linux_apparmor_userns_at(&sysctl_one, &disabled_aa, None).unwrap();
    assert_eq!(warn_disabled.status, CheckStatus::Warn, "{warn_disabled:?}");
    assert!(warn_disabled.message.contains("disabled"));
    assert_never_advises_global_sysctl_disable(&warn_disabled);

    // 6. sysctl=1 with bwrap-userns-restrict only returns Warn (never falsely claims loaded).
    let restrict_aa = dir.path().join("aa-restrict-only");
    fs::create_dir_all(&restrict_aa).unwrap();
    fs::write(
        restrict_aa.join("bwrap-userns-restrict"),
        UBUNTU_RESTRICT_PROFILE,
    )
    .unwrap();
    let restrict_loaded = dir.path().join("profiles-restrict-loaded");
    fs::write(
        &restrict_loaded,
        "unprivileged_userns (enforce)\nbwrap (enforce)\nunpriv_bwrap (enforce)\n",
    )
    .unwrap();
    let warn_restrict =
        check_linux_apparmor_userns_at(&sysctl_one, &restrict_aa, Some(&restrict_loaded)).unwrap();
    assert_eq!(warn_restrict.status, CheckStatus::Warn, "{warn_restrict:?}");
    assert!(warn_restrict.message.contains("bwrap-userns-restrict"));
    assert_never_advises_global_sysctl_disable(&warn_restrict);

    // Scoped agentic-org-bwrap alongside an active (non-disabled) bwrap-userns-restrict warns until disabled.
    fs::write(
        restrict_aa.join("agentic-org-bwrap"),
        VALID_SCOPED_BWRAP_PROFILE,
    )
    .unwrap();
    let warn_conflict = check_linux_apparmor_userns_at(&sysctl_one, &restrict_aa, None).unwrap();
    assert_eq!(warn_conflict.status, CheckStatus::Warn, "{warn_conflict:?}");
    assert!(warn_conflict.message.contains("conflicting"));
    assert_never_advises_global_sysctl_disable(&warn_conflict);

    // Disabling bwrap-userns-restrict via disable/ symlink makes the scoped profile Pass cleanly.
    fs::create_dir_all(restrict_aa.join("disable")).unwrap();
    symlink(
        restrict_aa.join("bwrap-userns-restrict"),
        restrict_aa.join("disable/bwrap-userns-restrict"),
    )
    .unwrap();
    let pass_after_disable =
        check_linux_apparmor_userns_at(&sysctl_one, &restrict_aa, Some(&loaded_profiles)).unwrap();
    assert_eq!(
        pass_after_disable.status,
        CheckStatus::Pass,
        "{pass_after_disable:?}"
    );
    // Even if disabled on disk, if unpriv_bwrap is still loaded in kernel profiles, warn!
    let mixed_loaded = dir.path().join("profiles-mixed");
    fs::write(
        &mixed_loaded,
        "agentic-org-bwrap (unconfined)\nunpriv_bwrap (enforce)\n",
    )
    .unwrap();
    let warn_stale_kernel =
        check_linux_apparmor_userns_at(&sysctl_one, &restrict_aa, Some(&mixed_loaded)).unwrap();
    assert_eq!(
        warn_stale_kernel.status,
        CheckStatus::Warn,
        "{warn_stale_kernel:?}"
    );
    assert!(warn_stale_kernel.message.contains("unpriv_bwrap"));

    // 7. Legacy Ubuntu 24.04 unconfined `bwrap` profile passes; capability-stripping `bwrap` fails.
    let legacy_aa = dir.path().join("aa-legacy");
    fs::create_dir_all(&legacy_aa).unwrap();
    fs::write(
        legacy_aa.join("bwrap"),
        "abi <abi/4.0>,\ninclude <tunables/global>\nprofile bwrap /usr/bin/bwrap flags=(unconfined) {\n  userns,\n}\n",
    )
    .unwrap();
    let legacy_loaded = dir.path().join("profiles-legacy-loaded");
    fs::write(&legacy_loaded, "bwrap (unconfined)\n").unwrap();
    let pass_legacy =
        check_linux_apparmor_userns_at(&sysctl_one, &legacy_aa, Some(&legacy_loaded)).unwrap();
    assert_eq!(pass_legacy.status, CheckStatus::Pass, "{pass_legacy:?}");

    let legacy_restrict_aa = dir.path().join("aa-legacy-restrict");
    fs::create_dir_all(&legacy_restrict_aa).unwrap();
    fs::write(legacy_restrict_aa.join("bwrap"), UBUNTU_RESTRICT_PROFILE).unwrap();
    let warn_legacy_restrict =
        check_linux_apparmor_userns_at(&sysctl_one, &legacy_restrict_aa, None).unwrap();
    assert_eq!(
        warn_legacy_restrict.status,
        CheckStatus::Warn,
        "{warn_legacy_restrict:?}"
    );

    // 8. Adversarial profile & sysctl checks: symlink, `deny userns`, FIFO, and unexpected value warn cleanly.
    let bad_aa = dir.path().join("aa-bad");
    fs::create_dir_all(&bad_aa).unwrap();
    symlink(&scoped_profile, bad_aa.join("agentic-org-bwrap")).unwrap();
    let warn_sym = check_linux_apparmor_userns_at(&sysctl_one, &bad_aa, None).unwrap();
    assert_eq!(warn_sym.status, CheckStatus::Warn, "{warn_sym:?}");

    fs::remove_file(bad_aa.join("agentic-org-bwrap")).unwrap();
    fs::write(
        bad_aa.join("agentic-org-bwrap"),
        "abi <abi/4.0>,\nprofile agentic-org-bwrap /{usr/,}bin/bwrap {\n  deny userns,\n}\n",
    )
    .unwrap();
    let warn_deny = check_linux_apparmor_userns_at(&sysctl_one, &bad_aa, None).unwrap();
    assert_eq!(warn_deny.status, CheckStatus::Warn, "{warn_deny:?}");

    let fifo_sysctl = dir.path().join("sysctl.fifo");
    let c_fifo = std::ffi::CString::new(fifo_sysctl.as_os_str().as_encoded_bytes()).unwrap();
    org_platform::posix::make_fifo(&c_fifo, 0o600).unwrap();
    let warn_fifo = check_linux_apparmor_userns_at(&fifo_sysctl, &empty_aa, None).unwrap();
    assert_eq!(warn_fifo.status, CheckStatus::Warn, "{warn_fifo:?}");

    let weird_sysctl = dir.path().join("sysctl-weird");
    fs::write(&weird_sysctl, "2\n").unwrap();
    let warn_weird = check_linux_apparmor_userns_at(&weird_sysctl, &empty_aa, None).unwrap();
    assert_eq!(warn_weird.status, CheckStatus::Warn, "{warn_weird:?}");
    assert_never_advises_global_sysctl_disable(&warn_weird);

    // 9. Unreadable sysctl returns Warn (guarded for non-root UID).
    if org_platform::posix::effective_uid() != 0 {
        let unreadable_sysctl = dir.path().join("sysctl-unreadable");
        fs::write(&unreadable_sysctl, "1\n").unwrap();
        fs::set_permissions(&unreadable_sysctl, fs::Permissions::from_mode(0o000)).unwrap();
        let warn_unreadable =
            check_linux_apparmor_userns_at(&unreadable_sysctl, &empty_aa, None).unwrap();
        fs::set_permissions(&unreadable_sysctl, fs::Permissions::from_mode(0o600)).unwrap();
        assert_eq!(
            warn_unreadable.status,
            CheckStatus::Warn,
            "{warn_unreadable:?}"
        );
        assert!(warn_unreadable
            .message
            .contains("could not read AppArmor userns sysctl"));
    }
}

#[test]
fn doctor_apparmor_userns_rejects_shadowing_wrong_attachment_enforcing_and_child_notation() {
    let dir = tempfile::tempdir().unwrap();
    let sysctl_one = dir.path().join("sysctl-1");
    fs::write(&sysctl_one, "1\n").unwrap();

    // 1. Coexisting active restricting /etc/apparmor.d/bwrap shadows valid agentic-org-bwrap -> Warn.
    let shadow_aa = dir.path().join("aa-shadow");
    fs::create_dir_all(&shadow_aa).unwrap();
    fs::write(
        shadow_aa.join("agentic-org-bwrap"),
        VALID_SCOPED_BWRAP_PROFILE,
    )
    .unwrap();
    fs::write(
        shadow_aa.join("bwrap"),
        "abi <abi/4.0>,\nprofile bwrap /usr/bin/bwrap {\n  userns,\n}\n",
    )
    .unwrap();
    let warn_shadow = check_linux_apparmor_userns_at(&sysctl_one, &shadow_aa, None).unwrap();
    assert_eq!(warn_shadow.status, CheckStatus::Warn, "{warn_shadow:?}");
    assert!(
        warn_shadow.message.contains("shadows"),
        "{}",
        warn_shadow.message
    );
    assert_never_advises_global_sysctl_disable(&warn_shadow);

    // Disabling the shadowing `bwrap` profile allows `agentic-org-bwrap` to Pass.
    fs::create_dir_all(shadow_aa.join("disable")).unwrap();
    symlink(shadow_aa.join("bwrap"), shadow_aa.join("disable/bwrap")).unwrap();
    let pass_unshadowed = check_linux_apparmor_userns_at(&sysctl_one, &shadow_aa, None).unwrap();
    assert_eq!(
        pass_unshadowed.status,
        CheckStatus::Pass,
        "{pass_unshadowed:?}"
    );

    // 2. Kernel loaded `bwrap (enforce)` alongside `agentic-org-bwrap (unconfined)` shadows it -> Warn.
    let shadow_loaded = dir.path().join("profiles-shadow-enforce");
    fs::write(
        &shadow_loaded,
        "agentic-org-bwrap (unconfined)\nbwrap (enforce)\n",
    )
    .unwrap();
    let warn_kernel_shadow =
        check_linux_apparmor_userns_at(&sysctl_one, &shadow_aa, Some(&shadow_loaded)).unwrap();
    assert_eq!(
        warn_kernel_shadow.status,
        CheckStatus::Warn,
        "{warn_kernel_shadow:?}"
    );
    assert!(warn_kernel_shadow.message.contains("shadows"));
    assert_never_advises_global_sysctl_disable(&warn_kernel_shadow);

    // 3. Nested child profile notation `bwrap//unpriv_bwrap (enforce)` in kernel profiles -> Warn.
    let child_loaded = dir.path().join("profiles-child-notation");
    fs::write(
        &child_loaded,
        "agentic-org-bwrap (unconfined)\nbwrap//unpriv_bwrap (enforce)\n",
    )
    .unwrap();
    let warn_child =
        check_linux_apparmor_userns_at(&sysctl_one, &shadow_aa, Some(&child_loaded)).unwrap();
    assert_eq!(warn_child.status, CheckStatus::Warn, "{warn_child:?}");
    assert!(warn_child.message.contains("unpriv_bwrap"));

    // 4. Kernel loaded `agentic-org-bwrap (enforce)` instead of unconfined -> Warn.
    let scoped_enforce_loaded = dir.path().join("profiles-scoped-enforce");
    fs::write(&scoped_enforce_loaded, "agentic-org-bwrap (enforce)\n").unwrap();
    let warn_scoped_enforce =
        check_linux_apparmor_userns_at(&sysctl_one, &shadow_aa, Some(&scoped_enforce_loaded))
            .unwrap();
    assert_eq!(
        warn_scoped_enforce.status,
        CheckStatus::Warn,
        "{warn_scoped_enforce:?}"
    );
    assert!(warn_scoped_enforce.message.contains("enforce"));

    // 5. Wrong attachment `/usr/bin/true` must NOT match `agentic-org-bwrap` profile name.
    let wrong_att_aa = dir.path().join("aa-wrong-att");
    fs::create_dir_all(&wrong_att_aa).unwrap();
    fs::write(
        wrong_att_aa.join("agentic-org-bwrap"),
        "abi <abi/4.0>,\nprofile agentic-org-bwrap /usr/bin/true flags=(unconfined) {\n  userns,\n}\n",
    )
    .unwrap();
    let warn_wrong_att = check_linux_apparmor_userns_at(&sysctl_one, &wrong_att_aa, None).unwrap();
    assert_eq!(
        warn_wrong_att.status,
        CheckStatus::Warn,
        "{warn_wrong_att:?}"
    );

    // Unattached profile without attachment path must also Warn.
    fs::write(
        wrong_att_aa.join("agentic-org-bwrap"),
        "abi <abi/4.0>,\nprofile agentic-org-bwrap flags=(unconfined) {\n  userns,\n}\n",
    )
    .unwrap();
    let warn_unattached = check_linux_apparmor_userns_at(&sysctl_one, &wrong_att_aa, None).unwrap();
    assert_eq!(
        warn_unattached.status,
        CheckStatus::Warn,
        "{warn_unattached:?}"
    );

    // 6. Default-deny enforcing (non-unconfined) `bwrap` and `agentic-org-bwrap` profiles -> Warn.
    let enforce_bwrap_aa = dir.path().join("aa-enforce-bwrap");
    fs::create_dir_all(&enforce_bwrap_aa).unwrap();
    fs::write(
        enforce_bwrap_aa.join("bwrap"),
        "abi <abi/4.0>,\nprofile bwrap /usr/bin/bwrap {\n  userns,\n}\n",
    )
    .unwrap();
    let warn_enforce_bwrap =
        check_linux_apparmor_userns_at(&sysctl_one, &enforce_bwrap_aa, None).unwrap();
    assert_eq!(
        warn_enforce_bwrap.status,
        CheckStatus::Warn,
        "{warn_enforce_bwrap:?}"
    );

    let enforce_scoped_aa = dir.path().join("aa-enforce-scoped");
    fs::create_dir_all(&enforce_scoped_aa).unwrap();
    fs::write(
        enforce_scoped_aa.join("agentic-org-bwrap"),
        "abi <abi/4.0>,\nprofile agentic-org-bwrap /{usr/,}bin/bwrap {\n  userns,\n}\n",
    )
    .unwrap();
    let warn_enforce_scoped =
        check_linux_apparmor_userns_at(&sysctl_one, &enforce_scoped_aa, None).unwrap();
    assert_eq!(
        warn_enforce_scoped.status,
        CheckStatus::Warn,
        "{warn_enforce_scoped:?}"
    );

    // 7. Unexpected sysctl content is sanitized (no raw ANSI escape sequences leak).
    let ansi_sysctl = dir.path().join("sysctl-ansi");
    fs::write(&ansi_sysctl, "\x1b[31m2\x1b[0m\n").unwrap();
    let warn_ansi = check_linux_apparmor_userns_at(&ansi_sysctl, &wrong_att_aa, None).unwrap();
    assert_eq!(warn_ansi.status, CheckStatus::Warn, "{warn_ansi:?}");
    assert!(
        !warn_ansi.message.contains('\x1b'),
        "raw ANSI escape must be sanitized: {:?}",
        warn_ansi.message
    );
    assert_never_advises_global_sysctl_disable(&warn_ansi);
}
