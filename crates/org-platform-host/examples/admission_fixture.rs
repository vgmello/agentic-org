//! The compiled worker the admission adapter's tests start: a stand-in for an office
//! worker that reports what it was started with. Not part of any product.
//!
//! Its arguments are commands, run in order:
//!
//! - `report`: print the arguments, the environment, the open descriptors, the working
//!   directory, the executable link, the effective user and the owner of
//!   `/proc/self/mem` (root owns it when the process is not dumpable), then `end`.
//! - `echo`: read one line from standard input; print it on standard output and on
//!   standard error.
//! - `connect <path>`: connect to the Unix socket at `path`, keep the connection and
//!   send `ready`.
//! - `relay`: read one line from that connection and print it.
//! - `hold`: wait until standard input is closed.
//! - `exit <code>`: exit with `code`.
//! - `admit <program> [command..]`: admit `program` as a development image through
//!   the platform's adapter, start it with the commands that follow, copy its report,
//!   say whether this process can open the child's memory and read its executable
//!   link, then let it go. For running the adapter as another user than the test's.

use std::ffi::OsStr;
use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::fs::MetadataExt;
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::sync::Mutex;

use org_platform_host::admission::{admission, ChildEnv, ExpectedImage, SigningRule};

fn report() {
    let mut out = std::io::stdout().lock();
    let mut line = |text: String| writeln!(out, "{text}").expect("standard output");
    for (index, arg) in std::env::args_os().enumerate() {
        line(format!("arg={index}={}", arg.to_string_lossy()));
    }
    for (name, value) in std::env::vars_os() {
        line(format!(
            "env={}={}",
            name.to_string_lossy(),
            value.to_string_lossy()
        ));
    }
    match std::fs::read_dir("/proc/self/fd") {
        Ok(entries) => {
            for entry in entries.flatten() {
                let target = std::fs::read_link(entry.path())
                    .map_or_else(|error| error.to_string(), |path| path.display().to_string());
                line(format!(
                    "fd={}={target}",
                    entry.file_name().to_string_lossy()
                ));
            }
        }
        Err(error) => line(format!("fd=unavailable={error}")),
    }
    let text = |path: std::io::Result<PathBuf>| {
        path.map_or_else(|error| error.to_string(), |path| path.display().to_string())
    };
    line(format!("cwd={}", text(std::env::current_dir())));
    line(format!(
        "exe={}",
        text(std::fs::read_link("/proc/self/exe"))
    ));
    line(format!("current_exe={}", text(std::env::current_exe())));
    // SAFETY: no arguments and no side effects; it cannot fail.
    line(format!("euid={}", unsafe { libc::geteuid() }));
    match std::fs::metadata("/proc/self/mem") {
        Ok(metadata) => line(format!("mem_uid={}", metadata.uid())),
        Err(error) => line(format!("mem_uid=unavailable={error}")),
    }
    line("end".into());
}

/// Admits `program` as a development image and starts it with `commands`.
fn admit(program: &OsStr, commands: &[&OsStr]) {
    let expected = ExpectedImage {
        program: PathBuf::from(program),
        sha256: None,
        macos_cdhash: None,
        macos_requirement: None,
        macos_signing: SigningRule {
            required_flags: 0,
            rejected_entitlements: &[],
            absent_entitlements: &[],
        },
        development: true,
    };
    let admission = admission(expected).expect("the admission");
    let started = admission
        .spawn(commands, &ChildEnv::new(), &Mutex::new(()))
        .expect("the start");
    let mut lines = BufReader::new(started.stdout).lines();
    for line in lines.by_ref() {
        let line = line.expect("the child's report");
        println!("{line}");
        if line == "end" {
            break;
        }
    }
    let pid = started.child.id();
    let probe = |outcome: std::io::Result<()>| match outcome {
        Ok(()) => "allowed".to_owned(),
        Err(error) => format!("{:?}", error.kind()),
    };
    println!(
        "probe_mem={}",
        probe(std::fs::File::open(format!("/proc/{pid}/mem")).map(drop))
    );
    println!(
        "probe_exe={}",
        probe(std::fs::read_link(format!("/proc/{pid}/exe")).map(drop))
    );
    drop(started.stdin);
    for line in lines {
        println!("{}", line.expect("the child's output"));
    }
}

fn main() {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    let mut args = args.iter().map(|arg| arg.as_os_str());
    let mut connection: Option<UnixStream> = None;
    while let Some(command) = args.next() {
        match command.to_str() {
            Some("report") => report(),
            Some("echo") => {
                let mut line = String::new();
                std::io::stdin().read_line(&mut line).expect("a line");
                print!("echo={line}");
                eprint!("stderr={line}");
            }
            Some("connect") => {
                let path = args.next().expect("a socket path");
                let mut stream = UnixStream::connect(path).expect("the connection");
                stream.write_all(b"ready\n").expect("the ready line");
                connection = Some(stream);
                println!("connected");
            }
            Some("relay") => {
                let stream = connection.as_ref().expect("a connection");
                let mut line = String::new();
                BufReader::new(stream)
                    .read_line(&mut line)
                    .expect("a line from the connection");
                print!("relay={line}");
            }
            Some("hold") => {
                let mut rest = Vec::new();
                let _ = std::io::stdin().read_to_end(&mut rest);
            }
            Some("exit") => {
                let code = args.next().and_then(OsStr::to_str).expect("an exit code");
                std::io::stdout().flush().expect("standard output");
                std::process::exit(code.parse().expect("a number"));
            }
            Some("admit") => {
                let program = args.next().expect("a program");
                let commands: Vec<&OsStr> = args.by_ref().collect();
                admit(program, &commands);
            }
            _ => panic!("unknown command {command:?}"),
        }
        std::io::stdout().flush().expect("standard output");
    }
}
