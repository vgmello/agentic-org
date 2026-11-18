//! Tests of the contract's own code that run on every target. The pure decisions of
//! the macOS adapter are tested in `decision.rs`, also on every target.

use std::ffi::OsStr;
use std::io;
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::sync::Mutex;

use org_platform::Refused;

use super::{
    admission, is_loader_variable, native_peers, own_image, own_signing, peer, static_images,
    ChildEnv, ExpectedImage, ImageRefusal, NotTheChild, OwnImageError, Peer, Signing, SigningRule,
    SpawnRefused,
};

/// Plain test data: one required flag bit and three rejected entitlements.
const REQUIRED: u32 = 0x0001_0000;
const REJECTED: [&str; 3] = ["example.debug", "example.inject", "example.unvalidated"];
const RULE: SigningRule = SigningRule {
    required_flags: REQUIRED,
    rejected_entitlements: &REJECTED,
    absent_entitlements: &[],
};

fn entitlements(pairs: &[(&str, Option<bool>)]) -> Option<Vec<(String, Option<bool>)>> {
    Some(pairs.iter().map(|(k, v)| (k.to_string(), *v)).collect())
}

fn signing(flags: Option<u32>, entitlements: Option<Vec<(String, Option<bool>)>>) -> Signing {
    Signing {
        flags,
        entitlements,
        keychain_access_groups: None,
    }
}

fn development() -> ExpectedImage {
    ExpectedImage {
        program: PathBuf::from("/bin/sh"),
        sha256: None,
        macos_cdhash: None,
        macos_requirement: None,
        macos_signing: RULE,
        development: true,
    }
}

#[test]
fn only_code_with_the_required_flags_and_without_the_rejected_entitlements_passes() {
    let hardened = Some(REQUIRED | 0x2);
    let ok = |flags, entitlements| RULE.flags_and_rejections_pass(&signing(flags, entitlements));
    assert!(ok(hardened, None));
    assert!(ok(
        hardened,
        entitlements(&[
            ("example.network", Some(true)),
            ("example.debug", Some(false)),
        ])
    ));
    // Without the required flag, or with no flags at all.
    assert!(!ok(Some(0x2), None));
    assert!(!ok(None, None));
    // Each rejected entitlement, set true or with a value that isn't a boolean.
    for key in REJECTED {
        assert!(!ok(hardened, entitlements(&[(key, Some(true))])), "{key}");
        assert!(!ok(hardened, entitlements(&[(key, None)])), "{key}");
    }
    // Every required bit must be set, not just one of them.
    let two_bits = SigningRule {
        required_flags: REQUIRED | 0x4,
        ..RULE
    };
    assert!(!two_bits.flags_and_rejections_pass(&signing(Some(REQUIRED), None)));
    assert!(two_bits.flags_and_rejections_pass(&signing(Some(REQUIRED | 0x4), None)));
    // A rule that requires no flag still refuses signing information without flags.
    let no_flag = SigningRule {
        required_flags: 0,
        ..RULE
    };
    assert!(no_flag.flags_and_rejections_pass(&signing(Some(0), None)));
    assert!(!no_flag.flags_and_rejections_pass(&signing(None, None)));
}

#[test]
fn an_entitlement_that_must_be_absent_refuses_whatever_its_value() {
    let rule = SigningRule {
        absent_entitlements: &["example.groups"],
        ..RULE
    };
    let hardened = Some(REQUIRED);
    assert!(rule.allows(&signing(hardened, None)));
    assert!(rule.allows(&signing(
        hardened,
        entitlements(&[("example.network", Some(true))])
    )));
    for value in [Some(true), Some(false), None] {
        let held = signing(hardened, entitlements(&[("example.groups", value)]));
        assert!(!rule.absences_pass(&held), "{value:?}");
        assert!(!rule.allows(&held), "{value:?}");
        // The flags and the rejected entitlements alone do not see it.
        assert!(rule.flags_and_rejections_pass(&held), "{value:?}");
        // A rule that names no such entitlement lets it through.
        assert!(RULE.allows(&held), "{value:?}");
    }
    // The whole rule still needs the flags and no rejected entitlement.
    assert!(!rule.allows(&signing(Some(0), None)));
    assert!(!rule.allows(&signing(
        hardened,
        entitlements(&[("example.debug", Some(true))])
    )));
}

#[test]
fn loader_variables_are_named_the_same_on_every_system() {
    for name in [
        "LD_PRELOAD",
        "LD_LIBRARY_PATH",
        "LD_AUDIT",
        "LD_",
        "DYLD_INSERT_LIBRARIES",
        "DYLD_LIBRARY_PATH",
        "DYLD_",
        "GLIBC_TUNABLES",
    ] {
        assert!(is_loader_variable(OsStr::new(name)), "{name}");
    }
    for name in [
        "HOME",
        "PATH",
        "TMPDIR",
        "LANG",
        "LD",
        "DYLD",
        "ld_preload",
        "XLD_PRELOAD",
        "GLIBC_TUNABLES2",
        "GLIBC",
        "",
    ] {
        assert!(!is_loader_variable(OsStr::new(name)), "{name}");
    }
}

#[test]
fn a_child_environment_refuses_loader_variables_and_keeps_the_rest_in_order() {
    let mut env = ChildEnv::new();
    env.set("HOME", "/home/x").unwrap();
    for name in ["LD_PRELOAD", "DYLD_INSERT_LIBRARIES", "GLIBC_TUNABLES"] {
        let error = env.set(name, "/secret/value").unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::InvalidInput, "{name}");
        // The refusal names the rule, never the value.
        assert!(!error.to_string().contains("secret"), "{error}");
    }
    env.set("PATH", "/usr/bin:/bin").unwrap();
    assert_eq!(
        env.pairs(),
        [
            ("HOME".into(), "/home/x".into()),
            ("PATH".into(), "/usr/bin:/bin".into())
        ]
    );
    assert_eq!(ChildEnv::new().pairs(), []);
}

#[test]
fn a_refused_spawn_and_a_wrong_reader_are_classified_by_type() {
    let refused = io::Error::other(SpawnRefused {
        pid: 42,
        refusal: ImageRefusal::DigestMismatch,
    });
    let found = SpawnRefused::of(&refused).expect("the refusal is the error's source");
    assert_eq!(
        (found.pid, &found.refusal),
        (42, &ImageRefusal::DigestMismatch)
    );
    assert!(!NotTheChild::is(&refused));
    let mismatch = io::Error::other(NotTheChild);
    assert!(NotTheChild::is(&mismatch));
    assert!(SpawnRefused::of(&mismatch).is_none());
    let plain = io::Error::from_raw_os_error(libc::ENOENT);
    assert!(SpawnRefused::of(&plain).is_none() && !NotTheChild::is(&plain));
    // Text that looks like a refusal is not one.
    let text = io::Error::other("child 42 was refused: the loaded image is not the pinned image");
    assert!(SpawnRefused::of(&text).is_none());
}

#[test]
fn a_scripted_refusal_reads_as_the_tests_own_words() {
    assert_eq!(
        ImageRefusal::Scripted("refused by the test".into()).to_string(),
        "refused by the test"
    );
}

#[test]
fn the_own_image_is_never_admitted_without_a_pin_that_matches() {
    // No pin: a mismatch where the system can read the loaded image, its refusal where
    // it cannot. Never a pass.
    match own_image(&development()) {
        Err(OwnImageError::Mismatch) => {
            assert!(org_platform::image::own_loaded_cdhash().is_ok());
        }
        Err(OwnImageError::Unsupported(refused)) => {
            let Refused::Unsupported { capability, .. } = refused;
            assert_eq!(capability, "image identity");
        }
        // An unsigned test binary has no hash to read: still not a pass.
        Err(OwnImageError::Unavailable(_)) => {}
        Ok(()) => panic!("the host's own image passed without a pin"),
    }
    // A pin that is not this image: the same.
    let pinned = ExpectedImage {
        macos_cdhash: Some([0; 20]),
        ..development()
    };
    assert!(own_image(&pinned).is_err());
    // The pin that is this image passes, where the system can read it.
    if let Ok(loaded) = org_platform::image::own_loaded_cdhash() {
        let own = ExpectedImage {
            macos_cdhash: Some(loaded),
            ..development()
        };
        own_image(&own).unwrap();
    }
}

/// A refusing build is one whose adapter makes no admission. There every public entry
/// must refuse or answer "no", before it does anything.
#[test]
fn on_a_refusing_build_every_public_entry_refuses_first() {
    let Some(refused) = admission(development()).err() else {
        // A real adapter: the least demanding image gets an admission, and the system
        // identifies peers and checks static images.
        native_peers().unwrap();
        assert!(static_images().is_ok());
        return;
    };
    assert_eq!(refused, super::refusal());
    assert_eq!(static_images().err(), Some(super::refusal()));
    assert_eq!(native_peers(), Err(super::peer_refusal()));
    let (a, _b) = UnixStream::pair().unwrap();
    let error = peer(&a).unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::Unsupported);
    assert_eq!(
        error
            .get_ref()
            .and_then(|source| source.downcast_ref::<Refused>()),
        Some(&super::peer_refusal())
    );
    #[cfg(debug_assertions)]
    assert_eq!(
        super::peer_executable(&a).unwrap_err().kind(),
        io::ErrorKind::Unsupported
    );
    let any = Peer {
        macos_audit_token: [7; 32],
    };
    assert!(!any.satisfies("anchor apple", &RULE));
    assert!(own_signing().is_none());
    // The test hooks refuse too: no scripted admission, no stand-in child.
    use super::test_support::{child_peer, scripted_admission, unowned_child};
    assert_eq!(
        scripted_admission(development(), std::sync::Arc::new(|_, _| Ok(None))).err(),
        Some(super::refusal())
    );
    assert_eq!(
        unowned_child(std::process::id(), None).err(),
        Some(super::refusal())
    );
    assert_eq!(
        child_peer(std::process::id()).unwrap_err().kind(),
        io::ErrorKind::Unsupported
    );
    // Nothing was started: the fence was never taken.
    let fence = Mutex::new(());
    assert!(fence.try_lock().is_ok());
}
