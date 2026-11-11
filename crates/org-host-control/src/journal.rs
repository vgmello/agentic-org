//! The host's op journal (X-02 §6.3): what makes two requests "the same op".
//!
//! Every journaled op names an `operation_id` (a ULID, with the org-command age and skew rules)
//! and keeps its result for 30 days. Its fingerprint is SHA-256 over the method and the params
//! **without `session` and `operation_id`**, so a retry after a new sign-in is the same op. The
//! principal is compared by account:
//! - the same `operation_id`, fingerprint and account (and for `account.create`, a password that
//!   matches the row's verifier): a replay of the stored result;
//! - the same `operation_id` with another fingerprint, another account or (`account.create`) a
//!   password that doesn't match the verifier: `operation_reused`.
//!
//! **The journal lookup comes first** for every journaled op, `tls.set` included: a known
//! `operation_id` is looked up before any of the op's checks, a stored result replays (a stored
//! `tls.set` success replays even if setup has since rewritten the origin), and only an unknown
//! or unstored one runs the checks.
//!
//! This crate defines the bytes the host hashes ([`fingerprint_input`]); the host computes the
//! SHA-256.
//!
//! **A secret is never part of a fingerprint.** The journal keeps fingerprints for 30 days, and
//! an unsalted hash of a password would undo its Argon2id hash. So `account.create`'s fields leave
//! out `password`, and `tls.set`'s fields are hashes of the certificate and the key, which the
//! host computes (X-02 §6.2).
//!
//! **Passwords on a replay** (X-02 §6.3):
//! - `account.create`'s journal row also keeps a **create-time verifier** of the password it
//!   carried: a copy of the Argon2id PHC string the create produced for the account (the same
//!   salt and parameters), never the password, a fast hash or a separate key. It is as strong as
//!   the account's own hash and survives a host restart with the row. A replay verifies its
//!   password against it through the same Argon2 pool, queue limit and rate limit as
//!   `account.verify`, never against the account's current hash, so it reveals nothing the
//!   replaying account didn't send. A mismatch is `operation_reused`: a retry with another
//!   password is **not** the same op, though its fingerprint is the same.
//! - `account.reset_password`, `account.set_password` and an account's revocation **delete** the
//!   create-time verifiers of the account they act on, found by the created account in each
//!   `account.create` row's result (`{ account }`), not by the row's `account` column (the
//!   creator). A replay of that create afterwards gets `operation_reused`, and no old hash
//!   outlives a reset. The host computes and stores the verifier (move 2b); this crate defines
//!   no type for it. A **refused** create (`already_exists`, `invalid_name`, stored) produced no
//!   hash and keeps no verifier: its refusal doesn't depend on the password, so a replay returns
//!   it whatever password it carries.
//! - A `bootstrap` replay checks the `login` and password as a sign-in instead, first: a `login`
//!   and password that don't sign in are `not_authorized`. Then its fingerprint (the normalized
//!   `login` and `hq_operation_id`, never the password or the TLS pair) and then the pair (see
//!   [`crate::ops::Bootstrap`]).
//!
//! **Logins are fingerprinted normalized** (X-02 §6.3; §6.2: NFC, case-folded), so `Admin` and
//! `admin` are the same op. This crate doesn't normalize and adds no dependency to do it: callers
//! pass the normalized `login` in the params they fingerprint, and the host normalizes before
//! fingerprinting (move 2b).

use org_proto::Call;
use serde::Serialize;

use crate::ops::{
    AccountCreate, AccountCreateParams, Bootstrap, BootstrapParams, OfficeRestart,
    OfficeRestartParams, OrganizationCreate, OrganizationCreateParams,
};

/// A journaled op's params. Each implementation destructures its params exhaustively, so adding
/// a param fails to compile until it is fingerprinted or explicitly excluded.
pub trait Journaled {
    /// The op's method.
    const METHOD: &'static str;
    /// The op's id.
    fn operation_id(&self) -> &str;
    /// The params that say what the op is, as compact JSON: everything except `session` and
    /// `operation_id`.
    fn fingerprint_fields(&self) -> Result<String, serde_json::Error>;
}

/// What the host hashes for an op's fingerprint: `method || 0x00 || fingerprint_fields()`, the
/// same form as an org command's fingerprint.
pub fn fingerprint_input<J: Journaled>(params: &J) -> Result<Vec<u8>, serde_json::Error> {
    let fields = params.fingerprint_fields()?;
    let mut out = Vec::with_capacity(J::METHOD.len() + 1 + fields.len());
    out.extend_from_slice(J::METHOD.as_bytes());
    out.push(0);
    out.extend_from_slice(fields.as_bytes());
    Ok(out)
}

impl Journaled for OrganizationCreateParams {
    const METHOD: &'static str = OrganizationCreate::METHOD;

    fn operation_id(&self) -> &str {
        &self.operation_id
    }

    fn fingerprint_fields(&self) -> Result<String, serde_json::Error> {
        #[derive(Serialize)]
        struct Fields<'a> {
            name: &'a str,
        }
        // Exhaustive, so a param added later must be fingerprinted or excluded here to compile.
        let OrganizationCreateParams {
            session: _,
            operation_id: _,
            name,
        } = self;
        serde_json::to_string(&Fields { name })
    }
}

impl Journaled for OfficeRestartParams {
    const METHOD: &'static str = OfficeRestart::METHOD;

    fn operation_id(&self) -> &str {
        &self.operation_id
    }

    fn fingerprint_fields(&self) -> Result<String, serde_json::Error> {
        #[derive(Serialize)]
        struct Fields<'a> {
            office_id: &'a str,
        }
        let OfficeRestartParams {
            session: _,
            operation_id: _,
            office_id,
        } = self;
        serde_json::to_string(&Fields { office_id })
    }
}

impl Journaled for AccountCreateParams {
    const METHOD: &'static str = AccountCreate::METHOD;

    fn operation_id(&self) -> &str {
        &self.operation_id
    }

    /// `login`, `admin` and `owner_of`. Never the password: the host checks a replay's password
    /// against the row's create-time verifier instead (module docs).
    ///
    /// `login` is used as given: callers pass it **normalized** (NFC, case-folded; X-02 §6.2,
    /// §6.3), because this crate doesn't normalize.
    fn fingerprint_fields(&self) -> Result<String, serde_json::Error> {
        #[derive(Serialize)]
        struct Fields<'a> {
            login: &'a str,
            admin: bool,
            owner_of: Option<&'a str>,
        }
        let AccountCreateParams {
            session: _,
            operation_id: _,
            password: _,
            login,
            admin,
            owner_of,
        } = self;
        serde_json::to_string(&Fields {
            login,
            admin: *admin,
            owner_of: owner_of.as_deref(),
        })
    }
}

impl Journaled for BootstrapParams {
    const METHOD: &'static str = Bootstrap::METHOD;

    fn operation_id(&self) -> &str {
        &self.operation_id
    }

    /// `login` and `hq_operation_id` (X-02 §6.3). Never the password, which a replay checks as
    /// a sign-in first, and never the TLS pair, which a replay compares after the fingerprint
    /// (`tls_mismatch`): so another `hq_operation_id` is `operation_reused`, and another pair
    /// passes the fingerprint and gets `tls_mismatch` with its guidance.
    ///
    /// `login` is used as given: callers pass it **normalized** (NFC, case-folded; X-02 §6.2,
    /// §6.3), because this crate doesn't normalize.
    fn fingerprint_fields(&self) -> Result<String, serde_json::Error> {
        #[derive(Serialize)]
        struct Fields<'a> {
            login: &'a str,
            hq_operation_id: &'a str,
        }
        let BootstrapParams {
            operation_id: _,
            password: _,
            tls: _,
            login,
            hq_operation_id,
        } = self;
        serde_json::to_string(&Fields {
            login,
            hq_operation_id,
        })
    }
}
