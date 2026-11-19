// Test-only startup hook, before libtest creates any threads. Lowering hard0 is
// irreversible for this process and its children; never restore/change it in a test.
// Each integration test binary and the library's unit-test binary include this module.
#[used]
#[cfg_attr(target_os = "macos", unsafe(link_section = "__DATA,__mod_init_func"))]
#[cfg_attr(target_os = "linux", unsafe(link_section = ".init_array"))]
static INITIALIZE: unsafe extern "C" fn() = disable_core_dumps;

unsafe extern "C" fn disable_core_dumps() {
    let limit = libc::rlimit {
        rlim_cur: 0,
        rlim_max: 0,
    };
    // SAFETY: valid limit record. No allocation/panic or Rust runtime is needed here.
    if unsafe { libc::setrlimit(libc::RLIMIT_CORE, &limit) } != 0 {
        let message = b"org-supervise tests: cannot disable core dumps before harness startup\n";
        // SAFETY: static bytes to the inherited diagnostic descriptor; no Rust IO/runtime.
        unsafe { libc::write(2, message.as_ptr().cast(), message.len()) };
        // SAFETY: startup cannot meet its test precondition. Exit before the harness,
        // without unwinding across the C initializer or silently running weaker tests.
        unsafe { libc::_exit(125) };
    }
}
