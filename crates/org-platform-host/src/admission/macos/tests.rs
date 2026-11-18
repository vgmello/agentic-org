//! Tests of the macOS adapter against the real kernel and Security framework. The policy
//! in them (requirements, flags, entitlement names) is test data, as a consumer passes it.

use std::ffi::{OsStr, OsString};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use super::*;
use crate::admission::test_support::{Script, Step};

/// The code-signature flag of the hardened runtime (`kSecCodeSignatureRuntime`).
const HARDENED_RUNTIME: u32 = 0x0001_0000;
/// Entitlements that let another process or library reach into a program.
const REJECTED: [&str; 3] = [
    "com.apple.security.get-task-allow",
    "com.apple.security.cs.allow-dyld-environment-variables",
    "com.apple.security.cs.disable-library-validation",
];
/// Hardened code without the rejected entitlements.
const HARDENED: SigningRule = SigningRule {
    required_flags: HARDENED_RUNTIME,
    rejected_entitlements: &REJECTED,
    absent_entitlements: &[],
};
/// The same, and no Keychain access group.
const HARDENED_WITHOUT_KEYCHAIN: SigningRule = SigningRule {
    absent_entitlements: &[code::KEYCHAIN_ACCESS_GROUPS],
    ..HARDENED
};
/// A Developer ID requirement no program on this machine satisfies.
const NOBODY: &str = "identifier \"com.example.nobody\" and anchor apple generic and \
                      certificate 1[field.1.2.840.113635.100.6.2.6] exists and \
                      certificate leaf[field.1.2.840.113635.100.6.1.13] exists and \
                      certificate leaf[subject.OU] = \"AB12CD34EF\"";

fn development(program: &str) -> ExpectedImage {
    ExpectedImage {
        program: PathBuf::from(program),
        sha256: None,
        macos_cdhash: None,
        macos_requirement: None,
        macos_signing: HARDENED,
        development: true,
    }
}

fn release(program: &str, requirement: &str, rule: SigningRule) -> ExpectedImage {
    ExpectedImage {
        macos_requirement: Some(requirement.to_owned()),
        macos_signing: rule,
        development: false,
        ..development(program)
    }
}

fn admission(expected: ExpectedImage) -> Admission {
    Admission::new(expected).unwrap()
}

fn scripted(
    expected: ExpectedImage,
    script: impl Fn(Step, u32) -> Result<Option<Peer>, String> + Send + Sync + 'static,
) -> Admission {
    let script: Script = Arc::new(script);
    Admission::scripted(expected, script).unwrap()
}

/// Starts the admission's program with `args` and no environment.
fn start(admission: &Admission, args: &[&str]) -> std::io::Result<Started> {
    let args: Vec<&OsStr> = args.iter().map(OsStr::new).collect();
    admission.start(&args, &ChildEnv::new(), &Mutex::new(()))
}

fn refusal(result: std::io::Result<Started>) -> ImageRefusal {
    let error = result.err().expect("the child was admitted");
    let refused = SpawnRefused::of(&error).expect("an image refusal");
    // SAFETY: signal 0 only checks the pid. The refused child is killed and reaped.
    assert!(
        unsafe { libc::kill(refused.pid as libc::pid_t, 0) } != 0,
        "a refused child was left behind"
    );
    refused.refusal.clone()
}

#[test]
fn a_socket_pair_reports_this_process() {
    let (a, _b) = UnixStream::pair().unwrap();
    assert!(audit_token(&a).unwrap().iter().any(|&b| b != 0));
    assert_eq!(
        peer(&a).unwrap().macos_audit_token,
        audit_token(&a).unwrap()
    );
    native_peers().unwrap();
    #[cfg(debug_assertions)]
    {
        assert_eq!(pid(&a).unwrap(), std::process::id() as libc::pid_t);
        let me = executable(std::process::id() as libc::pid_t).unwrap();
        assert_eq!(
            me.canonicalize().unwrap(),
            std::env::current_exe().unwrap().canonicalize().unwrap()
        );
        assert_eq!(
            peer_executable(&a).unwrap().canonicalize().unwrap(),
            std::env::current_exe().unwrap().canonicalize().unwrap()
        );
    }
}

/// The real Security-framework path, both ways, on this test process's own audit token: a
/// requirement it meets (its own signing identifier) passes, others don't. So broken
/// plumbing, which would fail closed, is caught.
#[test]
fn the_signature_check_accepts_what_this_process_satisfies_and_nothing_else() {
    let (a, _b) = UnixStream::pair().unwrap();
    let token = audit_token(&a).unwrap();
    let Some(identifier) = code::own_codesign_field("Identifier=") else {
        // An unsigned test binary (no linker signature, e.g. on Intel): nothing to accept.
        eprintln!("skipped: this test binary has no code signature");
        return;
    };
    assert!(
        code::code_satisfies(&token, &format!("identifier \"{identifier}\"")),
        "{identifier}"
    );
    assert!(!code::code_satisfies(
        &token,
        &format!("identifier \"{identifier}.not\"")
    ));
    assert!(!code::code_satisfies(&token, NOBODY));
    // Through the contract's entry: this test binary is not hardened, so it satisfies no
    // rule that requires the hardened runtime, whatever the requirement.
    let me = peer(&a).unwrap();
    assert!(!peer_satisfies(
        &me,
        &format!("identifier \"{identifier}\""),
        &HARDENED
    ));
    assert!(!peer_satisfies(&me, NOBODY, &HARDENED));
}

/// A held child runs nothing until it is released; one dropped while held never ran.
#[test]
fn a_held_child_runs_nothing_until_it_is_released() {
    let dir = tempfile::tempdir().unwrap();
    let (released, dropped) = (dir.path().join("released"), dir.path().join("dropped"));
    let sh = admission(development("/bin/sh"));
    let a = start(&sh, &["-c", &format!("touch '{}'", released.display())]).unwrap();
    let b = start(&sh, &["-c", &format!("touch '{}'", dropped.display())]).unwrap();
    std::thread::sleep(std::time::Duration::from_millis(300));
    assert!(!released.exists() && !dropped.exists(), "ran while held");
    // A development image is admitted without an identity.
    assert_eq!(a.child.0.admitted, None);
    let mut a = crate::admission::Held(a).release().unwrap();
    drop(crate::admission::Held(b));
    while !a.child.try_wait().unwrap() {
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert_eq!(a.child.ended().as_deref(), Some("exit code 0"));
    assert!(released.exists());
    assert!(!dropped.exists(), "a child dropped while held ran");
}

/// The child gets exactly the environment of its `ChildEnv`, and the caller's fence is
/// free again once the start returns (it is held only while the process is created).
#[test]
fn a_started_child_gets_its_environment_and_the_fence_is_free_afterwards() {
    let mut env = ChildEnv::new();
    env.set("ONLY", "this").unwrap();
    let fence = Mutex::new(());
    let sh = admission(development("/bin/sh"));
    let args = [OsStr::new("-c"), OsStr::new("echo \"$ONLY\"")];
    let started = crate::admission::Held(sh.start(&args, &env, &fence).unwrap())
        .release()
        .unwrap();
    assert!(fence.try_lock().is_ok(), "the fence outlived the spawn");
    drop(started.stdin);
    let mut out = String::new();
    let mut stdout = started.stdout;
    stdout.read_to_string(&mut out).unwrap();
    assert_eq!(out, "this\n");
    // A poisoned fence is taken all the same, as the host's own lock helper does.
    let poisoned = Arc::new(Mutex::new(()));
    let held = Arc::clone(&poisoned);
    let _ = std::thread::spawn(move || {
        let _guard = held.lock().unwrap();
        panic!("poison the fence");
    })
    .join();
    assert!(poisoned.is_poisoned());
    let args = [OsStr::new("-c"), OsStr::new("exit 0")];
    sh.start(&args, &ChildEnv::new(), &poisoned).unwrap();
}

/// A release image with no requirement is never started; a missing program is the system's
/// error, not a refusal.
#[test]
fn nothing_is_started_without_a_rule_or_without_a_program() {
    let unruled = ExpectedImage {
        development: false,
        ..development("/bin/sh")
    };
    let error = start(&admission(unruled), &["-c", "exit 0"])
        .err()
        .expect("a release image without a requirement was started");
    assert_eq!(error.kind(), std::io::ErrorKind::InvalidInput);
    assert!(SpawnRefused::of(&error).is_none());
    let dir = tempfile::tempdir().unwrap();
    let missing = dir.path().join("missing");
    let error = start(&admission(development(missing.to_str().unwrap())), &[])
        .err()
        .expect("a missing program was started");
    assert_eq!(error.kind(), std::io::ErrorKind::NotFound);
    assert!(SpawnRefused::of(&error).is_none());
}

/// The live probe reads a real child's signed entitlements. A suspended
/// `/bin/sleep` holds no `keychain-access-groups` entitlement and is accepted.
/// A suspended `/usr/libexec/security-sysdiagnose` (an Apple system tool that
/// never runs here) holds one and is refused. No local build product carries it.
#[test]
fn the_live_entitlement_probe_refuses_only_a_child_with_keychain_access_groups() {
    let probe = |program: &str| {
        let spawned = spawn::spawn_suspended(Path::new(program), &[], &[]).unwrap();
        let token = spawn::audit_token_of_child(spawned.process.id()).unwrap();
        code::without_entitlements(&token, &[code::KEYCHAIN_ACCESS_GROUPS])
    };
    assert!(probe("/bin/sleep"), "an unentitled child was refused");
    assert!(
        !probe("/usr/libexec/security-sysdiagnose"),
        "a child holding keychain-access-groups was accepted"
    );
}

/// The production start wires the live token, signature and entitlement probes. With an
/// Apple-anchored requirement (no signing team needed), hardened Apple binaries reach every
/// probe: `/usr/bin/scp` (no entitlements) is admitted; `/usr/bin/ssh` (hardened, only
/// keychain-access-groups) satisfies the signature check, as the rule without an absent
/// entitlement shows, yet the rule that forbids the Keychain entitlement refuses it;
/// unhardened `/bin/sleep` fails the signature check.
#[test]
fn a_production_start_uses_the_live_probes() {
    let start_with = |rule: SigningRule, program: &str| {
        start(&admission(release(program, "anchor apple", rule)), &[])
    };
    let scp = start_with(HARDENED_WITHOUT_KEYCHAIN, "/usr/bin/scp")
        .expect("a hardened child without a Keychain entitlement was refused");
    // It was identified, and is still held: not one instruction of it has run.
    assert_eq!(
        scp.child.0.admitted,
        Some(spawn::audit_token_of_child(scp.child.id()).unwrap())
    );
    assert!(
        start_with(HARDENED, "/usr/bin/ssh").is_ok(),
        "/usr/bin/ssh must satisfy the signature check for this test to isolate the probe"
    );
    assert_eq!(
        refusal(start_with(HARDENED_WITHOUT_KEYCHAIN, "/usr/bin/ssh")),
        ImageRefusal::NotTheImage
    );
    assert_eq!(
        refusal(start_with(HARDENED_WITHOUT_KEYCHAIN, "/bin/sleep")),
        ImageRefusal::NotTheImage,
        "an unhardened child was admitted"
    );
}

/// The live check on a child as the host checks it: spawned suspended, so its loader hasn't
/// run. Its code object is found and checked all the same: it meets a requirement it
/// satisfies (Apple's anchor), and a Developer ID requirement refuses it.
#[test]
fn a_suspended_child_is_checked_as_a_running_one_is() {
    let args = [OsStr::new("30")];
    let mut child = spawn::spawn_suspended(Path::new("/bin/sleep"), &args, &[]).unwrap();
    let pid = child.process.id();
    let token = spawn::audit_token_of_child(pid).unwrap();
    let met =
        code::code_for_audit_token(&token).is_some_and(|code| code::meets(&code, "anchor apple"));
    let check = admission(release("/bin/sleep", NOBODY, HARDENED));
    let verdict = check.admit(pid);
    child.process.kill();
    assert!(
        met,
        "a suspended child's code object wasn't found or checked"
    );
    assert_eq!(verdict, Err(ImageRefusal::NotTheImage));
}

/// The live check on a real, running child: `/bin/sleep` (Apple's) is refused by a Developer
/// ID requirement, and its re-check passes while it is still the same image.
#[test]
fn a_real_child_that_isnt_the_expected_image_is_refused() {
    let mut running = std::process::Command::new("/bin/sleep")
        .arg("30")
        .spawn()
        .unwrap();
    let check = admission(release("/bin/sleep", NOBODY, HARDENED));
    let verdict = check.admit(running.id());
    // Its token is real, and it does satisfy a requirement it meets (so the plumbing works).
    let token = spawn::audit_token_of_child(running.id()).unwrap();
    let met =
        code::code_for_audit_token(&token).is_some_and(|code| code::meets(&code, "anchor apple"));
    let admitted = Child::unowned(
        running.id(),
        Some(Peer {
            macos_audit_token: token,
        }),
    )
    .unwrap();
    let still = check.recheck(&admitted);
    running.kill().unwrap();
    running.wait().unwrap();
    assert_eq!(verdict, Err(ImageRefusal::NotTheImage));
    assert!(met, "the plumbing didn't accept /bin/sleep as Apple's");
    assert_eq!(still, Ok(()));
}

/// A pin is checked against the hash of the image the child really loaded, on top of a
/// scripted or a skipped check, at the admission and at every re-check.
#[test]
fn a_pin_checks_the_actual_child_kernel_hash_and_never_accepts_an_unidentified_child() {
    let me = std::env::current_exe().unwrap();
    let me = me.to_str().unwrap();
    // Every child of this test is this test binary held suspended: one loaded image.
    let probe = spawn::spawn_suspended(Path::new(me), &[], &[]).unwrap();
    let expected = org_platform::image::loaded_cdhash_for_audit_token(
        &spawn::audit_token_of_child(probe.process.id()).unwrap(),
    )
    .unwrap();
    drop(probe);
    assert_eq!(expected, org_platform::image::own_loaded_cdhash().unwrap());
    let pin = |hash: [u8; 20]| ExpectedImage {
        macos_cdhash: Some(hash),
        ..development(me)
    };
    let real = |_: Step, pid: u32| child_peer(pid).map(Some).map_err(|e| e.to_string());
    let pinned = scripted(pin(expected), real);
    let held = start(&pinned, &[]).unwrap();
    let token = held.child.0.admitted.expect("identified by the script");
    assert_eq!(token, spawn::audit_token_of_child(held.child.id()).unwrap());
    pinned.recheck(&held.child.0).unwrap();
    pinned.readmit(&held.child.0).unwrap();
    let mut different = expected;
    different[0] ^= 1;
    assert_eq!(
        refusal(start(&scripted(pin(different), real), &[])),
        ImageRefusal::DigestMismatch
    );
    // A development image admitted without a check is never the pinned image.
    assert_eq!(
        refusal(start(&admission(pin(expected)), &[])),
        ImageRefusal::Unidentified
    );
    // The re-check refuses a different pin, and a token that changed.
    assert_eq!(
        scripted(pin(different), real).recheck(&held.child.0),
        Err(ImageRefusal::DigestMismatch)
    );
    let mut other = token;
    other[0] ^= 1;
    let changed = scripted(pin(expected), move |step, _| {
        Ok(Some(Peer {
            macos_audit_token: if step == Step::Still { other } else { token },
        }))
    });
    assert_eq!(changed.recheck(&held.child.0), Err(ImageRefusal::Changed));
    assert_eq!(changed.readmit(&held.child.0), Ok(()));
    // A second admission that names another image than the first is refused.
    let renamed = scripted(pin(expected), move |_, _| {
        Ok(Some(Peer {
            macos_audit_token: other,
        }))
    });
    assert!(renamed.readmit(&held.child.0).is_err());
    // Once the child is gone, nothing about it passes.
    let gone = Child::unowned(
        held.child.id(),
        Some(Peer {
            macos_audit_token: token,
        }),
    )
    .unwrap();
    drop(held); // kill and reap this generated suspended test image
    assert!(pinned.readmit(&gone).is_err());
    assert!(pinned.recheck(&gone).is_err());
}

/// The scripted check stands in for the system's at each step, in the adapter's own order:
/// the admission while the child is still suspended, the re-check only for an identified
/// child, and its answers worded as the system's would be.
#[test]
fn a_scripted_check_is_asked_at_each_step_and_its_refusals_are_kept() {
    let steps = Arc::new(Mutex::new(Vec::new()));
    let seen = Arc::clone(&steps);
    let refusing = scripted(development("/bin/sleep"), move |step, pid| {
        let out = std::process::Command::new("/bin/ps")
            .args(["-o", "state=", "-p", &pid.to_string()])
            .output()
            .unwrap();
        seen.lock()
            .unwrap()
            .push((step, String::from_utf8_lossy(&out.stdout).trim().to_owned()));
        Err("refused by the test".into())
    });
    assert_eq!(
        refusal(start(&refusing, &["30"])),
        ImageRefusal::Scripted("refused by the test".into())
    );
    let steps = steps.lock().unwrap().clone();
    assert_eq!(steps.len(), 1, "{steps:?}");
    assert_eq!(steps[0].0, Step::Admit);
    assert!(
        steps[0].1.starts_with('T'),
        "not stopped at the check: {steps:?}"
    );
    // Admitted without an identity: the re-check asks nothing and passes.
    let unidentified = scripted(development("/bin/sleep"), |step, _| match step {
        Step::Admit => Ok(None),
        Step::Still => panic!("asked for the token of an unidentified child"),
    });
    let held = start(&unidentified, &["30"]).unwrap();
    unidentified.recheck(&held.child.0).unwrap();
    // Identified: a re-check without a token, with another token, or refused.
    let token = spawn::audit_token_of_child(held.child.id()).unwrap();
    let identified = Child::unowned(
        held.child.id(),
        Some(Peer {
            macos_audit_token: token,
        }),
    )
    .unwrap();
    let answering = |answer: Result<Option<Peer>, String>| {
        scripted(development("/bin/sleep"), move |_, _| answer.clone())
    };
    assert_eq!(
        answering(Ok(None)).recheck(&identified),
        Err(ImageRefusal::Identity("no token".into()))
    );
    assert_eq!(
        answering(Err("gone".into())).recheck(&identified),
        Err(ImageRefusal::Identity("gone".into()))
    );
    assert_eq!(
        answering(Ok(Some(Peer {
            macos_audit_token: [9; 32]
        })))
        .recheck(&identified),
        Err(ImageRefusal::Changed)
    );
    assert_eq!(
        answering(Ok(Some(Peer {
            macos_audit_token: token
        })))
        .recheck(&identified),
        Ok(())
    );
    // A re-admission that now identifies a child admitted without an identity is refused.
    assert_eq!(
        answering(Ok(Some(Peer {
            macos_audit_token: token
        })))
        .readmit(&held.child.0),
        Err(ImageRefusal::Readmitted)
    );
}

/// The reader of a connection is the child only when the peer's audit token names the
/// child's pid and, for an identified child, is exactly the admitted token.
#[test]
fn the_reader_is_the_child_only_by_pid_and_admitted_token() {
    let (a, _b) = UnixStream::pair().unwrap();
    let me = std::process::id();
    let token = audit_token(&a).unwrap();
    let any = admission(development("/bin/sh"));
    let child = |pid: u32, admitted: Option<AuditToken>| {
        Child::unowned(
            pid,
            admitted.map(|macos_audit_token| Peer { macos_audit_token }),
        )
        .unwrap()
    };
    // This process holds both ends: it is the peer of its own connection.
    any.verify_reader(&child(me, None), &a).unwrap();
    any.verify_reader(&child(me, Some(token)), &a).unwrap();
    // Another pid, or another image of the same pid.
    let mut other = token;
    other[28] ^= 1;
    for wrong in [
        child(me + 1, None),
        child(me + 1, Some(token)),
        child(me, Some(other)),
    ] {
        let error = any.verify_reader(&wrong, &a).unwrap_err();
        assert!(NotTheChild::is(&error), "{error}");
    }
    // A descriptor that is no socket: the system's error, not a mismatch.
    let file = std::fs::File::open("/dev/null").unwrap();
    let not_a_socket = UnixStream::from(std::os::fd::OwnedFd::from(file));
    let error = any
        .verify_reader(&child(me, None), &not_a_socket)
        .unwrap_err();
    assert!(!NotTheChild::is(&error), "{error}");
    // A real child that connects: its own connection passes, another process's does not.
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("s");
    let listener = std::os::unix::net::UnixListener::bind(&path).unwrap();
    let script = format!(
        "zmodload zsh/net/socket && zsocket '{}' && read -r -t 20 -u $REPLY _",
        path.display()
    );
    let zsh = admission(development("/bin/zsh"));
    let started = crate::admission::Held(start(&zsh, &["-f", "-c", &script]).unwrap())
        .release()
        .unwrap();
    let (connected, _) = listener.accept().unwrap();
    zsh.verify_reader(&started.child.0, &connected).unwrap();
    let error = zsh.verify_reader(&started.child.0, &a).unwrap_err();
    assert!(NotTheChild::is(&error), "{error}");
}

#[test]
fn the_signing_information_of_this_process_is_what_its_own_token_names() {
    let (a, _b) = UnixStream::pair().unwrap();
    let by_token = code::code_for_audit_token(&audit_token(&a).unwrap())
        .and_then(|code| code::signing(&code))
        .map(|signing| signing.flags);
    assert_eq!(own_signing().map(|signing| signing.flags), by_token);
}

#[test]
fn static_signature_query_addresses_the_held_descriptor_not_replaced_path() {
    let dir = tempfile::tempdir_in("/private/tmp").unwrap();
    let path = dir.path().join("image");
    std::fs::copy(std::env::current_exe().unwrap(), &path).unwrap();
    let file = File::open(&path).unwrap();
    let original = code::static_code(&file).unwrap();
    let (_, expected) = code::static_signing(&original).unwrap();
    std::fs::rename(&path, dir.path().join("retained")).unwrap();
    std::fs::write(&path, b"unsigned replacement").unwrap();
    let held = code::static_code(&file).unwrap();
    let (_, actual) = code::static_signing(&held).unwrap();
    assert_eq!(actual, expected);
    assert_eq!(actual, org_platform::image::own_loaded_cdhash().unwrap());
    let replacement = File::open(&path).unwrap();
    // Creating a static-code object alone does not establish a signature.
    assert!(code::static_signing(&code::static_code(&replacement).unwrap()).is_none());
    // Through the contract's entry: the held file's signature is established only under a
    // requirement it satisfies; the replacement's never is.
    let images = StaticImages::new().unwrap();
    if let Some(identifier) = code::own_codesign_field("Identifier=") {
        let found = images
            .signature(&file, &format!("identifier \"{identifier}\""))
            .unwrap();
        assert_eq!(found.cdhash, expected);
    }
    assert_eq!(
        images.signature(&file, NOBODY).err(),
        Some(StaticImageError::Refused)
    );
    assert_eq!(
        images.signature(&file, "not a requirement (").err(),
        Some(StaticImageError::Refused)
    );
    assert_eq!(
        images.signature(&replacement, "anchor apple").err(),
        Some(StaticImageError::Refused)
    );
}

/// An environment is passed exactly as given: the adapter adds and removes nothing.
#[test]
fn the_environment_of_a_child_is_exactly_its_child_env() {
    let mut env = ChildEnv::new();
    env.set(OsString::from("A"), OsString::from("1")).unwrap();
    env.set("B", "two words").unwrap();
    let sh = admission(development("/usr/bin/env"));
    let mut started = crate::admission::Held(sh.start(&[], &env, &Mutex::new(())).unwrap())
        .release()
        .unwrap();
    let mut out = String::new();
    started.stdout.read_to_string(&mut out).unwrap();
    let mut lines: Vec<&str> = out.lines().collect();
    lines.sort_unstable();
    assert_eq!(lines, ["A=1", "B=two words"]);
    started.stdin.write_all(b"").unwrap();
}
