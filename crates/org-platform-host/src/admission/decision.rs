//! The decisions of an adapter that identifies a child after it was started, pure.
//!
//! The macOS adapter starts a child suspended and identifies it by its audit token.
//! What it then decides is here, over injected probes, so that the decisions are
//! tested on every target and never ask the system for more than they need. The
//! adapter supplies the live probes (`macos.rs`).

use std::io;

use super::{ExpectedImage, ImageRefusal, SigningRule};

/// A process image as the system names it (an `audit_token_t`).
pub(super) type Token = [u8; 32];

/// The check of a child before its image is pinned: its token if it may be admitted
/// (`None`: not checked, a development image), or why not.
///
/// `token` gives the child's token; `satisfies` checks the code it names against the
/// requirement and the flags and rejected entitlements of the rule, on one code
/// object; `lacks` whether that code holds none of the entitlements that must be
/// absent. None is asked unless the check needs it, and `lacks` only when the rule
/// names such an entitlement.
pub(super) fn admit(
    expected: &ExpectedImage,
    token: impl FnOnce() -> Result<Token, String>,
    satisfies: impl FnOnce(&Token, &str, &SigningRule) -> bool,
    lacks: impl FnOnce(&Token, &[&str]) -> bool,
) -> Result<Option<Token>, ImageRefusal> {
    let Some(requirement) = &expected.macos_requirement else {
        return if expected.development {
            Ok(None)
        } else {
            // A release image with nothing to check it against is never admitted.
            Err(ImageRefusal::NotTheImage)
        };
    };
    let rule = &expected.macos_signing;
    let token = token().map_err(ImageRefusal::Identity)?;
    if satisfies(&token, requirement, rule)
        && (rule.absent_entitlements.is_empty() || lacks(&token, rule.absent_entitlements))
    {
        Ok(Some(token))
    } else {
        Err(ImageRefusal::NotTheImage)
    }
}

/// Whether `expected` is a release image pinned only by a digest of its bytes, which an
/// adapter that checks the loaded image cannot read: nothing this system can check pins
/// it. A development image is its own build output and is not judged by this.
fn pinned_by_bytes_alone(expected: &ExpectedImage) -> bool {
    !expected.development && expected.sha256.is_some() && expected.macos_cdhash.is_none()
}

/// The pin on top of an admission: `admitted` is what the check before it answered.
/// A pinned image must have been identified, and the hash of the image its token
/// names (`digest`) must be the pinned one. A release image pinned only by a digest of
/// its bytes is never admitted here ([`ImageRefusal::Unidentified`]): this adapter has
/// no way to check that pin, and treating it as no pin would admit an image the release
/// does not name.
pub(super) fn pinned(
    expected: &ExpectedImage,
    admitted: Result<Option<Token>, ImageRefusal>,
    digest: impl FnOnce(&Token) -> io::Result<[u8; 20]>,
) -> Result<Option<Token>, ImageRefusal> {
    let Some(pin) = expected.macos_cdhash else {
        if pinned_by_bytes_alone(expected) {
            // The refusal of the check before the pin still wins.
            admitted?;
            return Err(ImageRefusal::Unidentified);
        }
        return admitted;
    };
    let token = admitted?.ok_or(ImageRefusal::Unidentified)?;
    if digest(&token).map_err(|_| ImageRefusal::DigestUnavailable)? != pin {
        return Err(ImageRefusal::DigestMismatch);
    }
    Ok(Some(token))
}

/// Whether the child's token `now` is still the one admitted.
pub(super) fn unchanged(admitted: Token, now: Result<Token, String>) -> Result<(), ImageRefusal> {
    match now {
        Ok(now) if now == admitted => Ok(()),
        Ok(_) => Err(ImageRefusal::Changed),
        Err(why) => Err(ImageRefusal::Identity(why)),
    }
}

/// The admission of a running child once more: `again` is what the whole check
/// (requirement and pin) answers now, and it must be what the child was admitted with.
pub(super) fn readmitted(
    admitted: Option<Token>,
    again: Result<Option<Token>, ImageRefusal>,
) -> Result<(), ImageRefusal> {
    if again? != admitted {
        return Err(ImageRefusal::Readmitted);
    }
    Ok(())
}

/// The re-check of a running child: still the image `admitted` (`now` is asked only
/// when it was identified), and, when the image is pinned, identified and still the
/// pinned image.
pub(super) fn still(
    expected: &ExpectedImage,
    admitted: Option<Token>,
    now: impl FnOnce() -> Result<Token, String>,
    digest: impl FnOnce(&Token) -> io::Result<[u8; 20]>,
) -> Result<(), ImageRefusal> {
    match admitted {
        None => {}
        Some(admitted) => unchanged(admitted, now())?,
    }
    let Some(pin) = expected.macos_cdhash else {
        if pinned_by_bytes_alone(expected) {
            return Err(ImageRefusal::Unidentified);
        }
        return Ok(());
    };
    let token = admitted.ok_or(ImageRefusal::Unidentified)?;
    if digest(&token).map_err(|_| ImageRefusal::DigestUnavailable)? != pin {
        return Err(ImageRefusal::DigestMismatch);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;
    use std::path::PathBuf;

    /// Plain test data: one required flag bit, one rejected entitlement.
    const RULE: SigningRule = SigningRule {
        required_flags: 0x0001_0000,
        rejected_entitlements: &["example.rejected"],
        absent_entitlements: &[],
    };
    /// The same rule with an entitlement that must be absent.
    const RULE_WITH_ABSENCE: SigningRule = SigningRule {
        absent_entitlements: &["example.absent"],
        ..RULE
    };

    fn release(requirement: &str, rule: SigningRule) -> ExpectedImage {
        ExpectedImage {
            program: PathBuf::from("/x/program"),
            sha256: None,
            macos_cdhash: None,
            macos_requirement: Some(requirement.to_owned()),
            macos_signing: rule,
            development: false,
        }
    }

    fn development() -> ExpectedImage {
        ExpectedImage {
            macos_requirement: None,
            development: true,
            ..release("unused", RULE)
        }
    }

    /// An absence probe for checks that must never ask it.
    fn no_absence(_: &Token, _: &[&str]) -> bool {
        panic!("asked for the entitlements that must be absent")
    }

    fn no_digest(_: &Token) -> io::Result<[u8; 20]> {
        panic!("asked for the loaded image's hash")
    }

    #[test]
    fn a_child_is_admitted_only_with_a_token_that_satisfies_the_requirement() {
        let expected = release("R", RULE);
        let asked = Cell::new(None);
        let yes = |_: &Token, r: &str, rule: &SigningRule| {
            asked.set(Some((r.to_owned(), *rule)));
            true
        };
        assert_eq!(
            admit(&expected, || Ok([7; 32]), yes, no_absence),
            Ok(Some([7; 32]))
        );
        assert_eq!(asked.take(), Some(("R".to_owned(), RULE)));
        assert_eq!(
            admit(&expected, || Ok([7; 32]), |_, _, _| false, no_absence),
            Err(ImageRefusal::NotTheImage)
        );
        // No token: refused, and the requirement isn't even checked.
        assert_eq!(
            admit(
                &expected,
                || Err("gone".into()),
                |_, _, _| panic!("checked"),
                no_absence,
            ),
            Err(ImageRefusal::Identity("gone".into()))
        );
        // A development image skips the check and asks for nothing.
        let never = || -> Result<Token, String> { panic!("asked for a token") };
        assert_eq!(
            admit(&development(), never, |_, _, _| panic!(), no_absence),
            Ok(None)
        );
        // A release image with no requirement is refused, and asks for nothing.
        let unruled = ExpectedImage {
            development: false,
            ..development()
        };
        assert_eq!(
            admit(&unruled, never, |_, _, _| panic!(), no_absence),
            Err(ImageRefusal::NotTheImage)
        );
        // A requirement is checked even when the image is marked development.
        let both = ExpectedImage {
            development: true,
            ..release("R", RULE)
        };
        assert_eq!(
            admit(&both, || Ok([7; 32]), |_, _, _| false, no_absence),
            Err(ImageRefusal::NotTheImage)
        );
    }

    #[test]
    fn an_image_is_admitted_only_without_an_entitlement_that_must_be_absent() {
        let expected = release("R", RULE_WITH_ABSENCE);
        let probed = Cell::new(None);
        let probe = |free: bool| {
            let probed = &probed;
            move |token: &Token, absent: &[&str]| {
                probed.set(Some((*token, absent.join(","))));
                free
            }
        };
        let asked = Cell::new(None);
        let satisfied = |_: &Token, r: &str, _: &SigningRule| {
            asked.set(Some(r.to_owned()));
            true
        };
        // A satisfying signature without the entitlement is admitted.
        assert_eq!(
            admit(&expected, || Ok([7; 32]), satisfied, probe(true)),
            Ok(Some([7; 32]))
        );
        assert_eq!(asked.take().as_deref(), Some("R"));
        assert_eq!(probed.take(), Some(([7; 32], "example.absent".to_owned())));
        // The same satisfying signature holding the entitlement is refused.
        assert_eq!(
            admit(&expected, || Ok([7; 32]), |_, _, _| true, probe(false)),
            Err(ImageRefusal::NotTheImage)
        );
        assert_eq!(probed.take(), Some(([7; 32], "example.absent".to_owned())));
        // A signature that does not satisfy is refused before the probe is asked.
        assert_eq!(
            admit(&expected, || Ok([7; 32]), |_, _, _| false, no_absence),
            Err(ImageRefusal::NotTheImage)
        );
        // A rule that names no such entitlement never asks the probe.
        assert_eq!(
            admit(
                &release("R", RULE),
                || Ok([7; 32]),
                |_, _, _| true,
                no_absence
            ),
            Ok(Some([7; 32]))
        );
    }

    #[test]
    fn a_child_that_exec_ed_after_the_check_is_refused() {
        assert!(unchanged([7; 32], Ok([7; 32])).is_ok());
        assert_eq!(unchanged([7; 32], Ok([8; 32])), Err(ImageRefusal::Changed));
        assert_eq!(
            unchanged([7; 32], Err("gone".into())),
            Err(ImageRefusal::Identity("gone".into()))
        );
    }

    #[test]
    fn a_pin_needs_an_identified_child_whose_loaded_image_is_the_pinned_one() {
        let pin = ExpectedImage {
            macos_cdhash: Some([3; 20]),
            ..development()
        };
        let asked = Cell::new(None);
        let hash = |answer: [u8; 20]| {
            let asked = &asked;
            move |token: &Token| {
                asked.set(Some(*token));
                Ok(answer)
            }
        };
        assert_eq!(
            pinned(&pin, Ok(Some([7; 32])), hash([3; 20])),
            Ok(Some([7; 32]))
        );
        assert_eq!(asked.take(), Some([7; 32]));
        assert_eq!(
            pinned(&pin, Ok(Some([7; 32])), hash([4; 20])),
            Err(ImageRefusal::DigestMismatch)
        );
        assert_eq!(
            pinned(&pin, Ok(Some([7; 32])), |_| Err(io::Error::other("gone"))),
            Err(ImageRefusal::DigestUnavailable)
        );
        // A development image admitted without a check is never the pinned image.
        assert_eq!(
            pinned(&pin, Ok(None), no_digest),
            Err(ImageRefusal::Unidentified)
        );
        // The refusal of the check before the pin wins, and no hash is read.
        assert_eq!(
            pinned(&pin, Err(ImageRefusal::NotTheImage), no_digest),
            Err(ImageRefusal::NotTheImage)
        );
        // No pin: the admission stands as it is, and no hash is read.
        for admitted in [Ok(None), Ok(Some([7; 32])), Err(ImageRefusal::NotTheImage)] {
            assert_eq!(
                pinned(&development(), admitted.clone(), no_digest),
                admitted
            );
        }
    }

    /// A release image pinned only by the SHA-256 of its bytes names a pin this adapter
    /// cannot check: it is refused at the pin and at every re-check, identified or not,
    /// and no hash is read. The refusal of the check before the pin still wins. A
    /// development image with such a digest, and a release image with both identities,
    /// are judged as before.
    #[test]
    fn a_release_image_pinned_only_by_its_bytes_is_never_admitted() {
        let never = || -> Result<Token, String> { panic!("asked for a token") };
        let bytes_only = ExpectedImage {
            sha256: Some([9; 32]),
            ..release("R", RULE)
        };
        for admitted in [Ok(Some([7; 32])), Ok(None)] {
            assert_eq!(
                pinned(&bytes_only, admitted, no_digest),
                Err(ImageRefusal::Unidentified)
            );
        }
        assert_eq!(
            pinned(&bytes_only, Err(ImageRefusal::NotTheImage), no_digest),
            Err(ImageRefusal::NotTheImage)
        );
        assert_eq!(
            still(&bytes_only, None, never, no_digest),
            Err(ImageRefusal::Unidentified)
        );
        assert_eq!(
            still(&bytes_only, Some([7; 32]), || Ok([7; 32]), no_digest),
            Err(ImageRefusal::Unidentified)
        );
        // A changed token is still refused as that, first.
        assert_eq!(
            still(&bytes_only, Some([7; 32]), || Ok([8; 32]), no_digest),
            Err(ImageRefusal::Changed)
        );
        // The same without a requirement (nothing names a publisher at all).
        let unruled = ExpectedImage {
            macos_requirement: None,
            ..bytes_only.clone()
        };
        assert_eq!(
            pinned(&unruled, Ok(None), no_digest),
            Err(ImageRefusal::Unidentified)
        );
        // A development image carries the digest of a pin too (a debug host with a
        // selected release for a system that has no CodeDirectory hash): unchanged.
        let development_bytes = ExpectedImage {
            sha256: Some([9; 32]),
            ..development()
        };
        for admitted in [Ok(None), Ok(Some([7; 32]))] {
            assert_eq!(
                pinned(&development_bytes, admitted.clone(), no_digest),
                admitted
            );
        }
        assert_eq!(still(&development_bytes, None, never, no_digest), Ok(()));
        // Both identities: the CodeDirectory hash is the pin, as before.
        let both = ExpectedImage {
            macos_cdhash: Some([3; 20]),
            ..bytes_only
        };
        assert_eq!(
            pinned(&both, Ok(Some([7; 32])), |_| Ok([3; 20])),
            Ok(Some([7; 32]))
        );
        assert_eq!(
            still(&both, Some([7; 32]), || Ok([7; 32]), |_| Ok([3; 20])),
            Ok(())
        );
    }

    #[test]
    fn a_readmission_must_answer_what_the_child_was_admitted_with() {
        let a: Token = [1; 32];
        let b: Token = [2; 32];
        assert!(readmitted(Some(a), Ok(Some(a))).is_ok());
        assert!(readmitted(None, Ok(None)).is_ok());
        // Another image, an image where none was identified, and none where one was.
        for (admitted, again) in [(Some(a), Some(b)), (None, Some(a)), (Some(a), None)] {
            assert!(matches!(
                readmitted(admitted, Ok(again)),
                Err(ImageRefusal::Readmitted)
            ));
        }
        // A check that refuses now refuses with its own reason.
        assert!(matches!(
            readmitted(Some(a), Err(ImageRefusal::NotTheImage)),
            Err(ImageRefusal::NotTheImage)
        ));
        assert!(matches!(
            readmitted(Some(a), Err(ImageRefusal::DigestMismatch)),
            Err(ImageRefusal::DigestMismatch)
        ));
    }

    #[test]
    fn a_recheck_needs_the_same_token_and_the_pinned_image() {
        let never = || -> Result<Token, String> { panic!("asked for a token") };
        // Admitted without an identity and not pinned: nothing is asked.
        assert_eq!(still(&development(), None, never, no_digest), Ok(()));
        // Identified: the token now must be the admitted one.
        assert_eq!(
            still(&development(), Some([7; 32]), || Ok([7; 32]), no_digest),
            Ok(())
        );
        assert_eq!(
            still(&development(), Some([7; 32]), || Ok([8; 32]), no_digest),
            Err(ImageRefusal::Changed)
        );
        assert_eq!(
            still(
                &development(),
                Some([7; 32]),
                || Err("gone".into()),
                no_digest
            ),
            Err(ImageRefusal::Identity("gone".into()))
        );
        let pin = ExpectedImage {
            macos_cdhash: Some([3; 20]),
            ..development()
        };
        // Pinned: the admitted token's image must still be the pinned one.
        assert_eq!(
            still(&pin, Some([7; 32]), || Ok([7; 32]), |_| Ok([3; 20])),
            Ok(())
        );
        assert_eq!(
            still(&pin, Some([7; 32]), || Ok([7; 32]), |_| Ok([4; 20])),
            Err(ImageRefusal::DigestMismatch)
        );
        assert_eq!(
            still(
                &pin,
                Some([7; 32]),
                || Ok([7; 32]),
                |_| Err(io::Error::other("gone"))
            ),
            Err(ImageRefusal::DigestUnavailable)
        );
        // A changed token is refused before the hash is read.
        assert_eq!(
            still(&pin, Some([7; 32]), || Ok([8; 32]), no_digest),
            Err(ImageRefusal::Changed)
        );
        // Pinned but never identified: refused, with nothing asked.
        assert_eq!(
            still(&pin, None, never, no_digest),
            Err(ImageRefusal::Unidentified)
        );
    }
}
