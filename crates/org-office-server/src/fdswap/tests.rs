use super::*;
use std::os::fd::AsRawFd;

#[test]
fn a_duplicate_is_above_2_and_close_on_exec_from_the_start() {
    let _spawn = crate::transfer::tests::without_process_spawn();
    // fd 2 (stderr) exists in every test process.
    let copy = dup_cloexec(2).unwrap();
    assert!(copy.as_raw_fd() >= 3, "{copy:?}");
    assert!(is_cloexec(copy.as_raw_fd()));
    assert!(!is_cloexec(2), "stderr must stay inheritable");
    drop(copy);
}
