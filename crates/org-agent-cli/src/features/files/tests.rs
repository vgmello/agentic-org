//! Tests for the unified `agent-os file` slice and the two-layer `--attach` / `file upload` /
//! `file fetch` exfiltration defense (`TASK-98.4`, ADR-0037).

use std::fs;
use std::path::PathBuf;
use std::process::Command;

use base64::engine::general_purpose::STANDARD;
use base64::Engine as _;
use org_command::Refusal;
use org_proto::attachments::{ATTACHMENT_DOCUMENT_MAX_BYTES, ATTACHMENT_IMAGE_MAX_BYTES};
use org_proto::ErrorCode;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use crate::runtime::file_guard::{
    prepare_output_path, read_upload_file, strip_root_prefix_with_case, with_test_roots, FileRoots,
    RootEntry,
};
use crate::runtime::file_transfer::fetch_attachment_to_path;
use crate::runtime::transport::{OfficeTransport, TransportOffice};

const ATT_ID: &str = "0193a5c0-0000-7000-8000-000000000501";
const UPLOAD_ID: &str = "0193a5c0-0000-7000-8000-000000000502";
const PROJ_ID: &str = "0193a5c0-0000-7000-8000-000000000010";
const TASK_ID: &str = "0193a5c0-0000-7000-8000-000000000100";
const EMP_ID: &str = "0193a5c0-0000-7000-8000-000000000020";

struct FileMockTransport {
    caller_id: Option<String>,
    calls: Vec<(String, Value)>,
    uploaded: Vec<u8>,
    download_bytes: Vec<u8>,
    tamper_sha: bool,
}

impl FileMockTransport {
    fn new(caller_id: Option<&str>) -> Self {
        Self {
            caller_id: caller_id.map(str::to_owned),
            calls: Vec::new(),
            uploaded: Vec::new(),
            download_bytes: b"hello fetched attachment\n".to_vec(),
            tamper_sha: false,
        }
    }
}

fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

fn sample_attachment_json(scope: &str, target_kind: &str, target_id: Option<&str>) -> Value {
    let mut obj = json!({
        "id": ATT_ID,
        "scope": scope,
        "target_kind": target_kind,
        "filename": "spec.md",
        "mime_type": "text/markdown",
        "byte_count": 14,
        "title": "Spec",
        "artifact_status": "final",
        "uploaded_by_employee_id": EMP_ID,
        "created_at": "2026-11-27T00:00:00.000Z",
        "updated_at": "2026-11-27T00:00:00.000Z",
        "content_sha256": sha256_hex(b"# Spec\nhello!\n"),
    });
    if let Some(tid) = target_id {
        obj["target_id"] = json!(tid);
    }
    obj
}

impl OfficeTransport for FileMockTransport {
    fn caller_member_id(&self) -> Option<String> {
        self.caller_id.clone()
    }

    fn call_method(&mut self, method: &str, params: Value) -> Result<Value, Refusal> {
        self.calls.push((method.to_owned(), params.clone()));
        match method {
            "project.inspect" => Ok(json!({
                "id": PROJ_ID,
                "number": 1,
                "key": "P-1",
                "slug": "alpha",
                "name": "Alpha",
                "owner_team_id": "0193a5c0-0000-7000-8000-000000000011",
                "contributing_team_ids": [],
                "archived": false,
                "revision": 1
            })),
            "tasks.inspect" => Ok(json!({
                "task": {
                    "id": TASK_ID,
                    "office_code": "A",
                    "number": 1,
                    "key": "T-A1",
                    "revision": 1,
                    "team_id": "0193a5c0-0000-7000-8000-000000000011",
                    "type": "task",
                    "title": "Task",
                    "status": "todo",
                    "body_markdown": "",
                    "priority": "normal",
                    "labels": [],
                    "reference_files": [],
                    "modified_files": [],
                    "depends_on": [],
                    "dependents": [],
                    "created_by_kind": "employee",
                    "updated_by_kind": "employee",
                    "checklist_total": 0,
                    "checklist_done": 0,
                    "comment_count": 0,
                    "watcher_count": 0
                },
                "checklist": [],
                "comments": [],
                "attachments": [],
                "depends_on_tasks": [],
                "dependent_tasks": [],
                "watchers": [],
                "history": []
            })),
            "attachment.upload_begin" => {
                self.uploaded.clear();
                Ok(json!({
                    "upload": {
                        "upload_id": UPLOAD_ID,
                        "filename": params["filename"],
                        "mime_type": params["mime_type"],
                        "byte_count": params["byte_count"],
                        "received_bytes": 0,
                        "committed": false,
                        "expires_at": "2026-11-27T01:00:00.000Z"
                    },
                    "chunk_size_bytes": 8
                }))
            }
            "attachment.upload_chunk" => {
                let raw = STANDARD
                    .decode(params["data_base64"].as_str().unwrap())
                    .unwrap();
                self.uploaded.extend_from_slice(&raw);
                Ok(json!({
                    "upload_id": UPLOAD_ID,
                    "received_bytes": self.uploaded.len(),
                    "complete": false
                }))
            }
            "attachment.upload_commit" => Ok(json!({
                "upload": {
                    "upload_id": UPLOAD_ID,
                    "filename": "spec.md",
                    "mime_type": "text/markdown",
                    "byte_count": self.uploaded.len(),
                    "received_bytes": self.uploaded.len(),
                    "committed": true,
                    "expires_at": "2026-11-27T01:00:00.000Z",
                    "content_sha256": sha256_hex(&self.uploaded)
                }
            })),
            "attachment.attach" => {
                let target_kind = params["target_kind"].as_str().unwrap();
                let target_id = params.get("target_id").and_then(Value::as_str);
                let scope = if target_kind == "user_private" {
                    "user_private"
                } else {
                    "shared"
                };
                Ok(json!({
                    "attachment": sample_attachment_json(scope, target_kind, target_id)
                }))
            }
            "attachment.list" => {
                let target_kind = params["target_kind"].as_str().unwrap_or("organization");
                let target_id = params.get("target_id").and_then(Value::as_str);
                let scope = params["scope"].as_str().unwrap_or("shared");
                Ok(json!({
                    "attachments": [sample_attachment_json(scope, target_kind, target_id)],
                    "next_cursor": null
                }))
            }
            "attachment.inspect" | "attachment.update_meta" => Ok(json!({
                "attachment": sample_attachment_json("shared", "organization", None)
            })),
            "attachment.read_chunk" => {
                let offset = params["offset"].as_u64().unwrap() as usize;
                let end = (offset + 10).min(self.download_bytes.len());
                let slice = &self.download_bytes[offset..end];
                let eof = end >= self.download_bytes.len();
                let sha = if self.tamper_sha {
                    "0".repeat(64)
                } else {
                    sha256_hex(&self.download_bytes)
                };
                Ok(json!({
                    "attachment_id": ATT_ID,
                    "offset": offset,
                    "data_base64": STANDARD.encode(slice),
                    "total_bytes": self.download_bytes.len(),
                    "content_sha256": sha,
                    "eof": eof
                }))
            }
            other => panic!("unexpected RPC method: {other}"),
        }
    }
}

fn make_sandbox() -> (tempfile::TempDir, FileRoots, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let workspace = dir.path().join("workspace");
    let private_home = dir.path().join("agents/agent-self/private_home");
    let worktree = dir.path().join("wt-1");
    let session_tmp = dir.path().join("session-tmp");
    fs::create_dir_all(&workspace).unwrap();
    fs::create_dir_all(&private_home).unwrap();
    fs::create_dir_all(&worktree).unwrap();
    fs::create_dir_all(&session_tmp).unwrap();

    let roots = FileRoots {
        cwd: workspace.clone(),
        workspace_roots: vec![RootEntry::from_path(workspace.clone()).unwrap()],
        private_home: RootEntry::from_path(private_home),
        worktrees: vec![RootEntry::from_path(worktree).unwrap()],
        session_tmpdir: RootEntry::from_path(session_tmp),
        self_agent_id: Some("agent-self".to_owned()),
    };
    (dir, roots, workspace)
}

fn run_cli(
    transport: FileMockTransport,
    args: &[&str],
) -> (org_command::Output, FileMockTransport) {
    let mut office = TransportOffice(transport);
    let owned: Vec<String> = args.iter().map(|s| (*s).to_owned()).collect();
    let out = crate::execute_with_office("agent-os", &owned, &mut office);
    (out, office.0)
}

#[test]
fn file_upload_list_inspect_fetch_update_round_trip() {
    let (_tmp, roots, workspace) = make_sandbox();
    fs::write(workspace.join("spec.md"), b"# Spec\nhello!\n").unwrap();

    with_test_roots(roots, || {
        // 1. Org upload (chunked over 8-byte chunks).
        let (out, mock) = run_cli(
            FileMockTransport::new(Some(EMP_ID)),
            &[
                "file",
                "upload",
                "--path",
                "spec.md",
                "--scope",
                "org",
                "--title",
                "Spec",
                "--description",
                "Main spec",
                "--status",
                "review",
            ],
        );
        assert_eq!(out.exit_code, 0, "{}", out.stdout);
        assert_eq!(mock.uploaded, b"# Spec\nhello!\n");
        let methods: Vec<&str> = mock.calls.iter().map(|(m, _)| m.as_str()).collect();
        assert_eq!(
            methods,
            vec![
                "attachment.upload_begin",
                "attachment.upload_chunk",
                "attachment.upload_chunk",
                "attachment.upload_commit",
                "attachment.attach",
            ]
        );
        let attach_params = &mock.calls.last().unwrap().1;
        assert_eq!(attach_params["target_kind"], "organization");
        assert!(attach_params.get("target_id").is_none());
        assert_eq!(attach_params["artifact_status"], "review");

        // 2. Project upload resolves slug -> project UUIDv7.
        let (out, mock) = run_cli(
            FileMockTransport::new(Some(EMP_ID)),
            &[
                "file",
                "upload",
                "--path",
                "spec.md",
                "--scope",
                "project",
                "--project",
                "alpha",
            ],
        );
        assert_eq!(out.exit_code, 0, "{}", out.stdout);
        assert_eq!(mock.calls[0].0, "project.inspect");
        let attach_params = &mock.calls.last().unwrap().1;
        assert_eq!(attach_params["target_kind"], "project");
        assert_eq!(attach_params["target_id"], PROJ_ID);

        // 3. Task upload resolves T-A1 -> task UUIDv7.
        let (out, mock) = run_cli(
            FileMockTransport::new(Some(EMP_ID)),
            &[
                "file", "upload", "--path", "spec.md", "--scope", "task", "--task", "T-A1",
            ],
        );
        assert_eq!(out.exit_code, 0, "{}", out.stdout);
        assert_eq!(mock.calls[0].0, "tasks.inspect");
        let attach_params = &mock.calls.last().unwrap().1;
        assert_eq!(attach_params["target_kind"], "task");
        assert_eq!(attach_params["target_id"], TASK_ID);

        // 4. List infers scope from --project / --task and defaults to org.
        let (out, mock) = run_cli(
            FileMockTransport::new(Some(EMP_ID)),
            &["file", "list", "--project", "alpha", "--limit", "5"],
        );
        assert_eq!(out.exit_code, 0, "{}", out.stdout);
        assert_eq!(mock.calls[1].0, "attachment.list");
        assert_eq!(mock.calls[1].1["target_kind"], "project");
        assert_eq!(mock.calls[1].1["target_id"], PROJ_ID);
        assert_eq!(mock.calls[1].1["limit"], 5);

        // 5. Inspect by UUIDv7.
        let (out, mock) = run_cli(
            FileMockTransport::new(Some(EMP_ID)),
            &["file", "inspect", "--id", ATT_ID],
        );
        assert_eq!(out.exit_code, 0, "{}", out.stdout);
        assert_eq!(mock.calls[0].0, "attachment.inspect");

        // 6. Fetch downloads multi-chunk, verifies SHA-256, and writes regular file.
        let (out, mock) = run_cli(
            FileMockTransport::new(Some(EMP_ID)),
            &["file", "fetch", "--id", ATT_ID, "--output", "fetched.md"],
        );
        assert_eq!(out.exit_code, 0, "{}", out.stdout);
        assert_eq!(mock.calls.len(), 3, "25 bytes / 10-byte chunks = 3 calls");
        assert_eq!(
            fs::read(workspace.join("fetched.md")).unwrap(),
            b"hello fetched attachment\n"
        );

        // 7. Fetch with tampered SHA-256 fails closed without creating the output file.
        let mut bad_sha = FileMockTransport::new(Some(EMP_ID));
        bad_sha.tamper_sha = true;
        let (out, _) = run_cli(
            bad_sha,
            &["file", "fetch", "--id", ATT_ID, "--output", "tampered.md"],
        );
        assert_ne!(out.exit_code, 0, "{}", out.stdout);
        assert!(!workspace.join("tampered.md").exists());

        // 8. Update metadata & reject empty update.
        let (out, mock) = run_cli(
            FileMockTransport::new(Some(EMP_ID)),
            &[
                "file", "update", "--id", ATT_ID, "--title", "Final", "--status", "final",
            ],
        );
        assert_eq!(out.exit_code, 0, "{}", out.stdout);
        assert_eq!(mock.calls[0].0, "attachment.update_meta");

        let (out, mock) = run_cli(
            FileMockTransport::new(Some(EMP_ID)),
            &["file", "update", "--id", ATT_ID],
        );
        assert_ne!(out.exit_code, 0, "{}", out.stdout);
        assert!(mock.calls.is_empty());

        // 9. Zero positional args rejected cleanly with zero RPCs.
        let (out, mock) = run_cli(
            FileMockTransport::new(Some(EMP_ID)),
            &["file", "upload", "spec.md"],
        );
        assert_ne!(out.exit_code, 0, "{}", out.stdout);
        assert!(mock.calls.is_empty());
    });
}

#[test]
fn agent_private_scope_is_rejected_with_dm_attach_hint_while_person_is_allowed() {
    let (_tmp, roots, workspace) = make_sandbox();
    fs::write(workspace.join("spec.md"), b"hello").unwrap();

    with_test_roots(roots, || {
        // Agent caller -> upload & list with `--scope private` fail closed before any RPC.
        for args in [
            vec!["file", "upload", "--path", "spec.md", "--scope", "private"],
            vec!["file", "list", "--scope", "private"],
        ] {
            let (out, mock) = run_cli(FileMockTransport::new(Some(EMP_ID)), &args);
            assert_eq!(out.exit_code, 1, "{}", out.stdout);
            assert!(mock.calls.is_empty(), "{args:?} must make zero RPCs");
            let env: Value = serde_json::from_str(&out.stdout).unwrap();
            assert_eq!(env["error"]["code"], "not_authorized");
            let hint = env["error"]["hint"].as_str().unwrap();
            assert!(
                hint.contains("1:1") && hint.contains("--attach"),
                "expected 1:1 DM --attach hint, got: {hint}"
            );
        }

        // Grant-bound person caller (`caller_member_id() == None`) -> private upload succeeds.
        let (out, mock) = run_cli(
            FileMockTransport::new(None),
            &["file", "upload", "--path", "spec.md", "--scope", "private"],
        );
        assert_eq!(out.exit_code, 0, "{}", out.stdout);
        assert_eq!(mock.calls.last().unwrap().1["target_kind"], "user_private");
    });
}

#[test]
fn negative_exfiltration_guard_blocks_all_forbidden_paths_and_types() {
    let (tmp, roots, workspace) = make_sandbox();

    // Legitimate files inside allowed roots succeed: workspace, own private_home, worktree, session TMPDIR.
    fs::write(workspace.join("ok.txt"), b"workspace ok").unwrap();
    let own_priv = tmp.path().join("agents/agent-self/private_home/note.md");
    fs::write(&own_priv, b"own private ok").unwrap();
    let wt_file = tmp.path().join("wt-1/patch.txt");
    fs::write(&wt_file, b"worktree ok").unwrap();
    let stmp_file = tmp.path().join("session-tmp/scratch.json");
    fs::write(&stmp_file, b"{\"ok\":true}").unwrap();

    assert!(read_upload_file("ok.txt", &roots).is_ok());
    assert!(read_upload_file(own_priv.to_str().unwrap(), &roots).is_ok());
    assert!(read_upload_file(wt_file.to_str().unwrap(), &roots).is_ok());
    assert!(read_upload_file(stmp_file.to_str().unwrap(), &roots).is_ok());

    // 1. /etc/passwd
    let err = read_upload_file("/etc/passwd", &roots).unwrap_err();
    assert_eq!(err.code, ErrorCode::NotAuthorized, "{err:?}");

    // 2. ../ traversal escaping workspace
    let outside = tmp.path().join("outside.txt");
    fs::write(&outside, b"secret outside").unwrap();
    let err = read_upload_file("../outside.txt", &roots).unwrap_err();
    assert_eq!(err.code, ErrorCode::NotAuthorized, "{err:?}");

    // 3. Escaping symlink & local symlink & intermediate directory symlink & outside symlink pointing in.
    let escape_link = workspace.join("escape.txt");
    org_platform::fs::create_symlink(&outside, &escape_link).unwrap();
    assert_eq!(
        read_upload_file("escape.txt", &roots).unwrap_err().code,
        ErrorCode::NotAuthorized
    );

    let local_link = workspace.join("local_link.txt");
    org_platform::fs::create_symlink(&workspace.join("ok.txt"), &local_link).unwrap();
    assert_eq!(
        read_upload_file("local_link.txt", &roots).unwrap_err().code,
        ErrorCode::NotAuthorized
    );

    let real_dir = workspace.join("real_dir");
    fs::create_dir_all(&real_dir).unwrap();
    fs::write(real_dir.join("inside.txt"), b"inside").unwrap();
    let link_dir = workspace.join("link_dir");
    org_platform::fs::create_symlink(&real_dir, &link_dir).unwrap();
    assert_eq!(
        read_upload_file("link_dir/inside.txt", &roots)
            .unwrap_err()
            .code,
        ErrorCode::NotAuthorized
    );

    let outside_ws_link = tmp.path().join("outside_ws_link");
    org_platform::fs::create_symlink(&workspace, &outside_ws_link).unwrap();
    assert_eq!(
        read_upload_file(outside_ws_link.join("ok.txt").to_str().unwrap(), &roots)
            .unwrap_err()
            .code,
        ErrorCode::NotAuthorized
    );

    // 4. Denylisted office internals even when nested inside an allowed workspace root!
    let denylisted_paths = [
        "durable/blobs/sha256/ab/cd.txt",
        "durable/private/task-notes.db",
        "durable/private/task-notes.db-wal",
        "durable/private/user-space.db",
        "durable/private/other.db",
        "durable/messaging/chat.db",
        "custom/private/user-space.db",
        "custom/private/task-notes.db-wal",
        "attachment-staging/0193a5c0/raw.txt",
        "blob-outbox/0193a5c0/raw.txt",
        "runtime/off-1/protected/secret.txt",
        "local/offices/off-1/agents/other-agent/secret.txt",
        "agents/other-agent/private_home/secret.txt",
        "agents/other-agent/home/secret.txt",
    ];
    for rel in denylisted_paths {
        let full = workspace.join(rel);
        fs::create_dir_all(full.parent().unwrap()).unwrap();
        fs::write(&full, b"sensitive").unwrap();
        let err = read_upload_file(rel, &roots).unwrap_err();
        assert_eq!(
            err.code,
            ErrorCode::NotAuthorized,
            "denylisted path `{rel}` must fail with NotAuthorized, got {err:?}"
        );
    }

    // 4b. Own `private_home/note.md` (with or without `private_home` root) and a repo folder
    //     named `private_home/` must NOT be false-positively denied.
    let repo_priv_note = workspace.join("private_home/note.md");
    fs::create_dir_all(repo_priv_note.parent().unwrap()).unwrap();
    fs::write(&repo_priv_note, b"repo private_home note").unwrap();
    assert!(read_upload_file("private_home/note.md", &roots).is_ok());

    let ws_own_agent_priv = workspace.join("agents/agent-self/private_home/note.md");
    fs::create_dir_all(ws_own_agent_priv.parent().unwrap()).unwrap();
    fs::write(&ws_own_agent_priv, b"self agent private_home note").unwrap();
    assert!(read_upload_file("agents/agent-self/private_home/note.md", &roots).is_ok());

    let mut roots_without_private_home = roots.clone();
    roots_without_private_home.private_home = None;
    assert!(read_upload_file("private_home/note.md", &roots_without_private_home).is_ok());
    assert!(read_upload_file(
        "agents/agent-self/private_home/note.md",
        &roots_without_private_home
    )
    .is_ok());

    // 5. Global /tmp is NEVER allowed (even if a file exists there).
    let global_tmp_file = tempfile::Builder::new()
        .suffix(".txt")
        .tempfile_in("/tmp")
        .unwrap();
    fs::write(global_tmp_file.path(), b"global tmp").unwrap();
    let err = read_upload_file(global_tmp_file.path().to_str().unwrap(), &roots).unwrap_err();
    assert_eq!(err.code, ErrorCode::NotAuthorized, "{err:?}");

    // 6. Directory, trailing `/` / `/.` / `.`, and real POSIX FIFO fail closed without blocking.
    assert!(read_upload_file("real_dir", &roots).is_err());
    assert!(read_upload_file("real_dir/", &roots).is_err());
    assert_eq!(
        read_upload_file(".", &roots).unwrap_err().code,
        ErrorCode::InvalidRequest
    );
    assert_eq!(
        read_upload_file("real_dir/.", &roots).unwrap_err().code,
        ErrorCode::InvalidRequest
    );
    assert_eq!(
        read_upload_file("ok.txt/.", &roots).unwrap_err().code,
        ErrorCode::InvalidRequest
    );
    assert_eq!(
        prepare_output_path(".", &roots).unwrap_err().code,
        ErrorCode::InvalidRequest
    );
    assert_eq!(
        prepare_output_path("out.txt/.", &roots).unwrap_err().code,
        ErrorCode::InvalidRequest
    );

    let fifo_path = workspace.join("pipe.txt");
    let status = Command::new("mkfifo")
        .arg(&fifo_path)
        .status()
        .expect("mkfifo available on POSIX");
    assert!(status.success());
    let fifo_err = read_upload_file("pipe.txt", &roots).unwrap_err();
    assert_eq!(fifo_err.code, ErrorCode::InvalidRequest, "{fifo_err:?}");

    // 7. Magic-byte spoofing & disallowed extensions rejected BEFORE any upload RPC.
    let bad_files: &[(&str, &[u8])] = &[
        ("evil.txt", b"\x7fELF\x02\x01\x01\x00binary"),
        ("xss.md", b"<!DOCTYPE html><script>alert(1)</script>"),
        (
            "vector.png",
            b"<svg xmlns=\"http://www.w3.org/2000/svg\"></svg>",
        ),
        ("vector.txt", b"<?xml version=\"1.0\"?><svg></svg>"),
        ("fake.png", b"not actually a png"),
        ("fake.pdf", b"not actually a pdf"),
        ("script.sh", b"#!/bin/sh\necho hi\n"),
    ];
    for (name, bytes) in bad_files {
        fs::write(workspace.join(name), bytes).unwrap();
        let err = read_upload_file(name, &roots).unwrap_err();
        assert_eq!(
            err.code,
            ErrorCode::Unsupported,
            "spoofed/disallowed file `{name}` must fail with Unsupported, got {err:?}"
        );
    }

    // 8. Oversized image (> 10 MiB) and document (> 25 MiB) rejected with TooLarge BEFORE RPC.
    let mut big_png = vec![0u8; (ATTACHMENT_IMAGE_MAX_BYTES as usize) + 1];
    big_png[..8].copy_from_slice(b"\x89PNG\r\n\x1a\n");
    fs::write(workspace.join("huge.png"), &big_png).unwrap();
    assert_eq!(
        read_upload_file("huge.png", &roots).unwrap_err().code,
        ErrorCode::TooLarge
    );

    let big_txt_path = workspace.join("huge.txt");
    {
        let f = fs::File::create(&big_txt_path).unwrap();
        f.set_len(ATTACHMENT_DOCUMENT_MAX_BYTES + 1).unwrap();
    }
    assert_eq!(
        read_upload_file("huge.txt", &roots).unwrap_err().code,
        ErrorCode::TooLarge
    );

    // 9. Verify CLI `file upload` makes ZERO RPC calls on any blocked path or oversized file.
    with_test_roots(roots.clone(), || {
        for bad in [
            "/etc/passwd",
            "../outside.txt",
            "escape.txt",
            "durable/blobs/sha256/ab/cd.txt",
            "durable/private/task-notes.db",
            "durable/private/user-space.db",
            "attachment-staging/0193a5c0/raw.txt",
            "agents/other-agent/private_home/secret.txt",
            "runtime/off-1/protected/secret.txt",
            "evil.txt",
            "huge.png",
        ] {
            let (out, mock) = run_cli(
                FileMockTransport::new(Some(EMP_ID)),
                &["file", "upload", "--path", bad, "--scope", "org"],
            );
            assert_ne!(out.exit_code, 0, "blocked path `{bad}` must fail");
            assert!(
                mock.calls.is_empty(),
                "blocked path `{bad}` must make ZERO RPCs, made: {:?}",
                mock.calls
            );
        }
    });

    // 10. Unsafe `file fetch --output` targets fail closed BEFORE calling `attachment.read_chunk`.
    for bad_out in [
        "/etc/evil.txt",
        "../escape.txt",
        "local_link.txt",
        "link_dir/out.txt",
        "durable/blobs/sha256/ab/out.txt",
        "/tmp/agentic_org_forbidden_fetch_out.txt",
    ] {
        assert!(
            prepare_output_path(bad_out, &roots).is_err(),
            "unsafe fetch output `{bad_out}` must be refused"
        );
        let mut mock = FileMockTransport::new(Some(EMP_ID));
        assert!(fetch_attachment_to_path(&mut mock, ATT_ID, bad_out, &roots).is_err());
        assert!(
            mock.calls.is_empty(),
            "unsafe fetch output `{bad_out}` must fail BEFORE calling attachment.read_chunk"
        );
    }
}

#[test]
fn file_roots_reject_global_temp_system_roots_symlinks_and_untrusted_env_wideners() {
    let dir = tempfile::tempdir().unwrap();
    let valid_home = dir.path().join("agents/agent-1/home");
    let valid_tmp = dir.path().join("session-tmp");
    fs::create_dir_all(&valid_home).unwrap();
    fs::create_dir_all(&valid_tmp).unwrap();

    // Symlink pointing at `/tmp` must be rejected by canonical check for every root slot.
    let link_to_tmp = dir.path().join("link-to-tmp");
    org_platform::fs::create_symlink(std::path::Path::new("/tmp"), &link_to_tmp).unwrap();
    let link_to_etc = dir.path().join("link-to-etc");
    org_platform::fs::create_symlink(std::path::Path::new("/etc"), &link_to_etc).unwrap();

    // Every global temp, root, and OS/system path is rejected as cwd, HOME, or TMPDIR.
    let forbidden_paths = [
        "/",
        "/tmp",
        "/private/tmp",
        "/var/tmp",
        "/private/var/tmp",
        "/etc",
        "/etc/ssl",
        "/var",
        "/private/var",
        "/usr",
        "/System",
        "/proc",
        "/sys",
        "/dev",
        "/home",
        "/Users",
    ];
    for bad in forbidden_paths {
        let p = PathBuf::from(bad);
        let roots = FileRoots::from_parts(
            p.clone(),
            Some("agent-1".to_owned()),
            Some(p.clone()),
            Some(p),
        );
        assert!(
            roots.workspace_roots.is_empty(),
            "forbidden cwd `{bad}` must never become a workspace root: {:?}",
            roots.workspace_roots
        );
        assert!(
            roots.private_home.is_none(),
            "forbidden HOME `{bad}` must never become private_home: {:?}",
            roots.private_home
        );
        assert!(
            roots.session_tmpdir.is_none(),
            "forbidden TMPDIR `{bad}` must never become session_tmpdir: {:?}",
            roots.session_tmpdir
        );
    }

    // Symlinks to `/tmp` or `/etc` are rejected for cwd, HOME, and TMPDIR.
    for link in [&link_to_tmp, &link_to_etc] {
        let roots = FileRoots::from_parts(
            link.clone(),
            Some("agent-1".to_owned()),
            Some(link.clone()),
            Some(link.clone()),
        );
        assert!(roots.workspace_roots.is_empty(), "{link:?}");
        assert!(roots.private_home.is_none(), "{link:?}");
        assert!(roots.session_tmpdir.is_none(), "{link:?}");
    }

    // Even when cwd=/tmp, HOME=/tmp, TMPDIR=/private/tmp, reading a file in /tmp fails closed.
    let global_tmp_file = tempfile::Builder::new()
        .suffix(".txt")
        .tempfile_in("/tmp")
        .unwrap();
    fs::write(global_tmp_file.path(), b"global tmp secret").unwrap();
    let poisoned = FileRoots::from_parts(
        PathBuf::from("/tmp"),
        Some("agent-1".to_owned()),
        Some(PathBuf::from("/tmp")),
        Some(PathBuf::from("/private/tmp")),
    );
    let err = read_upload_file(global_tmp_file.path().to_str().unwrap(), &poisoned).unwrap_err();
    assert_eq!(err.code, ErrorCode::NotAuthorized, "{err:?}");

    // `private_home` is derived from `HOME` ONLY when `AGENT_OS_AGENT_ID` is set.
    let without_agent = FileRoots::from_parts(
        valid_tmp.clone(),
        None,
        Some(valid_home.clone()),
        Some(valid_tmp.clone()),
    );
    assert!(without_agent.self_agent_id.is_none());
    assert!(without_agent.private_home.is_none());
    assert!(without_agent.session_tmpdir.is_some());

    let with_agent = FileRoots::from_parts(
        valid_tmp.clone(),
        Some("agent-1".to_owned()),
        Some(valid_home.clone()),
        Some(valid_tmp),
    );
    assert_eq!(
        with_agent.private_home.as_ref().map(|e| &e.canonical),
        Some(&valid_home.canonicalize().unwrap())
    );

    // Setting untrusted `AGENT_OS_WORKTREES` / `AGENT_OS_PRIVATE_HOME` env vars MUST have zero
    // effect on `FileRoots::from_env()` (both `/etc` and an unrelated valid dir are ignored).
    let rogue_dir = dir.path().join("rogue-widener");
    fs::create_dir_all(&rogue_dir).unwrap();
    let rogue_canon = rogue_dir.canonicalize().unwrap();
    let etc_canon = PathBuf::from("/etc").canonicalize().unwrap();

    struct EnvGuard {
        prev_wt: Option<std::ffi::OsString>,
        prev_home: Option<std::ffi::OsString>,
    }
    impl Drop for EnvGuard {
        fn drop(&mut self) {
            match &self.prev_wt {
                Some(v) => std::env::set_var("AGENT_OS_WORKTREES", v),
                None => std::env::remove_var("AGENT_OS_WORKTREES"),
            }
            match &self.prev_home {
                Some(v) => std::env::set_var("AGENT_OS_PRIVATE_HOME", v),
                None => std::env::remove_var("AGENT_OS_PRIVATE_HOME"),
            }
        }
    }
    let _guard = EnvGuard {
        prev_wt: std::env::var_os("AGENT_OS_WORKTREES"),
        prev_home: std::env::var_os("AGENT_OS_PRIVATE_HOME"),
    };
    let joined_wt = std::env::join_paths([PathBuf::from("/etc"), rogue_dir.clone()]).unwrap();
    std::env::set_var("AGENT_OS_WORKTREES", &joined_wt);
    std::env::set_var("AGENT_OS_PRIVATE_HOME", "/etc");
    let from_env = FileRoots::from_env();
    assert!(
        !from_env
            .worktrees
            .iter()
            .any(|e| e.canonical == etc_canon || e.canonical == rogue_canon),
        "AGENT_OS_WORKTREES must be completely ignored: {:?}",
        from_env.worktrees
    );
    assert_ne!(
        from_env.private_home.as_ref().map(|e| &e.canonical),
        Some(&etc_canon),
        "AGENT_OS_PRIVATE_HOME=/etc must be ignored"
    );
    std::env::set_var("AGENT_OS_PRIVATE_HOME", &rogue_dir);
    let from_env2 = FileRoots::from_env();
    assert_ne!(
        from_env2.private_home.as_ref().map(|e| &e.canonical),
        Some(&rogue_canon),
        "AGENT_OS_PRIVATE_HOME must be completely ignored"
    );
}

#[test]
fn linked_git_worktrees_and_case_insensitive_prefix_stripping_work_cleanly() {
    // 1. Linked git worktree layout: `.git` is a regular file (`gitdir: ...`) whose `commondir`
    //    points back to `<main>/.git`, listing sibling linked worktrees under `worktrees/*/gitdir`.
    let dir = tempfile::tempdir().unwrap();
    let main_git = dir.path().join("main/.git");
    let wt_a = dir.path().join("wt-a");
    let wt_b = dir.path().join("wt-b");
    fs::create_dir_all(main_git.join("worktrees/wt-a")).unwrap();
    fs::create_dir_all(main_git.join("worktrees/wt-b")).unwrap();
    fs::create_dir_all(main_git.join("worktrees/poison")).unwrap();
    fs::create_dir_all(&wt_a).unwrap();
    fs::create_dir_all(&wt_b).unwrap();

    fs::write(
        wt_a.join(".git"),
        format!("gitdir: {}\n", main_git.join("worktrees/wt-a").display()),
    )
    .unwrap();
    fs::write(main_git.join("worktrees/wt-a/commondir"), "../..\n").unwrap();
    fs::write(
        main_git.join("worktrees/wt-a/gitdir"),
        format!("{}\n", wt_a.join(".git").display()),
    )
    .unwrap();
    fs::write(
        main_git.join("worktrees/wt-b/gitdir"),
        format!("{}\n", wt_b.join(".git").display()),
    )
    .unwrap();
    fs::write(
        wt_b.join(".git"),
        format!("gitdir: {}\n", main_git.join("worktrees/wt-b").display()),
    )
    .unwrap();
    // Poisoned worktree pointing at `/tmp/.git` must still be rejected by forbidden-root screening.
    fs::write(main_git.join("worktrees/poison/gitdir"), "/tmp/.git\n").unwrap();

    let roots = FileRoots::from_parts(wt_a.clone(), None, None, None);
    let wt_b_canon = wt_b.canonicalize().unwrap();
    let tmp_canon = PathBuf::from("/tmp").canonicalize().unwrap();
    assert!(
        roots.worktrees.iter().any(|e| e.canonical == wt_b_canon),
        "sibling linked worktree wt-b must be discovered from commondir: {:?}",
        roots.worktrees
    );
    assert!(
        !roots.worktrees.iter().any(|e| e.canonical == tmp_canon),
        "poisoned /tmp worktree must be rejected: {:?}",
        roots.worktrees
    );

    fs::write(wt_b.join("sibling.md"), b"from sibling worktree").unwrap();
    assert!(read_upload_file(wt_b.join("sibling.md").to_str().unwrap(), &roots).is_ok());

    // 2. Case-insensitive prefix stripping for macOS APFS/HFS+ (tested deterministically on Linux).
    assert_eq!(
        strip_root_prefix_with_case(
            std::path::Path::new("/Users/Alice/Repo/Sub/Spec.md"),
            std::path::Path::new("/users/alice/repo"),
            true,
        ),
        Some(PathBuf::from("Sub/Spec.md"))
    );
    assert_eq!(
        strip_root_prefix_with_case(
            std::path::Path::new("/Users/Alice/Repo2/Spec.md"),
            std::path::Path::new("/users/alice/repo"),
            true,
        ),
        None
    );
    assert_eq!(
        strip_root_prefix_with_case(
            std::path::Path::new("/Users/Alice/Repo/Spec.md"),
            std::path::Path::new("/users/alice/repo"),
            false,
        ),
        None
    );
}
