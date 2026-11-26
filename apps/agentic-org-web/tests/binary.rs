use std::process::Command;

#[test]
fn web_entrypoint_requires_production_handoff_before_opening_a_listener() {
    let sandbox = tempfile::tempdir().unwrap();
    for args in [
        vec![],
        vec!["--listen", "0.0.0.0:8443"],
        vec!["--serve", "extra"],
    ] {
        let result = Command::new(env!("CARGO_BIN_EXE_agentic-org-web"))
            .args(&args)
            .current_dir(sandbox.path())
            .output()
            .unwrap();
        // A command line it does not take is a usage error (exit 2), and opens nothing.
        assert_eq!(result.status.code(), Some(2), "{args:?}");
        assert!(String::from_utf8_lossy(&result.stderr).contains("no listener was opened"));
        assert!(result.stdout.is_empty());
    }
}

#[test]
fn help_and_version_answer_and_a_closed_standard_output_exits_0() {
    for (flag, expected) in [
        ("--help", "Usage: agentic-org-web --serve"),
        ("-h", "Usage: agentic-org-web --serve"),
        ("--version", "agentic-org-web "),
        ("-V", "agentic-org-web "),
    ] {
        let result = Command::new(env!("CARGO_BIN_EXE_agentic-org-web"))
            .arg(flag)
            .output()
            .unwrap();
        assert_eq!(result.status.code(), Some(0), "{flag}");
        assert!(
            String::from_utf8_lossy(&result.stdout).contains(expected),
            "{flag}"
        );
        assert!(result.stdout.ends_with(b"\n"), "{flag}");

        let (reader, writer) = std::io::pipe().unwrap();
        drop(reader);
        let closed = Command::new(env!("CARGO_BIN_EXE_agentic-org-web"))
            .arg(flag)
            .stdout(writer)
            .output()
            .unwrap();
        assert_eq!(closed.status.code(), Some(0), "{flag}: {closed:?}");
        assert!(closed.stderr.is_empty(), "{flag}");
    }
}
