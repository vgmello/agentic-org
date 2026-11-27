//! Engine-independent `AccountStore` suite (ADR-0018 ENG-3 for host storage).
//! Runs all 7 shared conformance cases against `MemoryHostStore` and `MemoryAccounts` in
//! `org-host`; `org-host-sqlite/tests/conformance.rs` runs the exact same 7 cases against
//! SQLite `HostDb`.

use super::memory::MemoryAccounts;
use crate::hostdb::memory::MemoryHostStore;
use crate::test_support::conformance::accounts::{self, AccountStoreCase, INSTANCE, NOW};

/// Runs `case` against both in-memory adapters.
fn each(case: impl Fn(AccountStoreCase<'_>)) {
    let host_mem = MemoryHostStore::new(INSTANCE, NOW);
    case(AccountStoreCase {
        store: &host_mem,
        links: &host_mem,
        revoke: &|id| host_mem.revoke_account(id),
    });
    let memory = MemoryAccounts::new(INSTANCE, NOW);
    case(AccountStoreCase {
        store: &memory,
        links: memory.links(),
        revoke: &|id| memory.revoke(id),
    });
}

#[test]
fn a_link_registers_the_administrator_once_and_is_consumed_with_it() {
    each(accounts::a_link_registers_the_administrator_once_and_is_consumed_with_it);
}

#[test]
fn credentials_are_found_by_the_nfc_case_folded_username_and_hold_only_the_hash() {
    each(accounts::credentials_are_found_by_the_nfc_case_folded_username_and_hold_only_the_hash);
}

#[test]
fn every_refusal_leaves_the_link_and_the_accounts_unchanged() {
    each(accounts::every_refusal_leaves_the_link_and_the_accounts_unchanged);
}

#[test]
fn the_schedule_starts_after_five_failures_and_doubles_to_its_cap() {
    accounts::the_schedule_starts_after_five_failures_and_doubles_to_its_cap();
}

#[test]
fn address_keys_group_ipv6_by_64_and_never_collide_with_a_login() {
    accounts::address_keys_group_ipv6_by_64_and_never_collide_with_a_login();
}

#[test]
fn a_flood_on_one_login_and_address_leaves_other_logins_and_addresses_free() {
    each(accounts::a_flood_on_one_login_and_address_leaves_other_logins_and_addresses_free);
}

#[test]
fn locked_attempts_change_nothing_and_success_or_reset_clear_the_key() {
    each(accounts::locked_attempts_change_nothing_and_success_or_reset_clear_the_key);
}
