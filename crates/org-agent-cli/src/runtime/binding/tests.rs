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
