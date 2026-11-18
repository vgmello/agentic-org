//! Engine-independent `BrowserSessions` suite (ADR-0018 ENG-3 for host storage).
//! Every case runs against the SQLite registry and the in-memory adapter.

use std::os::unix::fs::PermissionsExt;

use super::memory::MemorySessions;
use super::*;
use crate::hostdb::{HostDb, HostDbInit};

/// A valid registry instance ID (the format the links suite uses).
const INSTANCE: &str = "0190a5d2-7c3e-7a01-8b2c-3d4e5f6071b0";
const NOW: i64 = 1_800_000_000_000;
const ALICE: &str = "01J0000000000000000000000A";
const BOB: &str = "01J0000000000000000000000B";
const UNKNOWN: &str = "01J0000000000000000000000Z";
const PHC: &str = "$argon2id$v=19$m=65536,t=3,p=1$MDEyMzQ1Njc4OWFiY2RlZg$AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";

pub(crate) struct NoSecrets;
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

/// A registry with the accounts ALICE and BOB, created at `NOW`.
pub(crate) fn sqlite() -> (tempfile::TempDir, HostDb) {
    let home = tempfile::tempdir().unwrap();
    std::fs::set_permissions(home.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    let init = HostDbInit {
        instance_id: INSTANCE.into(),
        clock_high_ms: NOW,
    };
    let db = HostDb::create(home.path(), &NoSecrets, &init).unwrap();
    for (id, login) in [(ALICE, "alice"), (BOB, "bob")] {
        let verifier = crate::hostdb::journal::Verifier::new(PHC.into()).unwrap();
        db.seed_account(id, login, &verifier, id == ALICE, NOW)
            .unwrap();
    }
    db.relax_durability_for_tests();
    (home, db)
}

/// Runs `case` against both adapters.
fn each(case: impl Fn(&dyn BrowserSessions)) {
    let (_home, db) = sqlite();
    case(&db);
    case(&MemorySessions::new(&[ALICE, BOB], NOW));
}

pub(crate) fn origin() -> Origin {
    Origin::new(Scheme::Https, "office.example", 8443).unwrap()
}
/// Raw cookie and proof values derived from `n`; the host only sees their digests.
pub(crate) fn raw(n: u32) -> ([u8; 32], [u8; 32]) {
    let mut cookie = [0xC0; 32];
    let mut proof = [0x9F; 32];
    cookie[..4].copy_from_slice(&n.to_be_bytes());
    proof[..4].copy_from_slice(&n.to_be_bytes());
    (cookie, proof)
}
fn digests(n: u32) -> (SessionDigest, SessionDigest) {
    let (cookie, proof) = raw(n);
    (SessionDigest::cookie(&cookie), SessionDigest::proof(&proof))
}
pub(crate) fn new_session(account: &str, n: u32) -> NewSession {
    let (cookie, proof) = digests(n);
    NewSession::new(account, cookie, proof, origin()).unwrap()
}
fn find(store: &dyn BrowserSessions, n: u32, now: i64) -> Option<SessionRecord> {
    let (cookie, proof) = digests(n);
    store.find(&cookie, &proof, &origin(), now).unwrap()
}
fn created(admission: Admission) -> Vec<Ended> {
    match admission {
        Admission::Created { reaped } => reaped,
        Admission::Full { .. } => panic!("refused as full"),
    }
}

#[test]
fn digests_are_domain_separated_sha256() {
    use sha2::{Digest as _, Sha256};
    let value = [7u8; 32];
    let expect = |domain: &[u8]| -> [u8; 32] {
        Sha256::new()
            .chain_update(domain)
            .chain_update(value)
            .finalize()
            .into()
    };
    assert_eq!(
        SessionDigest::cookie(&value).0,
        expect(b"agentic-org/session-cookie/v1")
    );
    assert_eq!(
        SessionDigest::proof(&value).0,
        expect(b"agentic-org/session-proof/v1")
    );
    assert!(!SessionDigest::cookie(&value).ct_eq(&SessionDigest::proof(&value)));
    assert!(SessionDigest::cookie(&value).ct_eq(&SessionDigest::cookie(&value)));
    assert_eq!(
        format!("{:?}", SessionDigest::cookie(&value)),
        "SessionDigest(..)"
    );
}

#[test]
fn origins_are_bounded_lowercase_hosts_with_a_port() {
    assert!(Origin::new(Scheme::Http, "127.0.0.1", 8080).is_ok());
    assert!(Origin::new(Scheme::Https, "[::1]", 443).is_ok());
    for (host, port) in [("", 443), ("Office.example", 443), ("a b", 443), ("x", 0)] {
        assert_eq!(Origin::new(Scheme::Https, host, port), Err(InvalidOrigin));
    }
    assert!(Origin::new(Scheme::Https, &"a".repeat(256), 443).is_err());
    assert_eq!(Scheme::parse("https"), Some(Scheme::Https));
    assert_eq!(Scheme::parse("HTTPS"), None);
}

#[test]
fn created_session_is_found_by_its_digests_and_origin_only() {
    each(|store| {
        let new = new_session(ALICE, 1);
        assert_eq!(created(store.create(&new, NOW + 5).unwrap()), vec![]);
        assert!(org_proto::is_uuid_v7(&new.id));
        let record = find(store, 1, NOW + 6).unwrap();
        assert_eq!(
            record,
            SessionRecord {
                id: new.id.clone(),
                account_id: ALICE.into(),
                created_ms: NOW + 5,
                last_activity_ms: NOW + 5,
                origin: origin(),
            }
        );
        let (cookie, proof) = digests(1);
        let (other_cookie, other_proof) = digests(2);
        // Wrong proof, unknown cookie, and swapped roles are all not found.
        assert_eq!(
            store.find(&cookie, &other_proof, &origin(), NOW + 6),
            Ok(None)
        );
        assert_eq!(
            store.find(&other_cookie, &proof, &origin(), NOW + 6),
            Ok(None)
        );
        let (raw_cookie, _) = raw(1);
        let as_proof = SessionDigest::proof(&raw_cookie);
        assert_eq!(store.find(&cookie, &as_proof, &origin(), NOW + 6), Ok(None));
        // Every part of the origin binds.
        for other in [
            Origin::new(Scheme::Http, "office.example", 8443).unwrap(),
            Origin::new(Scheme::Https, "office.example.evil", 8443).unwrap(),
            Origin::new(Scheme::Https, "office.example", 443).unwrap(),
        ] {
            assert_eq!(store.find(&cookie, &proof, &other, NOW + 6), Ok(None));
        }
    });
}

#[test]
fn idle_lifetime_ends_thirty_days_after_last_activity() {
    each(|store| {
        let new = new_session(ALICE, 1);
        created(store.create(&new, NOW).unwrap());
        assert!(find(store, 1, NOW + IDLE_MS - 1).is_some());
        assert_eq!(find(store, 1, NOW + IDLE_MS), None);
        // Activity moves the boundary.
        store.touch(&[(&new.id, NOW + 1_000)], NOW + 1_000).unwrap();
        let record = find(store, 1, NOW + IDLE_MS).unwrap();
        assert_eq!(record.last_activity_ms, NOW + 1_000);
        assert_eq!(find(store, 1, NOW + 1_000 + IDLE_MS), None);
    });
}

#[test]
fn touch_is_monotone_and_ignores_unknown_ids() {
    each(|store| {
        let a = new_session(ALICE, 1);
        let b = new_session(BOB, 2);
        created(store.create(&a, NOW).unwrap());
        created(store.create(&b, NOW).unwrap());
        store
            .touch(
                &[
                    (&a.id, NOW + 50),
                    (&b.id, NOW + 20),
                    ("not-a-session", NOW + 99),
                ],
                NOW + 60,
            )
            .unwrap();
        store.touch(&[(&a.id, NOW + 10)], NOW + 60).unwrap();
        assert_eq!(find(store, 1, NOW + 60).unwrap().last_activity_ms, NOW + 50);
        assert_eq!(find(store, 2, NOW + 60).unwrap().last_activity_ms, NOW + 20);
        store.touch(&[], NOW + 60).unwrap();
    });
}

#[test]
fn end_removes_one_session_once() {
    each(|store| {
        let a = new_session(ALICE, 1);
        created(store.create(&a, NOW).unwrap());
        let ended = Ended {
            id: a.id.clone(),
            account_id: ALICE.into(),
        };
        assert_eq!(store.end(&a.id), Ok(Some(ended)));
        assert_eq!(store.end(&a.id), Ok(None));
        assert_eq!(find(store, 1, NOW), None);
    });
}

#[test]
fn end_account_keeps_the_excepted_session_and_other_accounts() {
    each(|store| {
        let sessions: Vec<_> = (1..=3).map(|n| new_session(ALICE, n)).collect();
        for s in &sessions {
            created(store.create(s, NOW).unwrap());
        }
        created(store.create(&new_session(BOB, 9), NOW).unwrap());
        let mut expected: Vec<_> = sessions[1..]
            .iter()
            .map(|s| Ended {
                id: s.id.clone(),
                account_id: ALICE.into(),
            })
            .collect();
        expected.sort();
        assert_eq!(
            store.end_account(ALICE, Some(&sessions[0].id)),
            Ok(expected)
        );
        assert!(find(store, 1, NOW).is_some());
        assert!(find(store, 9, NOW).is_some());
        assert_eq!(store.end_account(ALICE, None).unwrap().len(), 1);
        assert_eq!(store.end_account(ALICE, None), Ok(vec![]));
        assert!(find(store, 9, NOW).is_some());
    });
}

#[test]
fn reap_returns_exactly_the_expired_sessions() {
    each(|store| {
        let old = new_session(ALICE, 1);
        let fresh = new_session(BOB, 2);
        created(store.create(&old, NOW).unwrap());
        created(store.create(&fresh, NOW + 10).unwrap());
        assert_eq!(store.reap(NOW + IDLE_MS - 1), Ok(vec![]));
        assert_eq!(
            store.reap(NOW + IDLE_MS),
            Ok(vec![Ended {
                id: old.id.clone(),
                account_id: ALICE.into(),
            }])
        );
        assert!(find(store, 2, NOW + IDLE_MS).is_some());
    });
}

#[test]
fn the_clock_never_runs_backward() {
    each(|store| {
        let first = new_session(ALICE, 1);
        created(store.create(&first, NOW + 100).unwrap());
        created(store.create(&new_session(ALICE, 2), NOW).unwrap());
        assert_eq!(find(store, 2, NOW).unwrap().created_ms, NOW + 100);
        // A reap advances the clock; a later touch from an earlier caller time
        // is clamped to that clock, not to the caller's time.
        assert_eq!(store.reap(NOW + 500), Ok(vec![]));
        store.touch(&[(&first.id, NOW + 400)], NOW).unwrap();
        assert_eq!(find(store, 1, NOW).unwrap().last_activity_ms, NOW + 400);
        store.touch(&[(&first.id, NOW + 900)], NOW).unwrap();
        assert_eq!(find(store, 1, NOW).unwrap().last_activity_ms, NOW + 500);
    });
}

#[test]
fn a_far_future_touch_is_clamped_and_still_reaped_at_clock_plus_idle() {
    each(|store| {
        let s = new_session(ALICE, 1);
        created(store.create(&s, NOW).unwrap());
        store
            .touch(&[(&s.id, NOW + 10 * IDLE_MS)], NOW + 10)
            .unwrap();
        assert_eq!(find(store, 1, NOW + 10).unwrap().last_activity_ms, NOW + 10);
        assert_eq!(find(store, 1, NOW + 10 + IDLE_MS), None);
        assert_eq!(
            store.reap(NOW + 10 + IDLE_MS),
            Ok(vec![Ended {
                id: s.id.clone(),
                account_id: ALICE.into(),
            }])
        );
    });
}

#[test]
fn a_negative_time_is_refused_and_changes_nothing() {
    each(|store| {
        let s = new_session(ALICE, 1);
        assert_eq!(store.create(&s, -1), Err(StoreError::Unavailable));
        created(store.create(&s, NOW).unwrap());
        let (cookie, proof) = digests(1);
        assert_eq!(
            store.find(&cookie, &proof, &origin(), -1),
            Err(StoreError::Unavailable)
        );
        assert_eq!(
            store.touch(&[(&s.id, NOW + 5)], -1),
            Err(StoreError::Unavailable)
        );
        assert_eq!(store.reap(-1), Err(StoreError::Unavailable));
        assert_eq!(find(store, 1, NOW).unwrap().last_activity_ms, NOW);
    });
}

#[test]
fn an_expired_unreaped_duplicate_digest_is_reaped_and_replaced() {
    each(|store| {
        let old = new_session(ALICE, 1);
        created(store.create(&old, NOW).unwrap());
        // Same cookie digest, presented once the old session has expired.
        let again = new_session(BOB, 1);
        let at = NOW + IDLE_MS;
        assert_eq!(
            created(store.create(&again, at).unwrap()),
            vec![Ended {
                id: old.id.clone(),
                account_id: ALICE.into(),
            }]
        );
        assert_eq!(find(store, 1, at).unwrap().id, again.id);
    });
}

#[test]
fn unknown_account_or_duplicate_digest_is_unavailable_and_stores_nothing() {
    each(|store| {
        let unknown = new_session(UNKNOWN, 1);
        assert_eq!(store.create(&unknown, NOW), Err(StoreError::Unavailable));
        assert_eq!(find(store, 1, NOW), None);
        let first = new_session(ALICE, 2);
        created(store.create(&first, NOW).unwrap());
        // Same cookie digest under a new ID.
        let again = new_session(BOB, 2);
        assert_eq!(store.create(&again, NOW), Err(StoreError::Unavailable));
        assert_eq!(find(store, 2, NOW).unwrap().account_id, ALICE);
    });
}

#[test]
fn the_host_wide_bound_reaps_before_refusing_and_refuses_every_account_alike() {
    each(|store| {
        // The first session expires before the others.
        created(store.create(&new_session(ALICE, 0), NOW).unwrap());
        let later = NOW + 1_000;
        for n in 1..SESSION_LIMIT as u32 {
            let account = if n % 2 == 0 { ALICE } else { BOB };
            created(store.create(&new_session(account, n), later).unwrap());
        }
        // Full: nothing is expired, and both accounts get the same refusal.
        for (account, n) in [(ALICE, 90_001), (BOB, 90_002)] {
            assert_eq!(
                store.create(&new_session(account, n), later),
                Ok(Admission::Full { reaped: vec![] })
            );
            assert_eq!(find(store, n, later), None);
        }
        // The bound is checked before the account: an unknown account sees
        // the same refusal.
        assert_eq!(
            store.create(&new_session(UNKNOWN, 90_004), later),
            Ok(Admission::Full { reaped: vec![] })
        );
        // Once the first session's idle lifetime ends, it is reaped in the same
        // call and the new session is stored.
        let at = NOW + IDLE_MS;
        match store.create(&new_session(BOB, 90_003), at).unwrap() {
            Admission::Created { reaped } => {
                assert_eq!(reaped.len(), 1);
                assert_eq!(reaped[0].account_id, ALICE);
            }
            Admission::Full { .. } => panic!("an expired session must be reaped first"),
        }
        assert!(find(store, 90_003, at).is_some());
        assert_eq!(find(store, 0, at), None);
    });
}
