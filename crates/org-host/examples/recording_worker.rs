//! A compiled stand-in for an office worker that only records what it was given, for the
//! host's supervisor tests. Not part of any product, and never started outside a test.
//!
//! A system that executes the bytes it verified cannot start a `#!` script, so a test that
//! must show what a refused worker received needs a compiled program. This one speaks no
//! launch protocol. In the office home the host names (`ORG_OFFICE_HOME`) it writes:
//!
//! - `worker.started`: one line per run, `<pid> <arguments>`, before it reads anything;
//! - `worker.env`: the names of its environment variables, one line per run;
//! - `worker.stdin`: every byte it reads from standard input, as it arrives.
//!
//! It exits 0 once it has read one whole line (a first line, which is all a host writes there)
//! or when standard input ends, which is when the host closes it or is gone.

use std::fs::OpenOptions;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

fn append(home: &Path, name: &str, bytes: &[u8]) {
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(home.join(name))
        .expect("a file in the office home");
    file.write_all(bytes).expect("a write in the office home");
}

fn main() {
    let home = PathBuf::from(std::env::var_os("ORG_OFFICE_HOME").expect("ORG_OFFICE_HOME"));
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    append(
        &home,
        "worker.started",
        format!("{} {}\n", std::process::id(), arguments.join(" ")).as_bytes(),
    );
    let mut names: Vec<String> = std::env::vars_os()
        .map(|(name, _)| name.to_string_lossy().into_owned())
        .collect();
    names.sort();
    append(
        &home,
        "worker.env",
        format!("{}\n", names.join(",")).as_bytes(),
    );
    let mut stdin = std::io::stdin().lock();
    let mut buffer = [0u8; 4096];
    loop {
        match stdin.read(&mut buffer) {
            Ok(0) | Err(_) => return,
            Ok(count) => {
                append(&home, "worker.stdin", &buffer[..count]);
                if buffer[..count].contains(&b'\n') {
                    return;
                }
            }
        }
    }
}
