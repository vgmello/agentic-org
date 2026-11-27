//! Engine-neutral host-registry persistence contract (ADR-0018).
//!
//! [`HostStore`] is the host's storage port; [`Error`] and [`StoreFault`] carry no driver
//! types. The SQLite adapter lives under [`sqlite`] (the only module in `org-host` that
//! imports `rusqlite` or holds SQL), [`memory`] provides a second in-memory adapter in
//! tests, and `contract_tests` runs one conformance suite against both.

pub mod multi_person;
pub(crate) mod pending_recovery;
pub(crate) mod sqlite;

#[cfg(test)]
mod contract_tests;
#[cfg(test)]
mod testing;
#[cfg(test)]
pub(crate) use testing::memory;

use std::collections::BTreeSet;
use std::io;

use org_host_control::browser_accounts::OrgAccessEntry;
use org_office_launch::recovery::PublicRecord;

use crate::accounts::identity::{AccountId, LoginKey};
use crate::accounts::rules::Username;
use crate::accounts::store::{AccountStore, NewAccount, StoreError as AccountStoreError};
use crate::accounts::verifier::Verifier;
use crate::host_secrets::HostSecrets;
use crate::links::{Issued, LinkKind, OneTimeLinks, Subject, Token};
use crate::sessions::{BrowserSessions, Ended};

pub(crate) use sqlite::HostDb;

/// Sentinel password PHC stored for the implicit local-authless placeholder account (ADR-0034).
/// It is deliberately not a valid Argon2id PHC and can never satisfy password verification.
pub(crate) const LOCAL_AUTHLESS_VERIFIER: &str = "!local-authless";

/// Engine-neutral class of storage failure (ADR-0018).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StoreFault {
    /// The store is busy or locked by another writer (transient).
    Busy,
    /// A transient I/O failure in the storage engine.
    TransientIo,
    /// The storage engine ran out of memory (transient).
    OutOfMemory,
    /// A uniqueness, check, or foreign-key constraint refused the write.
    Constraint,
    /// The store is corrupt or not a valid database.
    Corrupt,
    /// Any other storage engine failure.
    Other,
}

impl StoreFault {
    /// Whether this storage fault may pass on its own and warrants retry rather than
    /// permanent refusal.
    pub fn is_transient(self) -> bool {
        matches!(self, Self::Busy | Self::TransientIo | Self::OutOfMemory)
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum Error {
    InvalidId,
    Expired,
    ClockSkew,
    OperationReused,
    Conflict,
    /// The account is not a live instance administrator.
    NotAdministrator,
    Unsafe,
    Exists,
    Empty,
    Version,
    /// The registry predates a table this build needs and holds a different one.
    Outdated,
    Io(io::ErrorKind),
    Store(StoreFault),
    Secret(crate::host_secrets::Error),
}

impl From<crate::accounts::verifier::InvalidVerifier> for Error {
    fn from(_: crate::accounts::verifier::InvalidVerifier) -> Self {
        Self::Unsafe
    }
}

impl From<io::Error> for Error {
    fn from(e: io::Error) -> Self {
        Self::Io(e.kind())
    }
}

impl From<crate::host_secrets::Error> for Error {
    fn from(e: crate::host_secrets::Error) -> Self {
        Self::Secret(e)
    }
}

/// What a new registry records. Accounts arrive later, through a one-time link.
pub(crate) struct HostDbInit {
    pub(crate) instance_id: String,
    pub(crate) clock_high_ms: i64,
}

/// Engine-neutral account snapshot types.
pub mod accounts {
    use super::journal::Verifier;
    use crate::accounts::identity::AccountId;

    #[derive(Clone, Debug, PartialEq, Eq)]
    pub struct Link {
        pub(in crate::hostdb) org_id: String,
        pub(in crate::hostdb) member_id: String,
    }

    /// Stored authority, including revoked accounts; office IDs come from the catalog later.
    #[derive(Clone, Debug, PartialEq, Eq)]
    pub struct Account {
        pub(in crate::hostdb) account_id: AccountId,
        pub(in crate::hostdb) login: String,
        pub(in crate::hostdb) admin: bool,
        pub(in crate::hostdb) revoked_at_ms: Option<i64>,
        pub(in crate::hostdb) password_changed_at_ms: i64,
        pub(in crate::hostdb) links: Vec<Link>,
        pub(in crate::hostdb) time_zone: Option<String>,
        pub(in crate::hostdb) last_org_id: Option<String>,
    }

    /// An owned PHC for hashing outside the database lock; deliberately has no Debug.
    pub struct Credentials {
        pub(in crate::hostdb) account: Account,
        pub(in crate::hostdb) verifier: Verifier,
    }

    impl Link {
        pub fn new(org_id: String, member_id: String) -> Self {
            Self { org_id, member_id }
        }
        pub fn org_id(&self) -> &str {
            &self.org_id
        }
        pub fn member_id(&self) -> &str {
            &self.member_id
        }
    }

    impl Account {
        pub fn new(
            account_id: AccountId,
            login: String,
            admin: bool,
            revoked_at_ms: Option<i64>,
            password_changed_at_ms: i64,
            links: Vec<Link>,
        ) -> Self {
            Self {
                account_id,
                login,
                admin,
                revoked_at_ms,
                password_changed_at_ms,
                links,
                time_zone: None,
                last_org_id: None,
            }
        }
        pub fn with_preferences(
            mut self,
            time_zone: Option<String>,
            last_org_id: Option<String>,
        ) -> Self {
            self.time_zone = time_zone;
            self.last_org_id = last_org_id;
            self
        }
        pub fn account_id(&self) -> &str {
            self.account_id.as_str()
        }
        pub fn login(&self) -> &str {
            &self.login
        }
        pub fn admin(&self) -> bool {
            self.admin
        }
        pub fn revoked_at_ms(&self) -> Option<i64> {
            self.revoked_at_ms
        }
        pub fn password_changed_at_ms(&self) -> i64 {
            self.password_changed_at_ms
        }
        pub fn links(&self) -> &[Link] {
            &self.links
        }
        pub fn time_zone(&self) -> Option<&str> {
            self.time_zone.as_deref()
        }
        pub fn last_org_id(&self) -> Option<&str> {
            self.last_org_id.as_deref()
        }
    }

    impl Credentials {
        pub fn new(account: Account, verifier: Verifier) -> Self {
            Self { account, verifier }
        }
        pub fn account(&self) -> &Account {
            &self.account
        }
        /// Borrow the checked verifier; password hashing belongs outside the registry lock.
        pub fn verifier(&self) -> &Verifier {
            &self.verifier
        }
    }
}

/// Engine-neutral operation journal types.
pub mod journal {
    use super::Error;
    use org_host_control::{journal::Journaled, HostOpResult};

    pub const DAY: i64 = 86_400_000;
    /// The most expired rows one journal maintenance batch removes, bounding every
    /// journaled write's work.
    pub const PRUNE_BATCH: u16 = 256;
    /// Hard upper bound on startup prune batches (1,024 × 256 = 262,144 rows per boot).
    pub const MAX_PRUNE_BATCHES: usize = 1024;
    pub type Stored = HostOpResult<serde_json::Value>;

    /// Summary of one startup [`HostStore::journal_prune_expired`](super::HostStore::journal_prune_expired) pass.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct PruneOutcome {
        pub removed: u64,
        pub more_remaining: bool,
    }

    pub struct Attempt<'a> {
        pub id: &'a str,
        pub kind: &'static str,
        pub fingerprint: [u8; 32],
        pub account: &'a str,
    }

    impl<'a> Attempt<'a> {
        pub fn new<J: Journaled>(op: &'a J, account: &'a str) -> Result<Self, Error> {
            Ok(Self {
                id: op.operation_id(),
                kind: J::METHOD,
                fingerprint: org_host_control::journal::fingerprint(op)
                    .map_err(|_| Error::Unsafe)?,
                account,
            })
        }
    }

    pub use crate::accounts::verifier::Verifier;

    pub enum Lookup {
        Vacant,
        Replay(Stored),
        NeedsVerification { verifier: Verifier, stored: Stored },
    }

    pub struct Effect {
        pub stored: Stored,
        /// Created account, not the acting account. Required only for a successful account.create.
        pub verifier: Option<(String, Verifier)>,
    }

    pub enum Write {
        Committed(Stored),
        Existing(Lookup),
    }

    pub fn id_time(id: &str) -> Result<i64, Error> {
        org_proto::uuid_v7_time_ms(id)
            .and_then(|n| i64::try_from(n).ok())
            .ok_or(Error::InvalidId)
    }
}

/// Engine-neutral login lockout types.
pub mod lockout {
    #[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
    pub enum Socket {
        Control,
        Local,
    }

    impl Socket {
        pub fn as_str(self) -> &'static str {
            match self {
                Self::Control => "control",
                Self::Local => "local",
            }
        }
    }

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub enum Outcome {
        Failure,
        Success,
        /// Only an already authorized password/reset operation may request this.
        Reset,
    }

    #[derive(Debug, PartialEq, Eq)]
    pub struct State {
        pub(in crate::hostdb) now_ms: i64,
        pub(in crate::hostdb) locked: bool,
        pub(in crate::hostdb) until_ms: i64,
    }

    impl State {
        pub fn new(now_ms: i64, locked: bool, until_ms: i64) -> Self {
            Self {
                now_ms,
                locked,
                until_ms,
            }
        }
        /// How long until the key accepts another attempt; zero when unlocked.
        pub fn retry_after_ms(&self) -> i64 {
            self.until_ms.saturating_sub(self.now_ms).max(0)
        }
        pub fn now_ms(&self) -> i64 {
            self.now_ms
        }
        pub fn is_locked(&self) -> bool {
            self.locked
        }
    }
}

/// Engine-neutral selected HQ provisioning types.
pub mod provisioning {
    use org_proto::selected_bootstrap::PreparedV1;
    use serde::{Deserialize, Serialize};

    #[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
    #[serde(deny_unknown_fields)]
    pub struct Reservation {
        pub operation_id: String,
        pub account: String,
        pub fingerprint: [u8; 32],
        pub prepared: PreparedV1,
        pub owner_member: String,
        pub recovery_handle: String,
        pub key_fingerprint: Option<String>,
        pub linked: bool,
    }

    /// Which of a plan's two catalog-unique values another create already holds.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub enum Taken {
        Name,
        Slug,
    }
}

/// Engine-neutral REC-DEL-1 recovery ledger types.
pub mod recovery {
    use org_office_launch::recovery::Request;
    use serde::{Deserialize, Serialize};

    #[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
    #[serde(rename_all = "snake_case")]
    pub enum State {
        Pending,
        Delivered,
        AckPending,
        Acknowledged,
    }

    impl From<State> for org_host_control::recovery::DeliveryState {
        fn from(s: State) -> Self {
            match s {
                State::Pending => Self::Pending,
                State::Delivered => Self::Delivered,
                State::AckPending => Self::AckPending,
                State::Acknowledged => Self::Acknowledged,
            }
        }
    }

    #[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
    #[serde(deny_unknown_fields)]
    pub struct Ledger {
        pub version: u32,
        pub binding: Request,
        pub public_key: String,
        pub package_sha256: String,
        pub state: State,
        pub ordinal: i64,
        pub first_shown_ms: Option<i64>,
        pub last_redelivered_ms: Option<i64>,
        pub redelivery_count: i64,
        pub acknowledged_ms: Option<i64>,
        pub acknowledged_by: Option<String>,
    }
}

/// Engine-neutral browser-session schema refusal text.
pub(crate) mod sessions {
    /// What the operator is told when `ensure_schema` refuses.
    pub(crate) const OUTDATED: &str =
        "host database from an earlier development build; run `agentic-org setup` on a new instance";
}

/// Engine-neutral web TLS persistence types.
pub mod web_tls {
    use org_host_control::ops::TlsRefusal;
    use org_host_control::HostOpResult;

    /// Who made the stored pair: the instance itself, or the operator (setup's configuration, or
    /// `tls.set`).
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub enum Source {
        Made,
        Supplied,
    }

    impl Source {
        pub fn as_str(self) -> &'static str {
            match self {
                Source::Made => "made",
                Source::Supplied => "supplied",
            }
        }

        pub fn parse(text: &str) -> Option<Self> {
            match text {
                "made" => Some(Source::Made),
                "supplied" => Some(Source::Supplied),
                _ => None,
            }
        }
    }

    /// The stored pair, as the registry holds it.
    #[derive(Clone, Debug, PartialEq, Eq)]
    pub struct StoredPair {
        pub source: Source,
        /// The chain, in the normal form `web_tls::check` returns.
        pub cert_pem: String,
        /// Names the key in the host's secrets (`tls.<hex>`).
        pub pair_fp: [u8; 32],
        pub set_at_ms: i64,
    }

    /// `tls.set`'s journaled result: `completed` (`{}`), or `rejected` with the one refusal that
    /// is stored, `tls_key_mismatch`.
    pub type TlsStored = HostOpResult<serde_json::Value, TlsRefusal>;

    /// A `tls.set` journal lookup.
    #[derive(Debug, PartialEq)]
    pub enum TlsLookup {
        Vacant,
        Replay(TlsStored),
    }
}

/// Engine-neutral host-registry persistence contract (ADR-0018).
///
/// Consuming host subsystems depend on this trait (or its focused sub-contracts
/// [`AccountStore`], [`OneTimeLinks`], [`BrowserSessions`]) rather than on SQLite internals.
pub trait HostStore: AccountStore + OneTimeLinks + BrowserSessions + Send + Sync {
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

    // Operation journal
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

    // Login lockout
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

    // Provisioning
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

    // Recovery
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

    // Web TLS
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
