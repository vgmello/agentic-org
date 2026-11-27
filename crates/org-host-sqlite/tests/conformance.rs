//! Full ADR-0018 ENG-3 shared conformance suite against SQLite `HostDb`.
//!
//! Runs all 49 shared engine-independent conformance cases exported by
//! `org_host::test_support::conformance`:
//! - 7 `AccountStore` cases,
//! - 12 `HostStore` cases,
//! - 16 `OneTimeLinks` cases,
//! - 14 `BrowserSessions` cases.

#![cfg(debug_assertions)]

use std::os::unix::fs::PermissionsExt;
use std::sync::Arc;

use org_host::ports::accounts::Verifier;
use org_host::ports::host_secrets::{Error as SecretsError, HostSecrets, Name, Value};
use org_host::ports::host_store::{HostDbInit, HostStore};
use org_host::test_support::conformance::{accounts, host_store, links, sessions};
use org_host_sqlite::test_support::{
    journal_prune_expired_capped, op_verifier_accounts, relax_durability,
    seed_expired_exempt_journal_rows,
};
use org_host_sqlite::HostDb;

struct NoSecrets;

impl HostSecrets for NoSecrets {
    fn put(&self, _: &Name, _: &Value) -> Result<(), SecretsError> {
        Ok(())
    }
    fn load(&self, _: &Name) -> Result<Option<Value>, SecretsError> {
        Ok(None)
    }
    fn delete(&self, _: &Name) -> Result<(), SecretsError> {
        Ok(())
    }
    fn list(&self) -> Result<Vec<Name>, SecretsError> {
        Ok(Vec::new())
    }
}

fn sqlite_db(instance: &str, clock_high_ms: i64) -> (tempfile::TempDir, HostDb) {
    let home = tempfile::tempdir().unwrap();
    std::fs::set_permissions(home.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    let init = HostDbInit {
        instance_id: instance.into(),
        clock_high_ms,
    };
    let db = HostDb::create(home.path(), &NoSecrets, &init).unwrap();
    (home, db)
}

fn run_account_case(case: impl FnOnce(accounts::AccountStoreCase<'_>)) {
    let (_home, db) = sqlite_db(accounts::INSTANCE, accounts::NOW);
    case(accounts::AccountStoreCase {
        store: &db,
        links: &db,
        revoke: &|id| db.change_authority_fixture(id, true).unwrap(),
    });
}

fn run_host_store_case(case: impl FnOnce(host_store::HostStoreHarness<'_>)) {
    let verifier = Verifier::new(host_store::PHC.into()).unwrap();
    let now = host_store::default_now_ms();
    let (_home, db) = sqlite_db(host_store::INSTANCE, now);
    relax_durability(&db);
    case(host_store::HostStoreHarness {
        store: &db,
        seed: &|id, login, admin| {
            db.seed_account(id, login, &verifier, admin, now).unwrap();
        },
        revoke: &|id| db.change_authority_fixture(id, true).unwrap(),
        seed_exempt_journal: &|reserved_id, staged_id, account, expires_at_ms| {
            seed_expired_exempt_journal_rows(&db, reserved_id, staged_id, account, expires_at_ms);
        },
    });
}

fn run_links_case(case: impl FnOnce(&dyn org_host::ports::OneTimeLinks)) {
    let (_home, db) = sqlite_db(links::INSTANCE, links::NOW);
    case(&db);
}

fn run_sessions_case(case: impl FnOnce(&dyn org_host::ports::BrowserSessions)) {
    let (_home, db) = sqlite_db(sessions::INSTANCE, sessions::NOW);
    for (id, login) in [(sessions::ALICE, "alice"), (sessions::BOB, "bob")] {
        let verifier = Verifier::new(sessions::PHC.into()).unwrap();
        db.seed_account(id, login, &verifier, id == sessions::ALICE, sessions::NOW)
            .unwrap();
    }
    relax_durability(&db);
    case(&db);
}

// -----------------------------------------------------------------------------
// AccountStore conformance (7 cases)
// -----------------------------------------------------------------------------

#[test]
fn sqlite_accounts_a_link_registers_the_administrator_once_and_is_consumed_with_it() {
    run_account_case(accounts::a_link_registers_the_administrator_once_and_is_consumed_with_it);
}

#[test]
fn sqlite_accounts_credentials_are_found_by_the_nfc_case_folded_username_and_hold_only_the_hash() {
    run_account_case(
        accounts::credentials_are_found_by_the_nfc_case_folded_username_and_hold_only_the_hash,
    );
}

#[test]
fn sqlite_accounts_every_refusal_leaves_the_link_and_the_accounts_unchanged() {
    run_account_case(accounts::every_refusal_leaves_the_link_and_the_accounts_unchanged);
}

#[test]
fn sqlite_accounts_the_schedule_starts_after_five_failures_and_doubles_to_its_cap() {
    accounts::the_schedule_starts_after_five_failures_and_doubles_to_its_cap();
}

#[test]
fn sqlite_accounts_address_keys_group_ipv6_by_64_and_never_collide_with_a_login() {
    accounts::address_keys_group_ipv6_by_64_and_never_collide_with_a_login();
}

#[test]
fn sqlite_accounts_a_flood_on_one_login_and_address_leaves_other_logins_and_addresses_free() {
    run_account_case(
        accounts::a_flood_on_one_login_and_address_leaves_other_logins_and_addresses_free,
    );
}

#[test]
fn sqlite_accounts_locked_attempts_change_nothing_and_success_or_reset_clear_the_key() {
    run_account_case(accounts::locked_attempts_change_nothing_and_success_or_reset_clear_the_key);
}

// -----------------------------------------------------------------------------
// HostStore conformance (12 cases)
// -----------------------------------------------------------------------------

#[test]
fn sqlite_host_store_instance_verification_and_organization_ownership() {
    run_host_store_case(host_store::instance_verification_and_organization_ownership);
}

#[test]
fn sqlite_host_store_credentials_lookup_and_admin_guarded_links() {
    run_host_store_case(host_store::credentials_lookup_and_admin_guarded_links);
}

#[test]
fn sqlite_host_store_login_lockout_backoff_and_pruning() {
    run_host_store_case(host_store::login_lockout_backoff_and_pruning);
}

#[test]
fn sqlite_host_store_provisioning_lifecycle() {
    run_host_store_case(host_store::provisioning_lifecycle);
}

#[test]
fn sqlite_host_store_web_tls_pair_and_journal_replay() {
    run_host_store_case(host_store::web_tls_pair_and_journal_replay);
}

#[test]
fn sqlite_host_store_recovery_ledger_and_reconciliation() {
    run_host_store_case(host_store::recovery_ledger_and_reconciliation);
}

#[test]
fn sqlite_host_store_account_preferences_and_local_authless_lifecycle() {
    run_host_store_case(host_store::account_preferences_and_local_authless_lifecycle);
}

#[test]
fn sqlite_host_store_multi_person_invites_resets_handovers_and_access_removal() {
    run_host_store_case(host_store::multi_person_invites_resets_handovers_and_access_removal);
}

#[test]
fn sqlite_host_store_operation_journal_lookup_and_commit() {
    run_host_store_case(host_store::operation_journal_lookup_and_commit);
}

#[test]
fn sqlite_host_store_operation_journal_prune_expired_and_rollback() {
    run_host_store_case(host_store::operation_journal_prune_expired_and_rollback);
}

#[test]
fn sqlite_host_store_journal_prune_batch_cap_and_startup_wiring() {
    host_store::journal_prune_batch_cap_and_startup_wiring(|clock_ms| {
        let (home, db) = sqlite_db(host_store::INSTANCE, clock_ms);
        relax_durability(&db);
        let db = Arc::new(db);
        let for_prune = Arc::clone(&db);
        let for_exempt = Arc::clone(&db);
        let for_verifiers = Arc::clone(&db);
        host_store::JournalPruneHarness {
            store: db as Arc<dyn HostStore>,
            prune_capped: Box::new(move |wall_ms, max_batches| {
                journal_prune_expired_capped(&for_prune, wall_ms, max_batches)
            }),
            seed_exempt_journal: Box::new(move |reserved, staged, account, expires_at_ms| {
                seed_expired_exempt_journal_rows(
                    &for_exempt,
                    reserved,
                    staged,
                    account,
                    expires_at_ms,
                );
            }),
            op_verifier_accounts: Box::new(move || op_verifier_accounts(&for_verifiers)),
            keep_alive: Box::new(home),
        }
    });
}

#[test]
fn sqlite_host_store_live_host_subsystems_run_end_to_end() {
    let (_db_home, db) = sqlite_db(host_store::INSTANCE, host_store::default_now_ms());
    let root = tempfile::tempdir().unwrap();
    std::fs::set_permissions(root.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    let state_root = root.path().canonicalize().unwrap();
    let store: Arc<dyn HostStore> = Arc::new(db);
    host_store::live_host_subsystems_run_end_to_end(store, &state_root);
}

// -----------------------------------------------------------------------------
// OneTimeLinks conformance (16 cases)
// -----------------------------------------------------------------------------

#[test]
fn sqlite_links_issued_link_redeems_once_then_is_used() {
    run_links_case(links::issued_link_redeems_once_then_is_used);
}

#[test]
fn sqlite_links_issue_first_issues_once_per_subject_ever_and_a_replay_has_no_secret() {
    run_links_case(links::issue_first_issues_once_per_subject_ever_and_a_replay_has_no_secret);
}

#[test]
fn sqlite_links_issuing_again_cancels_the_pending_link_of_the_same_subject_only() {
    run_links_case(links::issuing_again_cancels_the_pending_link_of_the_same_subject_only);
}

#[test]
fn sqlite_links_a_link_expires_after_24_hours_by_the_injected_clock() {
    run_links_case(links::a_link_expires_after_24_hours_by_the_injected_clock);
}

#[test]
fn sqlite_links_would_redeem_reports_every_state_without_changing_the_link() {
    run_links_case(links::would_redeem_reports_every_state_without_changing_the_link);
}

#[test]
fn sqlite_links_unknown_and_malformed_tokens_are_unknown() {
    run_links_case(links::unknown_and_malformed_tokens_are_unknown);
}

#[test]
fn sqlite_links_the_digest_is_bound_to_the_instance_and_kind() {
    let (_a, issuer) = sqlite_db(links::INSTANCE, links::NOW);
    let (_b, redeemer) = sqlite_db(links::OTHER_INSTANCE, links::NOW);
    links::the_digest_is_bound_to_the_instance_and_kind(&issuer, &redeemer);
}

#[test]
fn sqlite_links_subjects_are_bounded_printable_identifiers() {
    links::subjects_are_bounded_printable_identifiers();
}

#[test]
fn sqlite_links_token_parse_refuses_wrong_lengths_before_decoding() {
    links::token_parse_refuses_wrong_lengths_before_decoding();
}

#[test]
fn sqlite_links_token_parse_refuses_every_noncanonical_final_character() {
    links::token_parse_refuses_every_noncanonical_final_character();
}

#[test]
fn sqlite_links_encoded_token_text_is_zeroized_on_drop() {
    links::encoded_token_text_is_zeroized_on_drop();
}

#[test]
fn sqlite_links_token_is_zeroized_on_drop() {
    links::token_is_zeroized_on_drop();
}

#[test]
fn sqlite_links_digest_fields_have_explicit_boundaries() {
    links::digest_fields_have_explicit_boundaries();
}

#[test]
fn sqlite_links_token_text_round_trips_and_debug_hides_it() {
    links::token_text_round_trips_and_debug_hides_it();
}

#[test]
fn sqlite_links_a_failed_replacement_keeps_the_previous_pending_link() {
    run_links_case(links::a_failed_replacement_keeps_the_previous_pending_link);
}

#[test]
fn sqlite_links_peek_link_subject_reports_subject_only_when_pending_and_not_expired() {
    run_links_case(links::peek_link_subject_reports_subject_only_when_pending_and_not_expired);
}

// -----------------------------------------------------------------------------
// BrowserSessions conformance (14 cases)
// -----------------------------------------------------------------------------

#[test]
fn sqlite_sessions_digests_are_domain_separated_sha256() {
    sessions::digests_are_domain_separated_sha256();
}

#[test]
fn sqlite_sessions_origins_are_bounded_lowercase_hosts_with_a_port() {
    sessions::origins_are_bounded_lowercase_hosts_with_a_port();
}

#[test]
fn sqlite_sessions_created_session_is_found_by_its_digests_and_origin_only() {
    run_sessions_case(sessions::created_session_is_found_by_its_digests_and_origin_only);
}

#[test]
fn sqlite_sessions_idle_lifetime_ends_thirty_days_after_last_activity() {
    run_sessions_case(sessions::idle_lifetime_ends_thirty_days_after_last_activity);
}

#[test]
fn sqlite_sessions_touch_is_monotone_and_ignores_unknown_ids() {
    run_sessions_case(sessions::touch_is_monotone_and_ignores_unknown_ids);
}

#[test]
fn sqlite_sessions_end_removes_one_session_once() {
    run_sessions_case(sessions::end_removes_one_session_once);
}

#[test]
fn sqlite_sessions_end_account_keeps_the_excepted_session_and_other_accounts() {
    run_sessions_case(sessions::end_account_keeps_the_excepted_session_and_other_accounts);
}

#[test]
fn sqlite_sessions_reap_returns_exactly_the_expired_sessions() {
    run_sessions_case(sessions::reap_returns_exactly_the_expired_sessions);
}

#[test]
fn sqlite_sessions_the_clock_never_runs_backward() {
    run_sessions_case(sessions::the_clock_never_runs_backward);
}

#[test]
fn sqlite_sessions_a_far_future_touch_is_clamped_and_still_reaped_at_clock_plus_idle() {
    run_sessions_case(sessions::a_far_future_touch_is_clamped_and_still_reaped_at_clock_plus_idle);
}

#[test]
fn sqlite_sessions_a_negative_time_is_refused_and_changes_nothing() {
    run_sessions_case(sessions::a_negative_time_is_refused_and_changes_nothing);
}

#[test]
fn sqlite_sessions_an_expired_unreaped_duplicate_digest_is_reaped_and_replaced() {
    run_sessions_case(sessions::an_expired_unreaped_duplicate_digest_is_reaped_and_replaced);
}

#[test]
fn sqlite_sessions_unknown_account_or_duplicate_digest_is_unavailable_and_stores_nothing() {
    run_sessions_case(
        sessions::unknown_account_or_duplicate_digest_is_unavailable_and_stores_nothing,
    );
}

#[test]
fn sqlite_sessions_the_host_wide_bound_reaps_before_refusing_and_refuses_every_account_alike() {
    run_sessions_case(
        sessions::the_host_wide_bound_reaps_before_refusing_and_refuses_every_account_alike,
    );
}
