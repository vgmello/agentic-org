//! Consumer-owned public port contracts for `org-host` (ADR-0018, `TASK-231`).
//!
//! All live host subsystems depend on these engine-neutral traits rather than on SQLite or
//! platform-specific credential stores:
//! - [`HostStore`] — composite host-registry persistence contract (supertrait of
//!   [`AccountStore`], [`OneTimeLinks`], and [`BrowserSessions`]),
//! - [`AccountStore`] — browser administrator registration, credential lookup, and sign-in
//!   throttling,
//! - [`BrowserSessions`] — digest-only browser session persistence and idle expiry,
//! - [`OneTimeLinks`] — single-use, instance-bound link digest persistence,
//! - [`HostSecrets`] — host registry, TLS, writer, and recovery secret custody, and
//! - [`OfficeSecretsKeys`] — add-only per-office secrets key custody.
//!
//! Each domain submodule re-exports every public record, input, error, and shared rule type
//! required to implement and drive the corresponding trait from an external crate.

pub use crate::accounts::store::AccountStore;
pub use crate::host_secrets::HostSecrets;
pub use crate::hostdb::HostStore;
pub use crate::keys::OfficeSecretsKeys;
pub use crate::links::OneTimeLinks;
pub use crate::sessions::BrowserSessions;

/// Public types and shared rules for [`HostStore`].
pub mod host_store {
    pub use crate::hostdb::{
        accounts, journal, lockout, multi_person, provisioning, recovery, web_tls, Error,
        HostStore, StoreFault,
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
