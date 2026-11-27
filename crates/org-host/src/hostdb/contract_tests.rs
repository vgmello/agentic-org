//! Engine-independent `HostStore` conformance suite (ADR-0018 ENG-2 / ENG-3).
//!
//! Runs all 12 shared conformance cases against `MemoryHostStore` inside `org-host` (keeping
//! `org-host` 100% `rusqlite`-free); `org-host-sqlite/tests/conformance.rs` runs the exact
//! same 12 cases against real SQLite `HostDb`.

use std::os::unix::fs::PermissionsExt;
use std::sync::Arc;

use super::journal::Verifier;
use super::memory::MemoryHostStore;
use super::HostStore;
use crate::test_support::conformance::host_store::{
    self, default_now_ms, HostStoreHarness, JournalPruneHarness, INSTANCE, PHC,
};

fn each(case: impl Fn(HostStoreHarness<'_>)) {
    let verifier = Verifier::new(PHC.into()).unwrap();
    let now = default_now_ms();
    let memory = MemoryHostStore::new(INSTANCE, now);
    case(HostStoreHarness {
        store: &memory,
        seed: &|id, login, admin| {
            memory
                .seed_account(id, login, &verifier, admin, now)
                .unwrap();
        },
        revoke: &|id| memory.revoke_account(id),
        seed_exempt_journal: &|reserved_id, staged_id, account, expires_at_ms| {
            memory.seed_expired_exempt_journal_rows(reserved_id, staged_id, account, expires_at_ms);
        },
    });
}

#[test]
fn instance_verification_and_organization_ownership_conformance() {
    each(host_store::instance_verification_and_organization_ownership);
}

#[test]
fn credentials_lookup_and_admin_guarded_links_conformance() {
    each(host_store::credentials_lookup_and_admin_guarded_links);
}

#[test]
fn login_lockout_backoff_and_pruning_conformance() {
    each(host_store::login_lockout_backoff_and_pruning);
}

#[test]
fn provisioning_lifecycle_conformance() {
    each(host_store::provisioning_lifecycle);
}

#[test]
fn web_tls_pair_and_journal_replay_conformance() {
    each(host_store::web_tls_pair_and_journal_replay);
}

#[test]
fn recovery_ledger_and_reconciliation_conformance() {
    each(host_store::recovery_ledger_and_reconciliation);
}

#[test]
fn account_preferences_and_local_authless_lifecycle_conformance() {
    each(host_store::account_preferences_and_local_authless_lifecycle);
}

#[test]
fn multi_person_invites_resets_handovers_and_access_removal_conformance() {
    each(host_store::multi_person_invites_resets_handovers_and_access_removal);
}

#[test]
fn operation_journal_lookup_and_commit_conformance() {
    each(host_store::operation_journal_lookup_and_commit);
}

#[test]
fn operation_journal_prune_expired_and_rollback_conformance() {
    each(host_store::operation_journal_prune_expired_and_rollback);
}

#[test]
fn memory_journal_prune_batch_cap_and_startup_wiring() {
    host_store::journal_prune_batch_cap_and_startup_wiring(|clock_ms| {
        let memory = Arc::new(MemoryHostStore::new(INSTANCE, clock_ms));
        let for_prune = Arc::clone(&memory);
        let for_exempt = Arc::clone(&memory);
        let for_verifiers = Arc::clone(&memory);
        JournalPruneHarness {
            store: memory as Arc<dyn HostStore>,
            prune_capped: Box::new(move |wall_ms, max_batches| {
                for_prune.journal_prune_expired_capped(wall_ms, max_batches)
            }),
            seed_exempt_journal: Box::new(move |reserved, staged, account, expires_at_ms| {
                for_exempt.seed_expired_exempt_journal_rows(
                    reserved,
                    staged,
                    account,
                    expires_at_ms,
                );
            }),
            op_verifier_accounts: Box::new(move || for_verifiers.op_verifier_accounts()),
            keep_alive: Box::new(()),
        }
    });
}

#[test]
fn live_host_subsystems_run_end_to_end_on_memory_host_store() {
    let root = tempfile::tempdir().unwrap();
    std::fs::set_permissions(root.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    let state_root = root.path().canonicalize().unwrap();
    let store: Arc<dyn HostStore> = Arc::new(MemoryHostStore::new(INSTANCE, default_now_ms()));
    host_store::live_host_subsystems_run_end_to_end(store, &state_root);
}
