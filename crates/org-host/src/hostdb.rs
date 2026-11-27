//! Engine-neutral host-registry persistence contract (ADR-0018).
//!
//! [`HostStore`] is the host's storage port; [`Error`] and [`StoreFault`] carry no driver
//! types. The SQLite adapter lives in the separate `org-host-sqlite` crate, [`memory`]
//! provides an in-memory adapter in tests, and `contract_tests` runs the conformance suite
//! against it (with `org-host-sqlite` running the same conformance contract against SQLite).

pub mod multi_person;
pub(crate) mod pending_recovery;
mod traits;

#[cfg(test)]
mod contract_tests;
#[cfg(test)]
pub(crate) mod testing;
#[cfg(test)]
pub(crate) use testing::memory;

use std::io;

pub use traits::{
    HostAccountsStore, HostStore, LoginLockoutStore, OperationJournalStore, ProvisioningStore,
    RecoveryStore, WebTlsStore,
};

/// Sentinel password PHC stored for the implicit local-authless placeholder account (ADR-0034).
/// It is deliberately not a valid Argon2id PHC and can never satisfy password verification.
pub const LOCAL_AUTHLESS_VERIFIER: &str = "!local-authless";

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
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HostDbInit {
    pub instance_id: String,
    pub clock_high_ms: i64,
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
        pub fn id(&self) -> &AccountId {
            &self.account_id
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
        pub fn into_parts(self) -> (Account, Verifier) {
            (self.account, self.verifier)
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

    /// Summary of one startup [`OperationJournalStore::journal_prune_expired`](super::OperationJournalStore::journal_prune_expired) pass.
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
    pub const OUTDATED: &str =
        "host database from an earlier development build; run `agentic-org setup` on a new instance";
}

/// Engine-neutral web TLS persistence types.
pub mod web_tls {
    use org_host_control::ops::TlsRefusal;
    use org_host_control::HostOpResult;

    pub use org_host_tls::Source;

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
