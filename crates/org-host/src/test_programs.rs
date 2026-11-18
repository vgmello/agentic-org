//! Compiled programs the host's unit tests start as office workers.
//!
//! A host starts only a worker image that nobody else could have written, and a system that
//! executes the bytes it verified cannot start a `#!` script at all. A test that goes
//! through the platform's own admission therefore starts a compiled program, from a copy
//! that only this user can write: what cargo builds has the mode the account's umask gives
//! it, and may belong to another user than the one the tests run as.

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

/// The recording worker (`examples/recording_worker.rs`): it speaks no launch protocol and
/// records, in the office home it is given, that it started (`worker.started`), the names
/// of its environment variables (`worker.env`) and every byte of its standard input
/// (`worker.stdin`). This user's own copy, made once by each test process.
pub(crate) fn recording_worker() -> PathBuf {
    static COPY: OnceLock<PathBuf> = OnceLock::new();
    COPY.get_or_init(|| private_copy(&built_example("recording_worker")))
        .clone()
}

/// What the recording worker's runs in the office home `home` read from standard input, in
/// order (empty when none started or none was given anything).
pub(crate) fn recorded_stdin(home: &Path) -> Vec<u8> {
    fs::read(home.join("worker.stdin")).unwrap_or_default()
}

/// How many times the recording worker started in the office home `home`.
pub(crate) fn recorded_starts(home: &Path) -> usize {
    fs::read_to_string(home.join("worker.started"))
        .map(|text| text.lines().count())
        .unwrap_or(0)
}

/// An example of this crate, as `cargo test` built it beside this test binary.
fn built_example(name: &str) -> PathBuf {
    let test_binary = std::env::current_exe().unwrap();
    let profile = test_binary.parent().and_then(Path::parent).unwrap();
    let example = profile.join("examples").join(name);
    assert!(
        example.is_file(),
        "{} is not built: `cargo test -p org-host` builds it; a run of the library's tests \
         alone needs `cargo build -p org-host --example {name}` first",
        example.display()
    );
    example
}

/// A copy of `built` that only this user can write (mode 0700), beside it in the build
/// output, replacing any earlier copy in one step.
fn private_copy(built: &Path) -> PathBuf {
    let folder = built.parent().unwrap().join("private-programs");
    fs::create_dir_all(&folder).unwrap();
    let name = built.file_name().unwrap().to_str().unwrap();
    let staged = folder.join(format!(".{name}.{}", std::process::id()));
    let _ = fs::remove_file(&staged);
    fs::copy(built, &staged).unwrap();
    fs::set_permissions(&staged, fs::Permissions::from_mode(0o700)).unwrap();
    let private = folder.join(name);
    fs::rename(&staged, &private).unwrap();
    private
}

/// What this system's admission identifies an image by, as the platform says. A test whose
/// expected outcome differs by system takes it from this fact and expects exactly that one
/// outcome; no outcome of another system passes. The host's tests run only where there is an
/// admission adapter.
pub(crate) fn image_identity() -> org_platform_host::admission::ImageIdentity {
    org_platform_host::admission::image_identity()
        .expect("the host's tests need an admission adapter")
}
