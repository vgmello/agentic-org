//! The journal fingerprints of the host ops in [`super`]: one [`Journaled`] implementation per
//! journaled op's params, beside the params they destructure. `journal` defines the contract and
//! the hashing; it imports no op.

use org_proto::Call;
use serde::Serialize;

use super::{
    OfficeJoin, OfficeJoinParams, OfficeReEnroll, OfficeReEnrollParams, OfficeRemovePending,
    OfficeRemovePendingParams, OfficeRestart, OfficeRestartParams, OrgCreate, OrgCreateParams,
    TlsPairFingerprint, TlsSet, TlsSetParams,
};
use crate::journal::Journaled;

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
        // Exhaustive, so a param added later must be fingerprinted or excluded here to compile.
        let OfficeRestartParams {
            session: _,
            operation_id: _,
            office_id,
        } = self;
        serde_json::to_string(&Fields { office_id })
    }
}

impl Journaled for OrgCreateParams {
    const METHOD: &'static str = OrgCreate::METHOD;

    fn operation_id(&self) -> &str {
        &self.operation_id
    }

    fn fingerprint_fields(&self) -> Result<String, serde_json::Error> {
        #[derive(Serialize)]
        struct Fields<'a> {
            plan: &'a org_proto::selected_bootstrap::PlanV1,
        }
        let OrgCreateParams {
            session: _,
            operation_id: _,
            plan,
        } = self;
        serde_json::to_string(&Fields { plan })
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

fn sha256_hex(input: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let digest = Sha256::digest(input);
    let mut out = String::with_capacity(64);
    for b in digest {
        use std::fmt::Write as _;
        let _ = write!(&mut out, "{b:02x}");
    }
    out
}

impl Journaled for OfficeJoinParams {
    const METHOD: &'static str = OfficeJoin::METHOD;

    fn operation_id(&self) -> &str {
        &self.operation_id
    }

    fn fingerprint_fields(&self) -> Result<String, serde_json::Error> {
        #[derive(Serialize)]
        struct Fields {
            invitation_sha256: String,
            consent: bool,
        }
        let OfficeJoinParams {
            session: _,
            operation_id: _,
            invitation,
            consent,
        } = self;
        serde_json::to_string(&Fields {
            invitation_sha256: sha256_hex(invitation.expose().as_bytes()),
            consent: *consent,
        })
    }
}

impl Journaled for OfficeRemovePendingParams {
    const METHOD: &'static str = OfficeRemovePending::METHOD;

    fn operation_id(&self) -> &str {
        &self.operation_id
    }

    fn fingerprint_fields(&self) -> Result<String, serde_json::Error> {
        #[derive(Serialize)]
        struct Fields<'a> {
            office_id: &'a str,
        }
        let OfficeRemovePendingParams {
            session: _,
            operation_id: _,
            office_id,
        } = self;
        serde_json::to_string(&Fields { office_id })
    }
}

impl Journaled for OfficeReEnrollParams {
    const METHOD: &'static str = OfficeReEnroll::METHOD;

    fn operation_id(&self) -> &str {
        &self.operation_id
    }

    fn fingerprint_fields(&self) -> Result<String, serde_json::Error> {
        #[derive(Serialize)]
        struct Fields<'a> {
            office_id: &'a str,
            invitation_sha256: String,
            consent: bool,
        }
        let OfficeReEnrollParams {
            session: _,
            operation_id: _,
            office_id,
            invitation,
            consent,
        } = self;
        serde_json::to_string(&Fields {
            office_id,
            invitation_sha256: sha256_hex(invitation.expose().as_bytes()),
            consent: *consent,
        })
    }
}
