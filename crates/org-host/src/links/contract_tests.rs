//! Engine-independent `OneTimeLinks` suite (ADR-0018 ENG-3 for host storage).
//! Runs against `MemoryHostStore` and `MemoryLinks` in `org-host`; `org-host-sqlite` runs
//! the SQLite conformance suite against `HostDb`.

use super::memory::MemoryLinks;
use super::*;
use crate::hostdb::memory::MemoryHostStore;

const INSTANCE: &str = "0190a5d2-7c3e-7a01-8b2c-3d4e5f6071b0";
const OTHER_INSTANCE: &str = "0190a5d2-7c3e-7a01-8b2c-3d4e5f6071b1";
const NOW: i64 = 1_800_000_000_000;

/// Runs `case` against both in-memory adapters for `instance`.
fn each(case: impl Fn(&dyn OneTimeLinks)) {
    case(&MemoryHostStore::new(INSTANCE, NOW));
    case(&MemoryLinks::new(INSTANCE));
}

fn subject(s: &str) -> Subject {
    Subject::new(s).unwrap()
}
const REG: LinkKind = LinkKind::Registration;

#[test]
fn issued_link_redeems_once_then_is_used() {
    each(|links| {
        let s = subject("initial-administrator");
        let issued = links.issue(REG, &s, NOW).unwrap();
        assert!(org_proto::is_uuid_v7(&issued.link_id));
        assert_eq!(issued.expires_at_ms, NOW + 24 * 60 * 60 * 1000);
        let pending = links.pending(REG, &s, NOW).unwrap().unwrap();
        assert_eq!(pending.link_id, issued.link_id);
        assert_eq!(pending.issued_at_ms, NOW);
        let token = issued.token.encode();
        assert_eq!(token.len(), 43);
        assert_eq!(
            links.consume(REG, &token, NOW + 1).unwrap(),
            Ok(issued.link_id)
        );
        assert_eq!(
            links.consume(REG, &token, NOW + 2).unwrap(),
            Err(Refused::Used)
        );
        assert_eq!(links.pending(REG, &s, NOW + 2).unwrap(), None);
    });
}

#[test]
fn issue_first_issues_once_per_subject_ever_and_a_replay_has_no_secret() {
    each(|links| {
        let s = subject("administrator");
        let first = links.issue_first(REG, &s, NOW).unwrap().unwrap();
        assert!(links.issue_first(REG, &s, NOW + 1).unwrap().is_none());
        // Still refused after the first link is used, cancelled or expired.
        assert_eq!(
            links.consume(REG, &first.token.encode(), NOW + 2).unwrap(),
            Ok(first.link_id.clone())
        );
        assert!(links.issue_first(REG, &s, NOW + 3).unwrap().is_none());
        let other = subject("other");
        links.issue(REG, &other, NOW).unwrap();
        assert!(links.issue_first(REG, &other, NOW + 1).unwrap().is_none());
        // A first link for a fresh subject is pending and redeemable.
        let fresh = subject("fresh");
        let issued = links.issue_first(REG, &fresh, NOW).unwrap().unwrap();
        assert_eq!(
            links.pending(REG, &fresh, NOW).unwrap().unwrap().link_id,
            issued.link_id
        );
        // `issue` after `issue_first` still replaces it.
        let replaced = links.issue(REG, &fresh, NOW + 1).unwrap();
        assert_eq!(
            links.consume(REG, &issued.token.encode(), NOW + 2).unwrap(),
            Err(Refused::Cancelled)
        );
        assert_eq!(
            links
                .consume(REG, &replaced.token.encode(), NOW + 2)
                .unwrap(),
            Ok(replaced.link_id)
        );
    });
}

#[test]
fn issuing_again_cancels_the_pending_link_of_the_same_subject_only() {
    each(|links| {
        let s = subject("initial-administrator");
        let other = subject("another-subject");
        let first = links.issue(REG, &s, NOW).unwrap();
        let unrelated = links.issue(REG, &other, NOW).unwrap();
        let second = links.issue(REG, &s, NOW + 5).unwrap();
        assert_ne!(first.link_id, second.link_id);
        assert_eq!(
            links.pending(REG, &s, NOW + 5).unwrap().unwrap().link_id,
            second.link_id
        );
        assert_eq!(
            links.consume(REG, &first.token.encode(), NOW + 6).unwrap(),
            Err(Refused::Cancelled)
        );
        assert_eq!(
            links
                .consume(REG, &unrelated.token.encode(), NOW + 6)
                .unwrap(),
            Ok(unrelated.link_id)
        );
        assert_eq!(
            links.consume(REG, &second.token.encode(), NOW + 7).unwrap(),
            Ok(second.link_id)
        );
    });
}

#[test]
fn a_link_expires_after_24_hours_by_the_injected_clock() {
    each(|links| {
        let s = subject("initial-administrator");
        let issued = links.issue(REG, &s, NOW).unwrap();
        let last = NOW + REGISTRATION_TTL_MS - 1;
        assert!(links.pending(REG, &s, last).unwrap().is_some());
        assert_eq!(links.pending(REG, &s, last + 1).unwrap(), None);
        let token = issued.token.encode();
        assert_eq!(
            links.consume(REG, &token, last + 1).unwrap(),
            Err(Refused::Expired)
        );
        // Expiry does not consume: the link is still refused as expired, never used.
        assert_eq!(
            links.consume(REG, &token, last + 2).unwrap(),
            Err(Refused::Expired)
        );
        // A replacement after expiry is redeemable.
        let next = links.issue(REG, &s, last + 3).unwrap();
        assert_eq!(
            links.consume(REG, &next.token.encode(), last + 4).unwrap(),
            Ok(next.link_id)
        );
    });
}

#[test]
fn would_redeem_reports_every_state_without_changing_the_link() {
    each(|links| {
        let s = subject("initial-administrator");
        let first = links.issue(REG, &s, NOW).unwrap();
        let unknown = Token::parse(&"A".repeat(43)).unwrap();
        assert_eq!(
            links.redeemable(REG, &unknown, NOW).unwrap(),
            Err(Refused::Unknown)
        );
        // Asking any number of times leaves the link pending and redeemable.
        for _ in 0..3 {
            assert_eq!(links.redeemable(REG, &first.token, NOW).unwrap(), Ok(()));
        }
        assert_eq!(
            links.pending(REG, &s, NOW).unwrap().unwrap().link_id,
            first.link_id
        );
        let last = NOW + REGISTRATION_TTL_MS - 1;
        assert_eq!(
            links.redeemable(REG, &first.token, last + 1).unwrap(),
            Err(Refused::Expired)
        );
        let second = links.issue(REG, &s, NOW + 1).unwrap();
        assert_eq!(
            links.redeemable(REG, &first.token, NOW + 2).unwrap(),
            Err(Refused::Cancelled)
        );
        assert_eq!(
            links.consume(REG, &second.token.encode(), NOW + 3).unwrap(),
            Ok(second.link_id)
        );
        assert_eq!(
            links.redeemable(REG, &second.token, NOW + 4).unwrap(),
            Err(Refused::Used)
        );
    });
}

#[test]
fn unknown_and_malformed_tokens_are_unknown() {
    each(|links| {
        let s = subject("initial-administrator");
        let issued = links.issue(REG, &s, NOW).unwrap();
        let mut wrong = issued.token.encode().as_bytes().to_vec();
        wrong[0] = if wrong[0] == b'A' { b'B' } else { b'A' };
        let wrong = String::from_utf8(wrong).unwrap();
        let padded = format!("{}=", issued.token.encode().as_str());
        let short = &issued.token.encode()[..42];
        let mut noncanonical = issued.token.encode().as_bytes().to_vec();
        // The final character carries 2 unused bits; any of them set is not canonical.
        let last = noncanonical.len() - 1;
        noncanonical[last] = match ALPHABET
            .iter()
            .position(|a| *a == noncanonical[last])
            .unwrap()
        {
            p if p % 4 == 0 => ALPHABET[p + 1],
            p => ALPHABET[p - p % 4],
        };
        let noncanonical = String::from_utf8(noncanonical).unwrap();
        for token in ["", "!", short, &padded, &wrong, &noncanonical] {
            assert_eq!(
                links.consume(REG, token, NOW).unwrap(),
                Err(Refused::Unknown),
                "{token}"
            );
        }
        // Nothing above consumed the real link.
        assert_eq!(
            links.consume(REG, &issued.token.encode(), NOW).unwrap(),
            Ok(issued.link_id)
        );
    });
}

#[test]
fn the_digest_is_bound_to_the_instance_and_kind() {
    let token = Token::parse(&"A".repeat(43)).unwrap();
    let here = LinkDigest::of(INSTANCE, REG, &token);
    assert_ne!(here, LinkDigest::of(OTHER_INSTANCE, REG, &token));
    // `Registration` is the only kind today, so bind through the field the
    // kind fills: another kind name gives another digest for the same token.
    assert_eq!(here.0, digest(INSTANCE, REG.as_str(), &token.0));
    assert_ne!(here.0, digest(INSTANCE, "invitation", &token.0));
    // Fixed vector: SHA-256 over length-prefixed domain, instance and kind, then
    // the 32 zero token bytes.
    let mut input = Vec::new();
    for field in [
        &b"agentic-org/link/v1"[..],
        INSTANCE.as_bytes(),
        b"registration",
    ] {
        input.extend_from_slice(&(field.len() as u32).to_be_bytes());
        input.extend_from_slice(field);
    }
    input.extend_from_slice(&[0u8; 32]);
    assert_eq!(here.0, <[u8; 32]>::from(Sha256::digest(&input)));

    // A token issued by one instance never redeems at another, even when the
    // other instance holds a copy of the stored row.
    let issuer = crate::hostdb::memory::MemoryHostStore::new(INSTANCE, NOW);
    let redeemer = crate::hostdb::memory::MemoryHostStore::new(OTHER_INSTANCE, NOW);
    let s = subject("initial-administrator");
    let issued = issuer.issue(REG, &s, NOW).unwrap();
    redeemer
        .replace_pending(NewLink {
            link_id: &issued.link_id,
            kind: REG,
            subject: &s,
            digest: LinkDigest::of(INSTANCE, REG, &issued.token),
            issued_at_ms: NOW,
            expires_at_ms: issued.expires_at_ms,
        })
        .unwrap();
    assert_eq!(
        redeemer.consume(REG, &issued.token.encode(), NOW).unwrap(),
        Err(Refused::Unknown)
    );
    let issued = MemoryLinks::new(INSTANCE)
        .issue(REG, &subject("x"), NOW)
        .unwrap();
    assert_eq!(
        MemoryLinks::new(OTHER_INSTANCE)
            .consume(REG, &issued.token.encode(), NOW)
            .unwrap(),
        Err(Refused::Unknown)
    );
}

#[test]
fn subjects_are_bounded_printable_identifiers() {
    for bad in ["", " ", "a/b", "é", &"a".repeat(129)] {
        assert_eq!(Subject::new(bad), Err(Error::Invalid), "{bad}");
    }
    for good in ["initial-administrator", "setup:0190a1b2", &"a".repeat(128)] {
        assert!(Subject::new(good).is_ok());
    }
}

#[test]
fn token_parse_refuses_wrong_lengths_before_decoding() {
    let good = Token::generate().unwrap().encode();
    let good = good.as_str();
    for bad in [
        String::new(),
        good[..42].to_owned(),
        format!("{good}A"),
        format!("{good}="),
        "A".repeat(1 << 20),
    ] {
        assert!(Token::parse(&bad).is_none(), "{}", bad.len());
    }
    assert!(Token::parse(good).is_some());
}

#[test]
fn token_parse_refuses_every_noncanonical_final_character() {
    // 43 characters carry 258 bits for 256; the last character's low 2 bits
    // are unused. Exactly one of each group of four final characters parses.
    let prefix = "A".repeat(42);
    for (p, c) in ALPHABET.iter().enumerate() {
        let text = format!("{prefix}{}", *c as char);
        let parsed = Token::parse(&text);
        assert_eq!(parsed.is_some(), p % 4 == 0, "{text}");
        if let Some(token) = parsed {
            assert_eq!(token.encode().as_str(), text);
        }
    }
    // Shorter partial groups follow the same rule inside the decoder.
    assert_eq!(
        base64url_decode("AQ").as_deref().map(Vec::as_slice),
        Some(&[1u8][..])
    );
    assert!(base64url_decode("AR").is_none());
    assert_eq!(
        base64url_decode("AAE").as_deref().map(Vec::as_slice),
        Some(&[0u8, 1][..])
    );
    assert!(base64url_decode("AAF").is_none());
}

#[test]
fn encoded_token_text_is_zeroized_on_drop() {
    fn zeroized_on_drop<T: zeroize::ZeroizeOnDrop>(_: &T) {}
    zeroized_on_drop(&Token::generate().unwrap().encode());
}

#[test]
fn token_is_zeroized_on_drop() {
    // Compile-time check that the secret keeps its zeroizing destructor.
    fn zeroized_on_drop<T: zeroize::ZeroizeOnDrop>() {}
    zeroized_on_drop::<Token>();
}

#[test]
fn digest_fields_have_explicit_boundaries() {
    let t = [9u8; TOKEN_BYTES];
    // Moving bytes across the instance/kind boundary changes the digest.
    assert_ne!(digest("ab", "c", &t), digest("a", "bc", &t));
    assert_ne!(digest("", "abc", &t), digest("abc", "", &t));
}

#[test]
fn token_text_round_trips_and_debug_hides_it() {
    for _ in 0..64 {
        let token = Token::generate().unwrap();
        let text = token.encode();
        assert_eq!(text.len(), 43);
        assert!(text.bytes().all(|b| ALPHABET.contains(&b)));
        assert_eq!(Token::parse(&text).unwrap().0, token.0);
        assert_eq!(format!("{token:?}"), "Token(..)");
    }
}

#[test]
fn a_failed_replacement_keeps_the_previous_pending_link() {
    each(|links| {
        let s = subject("initial-administrator");
        let first = links.issue(REG, &s, NOW).unwrap();
        // Reusing the link ID fails the insert after the cancellation.
        let conflict = links.replace_pending(NewLink {
            link_id: &first.link_id,
            kind: REG,
            subject: &s,
            digest: LinkDigest::of(INSTANCE, REG, &Token::generate().unwrap()),
            issued_at_ms: NOW + 1,
            expires_at_ms: NOW + 2,
        });
        assert_eq!(conflict, Err(Error::Store(StoreFault::Constraint)));
        assert_eq!(
            links.pending(REG, &s, NOW + 1).unwrap().unwrap().link_id,
            first.link_id
        );
        assert_eq!(
            links.consume(REG, &first.token.encode(), NOW + 1).unwrap(),
            Ok(first.link_id)
        );
    });
}

#[test]
fn peek_link_subject_reports_subject_only_when_pending_and_not_expired() {
    each(|links| {
        let s = subject("invite:org-1:mem-1");
        let first = links.issue(REG, &s, NOW).unwrap();
        let unknown = Token::parse(&"A".repeat(43)).unwrap();
        assert_eq!(
            links.peek_link_subject(REG, &unknown, NOW).unwrap(),
            Err(Refused::Unknown)
        );
        // Peeking is non-consuming and returns the bound subject.
        for _ in 0..3 {
            assert_eq!(
                links.peek_link_subject(REG, &first.token, NOW).unwrap(),
                Ok(s.as_str().to_owned())
            );
        }
        // Wrong kind uses a distinct digest, so it is unknown.
        assert_eq!(
            links
                .peek_link_subject(LinkKind::SetPassword, &first.token, NOW)
                .unwrap(),
            Err(Refused::Unknown)
        );
        // Expired link reports Expired without consuming.
        let expired_at = NOW + REGISTRATION_TTL_MS;
        assert_eq!(
            links
                .peek_link_subject(REG, &first.token, expired_at)
                .unwrap(),
            Err(Refused::Expired)
        );
        // Replacing cancels the first link; consuming marks the second used.
        let second = links.issue(REG, &s, NOW + 1).unwrap();
        assert_eq!(
            links.peek_link_subject(REG, &first.token, NOW + 2).unwrap(),
            Err(Refused::Cancelled)
        );
        assert_eq!(
            links
                .peek_link_subject(REG, &second.token, NOW + 2)
                .unwrap(),
            Ok(s.as_str().to_owned())
        );
        assert_eq!(
            links.consume(REG, &second.token.encode(), NOW + 3).unwrap(),
            Ok(second.link_id)
        );
        assert_eq!(
            links
                .peek_link_subject(REG, &second.token, NOW + 4)
                .unwrap(),
            Err(Refused::Used)
        );
    });
}
