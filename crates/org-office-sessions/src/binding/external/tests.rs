use std::time::{Duration, Instant};

use crate::binding::{BindingMode, BindingRejection, RunBindingRegistry};

#[test]
fn external_tokens_live_apart_from_managed_runs() {
    let reg = RunBindingRegistry::new();
    let managed = reg.issue("emp-1", "run-1", "sess-1", 4242, None).unwrap();
    let external = reg.issue_external("emp-1", "ext-1", u64::MAX).unwrap();
    assert_ne!(managed.expose_token(), external.expose_token());
    assert!(external.is_external() && external.root_pid().is_none());
    assert_eq!(managed.root_pid(), Some(4242));

    // A managed teardown never revokes the external lease's token, and the reverse.
    reg.revoke("emp-1", None);
    assert!(!reg.has_active("emp-1"));
    assert!(reg.has_active_external("emp-1", 1));
    let managed = reg.issue("emp-1", "run-2", "sess-2", 4242, None).unwrap();
    assert!(reg.revoke_external("emp-1", None));
    assert!(reg.has_active("emp-1"));
    assert!(!reg.has_active_external("emp-1", 1));

    // A managed token never verifies as external and an external one never as managed.
    let external = reg.issue_external("emp-1", "ext-2", u64::MAX).unwrap();
    assert_eq!(
        reg.verify_external("emp-1", Some(managed.expose_token()), 1)
            .unwrap_err(),
        BindingRejection::UnknownToken
    );
    let pid = std::process::id() as libc::pid_t;
    assert_eq!(
        reg.verify_peer_with("emp-1", Some(external.expose_token()), Some(pid), |_, _| {
            true
        })
        .unwrap_err(),
        BindingRejection::UnknownToken
    );
    // Both are scrubbed by redaction.
    let tokens = reg.active_tokens();
    assert!(tokens.contains(&managed.expose_token().to_owned()));
    assert!(tokens.contains(&external.expose_token().to_owned()));
}

#[test]
fn external_verification_is_token_exact_and_expires() {
    let reg = RunBindingRegistry::new();
    let binding = reg.issue_external("emp-1", "ext-1", 1_000).unwrap();
    let token = binding.expose_token().to_owned();
    assert_eq!(
        reg.verify_external("emp-1", None, 10).unwrap_err(),
        BindingRejection::MissingToken
    );
    assert_eq!(
        reg.verify_external("emp-1", Some(""), 10).unwrap_err(),
        BindingRejection::MissingToken
    );
    assert_eq!(
        reg.verify_external("emp-2", Some(&token), 10).unwrap_err(),
        BindingRejection::UnknownToken
    );
    let mut flipped = token.clone().into_bytes();
    flipped[0] = if flipped[0] == b'a' { b'b' } else { b'a' };
    assert_eq!(
        reg.verify_external("emp-1", Some(std::str::from_utf8(&flipped).unwrap()), 10)
            .unwrap_err(),
        BindingRejection::UnknownToken
    );
    assert_eq!(
        reg.verify_external("emp-1", Some(&token[..token.len() - 1]), 10)
            .unwrap_err(),
        BindingRejection::UnknownToken
    );
    let ok = reg.verify_external("emp-1", Some(&token), 999).unwrap();
    assert_eq!(ok.session_id, "ext-1");
    assert_eq!(
        ok.mode,
        BindingMode::External {
            expires_at_ms: 1_000
        }
    );
    assert_eq!(
        reg.verify_external("emp-1", Some(&token), 1_000)
            .unwrap_err(),
        BindingRejection::Expired
    );
    assert!(reg.external_active("emp-1", "ext-1", 999));
    assert!(!reg.external_active("emp-1", "ext-1", 1_000));
    assert!(!reg.external_active("emp-1", "ext-other", 10));
}

#[test]
fn reissuing_supersedes_the_old_token_and_scoped_revoke_checks_the_lease() {
    let reg = RunBindingRegistry::new();
    let old = reg.issue_external("emp-1", "ext-1", u64::MAX).unwrap();
    let new = reg.issue_external("emp-1", "ext-2", u64::MAX).unwrap();
    assert!(reg
        .verify_external("emp-1", Some(old.expose_token()), 1)
        .is_err());
    assert!(reg
        .verify_external("emp-1", Some(new.expose_token()), 1)
        .is_ok());
    assert!(!reg.revoke_external("emp-1", Some("ext-1")));
    assert!(reg.has_active_external("emp-1", 1));
    assert!(reg.revoke_external("emp-1", Some("ext-2")));
    assert!(!reg.has_active_external("emp-1", 1));
}

#[test]
fn accepted_commands_fall_through_to_the_external_binding() {
    let reg = RunBindingRegistry::new();
    assert_eq!(reg.record_accepted_command("emp-1", "who", "who"), None);
    reg.issue_external("emp-1", "ext-1", u64::MAX).unwrap();
    assert!(matches!(
        reg.record_accepted_command("emp-1", "tasks.list", "tasks.list"),
        Some(BindingMode::External { .. })
    ));
    assert_eq!(reg.external_accepted_commands("emp-1").len(), 1);
    assert!(reg.accepted_commands("emp-1").is_empty());
}

/// The hot path of an externally bound command is one RAM lookup and one constant-time compare:
/// no I/O and no signature. Generous bounds keep this stable on a loaded CI machine while still
/// failing loudly if a disk read or a crypto verify ever lands on the path.
#[test]
fn hot_path_external_verification_is_sub_millisecond() {
    let reg = RunBindingRegistry::new();
    for i in 0..64 {
        reg.issue_external(format!("emp-{i}"), format!("ext-{i}"), u64::MAX)
            .unwrap();
    }
    let binding = reg.issue_external("emp-x", "ext-x", u64::MAX).unwrap();
    let token = binding.expose_token().to_owned();
    let mut samples = Vec::with_capacity(2_000);
    for _ in 0..2_000 {
        let start = Instant::now();
        assert!(reg.verify_external("emp-x", Some(&token), 1).is_ok());
        assert!(reg.external_active("emp-x", "ext-x", 1));
        samples.push(start.elapsed());
    }
    samples.sort();
    let median = samples[samples.len() / 2];
    let p95 = samples[samples.len() * 95 / 100];
    assert!(median < Duration::from_millis(1), "median {median:?}");
    assert!(p95 < Duration::from_millis(2), "p95 {p95:?}");
}
