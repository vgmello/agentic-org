use std::path::PathBuf;

use org_client::ClientError;
use org_proto::ErrorCode;

use super::{map_client_error, read_binding_file_token, Binding, MAX_BINDING_FILE_BYTES};
use crate::runtime::transport::hint_for;

#[test]
fn a_timeout_says_to_retry_with_the_same_idempotency_key() {
    let refusal = map_client_error(ClientError::Timeout);
    assert_eq!(refusal.code, ErrorCode::Timeout);
    assert_eq!(refusal.hint, hint_for(ErrorCode::Timeout));
    assert!(
        refusal.hint.contains("--idempotency-key"),
        "{}",
        refusal.hint
    );
}

#[test]
fn binding_debug_redacts_session_token() {
    let binding = Binding {
        socket: Some(PathBuf::from("/tmp/office.sock")),
        office_id: Some("0190a5d2-7c3e-7a01-8b2c-3d4e5f6071a0".into()),
        runtime_dir: Some(PathBuf::from("/tmp")),
        session_token: Some("super-secret-session-token".into()),
        agent_id: Some("0190a5d2-7c3e-7a01-8b2c-3d4e5f6071a1".into()),
        external_state: None,
        ..Binding::default()
    };
    let rendered = format!("{binding:?}");
    assert!(
        !rendered.contains("super-secret-session-token"),
        "{rendered}"
    );
    assert!(rendered.contains("<redacted>"), "{rendered}");
}

#[test]
fn binding_file_reads_trimmed_token() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("session-binding.v1");
    std::fs::write(&file, "  tok-from-file-123\n").unwrap();
    assert_eq!(
        read_binding_file_token(file.to_str().unwrap()).as_deref(),
        Some("tok-from-file-123")
    );
    assert_eq!(read_binding_file_token("/nonexistent/binding"), None);

    // Reject symlinks (O_NOFOLLOW) and oversized files.
    let link = dir.path().join("link-binding.v1");
    org_platform::fs::create_symlink(&file, &link).unwrap();
    assert_eq!(read_binding_file_token(link.to_str().unwrap()), None);

    let huge = dir.path().join("huge-binding.v1");
    std::fs::write(&huge, "x".repeat((MAX_BINDING_FILE_BYTES + 1) as usize)).unwrap();
    assert_eq!(read_binding_file_token(huge.to_str().unwrap()), None);
}

#[test]
fn binding_file_refuses_dangling_links_directories_trailing_slashes_and_blank_files() {
    let dir = tempfile::tempdir().unwrap();
    let dangling = dir.path().join("dangling-binding.v1");
    org_platform::fs::create_symlink(&dir.path().join("missing"), &dangling).unwrap();
    assert_eq!(read_binding_file_token(dangling.to_str().unwrap()), None);

    let folder = dir.path().join("folder-binding.v1");
    std::fs::create_dir(&folder).unwrap();
    assert_eq!(read_binding_file_token(folder.to_str().unwrap()), None);

    let file = dir.path().join("session-binding.v1");
    std::fs::write(&file, "tok").unwrap();
    let slashed = format!("{}/", file.display());
    assert_eq!(read_binding_file_token(&slashed), None);

    let blank = dir.path().join("blank-binding.v1");
    std::fs::write(&blank, " \n\t").unwrap();
    assert_eq!(read_binding_file_token(blank.to_str().unwrap()), None);

    // Exactly at the bound is still read.
    let full = dir.path().join("full-binding.v1");
    std::fs::write(&full, "y".repeat(MAX_BINDING_FILE_BYTES as usize)).unwrap();
    assert_eq!(
        read_binding_file_token(full.to_str().unwrap()).map(|t| t.len()),
        Some(MAX_BINDING_FILE_BYTES as usize)
    );
}

#[test]
fn conflict_api_error_populates_retry_with_and_error_conflict() {
    use org_proto::{ApiError, ConflictDetail, ConflictEvent, ConflictSummary};

    let detail = ConflictDetail {
        expected_revision: 2,
        current_revision: 4,
        changed_sections: vec!["title".into(), "checklist".into()],
        events_since: vec![ConflictEvent {
            revision: 3,
            occurred_at: Some("2026-11-27T10:00:00.000Z".into()),
            actor_kind: "employee".into(),
            actor_id: None,
            change: serde_json::json!({"type": "task_updated", "fields": ["title"]}),
        }],
        events_truncated: false,
        omitted_events: 0,
        reread_recommended: false,
        current_summary: Some(ConflictSummary {
            key: Some("T-A1".into()),
            title: "Latest".into(),
            status: "todo".into(),
            priority: "normal".into(),
            assignee_id: None,
            assigned_team_id: None,
            checklist_total: 1,
            checklist_done: 0,
            ac_total: 1,
            ac_done: 0,
            dod_total: 0,
            dod_done: 0,
            work_steps_total: 0,
            work_steps_done: 0,
            comment_count: 0,
            watcher_count: 0,
        }),
    };
    let refusal = map_client_error(ClientError::Api(ApiError::conflict(
        "revision conflict: expected 2, current is 4",
        detail.clone(),
    )));
    assert_eq!(refusal.code, ErrorCode::Conflict);
    assert_eq!(
        refusal
            .retry_with
            .as_ref()
            .and_then(|m| m.get("if-match"))
            .and_then(|v| v.as_u64()),
        Some(4)
    );
    assert_eq!(refusal.conflict(), Some(&detail));
    assert!(refusal.hint.contains("--if-match 4"), "{}", refusal.hint);
    assert!(!refusal.hint.contains("re-inspect"), "{}", refusal.hint);

    let mut reread = detail;
    reread.reread_recommended = true;
    let reread_refusal = map_client_error(ClientError::Api(ApiError::conflict(
        "revision conflict: expected 2, current is 4",
        reread,
    )));
    assert!(
        reread_refusal.hint.contains("re-inspect"),
        "{}",
        reread_refusal.hint
    );
}
