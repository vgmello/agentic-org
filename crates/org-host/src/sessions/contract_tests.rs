//! Engine-independent `BrowserSessions` suite (ADR-0018 ENG-3 for host storage).
//! Runs all 14 shared conformance cases against `MemoryHostStore` and `MemorySessions` in
//! `org-host`; `org-host-sqlite/tests/conformance.rs` runs the exact same 14 cases against
//! SQLite `HostDb`.

use super::memory::MemorySessions;
use super::BrowserSessions;
use crate::accounts::verifier::Verifier;
use crate::hostdb::memory::MemoryHostStore;
use crate::test_support::conformance::sessions::{self, ALICE, BOB, INSTANCE, NOW, PHC};

fn memory_host_store() -> MemoryHostStore {
    let db = MemoryHostStore::new(INSTANCE, NOW);
    for (id, login) in [(ALICE, "alice"), (BOB, "bob")] {
        let verifier = Verifier::new(PHC.into()).unwrap();
        db.seed_account(id, login, &verifier, id == ALICE, NOW)
            .unwrap();
    }
    db
}

/// Runs `case` against both in-memory adapters.
fn each(case: impl Fn(&dyn BrowserSessions)) {
    case(&memory_host_store());
    case(&MemorySessions::new(&[ALICE, BOB], NOW));
}

#[test]
fn digests_are_domain_separated_sha256() {
    sessions::digests_are_domain_separated_sha256();
}

#[test]
fn origins_are_bounded_lowercase_hosts_with_a_port() {
    sessions::origins_are_bounded_lowercase_hosts_with_a_port();
}

#[test]
fn created_session_is_found_by_its_digests_and_origin_only() {
    each(sessions::created_session_is_found_by_its_digests_and_origin_only);
}

#[test]
fn idle_lifetime_ends_thirty_days_after_last_activity() {
    each(sessions::idle_lifetime_ends_thirty_days_after_last_activity);
}

#[test]
fn touch_is_monotone_and_ignores_unknown_ids() {
    each(sessions::touch_is_monotone_and_ignores_unknown_ids);
}

#[test]
fn end_removes_one_session_once() {
    each(sessions::end_removes_one_session_once);
}

#[test]
fn end_account_keeps_the_excepted_session_and_other_accounts() {
    each(sessions::end_account_keeps_the_excepted_session_and_other_accounts);
}

#[test]
fn reap_returns_exactly_the_expired_sessions() {
    each(sessions::reap_returns_exactly_the_expired_sessions);
}

#[test]
fn the_clock_never_runs_backward() {
    each(sessions::the_clock_never_runs_backward);
}

#[test]
fn a_far_future_touch_is_clamped_and_still_reaped_at_clock_plus_idle() {
    each(sessions::a_far_future_touch_is_clamped_and_still_reaped_at_clock_plus_idle);
}

#[test]
fn a_negative_time_is_refused_and_changes_nothing() {
    each(sessions::a_negative_time_is_refused_and_changes_nothing);
}

#[test]
fn an_expired_unreaped_duplicate_digest_is_reaped_and_replaced() {
    each(sessions::an_expired_unreaped_duplicate_digest_is_reaped_and_replaced);
}

#[test]
fn unknown_account_or_duplicate_digest_is_unavailable_and_stores_nothing() {
    each(sessions::unknown_account_or_duplicate_digest_is_unavailable_and_stores_nothing);
}

#[test]
fn the_host_wide_bound_reaps_before_refusing_and_refuses_every_account_alike() {
    each(sessions::the_host_wide_bound_reaps_before_refusing_and_refuses_every_account_alike);
}
