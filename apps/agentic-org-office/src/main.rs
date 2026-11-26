//! # agentic-org-office: the office daemon
//!
//! A thin binary: it moves its control pipe out of the way, installs a panic hook, wires the
//! `agent-os` command catalogue into the session manager, and runs `org_office_server::run`.
//! All logic lives in library crates.

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

    // First thing, before any thread or child exists (see `org_office_server::fdswap`).
    let control = match org_office_server::fdswap::take_control_pipe() {
        Ok(control) => control,
        Err(error) => {
            eprintln!("agentic-org-office: cannot set up its descriptors: {error}");
            std::process::exit(1);
        }
    };
    org_office_server::install_panic_hook();
    // Composition root: agents' standing instructions list the live `agent-os` commands.
    org_office_sessions::install_command_catalogue(org_agent_cli::table::table);
    let args: Vec<String> = std::env::args().skip(1).collect();
    std::process::exit(org_office_server::run(&args, control));
}
