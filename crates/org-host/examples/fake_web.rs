//! A compiled stand-in for the web process, for the host's web supervisor tests. Not part of
//! any product, and never started outside a test.
//!
//! A system that executes the bytes it verified cannot start a `#!` script, so a test that
//! must show what the host does with a web process that misbehaves needs a compiled one.
//! The host starts its web process with one fixed argument and an empty environment, so
//! what this program does is not chosen there: it reads its steps, one per line, from the
//! file named like its own program file with `.steps` appended, and takes them in order.
//!
//! - `first-line`: read the host's first line from standard input and keep its rendezvous.
//! - `exit <status>`: exit now.
//! - `owner-only`: exit 7 unless the rendezvous socket has mode 0600, and 8 unless its
//!   folder has mode 0700.
//! - `connect`: connect to the rendezvous, unconfined.
//! - `confine`: confine this process and connect, as the real web process does
//!   (`org_platform::confinement::confine_web`), and keep what `ready` must report.
//! - `fork`: the steps after this one are taken by a forked process; this one waits for it
//!   and exits 0.
//! - `send <text>`: write the text and a line feed on the control connection.
//! - `ready`: send the ready line (after `confine`, with what confinement reported).
//! - `ack`: send the handoff acknowledgement.
//! - `await-handoff`: wait for one byte on the control connection.
//! - `pause <milliseconds>`: wait.
//! - `close`: close the control connection.
//! - `exec`: replace this process by this program again (a new image, the same process
//!   id), which takes the remaining steps on the same control connection.
//! - `stderr <text>`: write the text and a line feed on standard error.
//! - `stderr-raw <text>`: write the text on standard error, with no line feed.
//! - `env`: print `environment:` and then this process's environment variables, one
//!   `NAME=value` line each, on standard output.
//! - `stay`: stay alive until standard input ends (the host is gone or has dropped this
//!   child) or 20 seconds have passed, then exit 0.
//!
//! After the last step it exits 0. A step that fails ends it with a status from 90 up and a
//! line on standard error. After `confine` only what a confined process may do is used:
//! reads, writes, `poll` and the exit.

use std::io::{BufRead, Read, Write};
use std::os::fd::IntoRawFd;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::UnixStream;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use org_host_control::web::{LandlockEnforcement, LandlockStatus, WebReadyForSecrets};
use org_platform::{confinement, posix};

const ACK: &str = r#"{"v":1,"event":"fds.received","of":"handoff"}"#;
/// What `--continue` is followed by: the control connection's descriptor, then the steps.
const CONTINUE: &str = "--continue";

fn fail(status: i32, what: &str) -> ! {
    eprintln!("fake_web: {what}");
    std::process::exit(status)
}

/// This program's own file and the steps beside it. A child started from a sealed copy of
/// its image has no file of its own: its first argument is the path it was pinned at.
fn own_steps() -> (PathBuf, Vec<String>) {
    let candidates = [
        std::env::current_exe().ok(),
        std::env::args_os().next().map(PathBuf::from),
    ];
    for own in candidates.into_iter().flatten() {
        let mut name = own.clone().into_os_string();
        name.push(".steps");
        if let Ok(text) = std::fs::read_to_string(&name) {
            let steps = text
                .lines()
                .filter(|line| !line.trim().is_empty())
                .map(str::to_owned)
                .collect();
            return (own, steps);
        }
    }
    fail(90, "no steps file beside the program")
}

fn mode(path: &Path) -> u32 {
    match std::fs::metadata(path) {
        Ok(metadata) => metadata.permissions().mode() & 0o777,
        Err(error) => fail(93, &format!("no metadata of the rendezvous: {error}")),
    }
}

/// Waits `milliseconds` with `poll`, which a confined process may call.
fn pause(milliseconds: u64) {
    let _ = posix::poll(&mut [], Some(Duration::from_millis(milliseconds)));
}

/// Stays until standard input ends or 20 seconds have passed.
fn stay() {
    let until = Instant::now() + Duration::from_secs(20);
    let mut stdin = std::io::stdin().lock();
    loop {
        let left = until.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return;
        }
        let mut slot = [posix::poll_slot(0, libc::POLLIN)];
        let ready = posix::poll(&mut slot, Some(left)).unwrap_or(0);
        if ready > 0 && matches!(stdin.read(&mut [0u8; 64]), Ok(0) | Err(_)) {
            return;
        }
    }
}

fn main() {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    let (own, steps, mut control) = match arguments.as_slice() {
        [flag, descriptor, steps @ ..] if flag == CONTINUE => {
            let descriptor = descriptor
                .parse()
                .unwrap_or_else(|_| fail(90, "the control descriptor is not a number"));
            // The image this one replaced left its control connection open at this number
            // for it: a copy of it is this image's.
            let control = posix::testing::adopt_inherited(descriptor)
                .map(UnixStream::from)
                .unwrap_or_else(|_| fail(90, "the control descriptor is not open"));
            (own_steps().0, steps.to_vec(), Some(control))
        }
        _ => {
            let (own, steps) = own_steps();
            (own, steps, None)
        }
    };
    let mut rendezvous: Option<PathBuf> = None;
    let mut ready = WebReadyForSecrets { landlock: None };
    for (index, step) in steps.iter().enumerate() {
        let (word, rest) = step.split_once(' ').unwrap_or((step, ""));
        let send = |control: &mut Option<UnixStream>, text: &str| {
            let Some(stream) = control else {
                fail(94, "no control connection to send on")
            };
            if let Err(error) = stream.write_all(format!("{text}\n").as_bytes()) {
                fail(
                    94,
                    &format!("the control connection refused a line: {error}"),
                );
            }
        };
        let rendezvous_path = |rendezvous: &Option<PathBuf>| match rendezvous {
            Some(path) => path.clone(),
            None => fail(91, "no first line was read"),
        };
        match word {
            "first-line" => {
                let mut line = String::new();
                if std::io::stdin().lock().read_line(&mut line).is_err() {
                    fail(91, "no first line");
                }
                let first: serde_json::Value = serde_json::from_str(&line)
                    .unwrap_or_else(|_| fail(91, "the first line is not JSON"));
                let Some(path) = first["rendezvous"].as_str() else {
                    fail(91, "the first line names no rendezvous")
                };
                rendezvous = Some(PathBuf::from(path));
            }
            "exit" => std::process::exit(rest.parse().unwrap_or(90)),
            "owner-only" => {
                let path = rendezvous_path(&rendezvous);
                if mode(&path) != 0o600 {
                    std::process::exit(7);
                }
                if path.parent().map(mode) != Some(0o700) {
                    std::process::exit(8);
                }
            }
            "connect" => match UnixStream::connect(rendezvous_path(&rendezvous)) {
                Ok(stream) => control = Some(stream),
                Err(error) => fail(92, &format!("no connection to the rendezvous: {error}")),
            },
            "confine" => {
                let path = rendezvous_path(&rendezvous);
                let confined = confinement::disable_core_dumps()
                    .and_then(|()| confinement::confine_web(&path))
                    .unwrap_or_else(|error| fail(95, &format!("not confined: {error}")));
                ready.landlock = confined.enforcement.landlock_abi.map(|abi| LandlockStatus {
                    abi,
                    enforcement: LandlockEnforcement::FullyEnforced,
                });
                control = Some(confined.stream);
            }
            "fork" => {
                // This process has one thread; the new one goes on with the steps.
                match posix::fork_single_threaded() {
                    Ok(posix::Forked::Child(_)) => {}
                    Ok(posix::Forked::Parent(forked)) => {
                        // Waits for this process's own child.
                        let _ = posix::reap(forked);
                        std::process::exit(0);
                    }
                    Err(_) => fail(96, "no fork"),
                }
            }
            "send" => send(&mut control, rest),
            "ready" => {
                let line = serde_json::to_string(&ready)
                    .unwrap_or_else(|_| fail(94, "the ready line cannot be encoded"));
                send(&mut control, &line);
            }
            "ack" => send(&mut control, ACK),
            "await-handoff" => {
                let Some(stream) = control.as_mut() else {
                    fail(94, "no control connection to wait on")
                };
                if !matches!(stream.read(&mut [0u8; 1]), Ok(1)) {
                    fail(94, "nothing arrived on the control connection");
                }
            }
            "pause" => pause(rest.parse().unwrap_or(0)),
            "stderr" => eprintln!("{rest}"),
            "stderr-raw" => eprint!("{rest}"),
            "env" => {
                println!("environment:");
                for (name, value) in std::env::vars_os() {
                    println!("{}={}", name.to_string_lossy(), value.to_string_lossy());
                }
            }
            "close" => control = None,
            "exec" => {
                let Some(stream) = control.take() else {
                    fail(97, "no control connection to keep")
                };
                // Clears close-on-exec, so that the next image inherits it.
                if posix::set_close_on_exec(&stream, false).is_err() {
                    fail(97, "the control descriptor cannot be kept");
                }
                let descriptor = stream.into_raw_fd();
                let error = std::process::Command::new(&own)
                    .arg(CONTINUE)
                    .arg(descriptor.to_string())
                    .args(&steps[index + 1..])
                    .exec();
                fail(97, &format!("no exec: {error}"));
            }
            "stay" => {
                stay();
                break;
            }
            other => fail(90, &format!("an unknown step: {other}")),
        }
    }
}
