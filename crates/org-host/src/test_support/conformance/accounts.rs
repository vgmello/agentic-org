//! Shared engine-independent `AccountStore` conformance cases (ADR-0018 ENG-3).

use std::net::IpAddr;

use crate::accounts::identity::{AccountId, LoginKey};
use crate::accounts::rules::Username;
use crate::accounts::store::{
    backoff_ms, AccountStore, Attempt, ClientAddress, NewAccount, Registration, StoreError,
    Throttle, ThrottleKey, ADDRESS_DELAY_CAP_MS, LOGIN_DELAY_CAP_MS,
};
use crate::accounts::verifier::Verifier;
use crate::links::{Issued, LinkKind, OneTimeLinks, Refused, Subject, Token, REGISTRATION_TTL_MS};

/// A valid registry instance ID (the format the links suite uses).
pub const INSTANCE: &str = "0190a5d2-7c3e-7a01-8b2c-3d4e5f6071b0";
pub const NOW: i64 = 1_800_000_000_000;
pub const PHC: &str = "$argon2id$v=19$m=65536,t=3,p=1$MDEyMzQ1Njc4OWFiY2RlZg$AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";

/// One adapter with its link store and a way to revoke an account.
pub struct AccountStoreCase<'a> {
    pub store: &'a dyn AccountStore,
    pub links: &'a dyn OneTimeLinks,
    pub revoke: &'a dyn Fn(&AccountId),
}

fn issue(links: &dyn OneTimeLinks, now: i64) -> Issued {
    links
        .issue(LinkKind::Registration, &Subject::new("setup").unwrap(), now)
        .unwrap()
}

fn pending(links: &dyn OneTimeLinks, now: i64) -> bool {
    links
        .pending(LinkKind::Registration, &Subject::new("setup").unwrap(), now)
        .unwrap()
        .is_some()
}

fn account(name: &str) -> NewAccount {
    let verifier = Verifier::new(PHC.into()).unwrap();
    NewAccount::new(Username::new(name).unwrap(), verifier).unwrap()
}

fn registered(r: Registration) -> AccountId {
    match r {
        Registration::Registered { account_id } => account_id,
        other => panic!("not registered: {other:?}"),
    }
}

/// Records failures on `key`, each as soon as the previous lock ends.
fn flood(store: &dyn AccountStore, key: &ThrottleKey<'_>, from: i64, count: usize) -> i64 {
    let mut at = from;
    for _ in 0..count {
        let state = store.record(key, Attempt::Failure, at).unwrap();
        at += state.retry_after_ms;
    }
    at
}

pub fn a_link_registers_the_administrator_once_and_is_consumed_with_it(c: AccountStoreCase<'_>) {
    assert_eq!(c.store.administrator_exists(), Ok(false));
    let link = issue(c.links, NOW);
    let new = account("Ålice");
    let id = registered(
        c.store
            .register_administrator(&link.token, &new, NOW + 1)
            .unwrap(),
    );
    assert_eq!(id, new.account_id);
    assert_eq!(c.store.administrator_exists(), Ok(true));
    assert!(!pending(c.links, NOW + 1));
    // The same token never redeems again.
    assert_eq!(
        c.store
            .register_administrator(&link.token, &account("bob"), NOW + 2),
        Ok(Registration::Link(Refused::Used))
    );
    assert_eq!(
        c.store
            .credentials(&LoginKey::new("bob").unwrap())
            .map(|c| c.is_none()),
        Ok(true)
    );
}

pub fn credentials_are_found_by_the_nfc_case_folded_username_and_hold_only_the_hash(
    c: AccountStoreCase<'_>,
) {
    let link = issue(c.links, NOW);
    let new = account("Ålice");
    registered(
        c.store
            .register_administrator(&link.token, &new, NOW)
            .unwrap(),
    );
    for spelling in ["Ålice", "A\u{30A}LICE", "åLiCe"] {
        let found = c
            .store
            .credentials(&LoginKey::new(spelling).unwrap())
            .unwrap()
            .unwrap();
        assert_eq!(found.account_id, new.account_id);
        assert_eq!(found.login, "Ålice");
        assert!(found.admin && !found.revoked);
        assert_eq!(found.verifier.expose(), PHC);
    }
    assert!(c
        .store
        .credentials(&LoginKey::new("alice").unwrap())
        .unwrap()
        .is_none());
}

pub fn every_refusal_leaves_the_link_and_the_accounts_unchanged(c: AccountStoreCase<'_>) {
    // A token that was never issued: refused, the issued link stays pending.
    let unrelated = issue(c.links, NOW);
    let never_issued = Token::parse(&"A".repeat(43)).unwrap();
    assert_eq!(
        c.store
            .register_administrator(&never_issued, &account("alice"), NOW),
        Ok(Registration::Link(Refused::Unknown))
    );
    assert_eq!(c.store.administrator_exists(), Ok(false));
    assert!(pending(c.links, NOW));
    drop(unrelated);
    // A cancelled link: nothing changes.
    let link = issue(c.links, NOW);
    let other = issue(c.links, NOW); // cancels `link`
    assert_eq!(
        c.store
            .register_administrator(&link.token, &account("alice"), NOW),
        Ok(Registration::Link(Refused::Cancelled))
    );
    assert_eq!(c.store.administrator_exists(), Ok(false));
    // Expired by the injected clock.
    assert_eq!(
        c.store
            .register_administrator(&other.token, &account("alice"), NOW + REGISTRATION_TTL_MS),
        Ok(Registration::Link(Refused::Expired))
    );
    let first = issue(c.links, NOW + REGISTRATION_TTL_MS);
    let admin = registered(
        c.store
            .register_administrator(&first.token, &account("alice"), NOW + REGISTRATION_TTL_MS)
            .unwrap(),
    );
    // An administrator exists: a valid link stays pending.
    let t = NOW + REGISTRATION_TTL_MS + 1;
    let second = issue(c.links, t);
    assert_eq!(
        c.store
            .register_administrator(&second.token, &account("bob"), t),
        Ok(Registration::AdministratorExists)
    );
    assert!(pending(c.links, t));
    // Once revoked, a reused login key is refused and the link stays pending.
    (c.revoke)(&admin);
    assert_eq!(c.store.administrator_exists(), Ok(false));
    assert_eq!(
        c.store
            .register_administrator(&second.token, &account("ALICE"), t),
        Ok(Registration::LoginTaken)
    );
    assert!(pending(c.links, t));
    let revoked = c
        .store
        .credentials(&LoginKey::new("alice").unwrap())
        .unwrap()
        .unwrap();
    assert!(revoked.revoked);
    // The same link then registers a new administrator.
    registered(
        c.store
            .register_administrator(&second.token, &account("bob"), t)
            .unwrap(),
    );
    assert_eq!(c.store.administrator_exists(), Ok(true));
    assert_eq!(
        c.store
            .register_administrator(&second.token, &account("carol"), -1),
        Err(StoreError::Unavailable)
    );
}

pub fn the_schedule_starts_after_five_failures_and_doubles_to_its_cap() {
    let delays: Vec<_> = (1..=17)
        .map(|n| backoff_ms(n, LOGIN_DELAY_CAP_MS))
        .collect();
    assert_eq!(&delays[..5], [0; 5]);
    assert_eq!(&delays[5..11], [1_000, 2_000, 4_000, 8_000, 16_000, 32_000]);
    assert_eq!(delays[15], 900_000);
    assert_eq!(delays[16], 900_000);
    assert_eq!(backoff_ms(63, ADDRESS_DELAY_CAP_MS), 60_000);
    assert_eq!(backoff_ms(12, ADDRESS_DELAY_CAP_MS), 60_000);
    assert_eq!(backoff_ms(11, ADDRESS_DELAY_CAP_MS), 32_000);
}

pub fn address_keys_group_ipv6_by_64_and_never_collide_with_a_login() {
    let ip = |s: &str| ClientAddress::of(s.parse::<IpAddr>().unwrap());
    assert_eq!(ip("2001:db8:1:2::1"), ip("2001:db8:1:2:ffff::9"));
    assert_ne!(ip("2001:db8:1:2::1"), ip("2001:db8:1:3::1"));
    assert_eq!(ip("::ffff:203.0.113.7"), ip("203.0.113.7"));
    assert_ne!(ip("203.0.113.7"), ip("203.0.113.8"));
    let address = ip("203.0.113.7");
    let key = ThrottleKey::Address(&address);
    let login = LoginKey::new(key.as_str()).unwrap();
    assert_ne!(ThrottleKey::Login(&login).as_str(), key.as_str());
}

pub fn a_flood_on_one_login_and_address_leaves_other_logins_and_addresses_free(
    c: AccountStoreCase<'_>,
) {
    let x = LoginKey::new("x").unwrap();
    let y = LoginKey::new("y").unwrap();
    let a = ClientAddress::of("203.0.113.7".parse().unwrap());
    let b = ClientAddress::of("2001:db8::1".parse().unwrap());
    // Login X and address A fail 14 times together (as one sign-in records both).
    let mut at = NOW;
    for _ in 0..14 {
        let login = c
            .store
            .record(&ThrottleKey::Login(&x), Attempt::Failure, at)
            .unwrap();
        let address = c
            .store
            .record(&ThrottleKey::Address(&a), Attempt::Failure, at)
            .unwrap();
        assert!(address.retry_after_ms <= ADDRESS_DELAY_CAP_MS);
        at += login.retry_after_ms.max(address.retry_after_ms);
    }
    let login_x = c
        .store
        .record(&ThrottleKey::Login(&x), Attempt::Failure, at)
        .unwrap();
    assert!(login_x.locked && login_x.retry_after_ms > ADDRESS_DELAY_CAP_MS);
    let address_a = c
        .store
        .record(&ThrottleKey::Address(&a), Attempt::Failure, at)
        .unwrap();
    assert!(address_a.locked);
    assert!(address_a.retry_after_ms > 0 && address_a.retry_after_ms <= ADDRESS_DELAY_CAP_MS);
    // Login Y from address B: no delay. From address A: at most 60 s.
    for key in [ThrottleKey::Login(&y), ThrottleKey::Address(&b)] {
        assert_eq!(
            c.store.throttle(&key, at),
            Ok(Throttle {
                locked: false,
                retry_after_ms: 0
            })
        );
    }
    let wait = c
        .store
        .throttle(&ThrottleKey::Address(&a), at)
        .unwrap()
        .retry_after_ms;
    assert!(wait <= ADDRESS_DELAY_CAP_MS);
    assert!(
        !c.store
            .throttle(&ThrottleKey::Address(&a), at + wait)
            .unwrap()
            .locked
    );
    assert!(
        c.store
            .throttle(&ThrottleKey::Login(&x), at + wait)
            .unwrap()
            .locked
    );
}

pub fn locked_attempts_change_nothing_and_success_or_reset_clear_the_key(c: AccountStoreCase<'_>) {
    let x = LoginKey::new("x").unwrap();
    let key = ThrottleKey::Login(&x);
    let at = flood(c.store, &key, NOW, 6);
    let locked = c.store.record(&key, Attempt::Failure, at).unwrap();
    assert_eq!(
        locked,
        Throttle {
            locked: true,
            retry_after_ms: 2_000
        }
    );
    // A failure and a success while locked change nothing.
    assert_eq!(
        c.store
            .record(&key, Attempt::Failure, at + 500)
            .unwrap()
            .retry_after_ms,
        1_500
    );
    assert_eq!(
        c.store
            .record(&key, Attempt::Success, at + 500)
            .unwrap()
            .retry_after_ms,
        1_500
    );
    // A reset clears even a locked key.
    assert_eq!(
        c.store.record(&key, Attempt::Reset, at + 500),
        Ok(Throttle {
            locked: false,
            retry_after_ms: 0
        })
    );
    assert!(!c.store.throttle(&key, at + 500).unwrap().locked);
    // Success after the lock ends clears the count: five more failures are free.
    let at = flood(c.store, &key, at + 500, 6);
    c.store.record(&key, Attempt::Success, at).unwrap();
    assert_eq!(flood(c.store, &key, at, 5), at);
    assert_eq!(c.store.throttle(&key, -1), Err(StoreError::Unavailable));
    assert_eq!(
        c.store.record(&key, Attempt::Failure, -1),
        Err(StoreError::Unavailable)
    );
}
