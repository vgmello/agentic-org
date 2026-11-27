//! Consumer-owned public port contracts for `org-host` (ADR-0018, `TASK-231`, `TASK-256`).
//!
//! All live host subsystems depend on these engine-neutral traits rather than on SQLite or
//! platform-specific credential stores:
//! - [`HostStore`] — composite host-registry persistence contract (supertrait of
//!   [`AccountStore`], [`OneTimeLinks`], [`BrowserSessions`], [`HostAccountsStore`],
//!   [`OperationJournalStore`], [`LoginLockoutStore`], [`ProvisioningStore`],
//!   [`RecoveryStore`], and [`WebTlsStore`]),
//! - [`HostPorts`] — optional injected ports accepted by [`crate::Host::start_with_ports`],
//! - [`AccountStore`] — browser administrator registration, credential lookup, and sign-in
//!   throttling,
//! - [`BrowserSessions`] — digest-only browser session persistence and idle expiry,
//! - [`OneTimeLinks`] — single-use, instance-bound link digest persistence,
//! - [`HostSecrets`] — host registry, TLS, writer, and recovery secret custody, and
//! - [`OfficeSecretsKeys`] — add-only per-office secrets key custody.
//!
//! Each domain submodule re-exports every public record, input, error, and shared rule type
//! required to implement and drive the corresponding trait from an external crate.

use std::fmt;
use std::io;
use std::path::Path;
use std::sync::Arc;

pub use crate::accounts::store::AccountStore;
pub use crate::host_secrets::HostSecrets;
pub use crate::hostdb::{
    HostAccountsStore, HostStore, LoginLockoutStore, OperationJournalStore, ProvisioningStore,
    RecoveryStore, WebTlsStore,
};
pub use crate::keys::OfficeSecretsKeys;
pub use crate::links::OneTimeLinks;
pub use crate::sessions::BrowserSessions;

/// Consumer-owned factory port for checking, creating, and opening a local host-registry
/// store under `host.lock` and the instance root lock (ADR-0018).
///
/// Storage backend crates (such as `org-host-sqlite`) implement this trait so `org-host`
/// verifies registry existence, creates a first-start registry, and opens an existing
/// registry without depending on any concrete database engine.
pub trait HostStoreOpener: Send + Sync {
    /// Whether a persisted registry already exists in `office_home`.
    fn exists(&self, office_home: &Path) -> Result<bool, io::Error>;

    /// Creates a brand-new host registry at `office_home` for `init.instance_id` and returns
    /// the opened [`HostStore`].
    fn create(
        &self,
        office_home: &Path,
        custody: &dyn HostSecrets,
        init: &host_store::HostDbInit,
    ) -> Result<Arc<dyn HostStore>, host_store::Error>;

    /// Opens an existing host registry at `office_home`.
    fn open(
        &self,
        office_home: &Path,
        custody: &dyn HostSecrets,
    ) -> Result<Arc<dyn HostStore>, host_store::Error>;
}

/// Optional caller-injected persistence and custody ports for [`crate::Host::start_with_ports`]
/// and [`crate::Host::start_registered_with_ports`].
///
/// Registered starts must supply either a pre-opened `store` or a `store_opener` (for example
/// `org-host-sqlite`'s SQLite opener). Any custody field left `None` falls back to the local
/// host secrets or per-office secrets key store.
#[derive(Default)]
pub struct HostPorts {
    pub store: Option<Arc<dyn HostStore>>,
    pub store_opener: Option<Arc<dyn HostStoreOpener>>,
    pub secrets: Option<Arc<dyn HostSecrets>>,
    pub keys: Option<Arc<dyn OfficeSecretsKeys>>,
}

impl fmt::Debug for HostPorts {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("HostPorts")
            .field("store", &self.store.is_some())
            .field("store_opener", &self.store_opener.is_some())
            .field("secrets", &self.secrets.is_some())
            .field("keys", &self.keys.is_some())
            .finish()
    }
}

/// Public types and shared rules for [`HostStore`] and its focused domain sub-traits.
pub mod host_store {
    pub use crate::hostdb::pending_recovery::pending_recovery;
    pub use crate::hostdb::sessions::OUTDATED as SESSIONS_OUTDATED;
    pub use crate::hostdb::{
        accounts, journal, lockout, multi_person, provisioning, recovery, web_tls, Error,
        HostAccountsStore, HostDbInit, HostStore, LoginLockoutStore, OperationJournalStore,
        ProvisioningStore, RecoveryStore, StoreFault, WebTlsStore, LOCAL_AUTHLESS_VERIFIER,
    };
    pub use org_host_control::browser_accounts::OrgAccessEntry;
    pub use org_host_control::journal::Journaled;
    pub use org_host_control::ops::TlsRefusal;
    pub use org_host_control::recovery::{
        Audit as RecoveryAudit, DeliveryState as RecoveryDeliveryState,
        StatusResult as RecoveryStatusResult,
    };
    pub use org_host_control::HostOpResult;
    pub use org_office_launch::recovery::{PublicRecord, Request as RecoveryBinding};
    pub use org_proto::selected_bootstrap::{CreatorV1, PlanV1, PreparedV1};
    pub use org_proto::{ApiError, ErrorCode};
}

/// Public types, checked identities, and shared throttle rules for [`AccountStore`].
pub mod accounts {
    pub use crate::accounts::identity::{AccountId, InvalidIdentity, LoginKey};
    pub use crate::accounts::rules::{
        check_new_password, PasswordRule, Username, UsernameRule, PASSWORD_MAX_BYTES,
        PASSWORD_MIN_SCALARS, USERNAME_MAX_SCALARS,
    };
    pub use crate::accounts::store::{
        backoff_ms, AccountStore, Attempt, ClientAddress, Credentials, NewAccount, Registration,
        StoreError, Throttle, ThrottleKey, ADDRESS_DELAY_CAP_MS, LOGIN_DELAY_CAP_MS,
    };
    pub use crate::accounts::verifier::{InvalidVerifier, Verifier};
}

/// Public types and shared lifetime rules for [`BrowserSessions`].
pub mod sessions {
    pub use crate::sessions::{
        is_live, Admission, BrowserSessions, Ended, InvalidOrigin, NewSession, Origin, Scheme,
        SessionDigest, SessionRecord, StoreError, IDLE_MS, SESSION_LIMIT,
    };
}

/// Public types, digests, and shared redemption rules for [`OneTimeLinks`].
pub mod links {
    pub use crate::links::{
        issue_with, redeem_rule, Error, Issued, LinkDigest, LinkKind, NewLink, OneTimeLinks,
        Pending, Refused, StoreFault, Subject, Token, REGISTRATION_TTL_MS,
    };
}

/// Public types and errors for [`HostSecrets`].
pub mod host_secrets {
    pub use crate::host_secrets::{Error, HostSecrets, Name, Value, MAX_BYTES};
}

/// Public types and errors for [`OfficeSecretsKeys`].
pub mod keys {
    pub use crate::keys::{KeyStoreError, OfficeSecretsKeys};
    pub use org_office_launch::{is_plain_id, is_storage_id};
    pub use org_proto::OfficeSecretsKey;
}
