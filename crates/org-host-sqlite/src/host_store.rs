//! Focused host-registry domain port implementations for the local SQLite host registry
//! ([`HostDb`]). Together with `AccountStore`, `OneTimeLinks`, and `BrowserSessions`, these
//! satisfy the composite [`org_host::ports::HostStore`] via its blanket implementation.

use std::collections::BTreeSet;

use org_host::ports::accounts::{
    AccountId, LoginKey, NewAccount, StoreError as AccountStoreError, Username, Verifier,
};
use org_host::ports::host_secrets::HostSecrets;
use org_host::ports::host_store::{
    accounts, journal, lockout, multi_person, provisioning, recovery, web_tls, Error,
    HostAccountsStore, LoginLockoutStore, OperationJournalStore, ProvisioningStore, RecoveryStore,
    WebTlsStore,
};
use org_host::ports::links::{Error as LinkError, Issued, LinkKind, Subject, Token};
use org_host::ports::sessions::Ended;
use org_host_control::browser_accounts::OrgAccessEntry;
use org_office_launch::recovery::PublicRecord;

use super::HostDb;

impl HostAccountsStore for HostDb {
    fn verify_instance(&self, instance: &str) -> Result<(), Error> {
        HostDb::verify_instance(self, instance)
    }
    fn establish_organization_owner(
        &self,
        administrator: &accounts::Account,
        org: &str,
        member: &str,
    ) -> Result<(), Error> {
        HostDb::establish_organization_owner(self, administrator, org, member)
    }
    fn verify_selected_owner(&self, org: &str, account: &str, member: &str) -> Result<(), Error> {
        HostDb::verify_selected_owner(self, org, account, member)
    }
    fn has_administrator(&self) -> Result<bool, Error> {
        HostDb::administrator_exists(self)
    }
    fn account_credentials(
        &self,
        login_key: &LoginKey,
    ) -> Result<Option<accounts::Credentials>, Error> {
        HostDb::account_credentials(self, login_key)
    }
    fn account_authority(
        &self,
        account_id: &AccountId,
    ) -> Result<Option<accounts::Account>, Error> {
        HostDb::account_authority(self, account_id)
    }
    fn set_account_preferences(
        &self,
        account_id: &AccountId,
        time_zone: Option<Option<&str>>,
        last_org_id: Option<Option<&str>>,
    ) -> Result<(Option<String>, Option<String>), Error> {
        HostDb::set_account_preferences(self, account_id, time_zone, last_org_id)
    }
    fn init_local_authless_if_empty(&self, now_ms: i64) -> Result<Option<AccountId>, Error> {
        HostDb::init_local_authless_if_empty(self, now_ms)
    }
    fn local_authless_account(&self) -> Result<Option<AccountId>, Error> {
        HostDb::local_authless_account(self)
    }
    fn auth_enabled(&self) -> Result<bool, Error> {
        HostDb::auth_enabled(self)
    }
    fn enable_local_auth(
        &self,
        username: &Username,
        verifier: &Verifier,
        now_ms: i64,
    ) -> Result<AccountId, Error> {
        HostDb::enable_local_auth(self, username, verifier, now_ms)
    }
    fn active_administrator_account(&self) -> Result<Option<(AccountId, String)>, Error> {
        HostDb::active_administrator_account(self)
    }
    fn active_account_login(&self, account_id: &AccountId) -> Result<Option<String>, Error> {
        HostDb::active_account_login(self, account_id)
    }
    fn org_owner(&self, org_id: &str) -> Result<Option<(AccountId, String)>, Error> {
        HostDb::org_owner(self, org_id)
    }
    fn list_org_access(&self, org_id: &str, now_ms: i64) -> Result<Vec<OrgAccessEntry>, Error> {
        HostDb::list_org_access(self, org_id, now_ms)
    }
    fn issue_org_invite(
        &self,
        org_id: &str,
        member_id: &str,
        now_ms: i64,
    ) -> Result<Issued, multi_person::InviteIssueError> {
        HostDb::issue_org_invite(self, org_id, member_id, now_ms)
    }
    fn redeem_invite_register(
        &self,
        token: &Token,
        new: &NewAccount,
        now_ms: i64,
    ) -> Result<multi_person::InviteRegistration, AccountStoreError> {
        HostDb::redeem_invite_register(self, token, new, now_ms)
    }
    fn redeem_invite_attach(
        &self,
        token: &Token,
        account_id: &AccountId,
        now_ms: i64,
    ) -> Result<multi_person::InviteAttach, AccountStoreError> {
        HostDb::redeem_invite_attach(self, token, account_id, now_ms)
    }
    fn issue_admin_password_reset_link(
        &self,
        now_ms: i64,
    ) -> Result<Option<(String, Issued)>, LinkError> {
        HostDb::issue_admin_password_reset_link(self, now_ms)
    }
    fn issue_member_password_reset_link(
        &self,
        org_id: &str,
        member_id: &str,
        now_ms: i64,
    ) -> Result<Issued, multi_person::MemberResetIssueError> {
        HostDb::issue_member_password_reset_link(self, org_id, member_id, now_ms)
    }
    fn redeem_set_password(
        &self,
        token: &Token,
        verifier: &Verifier,
        now_ms: i64,
    ) -> Result<multi_person::SetPasswordOutcome, AccountStoreError> {
        HostDb::redeem_set_password(self, token, verifier, now_ms)
    }
    fn change_own_password(
        &self,
        account_id: &AccountId,
        keep_session_id: &str,
        verifier: &Verifier,
        now_ms: i64,
    ) -> Result<Vec<Ended>, Error> {
        HostDb::change_own_password(self, account_id, keep_session_id, verifier, now_ms)
    }
    fn remove_org_access(
        &self,
        org_id: &str,
        member_id: &str,
        now_ms: i64,
    ) -> Result<multi_person::RemoveAccessOutcome, Error> {
        HostDb::remove_org_access(self, org_id, member_id, now_ms)
    }
    fn handover_org_owner(
        &self,
        org_id: &str,
        from_account: &AccountId,
        to_member_id: &str,
    ) -> Result<multi_person::OrgOwnerHandoverOutcome, Error> {
        HostDb::handover_org_owner(self, org_id, from_account, to_member_id)
    }
    fn restore_org_owner(
        &self,
        org_id: &str,
        account: &AccountId,
        member_id: &str,
    ) -> Result<(), Error> {
        HostDb::restore_org_owner(self, org_id, account, member_id)
    }
    fn handover_administrator(
        &self,
        from_account: &AccountId,
        to_account: &AccountId,
    ) -> Result<multi_person::AdminHandoverOutcome, Error> {
        HostDb::handover_administrator(self, from_account, to_account)
    }
    fn issue_unless_administrator(
        &self,
        kind: LinkKind,
        subject: &Subject,
        now_ms: i64,
    ) -> Result<Option<Issued>, LinkError> {
        HostDb::issue_unless_administrator(self, kind, subject, now_ms)
    }
}

impl OperationJournalStore for HostDb {
    fn journal_lookup(
        &self,
        op: &journal::Attempt<'_>,
        wall_ms: i64,
    ) -> Result<journal::Lookup, Error> {
        HostDb::lookup(self, op, wall_ms)
    }
    fn journal_commit(
        &self,
        op: &journal::Attempt<'_>,
        wall_ms: i64,
        effect: journal::Effect,
    ) -> Result<journal::Write, Error> {
        HostDb::write(self, op, wall_ms, |_| Ok(effect))
    }
    fn journal_prune_expired(&self, wall_ms: i64) -> Result<journal::PruneOutcome, Error> {
        HostDb::prune_expired(self, wall_ms)
    }
}

impl LoginLockoutStore for HostDb {
    fn login_lockout(
        &self,
        socket: lockout::Socket,
        key: &str,
        wall_ms: u64,
    ) -> Result<lockout::State, Error> {
        HostDb::login_lockout(self, socket, key, wall_ms)
    }
    fn record_login(
        &self,
        socket: lockout::Socket,
        key: &str,
        wall_ms: u64,
        outcome: lockout::Outcome,
    ) -> Result<lockout::State, Error> {
        HostDb::record_login(self, socket, key, wall_ms, outcome)
    }
    fn record_login_capped(
        &self,
        socket: lockout::Socket,
        key: &str,
        wall_ms: u64,
        outcome: lockout::Outcome,
        cap_ms: i64,
    ) -> Result<lockout::State, Error> {
        HostDb::record_login_capped(self, socket, key, wall_ms, outcome, cap_ms)
    }
    fn prune_login_lockouts(&self, wall_ms: u64) -> Result<usize, Error> {
        HostDb::prune_login_lockouts(self, wall_ms)
    }
}

impl ProvisioningStore for HostDb {
    fn provisioning_taken(
        &self,
        name: &str,
        slug: &str,
    ) -> Result<Option<provisioning::Taken>, Error> {
        HostDb::provisioning_taken(self, name, slug)
    }
    fn provisioning_known(&self, operation: &str) -> Result<bool, Error> {
        HostDb::provisioning_known(self, operation)
    }
    fn provisioning_linked_to(
        &self,
        org: &str,
    ) -> Result<Option<provisioning::Reservation>, Error> {
        HostDb::provisioning_linked_to(self, org)
    }
    fn provisioning_for_org(&self, org: &str) -> Result<Option<provisioning::Reservation>, Error> {
        HostDb::provisioning_for_org(self, org)
    }
    fn provisioning_reserved(&self) -> Result<Vec<provisioning::Reservation>, Error> {
        HostDb::provisioning_reserved(self)
    }
    fn provisioning_of(&self, account: &str) -> Result<Vec<provisioning::Reservation>, Error> {
        HostDb::provisioning_of(self, account)
    }
    fn provisioning_lookup(
        &self,
        account: &str,
        operation: &str,
    ) -> Result<Option<provisioning::Reservation>, Error> {
        HostDb::provisioning_lookup(self, account, operation)
    }
    fn provisioning_offices(&self) -> Result<BTreeSet<String>, Error> {
        HostDb::provisioning_offices(self)
    }
    fn provisioning(
        &self,
        account: &accounts::Account,
        operation: &str,
        fingerprint: &[u8; 32],
        wall: i64,
    ) -> Result<Option<provisioning::Reservation>, Error> {
        HostDb::provisioning(self, account, operation, fingerprint, wall)
    }
    fn reserve_provisioning(
        &self,
        account: &accounts::Account,
        frozen: &provisioning::Reservation,
    ) -> Result<provisioning::Reservation, Error> {
        HostDb::reserve_provisioning(self, account, frozen)
    }
    fn provision_key_stored(
        &self,
        r: &provisioning::Reservation,
        fingerprint: &str,
    ) -> Result<(), Error> {
        HostDb::provision_key_stored(self, r, fingerprint)
    }
    fn provision_linked(&self, r: &provisioning::Reservation) -> Result<(), Error> {
        HostDb::provision_linked(self, r)
    }
}

impl RecoveryStore for HostDb {
    fn recovery_state(&self, operation: &str) -> Result<Option<recovery::State>, Error> {
        HostDb::recovery_state(self, operation)
    }
    fn recovery_observed(&self, operation: &str) -> Result<recovery::Ledger, Error> {
        HostDb::recovery_observed(self, operation)
    }
    fn recovery_source(&self, org: &str) -> Result<provisioning::Reservation, Error> {
        HostDb::recovery_source(self, org)
    }
    fn recovery_status(
        &self,
        expected: &recovery::Ledger,
        after: i64,
    ) -> Result<org_host_control::recovery::StatusResult, Error> {
        HostDb::recovery_status(self, expected, after)
    }
    fn recovery_prepare(&self, public: &PublicRecord) -> Result<recovery::Ledger, Error> {
        HostDb::recovery_prepare(self, public)
    }
    fn recovery_delivered(
        &self,
        expected: &recovery::Ledger,
        account: &str,
        redeliver: bool,
        wall: i64,
    ) -> Result<recovery::Ledger, Error> {
        HostDb::recovery_delivered(self, expected, account, redeliver, wall)
    }
    fn recovery_ack_intent(
        &self,
        expected: &recovery::Ledger,
        account: &str,
        wall: i64,
    ) -> Result<recovery::Ledger, Error> {
        HostDb::recovery_ack_intent(self, expected, account, wall)
    }
    fn recovery_reconcile(&self, custody: &dyn HostSecrets) -> Result<(), Error> {
        HostDb::recovery_reconcile(self, custody)
    }
}

impl WebTlsStore for HostDb {
    fn web_tls_pair(&self) -> Result<Option<web_tls::StoredPair>, Error> {
        HostDb::web_tls_pair(self)
    }
    fn set_web_tls_pair(&self, pair: &web_tls::StoredPair) -> Result<(), Error> {
        HostDb::set_web_tls_pair(self, pair)
    }
    fn tls_set_lookup(
        &self,
        op: &journal::Attempt<'_>,
        wall_ms: i64,
    ) -> Result<web_tls::TlsLookup, Error> {
        HostDb::tls_set_lookup(self, op, wall_ms)
    }
    fn tls_set_write(
        &self,
        op: &journal::Attempt<'_>,
        wall_ms: i64,
        stored: &web_tls::TlsStored,
        pair: Option<&web_tls::StoredPair>,
    ) -> Result<Option<web_tls::TlsStored>, Error> {
        HostDb::tls_set_write(self, op, wall_ms, stored, pair)
    }
}
