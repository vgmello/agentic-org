//! The host's op journal (X-02 §6.3): what makes two requests "the same op".
//!
//! Every journaled op names an `operation_id` (a ULID, with the org-command age and skew rules)
//! and keeps its result for 30 days. Its fingerprint is SHA-256 over the method and the params
//! **without `session` and `operation_id`**, so a retry after a new sign-in is the same op. The
//! principal is compared by account:
//! - the same `operation_id`, fingerprint and account: a replay of the stored result;
//! - the same `operation_id` with another fingerprint or another account: `operation_reused`.
//!
//! This crate defines the bytes the host hashes ([`fingerprint_input`]); the host computes the
//! SHA-256.
//!
//! **A secret is never part of a fingerprint.** The journal keeps fingerprints for 30 days, and
//! an unsalted hash of a password would undo its Argon2id hash. So `account.create`'s fields leave
//! out `password` (a retry with another password is the same op), and `tls.set`'s fields are
//! hashes of the certificate and the key, which the host computes (X-02 §6.2).

use org_proto::Call;
use serde::Serialize;

use crate::ops::{
    AccountCreate, AccountCreateParams, OfficeRestart, OfficeRestartParams, OrganizationCreate,
    OrganizationCreateParams,
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

    /// `login`, `admin` and `owner_of`. Never the password.
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
