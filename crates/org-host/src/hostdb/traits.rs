//! Cohesive, consumer-owned host-registry domain port traits (ADR-0018, `TASK-256`).
//!
//! Each host subsystem narrows its dependency to the focused sub-trait(s) it actually invokes:
//! - [`HostAccountsStore`] — instance/org ownership, account credentials/authority/preferences,
//!   local authless onboarding, multi-person invites/resets/handovers, and admin-guarded links,
//! - [`OperationJournalStore`] — idempotent host operation journal lookup, commit, and pruning,
//! - [`LoginLockoutStore`] — login throttle state and failure backoff persistence,
//! - [`ProvisioningStore`] — selected HQ organization reservation and lifecycle state,
//! - [`RecoveryStore`] — REC-DEL-1 recovery ledger and custody reconciliation,
//! - [`WebTlsStore`] — HTTPS listener TLS pair persistence and journaled `tls.set`.
//!
//! [`HostStore`] remains a zero-method composite supertrait with a blanket implementation so
//! storage adapters (SQLite `HostDb`, in-memory `MemoryHostStore`) implement each focused
//! sub-trait once and automatically satisfy `HostStore`.

use std::collections::BTreeSet;

use org_host_control::browser_accounts::OrgAccessEntry;
use org_office_launch::recovery::PublicRecord;

use super::{accounts, journal, lockout, multi_person, provisioning, recovery, web_tls, Error};
use crate::accounts::identity::{AccountId, LoginKey};
use crate::accounts::rules::Username;
use crate::accounts::store::{AccountStore, NewAccount, StoreError as AccountStoreError};
use crate::accounts::verifier::Verifier;
use crate::host_secrets::HostSecrets;
use crate::links::{Issued, LinkKind, OneTimeLinks, Subject, Token};
use crate::sessions::{BrowserSessions, Ended};

/// Instance/organization ownership, account authority, local authless onboarding, and
/// multi-person access/invitation/reset/handover persistence.
pub trait HostAccountsStore: Send + Sync {
    // Instance & organization ownership
    fn verify_instance(&self, instance: &str) -> Result<(), Error>;
    fn establish_organization_owner(
        &self,
        administrator: &accounts::Account,
        org: &str,
        member: &str,
    ) -> Result<(), Error>;
    fn verify_selected_owner(&self, org: &str, account: &str, member: &str) -> Result<(), Error>;

    // Accounts, preferences & administrator
    fn has_administrator(&self) -> Result<bool, Error>;
    fn account_credentials(
        &self,
        login_key: &LoginKey,
    ) -> Result<Option<accounts::Credentials>, Error>;
    fn account_authority(&self, account_id: &AccountId)
        -> Result<Option<accounts::Account>, Error>;
    fn set_account_preferences(
        &self,
        account_id: &AccountId,
        time_zone: Option<Option<&str>>,
        last_org_id: Option<Option<&str>>,
    ) -> Result<(Option<String>, Option<String>), Error>;

    // Local authless onboarding (ADR-0034)
    fn init_local_authless_if_empty(&self, now_ms: i64) -> Result<Option<AccountId>, Error>;
    fn local_authless_account(&self) -> Result<Option<AccountId>, Error>;
    fn auth_enabled(&self) -> Result<bool, Error> {
        Ok(self.local_authless_account()?.is_none())
    }
    fn enable_local_auth(
        &self,
        username: &Username,
        verifier: &Verifier,
        now_ms: i64,
    ) -> Result<AccountId, Error>;

    // Multi-person accounts, invitations, resets & handovers
    fn active_administrator_account(&self) -> Result<Option<(AccountId, String)>, Error>;
    fn active_account_login(&self, account_id: &AccountId) -> Result<Option<String>, Error>;
    fn org_owner(&self, org_id: &str) -> Result<Option<(AccountId, String)>, Error>;
    fn list_org_access(&self, org_id: &str, now_ms: i64) -> Result<Vec<OrgAccessEntry>, Error>;
    fn issue_org_invite(
        &self,
        org_id: &str,
        member_id: &str,
        now_ms: i64,
    ) -> Result<Issued, multi_person::InviteIssueError>;
    fn redeem_invite_register(
        &self,
        token: &Token,
        new: &NewAccount,
        now_ms: i64,
    ) -> Result<multi_person::InviteRegistration, AccountStoreError>;
    fn redeem_invite_attach(
        &self,
        token: &Token,
        account_id: &AccountId,
        now_ms: i64,
    ) -> Result<multi_person::InviteAttach, AccountStoreError>;
    fn issue_admin_password_reset_link(
        &self,
        now_ms: i64,
    ) -> Result<Option<(String, Issued)>, crate::links::Error>;
    fn issue_member_password_reset_link(
        &self,
        org_id: &str,
        member_id: &str,
        now_ms: i64,
    ) -> Result<Issued, multi_person::MemberResetIssueError>;
    fn redeem_set_password(
        &self,
        token: &Token,
        verifier: &Verifier,
        now_ms: i64,
    ) -> Result<multi_person::SetPasswordOutcome, AccountStoreError>;
    fn change_own_password(
        &self,
        account_id: &AccountId,
        keep_session_id: &str,
        verifier: &Verifier,
        now_ms: i64,
    ) -> Result<Vec<Ended>, Error>;
    fn remove_org_access(
        &self,
        org_id: &str,
        member_id: &str,
        now_ms: i64,
    ) -> Result<multi_person::RemoveAccessOutcome, Error>;
    fn handover_org_owner(
        &self,
        org_id: &str,
        from_account: &AccountId,
        to_member_id: &str,
    ) -> Result<multi_person::OrgOwnerHandoverOutcome, Error>;
    fn restore_org_owner(
        &self,
        org_id: &str,
        account: &AccountId,
        member_id: &str,
    ) -> Result<(), Error>;
    fn handover_administrator(
        &self,
        from_account: &AccountId,
        to_account: &AccountId,
    ) -> Result<multi_person::AdminHandoverOutcome, Error>;

    // One-time links with administrator guard
    fn issue_unless_administrator(
        &self,
        kind: LinkKind,
        subject: &Subject,
        now_ms: i64,
    ) -> Result<Option<Issued>, crate::links::Error>;
}

/// Idempotent host operation journal persistence.
pub trait OperationJournalStore: Send + Sync {
    fn journal_lookup(
        &self,
        op: &journal::Attempt<'_>,
        wall_ms: i64,
    ) -> Result<journal::Lookup, Error>;
    fn journal_commit(
        &self,
        op: &journal::Attempt<'_>,
        wall_ms: i64,
        effect: journal::Effect,
    ) -> Result<journal::Write, Error>;
    fn journal_prune_expired(&self, wall_ms: i64) -> Result<journal::PruneOutcome, Error>;
}

/// Per-socket and per-login authentication lockout persistence.
pub trait LoginLockoutStore: Send + Sync {
    fn login_lockout(
        &self,
        socket: lockout::Socket,
        key: &str,
        wall_ms: u64,
    ) -> Result<lockout::State, Error>;
    fn record_login(
        &self,
        socket: lockout::Socket,
        key: &str,
        wall_ms: u64,
        outcome: lockout::Outcome,
    ) -> Result<lockout::State, Error>;
    fn record_login_capped(
        &self,
        socket: lockout::Socket,
        key: &str,
        wall_ms: u64,
        outcome: lockout::Outcome,
        cap_ms: i64,
    ) -> Result<lockout::State, Error>;
    fn prune_login_lockouts(&self, wall_ms: u64) -> Result<usize, Error>;
}

/// Selected HQ organization provisioning reservation and linkage persistence.
pub trait ProvisioningStore: Send + Sync {
    fn provisioning_taken(
        &self,
        name: &str,
        slug: &str,
    ) -> Result<Option<provisioning::Taken>, Error>;
    fn provisioning_known(&self, operation: &str) -> Result<bool, Error>;
    fn provisioning_linked_to(&self, org: &str)
        -> Result<Option<provisioning::Reservation>, Error>;
    fn provisioning_for_org(&self, org: &str) -> Result<Option<provisioning::Reservation>, Error>;
    fn provisioning_reserved(&self) -> Result<Vec<provisioning::Reservation>, Error>;
    fn provisioning_of(&self, account: &str) -> Result<Vec<provisioning::Reservation>, Error>;
    fn provisioning_lookup(
        &self,
        account: &str,
        operation: &str,
    ) -> Result<Option<provisioning::Reservation>, Error>;
    fn provisioning_offices(&self) -> Result<BTreeSet<String>, Error>;
    fn provisioning(
        &self,
        account: &accounts::Account,
        operation: &str,
        fingerprint: &[u8; 32],
        wall: i64,
    ) -> Result<Option<provisioning::Reservation>, Error>;
    fn reserve_provisioning(
        &self,
        account: &accounts::Account,
        frozen: &provisioning::Reservation,
    ) -> Result<provisioning::Reservation, Error>;
    fn provision_key_stored(
        &self,
        r: &provisioning::Reservation,
        fingerprint: &str,
    ) -> Result<(), Error>;
    fn provision_linked(&self, r: &provisioning::Reservation) -> Result<(), Error>;
}

/// REC-DEL-1 recovery delivery ledger and custody reconciliation persistence.
pub trait RecoveryStore: Send + Sync {
    fn recovery_state(&self, operation: &str) -> Result<Option<recovery::State>, Error>;
    fn recovery_observed(&self, operation: &str) -> Result<recovery::Ledger, Error>;
    fn recovery_source(&self, org: &str) -> Result<provisioning::Reservation, Error>;
    fn recovery_status(
        &self,
        expected: &recovery::Ledger,
        after: i64,
    ) -> Result<org_host_control::recovery::StatusResult, Error>;
    fn recovery_prepare(&self, public: &PublicRecord) -> Result<recovery::Ledger, Error>;
    fn recovery_delivered(
        &self,
        expected: &recovery::Ledger,
        account: &str,
        redeliver: bool,
        wall: i64,
    ) -> Result<recovery::Ledger, Error>;
    fn recovery_ack_intent(
        &self,
        expected: &recovery::Ledger,
        account: &str,
        wall: i64,
    ) -> Result<recovery::Ledger, Error>;
    fn recovery_reconcile(&self, custody: &dyn HostSecrets) -> Result<(), Error>;
}

/// Browser HTTPS listener TLS pair and journaled `tls.set` persistence.
pub trait WebTlsStore: Send + Sync {
    fn web_tls_pair(&self) -> Result<Option<web_tls::StoredPair>, Error>;
    fn set_web_tls_pair(&self, pair: &web_tls::StoredPair) -> Result<(), Error>;
    fn tls_set_lookup(
        &self,
        op: &journal::Attempt<'_>,
        wall_ms: i64,
    ) -> Result<web_tls::TlsLookup, Error>;
    fn tls_set_write(
        &self,
        op: &journal::Attempt<'_>,
        wall_ms: i64,
        stored: &web_tls::TlsStored,
        pair: Option<&web_tls::StoredPair>,
    ) -> Result<Option<web_tls::TlsStored>, Error>;
}

/// Engine-neutral composite host-registry persistence contract (ADR-0018).
///
/// Thin zero-method supertrait combining every focused host-registry domain port so adapters
/// implement the focused traits and automatically satisfy `HostStore`.
pub trait HostStore:
    AccountStore
    + OneTimeLinks
    + BrowserSessions
    + HostAccountsStore
    + OperationJournalStore
    + LoginLockoutStore
    + ProvisioningStore
    + RecoveryStore
    + WebTlsStore
    + Send
    + Sync
{
}

impl<T> HostStore for T where
    T: AccountStore
        + OneTimeLinks
        + BrowserSessions
        + HostAccountsStore
        + OperationJournalStore
        + LoginLockoutStore
        + ProvisioningStore
        + RecoveryStore
        + WebTlsStore
        + Send
        + Sync
{
}
