use std::path::Path;

use super::git::sanitize_git_stderr;
use super::tree::parse_ls_tree_entries;
use super::*;
use crate::organization::records::OfficeCode;

#[test]
fn next_office_code_skips_used_and_ambiguous_letters() {
    let now = "2026-11-21T00:00:00.000Z".to_owned();
    let oid = Id::parse("01900000-0000-7000-8000-000000000001").unwrap();
    let mk = |code: &str| OfficeCode {
        code: code.into(),
        office_id: oid.clone(),
        issued_at: now.clone(),
        retired_at: None,
    };
    assert_eq!(next_office_code(&[mk("A")]).as_deref(), Some("B"));
    let all_single: Vec<OfficeCode> = b"ABCDEFGHJKLMNPQRSTUVWXYZ"
        .iter()
        .map(|&b| mk(&(b as char).to_string()))
        .collect();
    assert_eq!(next_office_code(&all_single).as_deref(), Some("AA"));
}

#[test]
fn check_git_source_rejects_ext_credentials_and_option_injection() {
    for bad in [
        ("ext::sh -c id", "org/state"),
        ("-u", "org/state"),
        ("file://relative/path", "org/state"),
        ("https://user:pass@example.test/org.git", "org/state"),
        ("https://example.test/org.git?token=secret", "org/state"),
        ("ssh://-oProxyCommand=id@example.test/org.git", "org/state"),
        ("ssh://-host/org.git", "org/state"),
        ("https://example.test/org.git", "-bad"),
        ("https://example.test/org.git", "org..state"),
        ("https://example.test/org.git", "org/@{0}"),
        ("https://example.test/org.git", "org/state.lock"),
    ] {
        assert!(
            check_git_source(bad.0, bad.1).is_err(),
            "expected rejection for {bad:?}"
        );
    }
    assert!(check_git_source("https://example.test/org.git", "org/state").is_ok());
    assert!(check_git_source("ssh://git@example.test/org.git", "org/state").is_ok());
    assert!(check_git_source("file:///tmp/repo.git", "org/state").is_ok());
}

#[test]
fn sanitize_git_stderr_prefers_fatal_and_redacts_staging_and_credentials() {
    let staging = Path::new("/tmp/org-import-12345");
    let stderr = "Cloning into '/tmp/org-import-12345/repo.git'...\nfatal: repository 'https://alice:topsecret@example.test/org.git' not found in /tmp/org-import-12345/repo.git\n";
    let msg = sanitize_git_stderr(stderr, staging, "clone failed");
    assert!(msg.starts_with("fatal:"), "{msg}");
    assert!(!msg.contains("/tmp/org-import-12345"), "{msg}");
    assert!(!msg.contains("topsecret"), "{msg}");
    assert!(msg.contains("https://example.test/org.git"), "{msg}");
}

#[test]
fn parse_ls_tree_rejects_symlinks_and_submodules_before_blob_read() {
    let oid = "0123456789abcdef0123456789abcdef01234567";
    let symlink_raw =
        format!("100644 blob {oid} 10\torganization.yaml\0120000 blob {oid} 8\tlink.yaml\0");
    match parse_ls_tree_entries(symlink_raw.as_bytes()).unwrap_err() {
        ImportError::InvalidFile { path, reason } => {
            assert_eq!(path, "state/link.yaml");
            assert!(reason.contains("symlink"), "{reason}");
        }
        other => panic!("expected symlink refusal, got {other:?}"),
    }

    let submodule_raw =
        format!("100644 blob {oid} 10\torganization.yaml\0160000 commit {oid} -\tsubmod\0");
    match parse_ls_tree_entries(submodule_raw.as_bytes()).unwrap_err() {
        ImportError::InvalidFile { path, reason } => {
            assert_eq!(path, "state/submod");
            assert!(reason.contains("submodule"), "{reason}");
        }
        other => panic!("expected submodule refusal, got {other:?}"),
    }
}
