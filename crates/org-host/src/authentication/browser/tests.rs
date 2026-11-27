use std::net::IpAddr;

use org_host_control::browser::{
    BrowserOrigin, Digest32, LinkRegisterParams, SessionSignInParams, SessionValidateParams,
};
use org_host_control::{LinkToken, Password};

use super::super::tests::fixture;
use super::*;
use crate::accounts::store::ADDRESS_DELAY_CAP_MS;
use crate::links::OneTimeLinks;

const PASSWORD: &str = "synthetic test password";

fn site() -> BrowserOrigin {
    BrowserOrigin {
        scheme: "https".into(),
        host: "org.example".into(),
        port: 443,
    }
}
fn sign_in(login: &str, password: &str, address: &str, seed: u8) -> SessionSignInParams {
    SessionSignInParams {
        login: login.into(),
        password: Password::new(password.into()).unwrap(),
        client_address: address.parse::<IpAddr>().unwrap(),
        cookie_digest: Digest32([seed; 32]),
        proof_digest: Digest32([seed ^ 0xff; 32]),
        origin: site(),
    }
}
fn now() -> i64 {
    i64::try_from(super::super::wall_ms().unwrap()).unwrap()
}
fn body(error: &ApiError) -> String {
    serde_json::to_string(error).unwrap()
}

#[test]
fn sign_in_validate_and_end_round_trip_with_one_ended_push() {
    let (_root, auth, _c) = fixture();
    let t = now();
    let mut ended = Vec::new();
    let view = auth
        .sign_in_browser(&sign_in("owner", PASSWORD, "192.0.2.1", 1), t, &mut ended)
        .unwrap();
    assert!(view.admin && ended.is_empty());
    assert_eq!(view.idle_expires_at_ms, (t + IDLE_MS) as u64);
    let validate = SessionValidateParams {
        cookie_digest: Digest32([1; 32]),
        proof_digest: Digest32([0xfe; 32]),
        origin: site(),
    };
    assert_eq!(
        auth.validate_browser(&validate, t + 1, &mut ended)
            .unwrap()
            .session_id,
        view.session_id
    );
    // Another origin, or a swapped proof, is the same refusal.
    let mut other = SessionValidateParams {
        origin: BrowserOrigin {
            port: 8443,
            ..site()
        },
        ..validate
    };
    assert_eq!(
        auth.validate_browser(&other, t + 1, &mut ended)
            .unwrap_err()
            .code,
        ErrorCode::NotAuthorized
    );
    other.origin = site();
    other.proof_digest = Digest32([2; 32]);
    assert_eq!(
        auth.validate_browser(&other, t + 1, &mut ended)
            .unwrap_err()
            .code,
        ErrorCode::NotAuthorized
    );
    // SES-7: idle 30 days ends it.
    let later = SessionValidateParams {
        proof_digest: Digest32([0xfe; 32]),
        ..other
    };
    assert_eq!(
        auth.validate_browser(&later, t + 2 + IDLE_MS, &mut ended)
            .unwrap_err()
            .code,
        ErrorCode::NotAuthorized
    );
    // The idle refusal itself ends nothing; the record is reaped (or ended) once.
    assert!(ended.is_empty());
    let ended_now = auth.end_browser(&view.session_id).unwrap();
    assert_eq!(
        ended_now
            .iter()
            .map(|e| (e.id.as_str(), e.account_id.as_str()))
            .collect::<Vec<_>>(),
        [(view.session_id.as_str(), view.account.id.as_str())]
    );
    assert!(auth.end_browser(&view.session_id).unwrap().is_empty());
}

#[test]
fn unknown_login_and_wrong_password_get_byte_identical_refusals() {
    let (_root, auth, _c) = fixture();
    let t = now();
    let mut ended = Vec::new();
    let wrong = auth
        .sign_in_browser(
            &sign_in("owner", "not the password", "192.0.2.1", 1),
            t,
            &mut ended,
        )
        .unwrap_err();
    let unknown = auth
        .sign_in_browser(&sign_in("nobody", PASSWORD, "192.0.2.2", 2), t, &mut ended)
        .unwrap_err();
    let malformed = auth
        .sign_in_browser(&sign_in("\u{0}", PASSWORD, "192.0.2.3", 3), t, &mut ended)
        .unwrap_err();
    assert_eq!(body(&wrong), body(&unknown));
    assert_eq!(body(&wrong), body(&malformed));
    // The wire bound applies before normalization.
    let huge = auth
        .sign_in_browser(
            &sign_in(&"a".repeat(4097), PASSWORD, "192.0.2.4", 4),
            t,
            &mut ended,
        )
        .unwrap_err();
    assert_eq!(body(&wrong), body(&huge));
}

#[test]
fn a_flood_on_one_login_and_address_spares_other_logins_and_caps_the_address_at_60_s() {
    let (_root, auth, _c) = fixture();
    let t = now();
    let mut ended = Vec::new();
    // The 7th failure on a login (unknown or not) is rate limited, with no hash.
    let mut limited = None;
    for i in 0..7 {
        let r = auth.sign_in_browser(
            &sign_in("victim", "wrong password!", "198.51.100.7", 10),
            t + i,
            &mut ended,
        );
        if let Err(e) = r {
            if e.code == ErrorCode::RateLimited {
                limited = Some(e);
                break;
            }
        }
    }
    let limited = limited.expect("rate limited within 7 failures");
    assert!(limited.retry_after_ms.unwrap() > 0);
    for _ in 0..40 {
        let _ = auth.sign_in_browser(
            &sign_in("victim", "wrong password!", "198.51.100.7", 10),
            t + 10,
            &mut ended,
        );
    }
    // Another login from another address is unaffected.
    auth.sign_in_browser(
        &sign_in("owner", PASSWORD, "203.0.113.9", 11),
        t + 10,
        &mut ended,
    )
    .unwrap();
    // From the flooding address: refused now, at most 60 s later it works.
    let refused = auth
        .sign_in_browser(
            &sign_in("owner", PASSWORD, "198.51.100.7", 12),
            t + 10,
            &mut ended,
        )
        .unwrap_err();
    assert_eq!(refused.code, ErrorCode::RateLimited);
    assert!(refused.retry_after_ms.unwrap() <= ADDRESS_DELAY_CAP_MS as u64);
    auth.sign_in_browser(
        &sign_in("owner", PASSWORD, "198.51.100.7", 13),
        t + 10 + ADDRESS_DELAY_CAP_MS,
        &mut ended,
    )
    .unwrap();
}

#[test]
fn removed_access_ends_every_session_of_the_account_and_reports_the_pushes() {
    let (_root, auth, _c) = fixture();
    let t = now();
    let mut ended = Vec::new();
    let a = auth
        .sign_in_browser(&sign_in("owner", PASSWORD, "192.0.2.1", 1), t, &mut ended)
        .unwrap();
    let b = auth
        .sign_in_browser(&sign_in("owner", PASSWORD, "192.0.2.1", 2), t, &mut ended)
        .unwrap();
    auth.memory_for_test()
        .change_authority_fixture(&AccountId::parse(&a.account.id).unwrap(), true)
        .unwrap();
    let validate = SessionValidateParams {
        cookie_digest: Digest32([1; 32]),
        proof_digest: Digest32([0xfe; 32]),
        origin: site(),
    };
    assert_eq!(
        auth.validate_browser(&validate, t + 1, &mut ended)
            .unwrap_err()
            .code,
        ErrorCode::NotAuthorized
    );
    let mut ids: Vec<_> = ended.iter().map(|e| e.id.clone()).collect();
    ids.sort();
    let mut want = vec![a.session_id, b.session_id];
    want.sort();
    assert_eq!(ids, want);
    // Revoked: the same refusal as a wrong password.
    let revoked = auth
        .sign_in_browser(
            &sign_in("owner", PASSWORD, "192.0.2.5", 3),
            t + 2,
            &mut ended,
        )
        .unwrap_err();
    assert_eq!(revoked.code, ErrorCode::NotAuthorized);
}

#[test]
fn registration_rules_and_used_links_are_refused_with_their_codes() {
    let (_root, auth, _c) = fixture();
    let t = now();
    let mut ended = Vec::new();
    let register = |link: &str, login: &str, password: &str| LinkRegisterParams {
        link: LinkToken::new(link.into()).unwrap(),
        login: login.into(),
        password: Password::new(password.into()).unwrap(),
        client_address: "192.0.2.50".parse().unwrap(),
        cookie_digest: Digest32([7; 32]),
        proof_digest: Digest32([8; 32]),
        origin: site(),
    };
    let e = auth
        .register(
            &register("not-a-link", "ada", "long enough password"),
            t,
            &mut ended,
        )
        .unwrap_err();
    assert_eq!(e.code, ErrorCode::NotFound);
    let issued = crate::links::OneTimeLinks::issue(
        &*auth.db,
        crate::links::LinkKind::Registration,
        &crate::links::Subject::new("setup").unwrap(),
        t,
    )
    .unwrap();
    let token = issued.token.encode();
    let e = auth
        .register(
            &register(&token, " ada", "long enough password"),
            t,
            &mut ended,
        )
        .unwrap_err();
    assert_eq!(
        (e.field.as_deref(), e.rule.as_deref()),
        (Some("login"), Some("format"))
    );
    let e = auth
        .register(&register(&token, "ada", "short"), t, &mut ended)
        .unwrap_err();
    assert_eq!(
        (e.field.as_deref(), e.rule.as_deref()),
        (Some("password"), Some("too_short"))
    );
    // The fixture already has an administrator: the link does not redeem.
    let e = auth
        .register(
            &register(&token, "ada", "long enough password"),
            t,
            &mut ended,
        )
        .unwrap_err();
    assert_eq!(e.code, ErrorCode::NotFound);
    assert!(!body(&e).contains("long enough"));
}

fn register_params(link: &str, seed: u8) -> LinkRegisterParams {
    LinkRegisterParams {
        link: LinkToken::new(link.into()).unwrap(),
        login: "ada".into(),
        password: Password::new("long enough password".into()).unwrap(),
        client_address: "192.0.2.60".parse().unwrap(),
        cookie_digest: Digest32([seed; 32]),
        proof_digest: Digest32([seed ^ 0xff; 32]),
        origin: site(),
    }
}

#[test]
fn a_setup_link_registers_the_administrator_once_and_then_is_not_found() {
    let (_root, auth) = super::super::tests::fixture_without_accounts();
    let t = now();
    let mut ended = Vec::new();
    let issued = crate::links::OneTimeLinks::issue(
        &*auth.db,
        crate::links::LinkKind::Registration,
        &crate::links::Subject::new("setup").unwrap(),
        t,
    )
    .unwrap();
    let token = issued.token.encode();
    let view = auth
        .register(&register_params(&token, 7), t, &mut ended)
        .unwrap();
    assert!(view.admin);
    assert_eq!(view.account.login, "ada");
    // The new administrator is signed in: the session validates.
    let validate = SessionValidateParams {
        cookie_digest: Digest32([7; 32]),
        proof_digest: Digest32([7 ^ 0xff; 32]),
        origin: site(),
    };
    assert_eq!(
        auth.validate_browser(&validate, t + 1, &mut ended)
            .unwrap()
            .session_id,
        view.session_id
    );
    let again = auth
        .register(&register_params(&token, 9), t + 2, &mut ended)
        .unwrap_err();
    assert_eq!(again.code, ErrorCode::NotFound);
    // The new login signs in with its password.
    auth.sign_in_browser(
        &sign_in("ada", "long enough password", "192.0.2.9", 3),
        t + 3,
        &mut ended,
    )
    .unwrap();
    assert!(ended.is_empty());
}

#[test]
fn the_seventh_failure_on_an_unknown_or_known_login_is_rate_limited() {
    for login in ["owner", "nobody"] {
        let (_root, auth, _c) = fixture();
        let t = now();
        let mut ended = Vec::new();
        // A fresh address per attempt isolates the per-login key.
        for i in 0u8..6 {
            let e = auth
                .sign_in_browser(
                    &sign_in(login, "wrong password!", &format!("192.0.2.{}", i + 1), i),
                    t + i64::from(i),
                    &mut ended,
                )
                .unwrap_err();
            assert_eq!(e.code, ErrorCode::NotAuthorized, "{login} attempt {i}");
        }
        let e = auth
            .sign_in_browser(
                &sign_in(login, "wrong password!", "192.0.2.77", 77),
                t + 6,
                &mut ended,
            )
            .unwrap_err();
        assert_eq!(e.code, ErrorCode::RateLimited, "{login}");
        assert!(e.retry_after_ms.unwrap() > 0);
        // Locked: even the right password is refused without a password check.
        let e = auth
            .sign_in_browser(
                &sign_in(login, PASSWORD, "192.0.2.78", 78),
                t + 7,
                &mut ended,
            )
            .unwrap_err();
        assert_eq!(e.code, ErrorCode::RateLimited, "{login}");
    }
}

#[test]
fn a_full_host_refuses_every_account_alike_and_reaps_expired_sessions_first() {
    use crate::sessions::{NewSession, SESSION_LIMIT};
    let (_root, auth, _c) = fixture();
    let t = now();
    let mut ended = Vec::new();
    let first = auth
        .sign_in_browser(&sign_in("owner", PASSWORD, "192.0.2.1", 1), t, &mut ended)
        .unwrap();
    let sessions: &dyn BrowserSessions = &*auth.db;
    let origin = Origin::new(Scheme::Https, "org.example", 443).unwrap();
    for n in 1..SESSION_LIMIT as u32 {
        let mut cookie = [0u8; 32];
        cookie[..4].copy_from_slice(&n.to_be_bytes());
        cookie[31] = 0xaa;
        let new = NewSession::new(
            &first.account.id,
            SessionDigest(cookie),
            SessionDigest([0x55; 32]),
            origin.clone(),
        )
        .unwrap();
        assert!(matches!(
            sessions.create(&new, t + 10).unwrap(),
            Admission::Created { .. }
        ));
    }
    let full = auth
        .sign_in_browser(
            &sign_in("owner", PASSWORD, "192.0.2.2", 2),
            t + 20,
            &mut ended,
        )
        .unwrap_err();
    assert_eq!(body(&full), body(&unavailable()));
    assert!(ended.is_empty());
    // Once the first session has idled out it is reaped, reported and replaced.
    let later = t + IDLE_MS + 5;
    auth.sign_in_browser(
        &sign_in("owner", PASSWORD, "192.0.2.3", 3),
        later,
        &mut ended,
    )
    .unwrap();
    assert_eq!(
        ended.iter().map(|e| e.id.clone()).collect::<Vec<_>>(),
        vec![first.session_id]
    );
}

fn saturate(auth: &Authentication) {
    auth.hashing.store(MAX_HASHES, Ordering::SeqCst);
}
fn permits_held(auth: &Authentication) -> usize {
    auth.hashing.load(Ordering::SeqCst)
}
fn register_from(link: &str, address: &str) -> LinkRegisterParams {
    LinkRegisterParams {
        client_address: address.parse().unwrap(),
        ..register_params(link, 40)
    }
}

/// With every hash permit held elsewhere, any path that reached Argon2 would
/// answer `unavailable`. A refused link and a locked key answer their own code
/// instead, so neither took a permit, and the held count is unchanged.
#[test]
fn refused_links_and_locked_keys_take_no_hash_permit() {
    let (_root, auth) = super::super::tests::fixture_without_accounts();
    let t = now();
    let mut ended = Vec::new();
    let subject = crate::links::Subject::new("setup").unwrap();
    let first = OneTimeLinks::issue(&*auth.db, LinkKind::Registration, &subject, t).unwrap();
    let live = OneTimeLinks::issue(&*auth.db, LinkKind::Registration, &subject, t).unwrap();
    let cancelled = first.token.encode();
    let live = live.token.encode();
    saturate(&auth);
    // Malformed, unknown and cancelled links: not_found, no permit.
    for link in ["not-a-link", &"A".repeat(43), &cancelled] {
        let e = auth
            .register(&register_from(link, "198.51.100.1"), t, &mut ended)
            .unwrap_err();
        assert_eq!(e.code, ErrorCode::NotFound, "{link}");
        assert_eq!(permits_held(&auth), MAX_HASHES);
    }
    // The probe is sensitive: a live link from a fresh address reaches the hash.
    let e = auth
        .register(&register_from(&live, "203.0.113.1"), t, &mut ended)
        .unwrap_err();
    assert_eq!(body(&e), body(&unavailable()));
    // Link refusals count on the address; once locked, even the live link is
    // refused before the link check and the hash.
    let mut limited = None;
    for i in 0..10 {
        let e = auth
            .register(
                &register_from("not-a-link", "198.51.100.1"),
                t + i,
                &mut ended,
            )
            .unwrap_err();
        if e.code == ErrorCode::RateLimited {
            limited = Some(i);
            break;
        }
        assert_eq!(e.code, ErrorCode::NotFound);
    }
    assert!(limited.is_some(), "a link-guessing address is rate limited");
    let e = auth
        .register(&register_from(&live, "198.51.100.1"), t + 10, &mut ended)
        .unwrap_err();
    assert_eq!(e.code, ErrorCode::RateLimited);
    assert!(e.retry_after_ms.unwrap() > 0);
    // The same address is locked for sign-in too: no permit taken there either.
    let e = auth
        .sign_in_browser(
            &sign_in("anyone", PASSWORD, "198.51.100.1", 5),
            t + 10,
            &mut ended,
        )
        .unwrap_err();
    assert_eq!(e.code, ErrorCode::RateLimited);
    assert_eq!(permits_held(&auth), MAX_HASHES);
    assert!(ended.is_empty());
    // The live link was never consumed by any refusal.
    auth.hashing.store(0, Ordering::SeqCst);
    auth.register(&register_from(&live, "203.0.113.2"), t + 11, &mut ended)
        .unwrap();
    assert_eq!(permits_held(&auth), 0);
}

#[test]
fn a_locked_login_is_refused_before_the_hash_permit() {
    let (_root, auth, _c) = fixture();
    let t = now();
    let mut ended = Vec::new();
    for i in 0u8..6 {
        auth.sign_in_browser(
            &sign_in("owner", "wrong password!", &format!("192.0.2.{}", i + 1), i),
            t + i64::from(i),
            &mut ended,
        )
        .unwrap_err();
    }
    assert_eq!(permits_held(&auth), 0);
    saturate(&auth);
    let e = auth
        .sign_in_browser(
            &sign_in("owner", PASSWORD, "192.0.2.99", 99),
            t + 6,
            &mut ended,
        )
        .unwrap_err();
    assert_eq!(e.code, ErrorCode::RateLimited);
    assert_eq!(permits_held(&auth), MAX_HASHES);
}

/// F2: a session whose account cannot be shown after the insert is ended and
/// reported, never left behind a refusal.
#[test]
fn a_session_created_for_a_revoked_account_is_ended_and_reported() {
    let (_root, auth, _c) = fixture();
    let t = now();
    let mut ended = Vec::new();
    let owner = auth
        .sign_in_browser(&sign_in("owner", PASSWORD, "192.0.2.1", 1), t, &mut ended)
        .unwrap();
    let account = AccountId::parse(&owner.account.id).unwrap();
    auth.memory_for_test()
        .change_authority_fixture(&account, true)
        .unwrap();
    let origin = Origin::new(Scheme::Https, "org.example", 443).unwrap();
    let e = auth
        .start(
            &account,
            SessionDigest([0x31; 32]),
            SessionDigest([0x32; 32]),
            origin.clone(),
            t + 1,
            &mut ended,
        )
        .unwrap_err();
    assert_eq!(body(&e), body(&denied()));
    assert_eq!(ended.len(), 1);
    assert_eq!(ended[0].account_id, owner.account.id);
    assert_ne!(ended[0].id, owner.session_id);
    let sessions: &dyn BrowserSessions = &*auth.db;
    assert_eq!(
        sessions
            .find(
                &SessionDigest([0x31; 32]),
                &SessionDigest([0x32; 32]),
                &origin,
                t + 2
            )
            .unwrap(),
        None
    );
    assert_eq!(sessions.end(&ended[0].id).unwrap(), None);
}

/// Step 5: browser sessions keep only the 30-day idle rule and the store's
/// host-wide bound; the native 12-hour absolute lifetime and the 32-per-connection
/// cap do not apply to them.
#[test]
fn browser_sessions_have_no_absolute_lifetime_and_no_per_connection_cap() {
    use super::super::{ABSOLUTE, MAX_CONNECTION_SESSIONS};
    let (_root, auth, _c) = fixture();
    let t = now();
    let mut ended = Vec::new();
    let view = auth
        .sign_in_browser(&sign_in("owner", PASSWORD, "192.0.2.1", 1), t, &mut ended)
        .unwrap();
    let validate = SessionValidateParams {
        cookie_digest: Digest32([1; 32]),
        proof_digest: Digest32([0xfe; 32]),
        origin: site(),
    };
    let absolute = i64::try_from(ABSOLUTE.as_millis()).unwrap();
    // Active use well past the native absolute lifetime keeps the session.
    for at in [t + absolute + 1, t + IDLE_MS - 1, t + 2 * IDLE_MS - 2] {
        let live = auth.validate_browser(&validate, at, &mut ended).unwrap();
        assert_eq!(live.session_id, view.session_id);
        assert_eq!(live.idle_expires_at_ms, (at + IDLE_MS) as u64);
    }
    // More sessions for one account than the native per-connection cap.
    let account = AccountId::parse(&view.account.id).unwrap();
    let origin = Origin::new(Scheme::Https, "org.example", 443).unwrap();
    for n in 0..=MAX_CONNECTION_SESSIONS as u8 {
        auth.start(
            &account,
            SessionDigest([0x80 + n; 32]),
            SessionDigest([0x77; 32]),
            origin.clone(),
            t + 2 * IDLE_MS,
            &mut ended,
        )
        .unwrap();
    }
    assert!(ended.is_empty());
}

/// No password or link secret reaches the host log over register and sign-in,
/// on success or refusal.
#[test]
fn register_and_sign_in_log_no_password_or_link_bytes() {
    let (_root, auth) = super::super::tests::fixture_without_accounts();
    let t = now();
    let issued = OneTimeLinks::issue(
        &*auth.db,
        LinkKind::Registration,
        &crate::links::Subject::new("setup").unwrap(),
        t,
    )
    .unwrap();
    let token = issued.token.encode();
    let ((), lines) = crate::host_log::capture(|| {
        let mut ended = Vec::new();
        let mut shown = Vec::new();
        let mut keep = |r: Result<BrowserSession, ApiError>| {
            shown.push(match r {
                Ok(v) => serde_json::to_string(&v).unwrap(),
                Err(e) => body(&e),
            })
        };
        keep(auth.register(&register_from("wrong-link", "192.0.2.7"), t, &mut ended));
        keep(auth.register(&register_from(&token, "192.0.2.7"), t, &mut ended));
        keep(auth.sign_in_browser(
            &sign_in("ada", "long enough password", "192.0.2.7", 1),
            t,
            &mut ended,
        ));
        keep(auth.sign_in_browser(
            &sign_in("ada", "a different secret", "192.0.2.7", 2),
            t,
            &mut ended,
        ));
        keep(auth.sign_in_browser(
            &sign_in("nobody", "a different secret", "192.0.2.7", 3),
            t,
            &mut ended,
        ));
        for text in &shown {
            for secret in ["long enough password", "a different secret", token.as_str()] {
                assert!(!text.contains(secret), "{text}");
            }
        }
    });
    for line in &lines {
        for secret in ["long enough password", "a different secret", token.as_str()] {
            assert!(!line.contains(secret), "{line}");
        }
    }
}
