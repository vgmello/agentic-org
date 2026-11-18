//! Engine-independent `OneTimeLinks` suite (ADR-0018 ENG-3 for host storage).
//! Every case runs against the SQLite registry and the in-memory adapter.

use std::os::unix::fs::PermissionsExt;

use super::memory::MemoryLinks;
use super::*;
use crate::hostdb::{HostDb, HostDbInit};

const INSTANCE: &str = "01J00000000000000000000000";
const OTHER_INSTANCE: &str = "01J0000000000000000000000Z";
const NOW: i64 = 1_800_000_000_000;

struct NoSecrets;
impl crate::host_secrets::HostSecrets for NoSecrets {
    fn put(
        &self,
        _: &crate::host_secrets::Name,
        _: &crate::host_secrets::Value,
    ) -> Result<(), crate::host_secrets::Error> {
        Ok(())
    }
    fn load(
        &self,
        _: &crate::host_secrets::Name,
    ) -> Result<Option<crate::host_secrets::Value>, crate::host_secrets::Error> {
        Ok(None)
    }
    fn delete(&self, _: &crate::host_secrets::Name) -> Result<(), crate::host_secrets::Error> {
        Ok(())
    }
    fn list(&self) -> Result<Vec<crate::host_secrets::Name>, crate::host_secrets::Error> {
        Ok(Vec::new())
    }
}

fn sqlite(instance: &str) -> (tempfile::TempDir, HostDb) {
    let home = tempfile::tempdir().unwrap();
    std::fs::set_permissions(home.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    let init = HostDbInit {
        instance_id: instance.into(),
        clock_high_ms: NOW,
    };
    let db = HostDb::create(home.path(), &NoSecrets, &init).unwrap();
    (home, db)
}

/// Runs `case` against both adapters for `instance`.
fn each(case: impl Fn(&dyn OneTimeLinks)) {
    let (_home, db) = sqlite(INSTANCE);
    case(&db);
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
fn unknown_and_malformed_tokens_are_unknown() {
    each(|links| {
        let s = subject("initial-administrator");
        let issued = links.issue(REG, &s, NOW).unwrap();
        let mut wrong = issued.token.encode().into_bytes();
        wrong[0] = if wrong[0] == b'A' { b'B' } else { b'A' };
        let wrong = String::from_utf8(wrong).unwrap();
        let padded = format!("{}=", issued.token.encode());
        let short = &issued.token.encode()[..42];
        let mut noncanonical = issued.token.encode().into_bytes();
        // The final character carries 4 unused bits; any of them set is not canonical.
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
    // Fixed vector: SHA-256("agentic-org/link/v1" || instance || kind || 32 zero bytes).
    let mut input = b"agentic-org/link/v1".to_vec();
    input.extend_from_slice(INSTANCE.as_bytes());
    input.extend_from_slice(b"registration");
    input.extend_from_slice(&[0u8; 32]);
    assert_eq!(here.0, <[u8; 32]>::from(Sha256::digest(&input)));

    // A token issued by one instance never redeems at another, even when the
    // other instance holds a copy of the stored row.
    let (_a, issuer) = sqlite(INSTANCE);
    let (_b, redeemer) = sqlite(OTHER_INSTANCE);
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
fn the_registry_file_holds_no_token_bytes() {
    let (home, db) = sqlite(INSTANCE);
    let s = subject("initial-administrator");
    let issued: Vec<_> = (0..4)
        .map(|i| db.issue(REG, &s, NOW + i).unwrap())
        .collect();
    let used = &issued[3];
    db.consume(REG, &used.token.encode(), NOW + 10)
        .unwrap()
        .unwrap();
    drop(db);
    let mut bytes = Vec::new();
    for entry in std::fs::read_dir(home.path()).unwrap() {
        bytes.extend(std::fs::read(entry.unwrap().path()).unwrap());
    }
    assert!(!bytes.is_empty());
    for link in &issued {
        let text = link.token.encode();
        assert!(
            !bytes.windows(32).any(|w| w == link.token.0),
            "raw token stored"
        );
        assert!(
            !bytes.windows(text.len()).any(|w| w == text.as_bytes()),
            "encoded token stored"
        );
        let digest = LinkDigest::of(INSTANCE, REG, &link.token).0;
        assert!(bytes.windows(32).any(|w| w == digest), "digest is stored");
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
        assert_eq!(conflict, Err(Error::Store));
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
