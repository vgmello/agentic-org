//! Shared fixture cleanup policy for library and process tests.
pub(crate) fn teardown_failed(error: std::io::Error) {
    if !std::thread::panicking() {
        panic!("helpers must finish before the case fence is released: {error}");
    }
}

#[test]
fn cleanup_error_preserves_an_active_panic_but_fails_outside_unwinding() {
    let error = std::panic::catch_unwind(|| {
        teardown_failed(std::io::Error::other("controlled-helper did not finish"));
    })
    .expect_err("normal teardown error must fail the case");
    let message = error
        .downcast_ref::<String>()
        .map(String::as_str)
        .or_else(|| error.downcast_ref::<&str>().copied())
        .expect("teardown panic must explain the helper error");
    assert!(message.contains("controlled-helper did not finish"));

    struct DuringPanic<'a>(&'a std::cell::Cell<bool>);
    impl Drop for DuringPanic<'_> {
        fn drop(&mut self) {
            // Catch inside this Drop so a counterfactual reports an assertion failure,
            // rather than aborting the test process through a second escaping panic.
            let result = std::panic::catch_unwind(|| {
                teardown_failed(std::io::Error::other("controlled-helper did not finish"));
            });
            self.0.set(result.is_ok());
        }
    }
    let preserved = std::cell::Cell::new(false);
    let original = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _cleanup = DuringPanic(&preserved);
        panic!("original case failure");
    }))
    .expect_err("original case failure must still propagate");
    assert_eq!(
        original.downcast_ref::<&str>(),
        Some(&"original case failure")
    );
    assert!(
        preserved.get(),
        "cleanup attempted a second panic during unwinding"
    );
}
