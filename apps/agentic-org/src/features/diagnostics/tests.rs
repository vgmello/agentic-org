//! Unit and adversarial security tests for `features::diagnostics`.

use std::fs;
use std::os::unix::fs::{symlink, PermissionsExt};
use std::path::Path;

use org_office::{EngineChoice, Office, TestOffice};
use serde_json::json;

use super::doctor::CheckStatus;
use super::org_integrity::check_org_state;
use super::paths::{
    normalize_lstat_path, probe_lock_held, read_regular_bounded, write_atomic_owner_only, LockProbe,
};
use super::redact::{redact_json, redact_lines, redact_text, REDACTED};
use super::sqlite_read::read_projected_tasks;
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
