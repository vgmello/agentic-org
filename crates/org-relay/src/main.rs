//! `org-relay`: Local spool relay daemon moving agent messages from `outbox/new` to `inbox/<to>/new`.
//!
//! Coupling metrics (per `msg/architecture-rules.md`):
//! - Efferent coupling (`Ce`): 1 (`org_msg`)
//! - Afferent coupling (`Ca`): 0 (Binary entrypoint)
//! - Instability (`I`): 1.00

use std::env;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::os::unix::io::AsRawFd;
use std::os::unix::net::UnixListener;
use std::process::{self, ExitCode};
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use org_msg::{json_quote, poll_unix_listener, Spool, SpoolError};

static STOP_REQUESTED: AtomicBool = AtomicBool::new(false);

const SIGINT: i32 = 2;
const SIGTERM: i32 = 15;
const LOCK_EX: i32 = 2;
const LOCK_NB: i32 = 4;
const LOCK_UN: i32 = 8;

extern "C" {
    fn signal(sig: i32, handler: extern "C" fn(i32)) -> usize;
    fn flock(fd: i32, operation: i32) -> i32;
}

extern "C" fn handle_signal(_sig: i32) {
    STOP_REQUESTED.store(true, Ordering::SeqCst);
}

fn install_signal_handlers() {
    unsafe {
        signal(SIGINT, handle_signal);
        signal(SIGTERM, handle_signal);
    }
}

struct RelayLockGuard {
    file: File,
}

impl RelayLockGuard {
    fn try_acquire(spool: &Spool) -> Result<Option<Self>, io::Error> {
        let lock_path = spool.relay_lock_path();
        if let Some(parent) = lock_path.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .mode(0o600)
            .open(&lock_path)?;
        let rc = unsafe { flock(file.as_raw_fd(), LOCK_EX | LOCK_NB) };
        if rc != 0 {
            return Ok(None);
        }
        file.set_len(0)?;
        let _ = writeln!(file, "{}", process::id());
        let _ = file.sync_all();
        Ok(Some(Self { file }))
    }
}

impl Drop for RelayLockGuard {
    fn drop(&mut self) {
        unsafe {
            let _ = flock(self.file.as_raw_fd(), LOCK_UN);
        }
    }
}

fn is_relay_running(spool: &Spool) -> (bool, Option<u32>) {
    let lock_path = spool.relay_lock_path();
    let Ok(mut file) = OpenOptions::new().read(true).write(true).open(&lock_path) else {
        return (false, None);
    };
    let rc = unsafe { flock(file.as_raw_fd(), LOCK_EX | LOCK_NB) };
    if rc == 0 {
        unsafe {
            let _ = flock(file.as_raw_fd(), LOCK_UN);
        }
        (false, None)
    } else {
        let mut s = String::new();
        let _ = file.read_to_string(&mut s);
        let pid = s.trim().parse::<u32>().ok();
        (true, pid)
    }
}

fn run_relay(spool: &Spool, once: bool) -> Result<i32, SpoolError> {
    let Some(_guard) = RelayLockGuard::try_acquire(spool)? else {
        eprintln!("org-relay: another relay instance is already running");
        return Ok(1);
    };

    let _ = spool.prune_old_cur();
    let initial = spool.deliver_all()?;
    if once {
        println!(
            "delivered={} dead={}",
            initial.delivered, initial.dead
        );
        return Ok(0);
    }

    let sock_path = spool.relay_socket_path();
    let _ = fs::remove_file(&sock_path);
    let listener = match UnixListener::bind(&sock_path) {
        Ok(l) => {
            let _ = fs::set_permissions(&sock_path, fs::Permissions::from_mode(0o600));
            let _ = l.set_nonblocking(true);
            Some(l)
        }
        Err(_) => None,
    };

    let poll_interval = Duration::from_millis(500);
    let mut last_sweep = Instant::now();

    while !STOP_REQUESTED.load(Ordering::SeqCst) {
        let mut nudged = false;
        if let Some(ref l) = listener {
            while let Ok((mut stream, _)) = l.accept() {
                let mut buf = [0u8; 16];
                let _ = stream.read(&mut buf);
                nudged = true;
            }
        }
        if nudged || spool.has_pending_outbox() || last_sweep.elapsed() >= poll_interval {
            let _ = spool.deliver_all();
            last_sweep = Instant::now();
        }
        if let Some(ref l) = listener {
            let remaining = poll_interval.saturating_sub(last_sweep.elapsed());
            let wait_slice = remaining.min(Duration::from_millis(2));
            let _ = poll_unix_listener(l, wait_slice);
        } else {
            thread::sleep(Duration::from_millis(2));
        }
    }

    let _ = spool.deliver_all();
    let _ = fs::remove_file(&sock_path);
    Ok(0)
}

fn run_status(spool: &Spool, json: bool) -> Result<i32, SpoolError> {
    let (running, pid) = is_relay_running(spool);
    let st = spool.status()?;
    if json {
        let pid_str = pid.map(|p| p.to_string()).unwrap_or_else(|| "null".into());
        let agents_json: Vec<String> = st
            .registered_agents
            .iter()
            .map(|a| json_quote(a))
            .collect();
        println!(
            "{{\"running\":{running},\"pid\":{pid_str},\"outbox_new\":{},\"dead\":{},\"registered_agents\":[{}]}}",
            st.outbox_new,
            st.dead,
            agents_json.join(",")
        );
    } else {
        let state = match (running, pid) {
            (true, Some(p)) => format!("running (pid {p})"),
            (true, None) => "running".to_string(),
            (false, _) => "stopped".to_string(),
        };
        println!("relay: {state}");
        println!("root: {}", st.root.display());
        println!("outbox/new: {}", st.outbox_new);
        println!("dead: {}", st.dead);
        println!(
            "registered: {}",
            if st.registered_agents.is_empty() {
                "(none)".to_string()
            } else {
                st.registered_agents.join(", ")
            }
        );
    }
    Ok(0)
}

fn print_usage() {
    eprintln!(
        "\
Usage:
  org-relay run [--once]
  org-relay status [--json]"
    );
}

fn main() -> ExitCode {
    install_signal_handlers();
    let args: Vec<String> = env::args().skip(1).collect();
    let Some(cmd) = args.first().map(|s| s.as_str()) else {
        print_usage();
        return ExitCode::from(1);
    };

    if matches!(cmd, "-h" | "--help" | "help") {
        print_usage();
        return ExitCode::from(0);
    }

    let spool = match Spool::open_from_env() {
        Ok(s) => s,
        Err(e) => {
            eprintln!("org-relay: {e}");
            return ExitCode::from(4);
        }
    };

    let res = match cmd {
        "run" => {
            let mut once = false;
            for arg in &args[1..] {
                match arg.as_str() {
                    "--once" => once = true,
                    other => {
                        eprintln!("org-relay: unknown flag '{other}'");
                        print_usage();
                        return ExitCode::from(1);
                    }
                }
            }
            run_relay(&spool, once)
        }
        "status" => {
            let mut json = false;
            for arg in &args[1..] {
                match arg.as_str() {
                    "--json" => json = true,
                    other => {
                        eprintln!("org-relay: unknown flag '{other}'");
                        print_usage();
                        return ExitCode::from(1);
                    }
                }
            }
            run_status(&spool, json)
        }
        other => {
            eprintln!("org-relay: unknown command '{other}'");
            print_usage();
            return ExitCode::from(1);
        }
    };

    match res {
        Ok(code) => ExitCode::from(code as u8),
        Err(e) => {
            eprintln!("org-relay: {e}");
            ExitCode::from(4)
        }
    }
}
