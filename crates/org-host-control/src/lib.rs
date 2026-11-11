//! # org-host-control: the host's private contract
//!
//! The contract between `org-office-host` and the two parties that talk to it directly
//! (X-02 §6, D9):
//! - **`org-gateway`**, over its **control connection**: the rendezvous connection the host
//!   accepted after checking the gateway's image (§5.1). The gateway signs people in, gets grants
//!   for the channels it opens, fetches its TLS pair and follows the host's events.
//! - **`agentic-org`** (setup and the local CLI), over **`host.sock`**, which speaks only this
//!   contract from the migration's move 2b.
//!
//! It is a two-party contract with its own version space, independent of `org-proto`'s
//! (ADR-0001): [`API_VERSION`] starts at 1 and `hello` negotiates [`SUPPORTED_API`] with
//! `org-proto`'s [`HelloParams`], answering with `instance_id`. The envelope is `org-proto`'s:
//! one JSON object per line, [`org_proto::Request`] and [`org_proto::Response`], each method a
//! [`Call`].
//!
//! **Authority.** Host ops identify the actor by a [`SessionHandle`] from `account.verify`, never
//! by an account id the client sends. The host owns the account registry and the sessions; it
//! holds no organization data and parses no HTTP.
//!
//! **Secrets** (§6.5) decode straight into secret newtypes ([`Password`], [`SessionHandle`],
//! [`TlsPrivateKey`], [`RecoveryCode`], and `org-proto`'s grant MAC): a redacted `Debug`, no
//! `Clone`, zeroed on drop. Responses carrying them are written through the host's secret-line
//! path. Until they decode, a request's secrets sit in `org-proto`'s envelope as raw JSON: its
//! `params` until `Request::params` decodes them, and (the exception §6.5 records) a grant MAC in
//! its `principal` until `Request::principal` does. That JSON is never logged: the envelope's
//! `Debug` elides both.
//!
//! **The listen configuration** ([`listen`], X-02 rev 14): the browser-facing origin
//! ([`listen::ListenOrigin`], the one implementation that parses, checks and serialises it), the
//! `hsts` setting, the per-bind snapshot, [`listen::ListenFailure`], and `tls.fetch`'s and
//! `tls.set`'s use of them.
//!
//! This crate holds types and pure functions only. Not here yet, because X-02 leaves their params
//! or shapes open: `account.link`, `account.unlink` and `account.revoke`, the instance status
//! read for `status` and `doctor` (with `gateway_failed`'s `start_failed` and `tls_missing`), the
//! event payloads, and the host-only results and refusals (`needs_local_session`).
//! Transfer-socket records are `org-office-launch`'s v3 `TransferRecord`, defined once there.
//! This crate doesn't re-export it, so that its only dependencies stay `org-proto` and serde
//! (X-02 §5.2, §11): the gateway imports it from `org-office-launch`.

#![deny(unsafe_code)]

use serde::{Deserialize, Serialize};

pub mod gateway;
pub mod journal;
pub mod listen;
pub mod ops;
mod secret;

pub use org_proto::{ApiRange, Call, Empty, Hello, HelloParams, HelloResult};
pub use secret::{Password, RecoveryCode, SessionHandle, TlsPrivateKey};

/// This contract's version. Separate from `org_proto::API_VERSION`.
pub const API_VERSION: u32 = 1;

/// The versions of this contract the host speaks, for `hello` on the control connection and on
/// `host.sock`.
pub const SUPPORTED_API: ApiRange = ApiRange { min: 1, max: 1 };

/// Names of the events the host sends on a control connection after `subscribe` (§6.1). Their
/// data shapes come with move 2b; `organization.created` and `office.status_changed` keep
/// `org-proto`'s ([`org_proto::OrganizationCreated`], [`org_proto::OfficeStatusChanged`]).
pub mod event {
    pub const ORGANIZATION_CREATED: &str = "organization.created";
    pub const ORGANIZATION_CHANGED: &str = "organization.changed";
    pub const OFFICE_STATUS_CHANGED: &str = "office.status_changed";
    pub const ACCOUNT_CHANGED: &str = "account.changed";
    /// The operator renewed the TLS pair: the gateway calls `tls.fetch` and swaps it.
    pub const TLS_RENEW: &str = "tls.renew";
}

/// The result of a journaled host op (§6.3): `accepted` (still in flight, e.g. a create waiting
/// for its office), `completed` with the op's result, or `rejected` with a refusal. A domain
/// refusal (`already_exists`, `invalid_name`) is an [`org_proto::ApiError`] and is stored;
/// `tls.set`'s refusals are [`ops::TlsRefusal`]s, of which only `tls_key_mismatch` is stored.
/// `not_authorized`, `unavailable` and `storage_error` are never stored: they're plain error
/// responses.
///
/// A status this build doesn't know decodes as [`HostOpResult::Unknown`]. It is never sent:
/// serializing it fails.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
#[non_exhaustive]
pub enum HostOpResult<T, R = org_proto::ApiError> {
    Accepted,
    Completed {
        result: T,
    },
    Rejected {
        refusal: R,
    },
    #[serde(other, skip_serializing)]
    Unknown,
}

#[cfg(test)]
mod tests;
