use super::*;
use std::fs;
use std::os::unix::fs::{symlink, PermissionsExt};

fn write_script(dir: &Path, name: &str, body: &str) -> PathBuf {
    let path = dir.join(name);
    fs::write(&path, body).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
    path
}

#[test]
fn https_fetcher_rejects_non_https_and_malformed_urls_before_spawn() {
    let fetcher = HttpsReleaseFetcher::with_program(PathBuf::from("/nonexistent/never-called"));
    for bad in [
        "http://example.com/pointer.v1.json",
        "http://127.0.0.1:8080/pointer.v1.json",
        "file:///tmp/pointer.v1.json",
        "https://user:pass@example.com/pointer.v1.json",
        "-o/tmp/pwn",
        "https://example.com/../etc/passwd",
    ] {
        assert_eq!(
            fetcher
                .fetch_bytes(bad, 1024, Duration::from_secs(5))
                .unwrap_err(),
            FetchError::InvalidUrl,
            "{bad}"
        );
    }
}

#[test]
fn https_fetcher_enforces_byte_cap_timeout_and_redacts_stderr() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();

    // 1. Overrun beyond max_bytes is rejected and kills child.
    let big = write_script(&root, "big.sh", "#!/bin/sh\nprintf '0123456789ABCDEF'\n");
    let fetcher = HttpsReleaseFetcher::with_program(big);
    assert_eq!(
        fetcher
            .fetch_bytes("https://example.com/p.json", 8, Duration::from_secs(5))
            .unwrap_err(),
        FetchError::TooLarge { limit: 8 }
    );

    // 2. Wall-clock timeout kills hung fetcher and returns TimedOut.
    let slow = write_script(&root, "slow.sh", "#!/bin/sh\nsleep 10\n");
    let fetcher = HttpsReleaseFetcher::with_program(slow);
    assert!(matches!(
        fetcher
            .fetch_bytes("https://example.com/p.json", 64, Duration::from_millis(150))
            .unwrap_err(),
        FetchError::TimedOut { .. }
    ));

    // 3. Stderr secrets are redacted before leaving the fetcher boundary.
    let bearer_val = ["Super", "Secret", "Token", "999"].concat();
    let user_pass = ["s3cr", "3t"].concat();
    let query_tok = ["ghp_", "1234567890", "abcdef"].concat();
    let fail_script = format!(
        "#!/bin/sh\necho 'Authorization: Bearer {bearer_val} https://alice:{user_pass}@example.com/x?token={query_tok}' >&2\nexit 22\n"
    );
    let failing = write_script(&root, "fail.sh", &fail_script);
    let fetcher = HttpsReleaseFetcher::with_program(failing);
    let err = fetcher
        .fetch_bytes("https://example.com/p.json", 64, Duration::from_secs(5))
        .unwrap_err();
    let msg = err.to_string();
    assert!(!msg.contains(&bearer_val), "{msg}");
    assert!(!msg.contains(&user_pass), "{msg}");
    assert!(!msg.contains(&query_tok), "{msg}");
    assert!(msg.contains("[REDACTED]"), "{msg}");
}

#[test]
fn https_fetcher_passes_q_first_and_globoff_before_double_dash() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let dump_args = write_script(&root, "dump-args.sh", "#!/bin/sh\nprintf '%s\\n' \"$@\"\n");
    let fetcher = HttpsReleaseFetcher::with_program(dump_args);
    let url = "https://releases.example.com/v1/release-pointer.v1.json";
    let out = fetcher
        .fetch_bytes(url, 4096, Duration::from_secs(5))
        .unwrap();
    let args: Vec<&str> = std::str::from_utf8(&out).unwrap().lines().collect();
    assert_eq!(
        args.first().copied(),
        Some("-q"),
        "curl requires -q as the very first arg to disable ~/.curlrc: {args:?}"
    );
    let globoff_pos = args
        .iter()
        .position(|a| *a == "--globoff")
        .expect("curl invocation must include --globoff");
    let dd_pos = args
        .iter()
        .position(|a| *a == "--")
        .expect("curl invocation must include --");
    assert!(globoff_pos < dd_pos, "--globoff must precede --: {args:?}");
    assert_eq!(args.get(dd_pos + 1).copied(), Some(url));
}

#[test]
fn child_reap_guard_kills_and_reaps_child_on_drop() {
    let child = Command::new("/bin/sh")
        .args(["-c", "sleep 30"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let guard = ChildReapGuard::new(child);
    let slot = Arc::clone(guard.slot());
    drop(guard);
    assert!(
        slot.lock().unwrap().is_none(),
        "dropping ChildReapGuard must kill and reap the spawned child"
    );
}

#[test]
fn local_directory_fetcher_refuses_symlinks_traversal_and_oversized_files() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let mirror = root.join("mirror");
    let outside = root.join("outside.txt");
    fs::create_dir(&mirror).unwrap();
    fs::write(&outside, b"secret-outside").unwrap();
    fs::write(mirror.join("ok.bin"), b"hello").unwrap();
    symlink(&outside, mirror.join("link.bin")).unwrap();

    let fetcher = LocalDirectoryFetcher::new(&mirror).unwrap();
    let ok_url = format!("file://{}/ok.bin", mirror.display());
    assert_eq!(
        fetcher
            .fetch_bytes(&ok_url, 16, Duration::from_secs(1))
            .unwrap(),
        b"hello"
    );

    // Oversized file fails closed.
    assert_eq!(
        fetcher
            .fetch_bytes(&ok_url, 3, Duration::from_secs(1))
            .unwrap_err(),
        FetchError::TooLarge { limit: 3 }
    );

    // Symlink inside mirror is refused.
    let link_url = format!("file://{}/link.bin", mirror.display());
    assert!(fetcher
        .fetch_bytes(&link_url, 64, Duration::from_secs(1))
        .is_err());

    // Outside mirror & traversal URLs are refused.
    let outside_url = format!("file://{}", outside.display());
    assert_eq!(
        fetcher
            .fetch_bytes(&outside_url, 64, Duration::from_secs(1))
            .unwrap_err(),
        FetchError::InvalidUrl
    );
    let traversal_url = format!("file://{}/../outside.txt", mirror.display());
    assert_eq!(
        fetcher
            .fetch_bytes(&traversal_url, 64, Duration::from_secs(1))
            .unwrap_err(),
        FetchError::InvalidUrl
    );
}
