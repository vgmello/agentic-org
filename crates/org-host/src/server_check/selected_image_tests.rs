use super::*;
#[test]
fn selected_pin_checks_actual_child_kernel_hash_and_never_accepts_debug_skip() {
    let child = crate::spawn::spawn_suspended(&std::env::current_exe().unwrap(), &[], &[]).unwrap();
    let pid = child.process.id();
    let token = crate::spawn::audit_token_of_child(pid).unwrap();
    let expected = org_socket::image::loaded_cdhash_for_audit_token(&token).unwrap();
    let platform =
        ServerCheck::Scripted(Scripted(std::sync::Arc::new(move |_, _| Ok(Some(token)))));
    let pinned = platform.clone().pinned(expected);
    assert_eq!(pinned.admit(pid).unwrap(), Some(token));
    pinned.still(pid, Some(token)).unwrap();
    let mut different = expected;
    different[0] ^= 1;
    assert!(platform.pinned(different).admit(pid).is_err());
    assert!(ServerCheck::Skip.pinned(expected).admit(pid).is_err());
    drop(child); // kill and reap this generated suspended test image
    assert!(pinned.admit(pid).is_err());
    assert!(pinned.still(pid, Some(token)).is_err());
}
