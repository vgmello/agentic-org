//! The office-local family (`docs/design/office-store.md` §5, "Protected local records"):
//! what one office keeps under `durable/local/offices/<office-id>/`, never published to
//! Git, never copied to another office and never exported.
//!
//! It owns two files and no rows ([`records`]):
//!
//! - the HQ's CA key record, written in the organization's first journal entry beside
//!   the organization family's public escrow package (`state/security/ca-escrow.v1.bin`);
//!   the CA private key is in it only wrapped by the host-held office secrets key
//!   (ADR-0016), and so it is in the journal until that entry's segment is released;
//! - the clean-stop marker an orderly stop leaves for the next opening ([`clean_stop`]).
//!
//! Nothing here is on a hot path: the CA records are read at opening and by the recovery
//! read, through the files ([`read_ca`]).

pub mod clean_stop;
pub mod projector;
pub mod records;
#[cfg(any(test, feature = "testkit"))]
pub mod testkit;

use std::fmt;

use org_store::{DurablePath, Files, FilesError, Id, OfficeIdentity, Operation, Sha256};

use self::records::{CaKey, Malformed};
use crate::organization::records as organization;

/// The CA material a creation commits: generated and validated by the worker (`org-pki`),
/// frozen before the first attempt, and the same on every retry.
#[derive(Clone, PartialEq, Eq)]
pub struct CaMaterial {
    pub public_key: [u8; 65],
    /// The ADR-0007 escrow package: the bytes of `state/security/ca-escrow.v1.bin`.
    pub package: Vec<u8>,
    /// The CA private key as an `org_pki::wrap` record.
    pub wrapped_ca_key: Vec<u8>,
}

impl fmt::Debug for CaMaterial {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("CaMaterial(<redacted>)")
    }
}

impl CaMaterial {
    /// The CA key record this material makes for `here`'s creation operation
    /// `operation_id`.
    pub fn record(&self, here: &OfficeIdentity, operation_id: &Id) -> CaKey {
        CaKey {
            organization_id: here.organization_id.clone(),
            office_id: here.office_id.clone(),
            operation_id: operation_id.clone(),
            public_key: self.public_key,
            package_sha256: Sha256::of(&self.package),
            wrapped_ca_key: self.wrapped_ca_key.clone(),
        }
    }
}

impl CaKey {
    /// `operation`, with this record part of what a retry is matched on: a retry with
    /// regenerated material conflicts, one with the retained material gets the original
    /// reply. The record names the package by its hash, so the package is covered too.
    pub fn bind(&self, operation: Operation) -> Operation {
        let record = Sha256::of(&records::encode_ca_key(self));
        let parts: [&[u8]; 3] = [
            operation.fingerprint.as_bytes(),
            b"\0ca-key.v1\0",
            record.as_bytes(),
        ];
        Operation {
            fingerprint: Sha256::of_parts(&parts),
            ..operation
        }
    }
}

/// The CA records of an HQ, as its files hold them.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct CaRecords {
    pub key: CaKey,
    /// The escrow package.
    pub package: Vec<u8>,
}

/// Why the CA records cannot be used.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum CaError {
    /// One of the two files is there without the other: forward repair, never a reset.
    Missing(DurablePath),
    Malformed(DurablePath, Malformed),
    /// The records disagree with each other or with the organization they are read for.
    Mismatch(&'static str),
    Files(FilesError),
}

impl fmt::Display for CaError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Missing(path) => write!(f, "{path} is missing beside the other CA record"),
            Self::Malformed(path, why) => write!(f, "{path}: {why}"),
            Self::Mismatch(why) => write!(f, "the CA records do not match: {why}"),
            Self::Files(error) => error.fmt(f),
        }
    }
}

impl std::error::Error for CaError {}

impl From<FilesError> for CaError {
    fn from(error: FilesError) -> Self {
        Self::Files(error)
    }
}

impl CaRecords {
    /// The records are `office`'s, of `organization`, made by creation operation
    /// `operation`, and the key record names this escrow package and its public key.
    pub fn check(&self, organization: &Id, office: &Id, operation: &Id) -> Result<(), CaError> {
        let key = &self.key;
        if key.organization_id != *organization || key.office_id != *office {
            return Err(CaError::Mismatch("another organization's or office's"));
        }
        if key.operation_id != *operation {
            return Err(CaError::Mismatch("made by another operation"));
        }
        if Sha256::of(&self.package) != key.package_sha256 {
            return Err(CaError::Mismatch(
                "the escrow package is not the one recorded",
            ));
        }
        let public = organization::ca_escrow_public_key(&self.package, organization)
            .map_err(|_| CaError::Mismatch("the escrow package is not sealed for it"))?;
        if public != key.public_key {
            return Err(CaError::Mismatch("the public keys differ"));
        }
        Ok(())
    }
}

/// `office`'s CA records: `None` when neither file is there (a satellite, or an HQ before
/// its creation is materialized), an error when only one is or one is not in its form.
pub fn read_ca(files: &Files, office: &Id) -> Result<Option<CaRecords>, CaError> {
    let (key_path, package_path) = (records::ca_key_path(office), organization::ca_escrow_path());
    let key = files.read(&key_path)?;
    let package = files.read(&package_path)?;
    let (key, package) = match (key, package) {
        (None, None) => return Ok(None),
        (Some(key), Some(package)) => (key, package),
        (Some(_), None) => return Err(CaError::Missing(package_path)),
        (None, Some(_)) => return Err(CaError::Missing(key_path)),
    };
    let key = records::decode_ca_key(&key).map_err(|why| CaError::Malformed(key_path, why))?;
    Ok(Some(CaRecords {
        key,
        package: package.to_vec(),
    }))
}
