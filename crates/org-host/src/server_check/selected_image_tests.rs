use super::*;
#[test]
fn selected_pin_checks_actual_child_kernel_hash_and_never_accepts_debug_skip() {
    let child = crate::spawn::spawn_suspended(&std::env::current_exe().unwrap(), &[], &[]).unwrap();
    let pid = child.process.id();
    let token = crate::spawn::audit_token_of_child(pid).unwrap();
    let expected = org_platform::image::loaded_cdhash_for_audit_token(&token).unwrap();
    let platform =
        ServerCheck::Scripted(Scripted(std::sync::Arc::new(move |_, _| Ok(Some(token)))));
    let pinned = platform.clone().pinned(expected);
    assert_eq!(pinned.admit(pid).unwrap(), Some(token));
    pinned.still(pid, Some(token)).unwrap();
    let mut different = expected;
    different[0] ^= 1;
    assert!(platform.clone().pinned(different).admit(pid).is_err());
    assert!(ServerCheck::Skip.pinned(expected).admit(pid).is_err());
    // The pre-first-line re-check refuses a different image, and the inner check's refusal.
    assert!(platform
        .clone()
        .pinned(different)
        .still(pid, Some(token))
        .is_err());
    let mut other = token;
    other[0] ^= 1;
    let changed = ServerCheck::Scripted(Scripted(std::sync::Arc::new(move |step, _| {
        Ok(Some(if step == Step::Still { other } else { token }))
    })));
    assert!(changed.pinned(expected).still(pid, Some(token)).is_err());
    drop(child); // kill and reap this generated suspended test image
    assert!(pinned.admit(pid).is_err());
    assert!(pinned.still(pid, Some(token)).is_err());
}

#[test]
fn a_refused_or_pinned_check_never_admits_through_the_plain_decision() {
    let never = || -> Result<AuditToken, String> { panic!("asked for a token") };
    assert!(ServerCheck::WebRefused("refused")
        .admit(std::process::id())
        .is_err());
    assert!(admit(
        &ServerCheck::WebRefused("refused"),
        never,
        |_, _| panic!("signature"),
        |_| panic!("keychain"),
    )
    .is_err());
    // A pinned check is decided only by the live kernel-image adapter.
    assert!(admit(
        &ServerCheck::Skip.pinned([0; 20]),
        never,
        |_, _| panic!("signature"),
        |_| panic!("keychain"),
    )
    .is_err());
}
