//! CLI migration checks never use the caller's live office or spool.
use std::process::Command;

#[test]
fn help_and_version_do_not_start_a_host() {
    let sandbox = tempfile::tempdir().unwrap();
    for args in [
        vec![],
        vec!["--help"],
        vec!["--version"],
        vec!["serve", "--help"],
        vec!["serve", "--version"],
    ] {
        let result = Command::new(env!("CARGO_BIN_EXE_agentic-org"))
            .args(args)
            .current_dir(sandbox.path())
            .env("ORG_OFFICE_HOME", sandbox.path().join("office"))
            .env("ORG_OFFICE_DEV_KEY_DIR", sandbox.path().join("keys"))
            .output()
            .unwrap();
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        assert!(String::from_utf8_lossy(&result.stdout).contains("agentic-org"));
        assert_eq!(std::fs::read_dir(sandbox.path()).unwrap().count(), 0);
    }
}

#[test]
fn unsupported_bootstrap_never_creates_partial_offices_or_service_state() {
    let sandbox = tempfile::tempdir().unwrap();
    for args in [
        vec!["setup"],
        vec!["setup", "--offline"],
        vec!["setup", "--hq", "https://hq.invalid"],
        vec!["--server"],
    ] {
        let result = Command::new(env!("CARGO_BIN_EXE_agentic-org"))
            .args(args)
            .current_dir(sandbox.path())
            .env("ORG_OFFICE_HOME", sandbox.path().join("office"))
            .env("ORG_OFFICE_DEV_KEY_DIR", sandbox.path().join("keys"))
            .output()
            .unwrap();
        assert!(!result.status.success());
        assert!(!String::from_utf8_lossy(&result.stdout).contains("ready"));
        assert_eq!(std::fs::read_dir(sandbox.path()).unwrap().count(), 0);
    }
}

#[test]
fn release_layout_has_four_production_executables_and_a_library_only_host() {
    let metadata = Command::new(env!("CARGO"))
        .current_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."))
        .args([
            "metadata",
            "--format-version",
            "1",
            "--no-deps",
            "--locked",
            "--offline",
        ])
        .output()
        .unwrap();
    assert!(
        metadata.status.success(),
        "{}",
        String::from_utf8_lossy(&metadata.stderr)
    );
    let metadata: serde_json::Value = serde_json::from_slice(&metadata.stdout).unwrap();
    let mut binaries = Vec::new();
    for package in metadata["packages"].as_array().unwrap() {
        for target in package["targets"].as_array().unwrap() {
            if target["kind"] == serde_json::json!(["bin"])
                && target.get("required-features").is_none()
            {
                assert_ne!(package["name"], "org-office-host", "host must be a library");
                binaries.push(target["name"].as_str().unwrap().to_owned());
            }
        }
    }
    binaries.sort();
    assert_eq!(
        binaries,
        [
            "agent-os",
            "agentic-org",
            "agentic-org-office",
            "agentic-org-web"
        ]
    );
}
