//! Engine-neutral host-registry persistence contract (ADR-0018).
//!
//! [`HostStore`] is the host's storage port; [`Error`] and [`StoreFault`] carry no driver
//! types. The SQLite adapter lives under [`sqlite`] (the only module in `org-host` that
//! imports `rusqlite` or holds SQL), [`memory`] provides a second in-memory adapter in
//! tests, and `contract_tests` runs one conformance suite against both.

pub(crate) mod sqlite;

#[cfg(test)]
mod contract_tests;
#[cfg(test)]
mod testing;
#[cfg(test)]
pub(crate) use testing::memory;

use std::collections::BTreeSet;
use std::io;

use org_office_launch::recovery::PublicRecord;

use crate::accounts::identity::{AccountId, LoginKey};
use crate::accounts::store::AccountStore;
use crate::host_secrets::HostSecrets;
use crate::links::{Issued, LinkKind, OneTimeLinks, Subject};
use crate::sessions::BrowserSessions;

pub(crate) use sqlite::HostDb;

/// Engine-neutral class of storage failure (ADR-0018).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum StoreFault {
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
    pub(crate) fn is_transient(self) -> bool {
        matches!(self, Self::Busy | Self::TransientIo | Self::OutOfMemory)
    }
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Error {
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
pub(crate) mod accounts {
    use super::journal::Verifier;
    use crate::accounts::identity::AccountId;

    #[derive(Clone, Debug, PartialEq, Eq)]
    pub(crate) struct Link {
        pub(in crate::hostdb) org_id: String,
        pub(in crate::hostdb) member_id: String,
    }

    /// Stored authority, including revoked accounts; office IDs come from the catalog later.
    #[derive(Clone, Debug, PartialEq, Eq)]
    pub(crate) struct Account {
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
    pub(crate) struct Credentials {
        pub(in crate::hostdb) account: Account,
        pub(in crate::hostdb) verifier: Verifier,
    }

    impl Link {
        pub(crate) fn org_id(&self) -> &str {
            &self.org_id
        }
        pub(crate) fn member_id(&self) -> &str {
            &self.member_id
        }
    }

    impl Account {
        pub(crate) fn account_id(&self) -> &str {
            self.account_id.as_str()
        }
        pub(crate) fn login(&self) -> &str {
            &self.login
        }
        pub(crate) fn admin(&self) -> bool {
            self.admin
        }
        pub(crate) fn revoked_at_ms(&self) -> Option<i64> {
            self.revoked_at_ms
        }
        pub(crate) fn password_changed_at_ms(&self) -> i64 {
            self.password_changed_at_ms
        }
        pub(crate) fn links(&self) -> &[Link] {
            &self.links
        }
        pub(crate) fn time_zone(&self) -> Option<&str> {
            self.time_zone.as_deref()
        }
        pub(crate) fn last_org_id(&self) -> Option<&str> {
            self.last_org_id.as_deref()
        }
    }

    impl Credentials {
        pub(crate) fn account(&self) -> &Account {
            &self.account
        }
        /// Borrow the checked verifier; password hashing belongs outside the registry lock.
        pub(crate) fn verifier(&self) -> &Verifier {
            &self.verifier
        }
    }
}

/// Engine-neutral operation journal types.
pub(crate) mod journal {
    use super::Error;
    use org_host_control::{journal::Journaled, HostOpResult};

    pub(in crate::hostdb) const DAY: i64 = 86_400_000;
    pub(crate) type Stored = HostOpResult<serde_json::Value>;

    pub(crate) struct Attempt<'a> {
        pub(crate) id: &'a str,
        pub(crate) kind: &'static str,
        pub(crate) fingerprint: [u8; 32],
        pub(crate) account: &'a str,
    }

    impl<'a> Attempt<'a> {
        pub(crate) fn new<J: Journaled>(op: &'a J, account: &'a str) -> Result<Self, Error> {
            Ok(Self {
                id: op.operation_id(),
                kind: J::METHOD,
                fingerprint: org_host_control::journal::fingerprint(op)
                    .map_err(|_| Error::Unsafe)?,
                account,
            })
        }
    }

    pub(crate) use crate::accounts::verifier::Verifier;

    #[cfg_attr(
        not(test),
        allow(
            dead_code,
            reason = "hostdb idempotency contract consumed by upcoming host wiring and tests"
        )
    )]
    pub(crate) enum Lookup {
        Vacant,
        Replay(Stored),
        NeedsVerification { verifier: Verifier, stored: Stored },
    }

    #[cfg_attr(
        not(test),
        allow(
            dead_code,
            reason = "hostdb idempotency contract consumed by upcoming host wiring and tests"
        )
    )]
    pub(crate) struct Effect {
        pub(crate) stored: Stored,
        /// Created account, not the acting account. Required only for a successful account.create.
        pub(crate) verifier: Option<(String, Verifier)>,
    }

    #[cfg_attr(
        not(test),
        allow(
            dead_code,
            reason = "hostdb idempotency contract consumed by upcoming host wiring and tests"
        )
    )]
    pub(crate) enum Write {
        Committed(Stored),
        Existing(Lookup),
    }

    pub(in crate::hostdb) fn id_time(id: &str) -> Result<i64, Error> {
        org_proto::uuid_v7_time_ms(id)
            .and_then(|n| i64::try_from(n).ok())
            .ok_or(Error::InvalidId)
    }
}

/// Engine-neutral login lockout types.
pub(crate) mod lockout {
    #[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
    pub(crate) enum Socket {
        Control,
        Local,
    }

    impl Socket {
        pub(in crate::hostdb) fn as_str(self) -> &'static str {
            match self {
                Self::Control => "control",
                Self::Local => "local",
            }
        }
    }

    #[derive(Clone, Copy, PartialEq, Eq)]
    pub(crate) enum Outcome {
        Failure,
        Success,
        /// Only an already authorized password/reset operation may request this.
        Reset,
    }

    #[derive(Debug, PartialEq, Eq)]
    pub(crate) struct State {
        pub(in crate::hostdb) now_ms: i64,
        pub(in crate::hostdb) locked: bool,
        pub(in crate::hostdb) until_ms: i64,
    }

    impl State {
        /// How long until the key accepts another attempt; zero when unlocked.
        pub(crate) fn retry_after_ms(&self) -> i64 {
            self.until_ms.saturating_sub(self.now_ms).max(0)
        }
        pub(crate) fn now_ms(&self) -> i64 {
            self.now_ms
        }
        pub(crate) fn is_locked(&self) -> bool {
            self.locked
        }
    }
}

/// Engine-neutral selected HQ provisioning types.
pub(crate) mod provisioning {
    use org_proto::selected_bootstrap::PreparedV1;
    use serde::{Deserialize, Serialize};

    #[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
    #[serde(deny_unknown_fields)]
    pub(crate) struct Reservation {
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
    pub(crate) enum Taken {
        Name,
        Slug,
    }
}

/// Engine-neutral REC-DEL-1 recovery ledger types.
pub(crate) mod recovery {
    use org_office_launch::recovery::Request;
    use serde::{Deserialize, Serialize};

    #[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
    #[serde(rename_all = "snake_case")]
    pub(crate) enum State {
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
    pub(crate) struct Ledger {
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
pub(crate) mod web_tls {
    use org_host_control::ops::TlsRefusal;
    use org_host_control::HostOpResult;

    /// Who made the stored pair: the instance itself, or the operator (setup's configuration, or
    /// `tls.set`).
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub(crate) enum Source {
        Made,
        Supplied,
    }

    impl Source {
        pub(crate) fn as_str(self) -> &'static str {
            match self {
                Source::Made => "made",
                Source::Supplied => "supplied",
            }
        }

        pub(crate) fn parse(text: &str) -> Option<Self> {
            match text {
                "made" => Some(Source::Made),
                "supplied" => Some(Source::Supplied),
                _ => None,
            }
        }
    }

    /// The stored pair, as the registry holds it.
    #[derive(Clone, Debug, PartialEq, Eq)]
    pub(crate) struct StoredPair {
        pub(crate) source: Source,
        /// The chain, in the normal form `web_tls::check` returns.
        pub(crate) cert_pem: String,
        /// Names the key in the host's secrets (`tls.<hex>`).
        pub(crate) pair_fp: [u8; 32],
        pub(crate) set_at_ms: i64,
    }

    /// `tls.set`'s journaled result: `completed` (`{}`), or `rejected` with the one refusal that
    /// is stored, `tls_key_mismatch`.
    pub(crate) type TlsStored = HostOpResult<serde_json::Value, TlsRefusal>;

    /// A `tls.set` journal lookup.
    #[derive(Debug, PartialEq)]
    pub(crate) enum TlsLookup {
        Vacant,
        Replay(TlsStored),
    }
}

/// Engine-neutral host-registry persistence contract (ADR-0018).
///
/// Consuming host subsystems depend on this trait (or its focused sub-contracts
/// [`AccountStore`], [`OneTimeLinks`], [`BrowserSessions`]) rather than on SQLite internals.
#[cfg_attr(
    not(test),
    allow(
        dead_code,
        reason = "HostStore trait methods are tested now and composed as host wiring lands"
    )
)]
pub(crate) trait HostStore:
    AccountStore + OneTimeLinks + BrowserSessions + Send + Sync
{
    // Instance & organization ownership
    fn verify_instance(&self, instance: &str) -> Result<(), Error>;
    fn establish_organization_owner(
        &self,
        administrator: &accounts::Account,
        org: &str,
        member: &str,
    ) -> Result<(), Error>;
    fn verify_selected_owner(&self, org: &str, account: &str, member: &str) -> Result<(), Error>;

    // Accounts & administrator
    fn has_administrator(&self) -> Result<bool, Error>;
    fn account_credentials(
        &self,
        login_key: &LoginKey,
    ) -> Result<Option<accounts::Credentials>, Error>;
    fn account_authority(&self, account_id: &AccountId)
        -> Result<Option<accounts::Account>, Error>;

    // One-time links with administrator guard
    fn issue_unless_administrator(
        &self,
        kind: LinkKind,
        subject: &Subject,
        now_ms: i64,
    ) -> Result<Option<Issued>, crate::links::Error>;

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
