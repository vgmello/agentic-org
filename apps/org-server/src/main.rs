//! # org-server: the office daemon
//!
//! A thin binary: it moves its control pipe out of the way, installs a panic hook, and runs
//! `org_server::run`. All logic is in the library.

fn main() {
    // Before startup concurrency or key material: inherited by all threads and children.
    let limits = libc::rlimit {
        rlim_cur: 0,
        rlim_max: 0,
    };
    // SAFETY: RLIMIT_CORE accepts this valid limits structure.
    if unsafe { libc::setrlimit(libc::RLIMIT_CORE, &limits) } != 0 {
        eprintln!(
            "cannot disable core dumps: {}",
            std::io::Error::last_os_error()
        );
        std::process::exit(1);
    }

    // First thing, before any thread or child exists (see `org_server::fdswap`).
    let control = match org_server::fdswap::take_control_pipe() {
        Ok(control) => control,
        Err(error) => {
            eprintln!("org-server: cannot set up its descriptors: {error}");
            std::process::exit(1);
        }
    };
    org_server::install_panic_hook();
    let args: Vec<String> = std::env::args().skip(1).collect();
    std::process::exit(org_server::run(&args, control));
}
