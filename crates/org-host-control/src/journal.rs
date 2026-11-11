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
//! or unstored one runs the checks. **A `tls.set` whose fingerprint can't be computed** (its
//! [`Journaled::fingerprint_fields`] fails: the PEM reader refuses the pair) is answered at the
//! lookup with `tls_invalid`, unstored, whether or not its `operation_id` is known.
//!
//! This crate defines the bytes the host hashes ([`fingerprint_input`]); the host computes the
//! SHA-256. `tls.set`'s fields are themselves hashes, which this crate computes
//! ([`crate::ops::TlsPairFingerprint`]).
//!
//! **A secret is never part of a fingerprint.** The journal keeps fingerprints for 30 days, and
//! an unsalted hash of a password would undo its Argon2id hash. So `account.create`'s fields leave
//! out `password`, and `tls.set`'s fields are SHA-256 hashes of the certificate's and the key's
//! DER, computed from the secret type by [`crate::ops::TlsPairFingerprint`] (X-02 §6.2).
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
//!   `login` and `hq_operation_id`, never the password or the TLS pair) and then the pair, by
//!   [`crate::ops::TlsPair::fingerprint`] (see [`crate::ops::Bootstrap`]).
//!
//! **Logins are fingerprinted normalized** (X-02 §6.3; §6.2: NFC, case-folded), so `Admin` and
//! `admin` are the same op. This crate doesn't normalize and adds no dependency to do it: callers
//! pass the normalized `login` in the params they fingerprint, and the host normalizes before
//! fingerprinting (move 2b).

use org_proto::Call;
use serde::Serialize;

use crate::ops::{
    AccountCreate, AccountCreateParams, Bootstrap, BootstrapParams, OfficeRestart,
    OfficeRestartParams, OrganizationCreate, OrganizationCreateParams, TlsPairFingerprint, TlsSet,
    TlsSetParams,
};

/// A journaled op's params. Each implementation destructures its params exhaustively, so adding
/// a param fails to compile until it is fingerprinted or explicitly excluded.
pub trait Journaled {
    /// The op's method.
    const METHOD: &'static str;
    /// The op's id.
    fn operation_id(&self) -> &str;
    /// The params that say what the op is, as compact JSON: everything except `session` and
    /// `operation_id`. An error means the op has no fingerprint, so the journal lookup can't run:
    /// for `tls.set`, a pair the PEM reader refuses, answered `tls_invalid`, unstored.
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
    /// a sign-in first, and never the TLS pair, which a replay compares after the fingerprint by
    /// [`crate::ops::TlsPair::fingerprint`] (`tls_mismatch`; a pair the PEM reader refuses has no
    /// fingerprint and is `tls_invalid`, unstored): so another `hq_operation_id` is
    /// `operation_reused`, and another pair passes the fingerprint and gets `tls_mismatch` with
    /// its guidance.
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

impl Journaled for TlsSetParams {
    const METHOD: &'static str = TlsSet::METHOD;

    fn operation_id(&self) -> &str {
        &self.operation_id
    }

    /// The pair's [`TlsPairFingerprint`], serialized: `{"cert_chain_sha256", "key_der_sha256"}`
    /// (X-02 §6.2), the SHA-256, as lowercase hex, of `cert`'s whole chain (every `CERTIFICATE`
    /// block's DER, each with a u32 big-endian length prefix, in order) and of the DER of `key`'s
    /// first `PRIVATE KEY`, `RSA PRIVATE KEY` or `EC PRIVATE KEY` block.
    /// Never the pair itself, so the journal holds no key and the stored result echoes neither.
    /// It is the same implementation as [`crate::ops::TlsPair::fingerprint`], so a pair
    /// fingerprints the same in `tls.set` and `bootstrap`.
    ///
    /// **An error means the pair has no fingerprint** (serializing the two hashes can't fail): a
    /// block that's missing, or one the PEM reader ([`crate::pem`]) refuses. It never quotes the
    /// pair. The host needs the fingerprint for the journal lookup, so it answers such a `tls.set`
    /// there with `tls_invalid` ([`crate::ops::TlsRefusal::TlsInvalid`]), unstored, before any
    /// other check and whether or not the `operation_id` is known: never a replay or
    /// `operation_reused`.
    fn fingerprint_fields(&self) -> Result<String, serde_json::Error> {
        let TlsSetParams {
            session: _,
            operation_id: _,
            cert,
            key,
        } = self;
        let fingerprint = TlsPairFingerprint::of(cert, key).map_err(|_| {
            <serde_json::Error as serde::ser::Error>::custom(
                "tls.set's pair has no PEM block it can be fingerprinted by",
            )
        })?;
        serde_json::to_string(&fingerprint)
    }
}
