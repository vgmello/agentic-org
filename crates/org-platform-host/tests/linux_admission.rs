//! The Linux admission adapter through the crate's public contract alone, as the host
//! composes it: no test hook, no test adapter. It needs the Linux kernel, so the whole
//! file is for GNU/Linux; the adapter's own tests are in
//! `src/admission/linux/tests.rs`.
//!
//! The order below is the host's order for a worker (host boundary design, "Admission
//! and the verified rendezvous"): admit the image by its digest, start the worker with
//! its three pipes, accept its connection on the rendezvous, require that the
//! connection is the worker's, and only then send anything over it.
#![cfg(all(target_os = "linux", target_env = "gnu"))]

use std::ffi::OsStr;
use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::UnixListener;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use org_platform_host::admission::{
    admission, own_image, peer, AdmissionError, ChildEnv, ExpectedImage, ImageRefusal, NotTheChild,
    SigningRule,
};
use sha2::{Digest, Sha256};

/// A copy of the compiled fixture (`examples/admission_fixture.rs`) in a private
/// directory, writable by its owner alone, as a pinned image must be.
fn pinned_fixture(dir: &Path) -> PathBuf {
    let test_binary = std::env::current_exe().unwrap();
    let profile = test_binary.parent().and_then(Path::parent).unwrap();
    let built = profile.join("examples").join("admission_fixture");
    assert!(
        built.is_file(),
        "{} is not built: `cargo test -p org-platform-host` builds it",
        built.display()
    );
    let pinned = dir.join("worker");
    fs::copy(built, &pinned).unwrap();
    fs::set_permissions(&pinned, fs::Permissions::from_mode(0o755)).unwrap();
    pinned
}

/// What a release host expects of `program`: the digest from its verified manifest.
fn release(program: &Path, sha256: [u8; 32]) -> ExpectedImage {
    ExpectedImage {
        program: program.to_owned(),
        sha256: Some(sha256),
        macos_cdhash: None,
        macos_requirement: None,
        macos_signing: SigningRule {
            required_flags: 0,
            rejected_entitlements: &[],
            absent_entitlements: &[],
        },
        development: false,
    }
}

#[test]
fn a_worker_admitted_by_its_digest_gets_a_line_only_on_its_own_verified_connection() {
    let dir = tempfile::tempdir().unwrap();
    let worker = pinned_fixture(dir.path());
    let digest: [u8; 32] = Sha256::digest(fs::read(&worker).unwrap()).into();

    // Once per host lifetime: the image is read, checked and sealed.
    let admission = admission(release(&worker, digest)).unwrap();
    assert_eq!(admission.image_sha256(), Some(digest));
    // Replacing the file afterwards changes nothing about what is started.
    fs::write(&worker, b"#!/bin/sh\necho replaced\n").unwrap();

    let rendezvous = dir.path().join("rendezvous");
    let listener = UnixListener::bind(&rendezvous).unwrap();
    listener.set_nonblocking(true).unwrap();
    let mut env = ChildEnv::new();
    env.set("OFFICE_HOME", "/somewhere").unwrap();
    // A loader variable never reaches a child: the environment cannot hold one.
    assert!(env.set("LD_PRELOAD", "/tmp/injected.so").is_err());
    let args = ["connect", rendezvous.to_str().unwrap(), "relay", "report"];
    let args: Vec<&OsStr> = args.iter().map(OsStr::new).collect();
    let fence = Mutex::new(());
    let mut worker_process = admission.spawn(&args, &env, &fence).unwrap();

    let until = Instant::now() + Duration::from_secs(20);
    let mut stream = loop {
        match listener.accept() {
            Ok((stream, _)) => break stream,
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                assert!(Instant::now() < until, "the worker did not connect");
                std::thread::sleep(Duration::from_millis(2));
            }
            Err(error) => panic!("{error}"),
        }
    };
    stream.set_nonblocking(false).unwrap();
    // The peer check, before anything is read or written.
    admission
        .verify_reader(&worker_process.child, &stream)
        .unwrap();
    assert_eq!(peer(&stream).unwrap().pid, Some(worker_process.child.id()));
    let mut ready = String::new();
    BufReader::new(&stream).read_line(&mut ready).unwrap();
    assert_eq!(ready, "ready\n");
    // Still the child, right before the line that would carry a secret.
    admission
        .verify_reader(&worker_process.child, &stream)
        .unwrap();
    admission.recheck(&worker_process.child).unwrap();
    stream.write_all(b"for the verified worker only\n").unwrap();

    let mut output = String::new();
    worker_process.stdout.read_to_string(&mut output).unwrap();
    let lines: Vec<&str> = output.lines().collect();
    assert_eq!(
        &lines[..2],
        ["connected", "relay=for the verified worker only"]
    );
    // What ran is the sealed image, with exactly the environment it was given.
    assert!(lines.contains(&"exe=/memfd:worker (deleted)"), "{output}");
    let environment: Vec<&&str> = lines.iter().filter(|l| l.starts_with("env=")).collect();
    assert_eq!(environment, [&"env=OFFICE_HOME=/somewhere"]);

    while !worker_process.child.try_wait().unwrap() {
        assert!(Instant::now() < until, "the worker did not exit");
        std::thread::sleep(Duration::from_millis(2));
    }
    assert_eq!(worker_process.child.ended().as_deref(), Some("exit code 0"));
    // The worker is gone: its old connection is nobody's.
    let gone = admission
        .verify_reader(&worker_process.child, &stream)
        .unwrap_err();
    assert!(NotTheChild::is(&gone), "{gone}");
}

#[test]
fn an_image_that_is_not_the_expected_bytes_has_no_admission_to_start_it_with() {
    let dir = tempfile::tempdir().unwrap();
    let worker = pinned_fixture(dir.path());
    let mut bytes = fs::read(&worker).unwrap();
    let digest: [u8; 32] = Sha256::digest(&bytes).into();
    bytes[0] ^= 1;
    fs::write(&worker, &bytes).unwrap();
    match admission(release(&worker, digest)) {
        Err(AdmissionError::Image(ImageRefusal::DigestMismatch)) => {}
        Err(other) => panic!("refused for another reason: {other}"),
        Ok(_) => panic!("changed bytes were admitted"),
    }
    // A release image with no digest has no publisher identity at all.
    let unpinned = ExpectedImage {
        sha256: None,
        ..release(&worker, digest)
    };
    assert!(matches!(
        admission(unpinned),
        Err(AdmissionError::Image(ImageRefusal::Unpinned))
    ));
}

#[test]
fn this_test_binary_is_its_own_image_only_under_its_own_digest() {
    let program = std::env::current_exe().unwrap();
    let digest: [u8; 32] = Sha256::digest(fs::read("/proc/self/exe").unwrap()).into();
    own_image(&release(&program, digest)).unwrap();
    assert!(own_image(&release(&program, [0; 32])).is_err());
}
