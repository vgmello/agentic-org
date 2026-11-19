use super::*;
use org_platform_host::admission::{ChildEnv, Held, Peer, SpawnRefused};
use org_platform_host::testing::admission::{child_peer, unowned_child};
use std::sync::Arc;

/// Whether this system identifies an image by its code signature (macOS), the identity
/// every test here pins. Elsewhere they have nothing to check.
fn code_signatures() -> bool {
    org_platform_host::admission::image_identity() == Ok(ImageIdentity::CodeSignature)
}

/// Starts a child of `check` from this test binary, checked and held (never let run).
fn start(check: &ServerCheck) -> Result<Held, ImageRefusal> {
    check
        .admission(&std::env::current_exe().unwrap())
        .unwrap()
        .start(&[], &ChildEnv::new(), &crate::cloexec::SPAWN_LOCK)
        .map_err(|error| {
            SpawnRefused::of(&error)
                .expect("an image refusal")
                .refusal
                .clone()
        })
}

#[test]
fn selected_pin_checks_actual_child_kernel_hash_and_never_accepts_debug_skip() {
    if !code_signatures() {
        return;
    }
    let me = std::env::current_exe().unwrap();
    // The script admits every child with its real token.
    let platform = ServerCheck::Scripted(Scripted(Arc::new(|_, pid| {
        child_peer(pid)
            .map(|peer| Some(peer.macos_audit_token))
            .map_err(|e| e.to_string())
    })));
    let child = start(&platform).unwrap();
    let pid = child.child().id();
    let token = child_peer(pid).unwrap().macos_audit_token;
    let expected = org_platform::image::loaded_cdhash_for_audit_token(&token).unwrap();
    // A pin of this system's identity: the loaded image's CodeDirectory hash. The file's
    // digest is the other identity of a manifest entry, which this system does not read.
    let pin = |cdhash: [u8; 20]| Pin {
        sha256: [0; 32],
        macos_cdhash: Some(cdhash),
    };
    let pinned = platform.clone().pinned(pin(expected));
    let admission = |check: &ServerCheck| check.admission(&me).unwrap();
    // Every child here is this test binary: the pin admits a fresh one, and re-checks this one.
    drop(start(&pinned).unwrap());
    admission(&pinned).recheck(child.child()).unwrap();
    admission(&pinned).readmit(child.child()).unwrap();
    let mut different = expected;
    different[0] ^= 1;
    assert_eq!(
        start(&platform.clone().pinned(pin(different))).err(),
        Some(ImageRefusal::DigestMismatch)
    );
    assert_eq!(
        start(&ServerCheck::Skip.pinned(pin(expected))).err(),
        Some(ImageRefusal::Unidentified)
    );
    // The pre-first-line re-check refuses a different image, and the inner check's refusal.
    assert!(admission(&platform.clone().pinned(pin(different)))
        .recheck(child.child())
        .is_err());
    let mut other = token;
    other[0] ^= 1;
    let changed = ServerCheck::Scripted(Scripted(Arc::new(move |step, _| {
        Ok(Some(if step == Step::Still { other } else { token }))
    })));
    let refusal = admission(&changed.clone().pinned(pin(expected)))
        .recheck(child.child())
        .unwrap_err();
    assert!(changed.why(&refusal).contains("changed"), "{refusal:?}");
    // The same child as a handle that outlives it: once it is gone, nothing about it passes.
    let gone = unowned_child(pid, Some(Peer::from_macos_audit_token(token))).unwrap();
    drop(child); // kill and reap this generated suspended test image
    assert!(admission(&pinned).readmit(&gone).is_err());
    assert!(admission(&pinned).recheck(&gone).is_err());
}

#[test]
fn a_refused_or_pinned_check_never_admits_through_the_plain_decision() {
    if !code_signatures() {
        return;
    }
    let me = std::env::current_exe().unwrap();
    // A refused check names no image: no admission exists, so nothing can be started.
    assert!(ServerCheck::WebRefused("refused").expected(&me).is_err());
    assert!(ServerCheck::WebRefused("refused").admission(&me).is_err());
    // A pinned check is decided only by the live kernel-image adapter: a debug build's
    // unchecked child is never the pinned image.
    assert_eq!(
        start(&ServerCheck::Skip.pinned(Pin {
            sha256: [0; 32],
            macos_cdhash: Some([0; 20]),
        }))
        .err(),
        Some(ImageRefusal::Unidentified)
    );
}
