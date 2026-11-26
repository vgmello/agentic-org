use super::config::root_refusal;
use super::*;

#[test]
fn only_root_is_refused_the_service() {
    let refusal = root_refusal(0).unwrap_err();
    assert!(refusal.contains("does not run as root"), "{refusal}");
    assert!(refusal.contains("service user"), "{refusal}");
    for user in [1, 501, 1000, 61000, u32::MAX] {
        assert_eq!(root_refusal(user), Ok(()), "{user}");
    }
    // The composition's check is this rule for the process's own effective user.
    assert_eq!(Config::refuse_root().is_err(), home::euid() == 0);
}

/// Set in the child of [`a_host_start_closes_the_process_before_it_reads_a_key`].
#[cfg(debug_assertions)]
const START_CHILD: &str = "ORG_HOST_TEST_START_CHILD";

/// A host start closes its process to the other processes of its user before it reaches
/// its key store: a start that is refused before any store is chosen (a relative home)
/// has already done it. A child of the test starts the host, so the test binary itself is
/// not changed for the other tests.
#[cfg(debug_assertions)]
#[test]
fn a_host_start_closes_the_process_before_it_reads_a_key() {
    use org_platform::process::testing::hardened;
    if std::env::var_os(START_CHILD).is_some() {
        // A fresh process is not closed, unless its environment already forbids core
        // dumps on a system where that is all there is to it.
        if hardened().unwrap() {
            println!("this process was closed before the start: only the end is shown");
        }
        // No development key folder is set: reaching the key store would say so.
        let refused = Host::start(Config::new("relative/home", "/nonexistent/worker"));
        assert!(
            matches!(refused, Err(HostError::RelativeHome { .. })),
            "{:?}",
            refused.err().map(|e| e.to_string())
        );
        assert!(hardened().unwrap(), "the start did not close the process");
        println!("closed before the key store");
        return;
    }
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "tests::a_host_start_closes_the_process_before_it_reads_a_key",
            "--nocapture",
            "--test-threads=1",
        ])
        .env(START_CHILD, "1")
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(
        output.status.success() && text.contains("closed before the key store"),
        "{text}\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
}
