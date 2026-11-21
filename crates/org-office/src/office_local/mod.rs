//! The office-local family (`docs/design/office-store.md` §5, "Protected local records"):
//! what one office keeps under `durable/local/offices/<office-id>/`, never published to
//! Git, never copied to another office and never exported.
//!
//! It owns local files written outside the acceptance journal ([`records`]):
//!
//! - the HQ's CA key record (`security/ca-key.v1.json`), written atomically to local
//!   security storage outside `durable/recovery/` segments (ADR-0016, TASK-10 / H-06),
//!   so wrapped CA key ciphertext never enters the acceptance journal, Git, or state
//!   exports; only the public escrow package (`state/security/ca-escrow.v1.bin`) is
//!   committed in Entry 1;
//! - the clean-stop marker an orderly stop leaves for the next opening ([`clean_stop`]).
//!
//! Nothing here is on a hot path: the CA records are read at opening and by the recovery
//! read, through the files ([`read_ca`]).

pub mod clean_stop;
pub mod projector;
pub mod records;
#[cfg(any(test, feature = "testkit"))]
pub mod testkit;

use std::{fmt, fs, io, path::Path};

use org_store::{DurablePath, Files, FilesError, Id, OfficeIdentity, Operation, Sha256};

use self::records::{CaKey, Malformed};
use crate::{
    organization::records as organization,
    private_fs::{private_dir, tighten_dir},
};

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
    /// `operation`, with this record's public CA identity part of what a retry is matched
    /// on: a retry with regenerated CA material conflicts, one with the retained material
    /// gets the original reply. Only public fields (`organization_id`, `office_id`,
    /// `operation_id`, `public_key`, `package_sha256`) enter the fingerprint so no wrapped
    /// CA key ciphertext is hashed into journal/receipt metadata (TASK-10 / H-06).
    pub fn bind(&self, operation: Operation) -> Operation {
        let parts: [&[u8]; 7] = [
            operation.fingerprint.as_bytes(),
            b"\0ca-public.v1\0",
            self.organization_id.as_str().as_bytes(),
            self.office_id.as_str().as_bytes(),
            self.operation_id.as_str().as_bytes(),
            &self.public_key,
            self.package_sha256.as_bytes(),
        ];
        Operation {
            fingerprint: Sha256::of_parts(&parts),
            ..operation
        }
    }
}

/// Atomically writes `record` to `durable/local/offices/<office>/security/<name>` with
/// private `0700` parent directories and `0600` file permissions, outside `durable/recovery/`.
fn write_security_file(
    durable: &Path,
    office: &Id,
    name: &str,
    bytes: &[u8],
    replace: bool,
) -> io::Result<()> {
    let local_dir = durable.join("local");
    let offices_dir = local_dir.join("offices");
    let office_dir = offices_dir.join(office.as_str());
    let security_dir = office_dir.join("security");
    for dir in [&local_dir, &offices_dir, &office_dir, &security_dir] {
        private_dir(dir)?;
        tighten_dir(dir)?;
    }
    org_store::publish_one(&security_dir, name, bytes, replace).map_err(io::Error::other)
}

/// Writes `ca-key.v1.json` directly into local security storage (outside `durable/recovery/`).
/// Refuses to overwrite an existing file with different bytes.
pub fn write_ca_key(durable: &Path, office: &Id, record: &CaKey) -> io::Result<()> {
    let encoded = records::encode_ca_key(record);
    let path = durable.join(records::ca_key_path(office).as_str());
    if let Ok(existing) = fs::read(&path) {
        if existing == encoded {
            return Ok(());
        }
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "a different CA key record already exists",
        ));
    }
    write_security_file(durable, office, "ca-key.v1.json", &encoded, false)
}

/// Stages `ca-key.v1.staging` in local security storage before Entry 1 commits, so a crash
/// after Entry 1 appends can recover the exact wrapped key without ever storing key
/// ciphertext inside `durable/recovery/` journal segments.
pub fn stage_ca_key(durable: &Path, office: &Id, record: &CaKey) -> io::Result<()> {
    let encoded = records::encode_ca_key(record);
    write_security_file(durable, office, "ca-key.v1.staging", &encoded, true)
}

/// Removes `ca-key.v1.staging` if present and syncs its parent folder.
pub fn remove_staged_ca_key(durable: &Path, office: &Id) -> io::Result<()> {
    let path = durable.join(records::ca_key_staging_path(office).as_str());
    match fs::remove_file(&path) {
        Ok(()) => {
            if let Some(parent) = path.parent() {
                fs::File::open(parent)?.sync_all()?;
            }
            Ok(())
        }
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e),
    }
}

/// Promotes `ca-key.v1.staging` (if present and matching `package`) to `ca-key.v1.json`
/// and removes the staging file.
pub fn promote_staged_ca_key(
    durable: &Path,
    files: &Files,
    organization: &Id,
    office: &Id,
    operation: &Id,
) -> Result<(), CaError> {
    let staging_path = records::ca_key_staging_path(office);
    let Some(staged_bytes) = files.read(&staging_path)? else {
        return Ok(());
    };
    let Some(package) = files.read(&organization::ca_escrow_path())? else {
        // Entry 1 never committed; drop the uncommitted staging file.
        let _ = remove_staged_ca_key(durable, office);
        return Ok(());
    };
    let key = records::decode_ca_key(&staged_bytes)
        .map_err(|why| CaError::Malformed(staging_path, why))?;
    let records = CaRecords {
        key,
        package: package.to_vec(),
    };
    records.check(organization, office, operation)?;
    write_ca_key(durable, office, &records.key).map_err(|e| {
        CaError::Files(FilesError::Io {
            path: records::ca_key_path(office).to_string(),
            action: "write",
            message: e.to_string(),
            flush: false,
        })
    })?;
    let _ = remove_staged_ca_key(durable, office);
    Ok(())
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

#[cfg(test)]
mod tests;
