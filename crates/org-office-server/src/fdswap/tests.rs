use super::*;

#[test]
fn a_duplicate_is_above_2_and_close_on_exec_from_the_start() {
    let _spawn = crate::transfer::tests::without_process_spawn();
    // fd 2 (stderr) exists in every test process.
    let copy = dup_cloexec(2).unwrap();
    assert!(copy >= 3, "{copy}");
    assert!(is_cloexec(copy));
    assert!(!is_cloexec(2), "stderr must stay inheritable");
    // SAFETY: closing the descriptor this test just made.
    unsafe { libc::close(copy) };
}
