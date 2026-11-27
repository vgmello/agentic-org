//! Office-owned sub-agents (`SUB-1..7`) and run-binding token enforcement (`CLI-12`).

use super::*;

// ---------------------------------------------------------------------------
// SUB-1..5, SUB-7: Sub-agents & background task tracking/reaping
// ---------------------------------------------------------------------------

#[test]
fn subagents_enforce_tool_subset_usage_attribution_and_reaping() {
    let dir = tempfile::tempdir().unwrap();
    let parent = vec![
        ToolName::try_from("Read").unwrap(),
        ToolName::try_from("Grep").unwrap(),
    ];
    let requested = vec![
        ToolName::try_from("Read").unwrap(),
        ToolName::try_from("Bash").unwrap(),
    ];
    let sub = subset_tools(Some(&parent), Some(&requested)).unwrap();
    assert_eq!(sub, vec![ToolName::try_from("Read").unwrap()]);

    let def = build_general_subagent_definition(Some(&parent), Some(&requested));
    let written = write_subagent_definition(dir.path(), &def).unwrap();
    assert!(written.exists());
    let loaded: serde_json::Value = serde_json::from_slice(&fs::read(&written).unwrap()).unwrap();
    assert_eq!(loaded["tools"], serde_json::json!(["Read"]));

    let mut bg = BackgroundRegistry::new();
    bg.register(
        "sub-1",
        BackgroundKind::Subagent,
        "research",
        Some(1234),
        "t0",
    );
    bg.register(
        "bg-2",
        BackgroundKind::BackgroundTask,
        "build",
        Some(1235),
        "t0",
    );
    assert_eq!(bg.active_count(), 2);

    let mut u = Usage::new(UsageScope::TurnDelta, UsageScope::TurnDelta);
    u.input = 100;
    u.output = 50;
    bg.record_usage(&u);
    assert_eq!(bg.usage().input_tokens, 100);
    assert_eq!(bg.usage().output_tokens, 50);
    assert_eq!(bg.usage().subagent_invocations, 1);

    // Completing sub-1 emits a wake across turns while bg-2 stays running.
    let wake = bg.complete("sub-1", true, "found 3 files", "t1").unwrap();
    assert_eq!(wake.task_id, "sub-1");
    assert_eq!(bg.active_count(), 1);
    assert_eq!(bg.drain_wakes().len(), 1);

    // Pause/Stop/Offboard/Stall reaps remaining bg-2.
    let reaped = bg.reap_all("t2");
    assert_eq!(reaped, vec![("bg-2".to_owned(), Some(1235))]);
    assert_eq!(bg.active_count(), 0);
    assert!(bg
        .views()
        .iter()
        .any(|v| v.task_id == "bg-2" && v.state == "reaped"));
}

// ---------------------------------------------------------------------------
// CLI-12, SUB-3: Run binding token & descendant verification
// ---------------------------------------------------------------------------

#[test]
fn run_binding_enforces_token_descendant_and_revocation() {
    let reg = RunBindingRegistry::new();
    let self_pid = std::process::id() as libc::pid_t;
    let binding = reg
        .issue("emp-1", "run-1", "sess-1", self_pid, None)
        .unwrap();
    assert!(!format!("{binding:?}").contains(binding.expose_token()));

    // Missing token -> refused
    assert_eq!(
        reg.verify_peer("emp-1", None, Some(self_pid)).unwrap_err(),
        BindingRejection::MissingToken
    );
    // Wrong token -> refused
    assert_eq!(
        reg.verify_peer("emp-1", Some("wrong"), Some(self_pid))
            .unwrap_err(),
        BindingRejection::UnknownToken
    );
    // Valid token + self PID -> accepted
    assert!(reg
        .verify_peer("emp-1", Some(binding.expose_token()), Some(self_pid))
        .is_ok());
    // Valid token + real child process (sub-agent / child shell, SUB-3) -> accepted
    let mut child = std::process::Command::new("/bin/sh")
        .args(["-c", "sleep 3"])
        .spawn()
        .unwrap();
    let child_pid = child.id() as libc::pid_t;
    let child_ok = reg.verify_peer("emp-1", Some(binding.expose_token()), Some(child_pid));
    let _ = child.kill();
    let _ = child.wait();
    assert!(child_ok.is_ok(), "child process must pass descendant check");

    // Non-descendant PID (init pid 1) -> refused
    assert_eq!(
        reg.verify_peer("emp-1", Some(binding.expose_token()), Some(1))
            .unwrap_err(),
        BindingRejection::NotDescendant
    );

    // Revocation invalidates token immediately
    reg.revoke("emp-1", Some("run-1"));
    assert_eq!(
        reg.verify_peer("emp-1", Some(binding.expose_token()), Some(self_pid))
            .unwrap_err(),
        BindingRejection::UnknownToken
    );
}
