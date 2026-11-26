//! Unit tests for pending approval codes, challenge nonces and Ed25519 verification.

use base64::Engine as _;
use org_proto::external_auth::{
    challenge_message, ChallengePurpose, ExternalLoginState, APPROVAL_CODE_TTL_SECS,
    CHALLENGE_TTL_SECS,
};
use org_proto::ErrorCode;
use ring::rand::SystemRandom;
use ring::signature::{Ed25519KeyPair, KeyPair};

use super::codes::{normalize_code, MAX_PENDING_PER_EMPLOYEE, MAX_PENDING_TOTAL};
use super::nonces::{verify_signature, NonceEntry, MAX_NONCES_PER_SESSION};
use super::{decode_pubkey, pubkey_fingerprint, ExternalLeaseRecord, NonceBook, PendingLogins};

const KEY: &str = "MCowBQYDK2VwAyEAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=";

fn b64(bytes: &[u8]) -> String {
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

fn keypair() -> Ed25519KeyPair {
    let pkcs8 = Ed25519KeyPair::generate_pkcs8(&SystemRandom::new()).unwrap();
    Ed25519KeyPair::from_pkcs8(pkcs8.as_ref()).unwrap()
}

#[test]
fn codes_normalize_crockford_and_refuse_anything_else() {
    assert_eq!(
        normalize_code("abcd-efgh"),
        Some(("ABCD".into(), "EFGH".into()))
    );
    assert_eq!(
        normalize_code(" oIlL efgh "),
        Some(("0111".into(), "EFGH".into()))
    );
    for bad in [
        "",
        "ABCD",
        "ABCD-EFG",
        "ABCD-EFGHJ",
        "ABCU-EFGH",
        "AB*D-EFGH",
        "ÄBCD-EFGH",
    ] {
        assert_eq!(normalize_code(bad), None, "{bad:?}");
    }
    assert_eq!(normalize_code(&"A".repeat(4096)), None);
}

#[test]
fn a_pending_login_locks_after_three_wrong_codes_and_never_leaks_its_secret() {
    let mut book = PendingLogins::default();
    let (entry, code) = book
        .insert("req-1".into(), "emp-1", KEY, "laptop", 1_000)
        .unwrap();
    assert!(!format!("{entry:?}").contains(&code[5..]));
    let (lookup, secret) = normalize_code(&code).unwrap();
    let wrong = |n: usize| {
        let ch = if secret.as_bytes()[n % 4] == b'Z' {
            'Y'
        } else {
            'Z'
        };
        let mut s: Vec<char> = secret.chars().collect();
        s[n % 4] = ch;
        format!("{lookup}-{}", s.into_iter().collect::<String>())
    };
    // Finding the request a code names never spends an attempt, however often and with
    // whatever secret: only `check_code` (after the caller's authority) does.
    for n in 0..10 {
        let found = book.find_live(&wrong(n), 2_000).unwrap();
        assert_eq!(found.request_id, "req-1");
        assert_eq!(found.employee_id, "emp-1");
    }
    let target = book.find_live(&code, 2_000).unwrap();
    let first = book.check_code(&wrong(0), &target, 2_000).unwrap_err();
    assert_eq!(first.code, ErrorCode::NotAuthorized);
    assert!(first.message.contains("2 attempt"), "{}", first.message);
    book.check_code(&wrong(1), &target, 2_000).unwrap_err();
    let locked = book.check_code(&wrong(2), &target, 2_000).unwrap_err();
    assert!(locked.message.contains("locked"), "{}", locked.message);
    // The right code no longer works, and polling says so.
    let after = book.check_code(&code, &target, 2_000).unwrap_err();
    assert_eq!(after.code, ErrorCode::Conflict);
    assert_eq!(
        book.find_live(&code, 2_000).unwrap_err().code,
        ErrorCode::Conflict
    );
    assert_eq!(
        book.poll("req-1", 2_000).unwrap().state,
        ExternalLoginState::Locked
    );
}

#[test]
fn codes_expire_after_ten_minutes_and_unknown_lookups_are_not_found() {
    let mut book = PendingLogins::default();
    let (_, code) = book
        .insert("req-1".into(), "emp-1", KEY, "laptop", 0)
        .unwrap();
    let ttl_ms = APPROVAL_CODE_TTL_SECS * 1000;
    let target = book.find_live(&code, ttl_ms - 1).unwrap();
    assert_eq!(
        book.check_code(&code, &target, ttl_ms - 1)
            .unwrap()
            .request_id,
        "req-1"
    );
    let expired = book.check_code(&code, &target, ttl_ms).unwrap_err();
    assert_eq!(expired.code, ErrorCode::Conflict);
    assert_eq!(
        book.find_live(&code, ttl_ms).unwrap_err().code,
        ErrorCode::Conflict
    );
    assert_eq!(
        book.poll("req-1", ttl_ms).unwrap().state,
        ExternalLoginState::Expired
    );
    // Long after, the request is forgotten entirely.
    assert!(book.poll("req-1", ttl_ms * 10).is_none());
    assert_eq!(book.len(), 0);
    let (lookup, _) = normalize_code(&code).unwrap();
    let other = if lookup == "0000" {
        "1111-1111"
    } else {
        "0000-0000"
    };
    assert_eq!(
        book.find_live(other, 0).unwrap_err().code,
        ErrorCode::NotFound
    );
    assert_eq!(
        book.find_live("not-a-code!", 0).unwrap_err().code,
        ErrorCode::InvalidRequest
    );
    // A target whose lookup now names another request is refused without an attempt.
    let (_, fresh) = book
        .insert("req-2".into(), "emp-2", KEY, "laptop", ttl_ms * 10)
        .unwrap();
    let stale = super::codes::CodeTarget {
        request_id: "req-1".into(),
        employee_id: "emp-1".into(),
    };
    assert_eq!(
        book.check_code(&fresh, &stale, ttl_ms * 10)
            .unwrap_err()
            .code,
        ErrorCode::Conflict
    );
    assert_eq!(
        book.poll("req-2", ttl_ms * 10).unwrap().state,
        ExternalLoginState::Pending
    );
}

#[test]
fn settled_records_make_room_so_the_book_never_grows_past_its_cap() {
    use super::codes::MAX_ENTRIES_TOTAL;
    let mut book = PendingLogins::default();
    let mut n = 0u32;
    // Round after round of logins, each approved at once (settled records stay pollable for
    // two minutes): the total stays bounded and every new login still fits.
    for round in 0..6u64 {
        let now = 1_000 + round;
        for i in 0..MAX_PENDING_TOTAL {
            n += 1;
            let employee = format!("emp-{}", i / MAX_PENDING_PER_EMPLOYEE);
            let (entry, _) = book
                .insert(format!("req-{n}"), &employee, KEY, "laptop", now)
                .unwrap_or_else(|e| panic!("round {round}, login {i}: {e:?}"));
            book.mark_approved(&entry.request_id, "ext-1", "2099-01-01T00:00:00.000Z", now);
            assert!(book.len() <= MAX_ENTRIES_TOTAL, "{}", book.len());
        }
    }
    assert_eq!(book.len(), MAX_ENTRIES_TOTAL);
    // The newest settled records are the ones still pollable; the oldest made room.
    assert!(book.poll(&format!("req-{n}"), 1_010).is_some());
    assert!(book.poll("req-1", 1_010).is_none());
}

#[test]
fn pending_logins_are_bounded_per_agent_and_per_office() {
    let mut book = PendingLogins::default();
    for i in 0..MAX_PENDING_PER_EMPLOYEE {
        book.insert(format!("req-{i}"), "emp-1", KEY, "l", 0)
            .unwrap();
    }
    let refused = book
        .insert("req-x".into(), "emp-1", KEY, "l", 0)
        .unwrap_err();
    assert_eq!(refused.code, ErrorCode::Unavailable);
    for i in 0..(MAX_PENDING_TOTAL - MAX_PENDING_PER_EMPLOYEE) {
        book.insert(format!("o-{i}"), &format!("emp-o{i}"), KEY, "l", 0)
            .unwrap();
    }
    assert_eq!(
        book.insert("req-y".into(), "emp-new", KEY, "l", 0)
            .unwrap_err()
            .code,
        ErrorCode::Unavailable
    );
    // Approved requests stop counting and stay pollable.
    book.mark_approved("req-0", "ext-1", "2026-11-27T00:00:00.000Z", 1);
    let polled = book.poll("req-0", 2).unwrap();
    assert_eq!(polled.state, ExternalLoginState::Approved);
    assert_eq!(polled.session_id.as_deref(), Some("ext-1"));
    book.insert("req-z".into(), "emp-1", KEY, "l", 2).unwrap();
}

fn nonce_entry(session: &str, purpose: ChallengePurpose) -> impl FnOnce(u64, u64) -> NonceEntry {
    let session = session.to_owned();
    move |issued_ms, expires_ms| NonceEntry {
        employee_id: "emp-1".into(),
        session_id: session,
        pubkey_ed25519_b64: KEY.into(),
        purpose,
        issued_ms,
        expires_ms,
    }
}

#[test]
fn nonces_are_single_use_expire_and_stay_bounded_per_session() {
    let mut book = NonceBook::default();
    let (nonce, entry) = book
        .issue(nonce_entry("ext-1", ChallengePurpose::Exchange), 1_000)
        .unwrap();
    assert_eq!(nonce.len(), 64);
    assert_eq!(entry.expires_ms, 1_000 + CHALLENGE_TTL_SECS * 1000);
    assert_eq!(book.take(&nonce, 1_001).unwrap().session_id, "ext-1");
    assert!(book.take(&nonce, 1_001).is_none(), "a nonce is single use");

    let (late, _) = book
        .issue(nonce_entry("ext-1", ChallengePurpose::Exchange), 0)
        .unwrap();
    assert!(book.take(&late, CHALLENGE_TTL_SECS * 1000).is_none());

    for t in 0..(MAX_NONCES_PER_SESSION as u64 + 3) {
        book.issue(nonce_entry("ext-1", ChallengePurpose::Exchange), 10 + t)
            .unwrap();
    }
    assert_eq!(book.len(), MAX_NONCES_PER_SESSION);
    book.purge_member("emp-1");
    assert_eq!(book.len(), 0);
}

#[test]
fn signatures_verify_only_for_the_exact_key_message_and_shape() {
    let pair = keypair();
    let message = challenge_message(ChallengePurpose::Exchange, "o", "m", "s", "n");
    let sig = b64(pair.sign(&message).as_ref());
    let pubkey = pair.public_key().as_ref().to_vec();
    assert!(verify_signature(&pubkey, &message, &sig));
    let logout = challenge_message(ChallengePurpose::Logout, "o", "m", "s", "n");
    assert!(!verify_signature(&pubkey, &logout, &sig));
    let other = keypair();
    assert!(!verify_signature(
        other.public_key().as_ref(),
        &message,
        &sig
    ));
    assert!(!verify_signature(&pubkey, &message, "not base64!"));
    assert!(!verify_signature(&pubkey, &message, &b64(&[0u8; 63])));
    assert!(!verify_signature(&pubkey, &message, &"A".repeat(4096)));
}

#[test]
fn public_keys_must_be_32_bytes_and_fingerprints_are_stable() {
    let pair = keypair();
    let encoded = b64(pair.public_key().as_ref());
    let decoded = decode_pubkey(&encoded).unwrap();
    assert_eq!(decoded, pair.public_key().as_ref());
    assert_eq!(pubkey_fingerprint(&decoded).len(), 16);
    assert_eq!(pubkey_fingerprint(&decoded), pubkey_fingerprint(&decoded));
    assert!(decode_pubkey(&b64(&[1u8; 31])).is_none());
    assert!(decode_pubkey(&b64(&[1u8; 33])).is_none());
    assert!(decode_pubkey("%%%").is_none());
    assert!(decode_pubkey(&"A".repeat(200)).is_none());
}

#[test]
fn leases_are_active_only_unrevoked_and_before_expiry() {
    let mut lease = ExternalLeaseRecord {
        version: 1,
        session_id: "ext-1".into(),
        employee_id: "emp-1".into(),
        pubkey_ed25519_b64: KEY.into(),
        workspace_label: "laptop".into(),
        approved_by: "owner".into(),
        approved_at: "2026-11-26T00:00:00.000Z".into(),
        expires_at: "2026-11-27T00:00:00.000Z".into(),
        revoked_at: None,
    };
    let exp = lease.expires_at_ms();
    assert!(exp > 0);
    assert!(lease.is_active(exp - 1));
    assert!(!lease.is_active(exp));
    lease.revoked_at = Some("2026-11-26T01:00:00.000Z".into());
    assert!(!lease.is_active(exp - 1));
    lease.revoked_at = None;
    lease.expires_at = "garbage".into();
    assert!(!lease.is_active(0), "an unreadable expiry is expired");
}
