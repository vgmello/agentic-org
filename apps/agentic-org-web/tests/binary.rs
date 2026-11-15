use std::process::Command;

#[test]
fn web_entrypoint_requires_production_handoff_before_opening_a_listener() {
    let sandbox = tempfile::tempdir().unwrap();
    for args in [vec![], vec!["--listen", "0.0.0.0:8443"]] {
        let result = Command::new(env!("CARGO_BIN_EXE_agentic-org-web"))
            .args(args)
            .current_dir(sandbox.path())
            .output()
            .unwrap();
        assert!(!result.status.success());
        assert!(String::from_utf8_lossy(&result.stderr).contains("no listener was opened"));
        assert!(result.stdout.is_empty());
    }
}
