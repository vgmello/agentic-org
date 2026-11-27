//! Tests of the contract's own code that run on every target. Those that need a real
//! adapter are in `adapter_tests.rs`.

#[test]
fn core_dump_limits_and_query_errors_fail_closed() {
    for (soft, hard) in [
        (1, 1),
        (0, 1),
        (1, 0),
        (libc::RLIM_INFINITY, libc::RLIM_INFINITY),
    ] {
        let error = super::check_core_limit(Ok(libc::rlimit {
            rlim_cur: soft,
            rlim_max: hard,
        }))
        .unwrap_err();
        assert!(error.to_string().contains("RLIMIT_CORE"));
    }
    super::check_core_limit(Ok(libc::rlimit {
        rlim_cur: 0,
        rlim_max: 0,
    }))
    .unwrap();
    let error =
        super::check_core_limit(Err(std::io::Error::from_raw_os_error(libc::EACCES))).unwrap_err();
    assert_eq!(error.raw_os_error(), Some(libc::EACCES));
}

#[test]
fn refuses_every_root_credential_and_setid_provenance() {
    assert!(!super::privileged_credentials(501, 501, 501, false));
    assert!(super::privileged_credentials(0, 501, 501, false));
    assert!(super::privileged_credentials(501, 0, 501, false));
    assert!(super::privileged_credentials(501, 501, 0, false));
    assert!(super::privileged_credentials(501, 501, 501, true));
}

#[test]
fn a_mark_variable_name_is_one_plain_environment_name() {
    assert_eq!(
        super::mark_prefix("ORG_SESSION_MARK").unwrap(),
        b"ORG_SESSION_MARK="
    );
    for name in ["", "A=B", "A\0B"] {
        let error = super::mark_prefix(name).unwrap_err();
        assert_eq!(error.kind(), std::io::ErrorKind::InvalidInput, "{name:?}");
    }
}

/// A refusing build is one whose adapter refuses the first step of [`super::validate`].
/// There every public function must return the same refusal, before it does anything.
#[test]
fn on_a_refusing_build_every_public_function_refuses_first() {
    use std::io;

    fn refusal<T>(result: io::Result<T>) -> bool {
        result.err().is_some_and(|error| {
            error.kind() == io::ErrorKind::Unsupported
                && error
                    .get_ref()
                    .and_then(|source| source.downcast_ref::<crate::Refused>())
                    == Some(&crate::Refused::unsupported("process"))
        })
    }

    if !refusal(super::native::uids()) {
        return;
    }
    // SAFETY: getpid has no arguments or side effects.
    let own = unsafe { libc::getpid() };
    assert!(refusal(super::validate()));
    assert!(refusal(super::Watch::new(own)));
    assert!(refusal(super::identity(own)));
    assert!(refusal(super::try_exit(own)));
    assert!(refusal(super::release_spawned(own, || panic!(
        "a refusing build asked about a child"
    ))));
    assert!(!super::is_exited_group_denial(
        &io::Error::from_raw_os_error(libc::EPERM)
    ));
    assert!(refusal(super::group_contains_only_leader(own)));
    assert!(refusal(super::spawn_piped(
        c"/bin/sh",
        &[c"/bin/sh".into()],
        &[],
        c"/"
    )));
    assert!(refusal(super::entropy(&mut [0; 32])));
    assert!(refusal(super::RecoveryScan::new("ORG_SESSION_MARK")));
    assert_eq!(super::process_tree_rss_bytes(own as u32), None);
}
