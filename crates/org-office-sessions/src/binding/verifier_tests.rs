//! Unit and hot-path latency tests for [`RunBindingVerifier`], [`PeerIdentity`], and
//! [`LocalPeerPidVerifier`] (`TASK-233`).

use std::sync::Arc;
use std::time::{Duration, Instant};

use super::{
    BindingRejection, LocalPeerPidVerifier, PeerIdentity, RunBindingRegistry, RunBindingVerifier,
    VerifiedRunBinding,
};

#[test]
fn peer_identity_fails_closed_on_non_positive_pids() {
    assert_eq!(PeerIdentity::none().local_pid(), None);
    assert_eq!(PeerIdentity::from_pid(None).local_pid(), None);
    assert_eq!(PeerIdentity::from_pid(Some(0)).local_pid(), None);
    assert_eq!(PeerIdentity::from_pid(Some(-1)).local_pid(), None);
    assert_eq!(PeerIdentity::from_raw_pid(None).local_pid(), None);
    assert_eq!(PeerIdentity::from_raw_pid(Some(0)).local_pid(), None);
    assert_eq!(PeerIdentity::from_raw_pid(Some(-42)).local_pid(), None);

    let self_pid = std::process::id();
    let from_i32 = PeerIdentity::from_pid(Some(self_pid as i32));
    assert_eq!(from_i32.local_pid(), Some(self_pid));
    let from_raw = PeerIdentity::from_raw_pid(Some(self_pid as libc::pid_t));
    assert_eq!(from_raw, from_i32);
}

#[test]
fn local_peer_pid_verifier_enforces_token_peer_descendant_and_revocation() {
    let reg = RunBindingRegistry::new();
    let self_pid = std::process::id() as libc::pid_t;
    let verifier: Arc<dyn RunBindingVerifier> = Arc::new(LocalPeerPidVerifier::new(reg.clone()));

    assert!(!verifier.has_active("emp-1"));
    let binding = reg
        .issue("emp-1", "run-1", "sess-1", self_pid, None)
        .unwrap();
    assert!(verifier.has_active("emp-1"));

    let self_peer = PeerIdentity::from_raw_pid(Some(self_pid));

    assert_eq!(
        verifier.verify("emp-1", None, &self_peer).unwrap_err(),
        BindingRejection::MissingToken
    );
    assert_eq!(
        verifier.verify("emp-1", Some(""), &self_peer).unwrap_err(),
        BindingRejection::MissingToken
    );
    assert_eq!(
        verifier
            .verify("emp-1", Some(binding.expose_token()), &PeerIdentity::none())
            .unwrap_err(),
        BindingRejection::MissingPeerPid
    );
    let wrong_same_len = "0".repeat(binding.expose_token().len());
    assert_eq!(
        verifier
            .verify("emp-1", Some(&wrong_same_len), &self_peer)
            .unwrap_err(),
        BindingRejection::UnknownToken
    );
    assert_eq!(
        verifier
            .verify("emp-other", Some(binding.expose_token()), &self_peer)
            .unwrap_err(),
        BindingRejection::UnknownToken
    );

    let verified = verifier
        .verify_binding("emp-1", Some(binding.expose_token()), &self_peer)
        .unwrap();
    assert!(matches!(
        verified,
        VerifiedRunBinding::Managed(ref active) if active.run_id == "run-1"
    ));

    let mut child = std::process::Command::new("/bin/sh")
        .args(["-c", "sleep 3"])
        .spawn()
        .unwrap();
    let child_peer = PeerIdentity::from_raw_pid(Some(child.id() as libc::pid_t));
    let child_ok = verifier.verify("emp-1", Some(binding.expose_token()), &child_peer);
    let _ = child.kill();
    let _ = child.wait();
    assert!(
        child_ok.is_ok(),
        "descendant child process must pass LocalPeerPidVerifier"
    );

    assert_eq!(
        verifier
            .verify(
                "emp-1",
                Some(binding.expose_token()),
                &PeerIdentity::from_pid(Some(1)),
            )
            .unwrap_err(),
        BindingRejection::NotDescendant
    );

    reg.revoke("emp-1", Some("run-1"));
    assert!(!verifier.has_active("emp-1"));
    assert_eq!(
        verifier
            .verify("emp-1", Some(binding.expose_token()), &self_peer)
            .unwrap_err(),
        BindingRejection::UnknownToken
    );
}

#[test]
fn local_peer_pid_verifier_hot_path_latency_within_doc7_budget() {
    const ITERATIONS: usize = 1_000;
    const MAX_MEDIAN: Duration = Duration::from_millis(1);
    const MAX_P95: Duration = Duration::from_millis(2);

    let reg = RunBindingRegistry::new();
    let self_pid = std::process::id() as libc::pid_t;
    let binding = reg
        .issue("emp-perf", "run-perf", "sess-perf", self_pid, None)
        .unwrap();
    let token = binding.expose_token().to_owned();
    let peer = PeerIdentity::from_raw_pid(Some(self_pid));
    let verifier = LocalPeerPidVerifier::new(reg);

    // 1. Injected descendant check (pure RAM + constant-time token compare).
    let mut injected_samples = Vec::with_capacity(ITERATIONS);
    for _ in 0..ITERATIONS {
        let start = Instant::now();
        let ok = verifier
            .verify_with("emp-perf", Some(&token), &peer, |p, r| p == r)
            .is_ok();
        let elapsed = start.elapsed();
        assert!(ok);
        injected_samples.push(elapsed);
    }
    injected_samples.sort_unstable();
    let injected_p50 = injected_samples[ITERATIONS / 2];
    let injected_p95 = injected_samples[(ITERATIONS * 95) / 100];
    assert!(
        injected_p50 <= MAX_MEDIAN && injected_p95 <= MAX_P95,
        "injected verify latency exceeded budget: p50={injected_p50:?}, p95={injected_p95:?}"
    );

    // 2. Real self-PID kernel check through the `RunBindingVerifier` trait.
    let mut real_samples = Vec::with_capacity(ITERATIONS);
    for _ in 0..ITERATIONS {
        let start = Instant::now();
        let ok = verifier.verify("emp-perf", Some(&token), &peer).is_ok();
        let elapsed = start.elapsed();
        assert!(ok);
        real_samples.push(elapsed);
    }
    real_samples.sort_unstable();
    let real_p50 = real_samples[ITERATIONS / 2];
    let real_p95 = real_samples[(ITERATIONS * 95) / 100];
    assert!(
        real_p50 <= MAX_MEDIAN && real_p95 <= MAX_P95,
        "real self-PID verify latency exceeded budget: p50={real_p50:?}, p95={real_p95:?}"
    );
}
