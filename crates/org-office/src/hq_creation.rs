//! Worker-local creation receipt. Private CA bytes never enter organization
//! replication or the selected public/protected acceptance capture allowlist.
use crate::{store::hq_creation as storage, Office, OfficeError};

#[derive(Clone)]
pub struct Binding {
    pub operation_id: String,
    pub owner_member_id: String,
    pub public_root: std::path::PathBuf,
    pub bootstrap: Option<org_proto::selected_bootstrap::PreparedV1>,
}
impl Binding {
    pub(crate) fn validate(&self) -> Result<(), OfficeError> {
        // The creator's person record and the protected owner principal are one id.
        if self
            .bootstrap
            .as_ref()
            .is_some_and(|b| !b.valid() || b.creator.employee_id != self.owner_member_id)
            || org_proto::ulid_time_ms(&self.operation_id).is_none()
            || !org_proto::is_valid_member_id(&self.owner_member_id)
            || !self.public_root.is_absolute()
            || self.public_root.to_str().is_none()
            || self.public_root.components().any(|c| {
                !matches!(
                    c,
                    std::path::Component::RootDir | std::path::Component::Normal(_)
                )
            })
            || self.public_root.parent().is_none()
        {
            return Err(invalid());
        }
        Ok(())
    }
}
/// Copied/public creation record. It cannot carry usable private key material.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PublicRecord {
    pub version: u32,
    pub organization_id: String,
    pub operation_id: String,
    pub public_key: [u8; 65],
    pub package: Vec<u8>,
}
pub struct LocalRecord {
    pub organization_id: String,
    pub operation_id: String,
    pub public_key: [u8; 65],
    pub package: Vec<u8>,
    /// The CA private key as an `org_pki::wrap` record (opaque here).
    pub(crate) wrapped_ca_key: Vec<u8>,
}
impl std::fmt::Debug for LocalRecord {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("LocalCaRecord(<redacted>)")
    }
}
impl LocalRecord {
    /// The worker must validate the cryptographic package/key with org-pki
    /// before handing frozen bytes to storage. The domain owns only persistence.
    pub fn new(
        organization_id: String,
        operation_id: String,
        public_key: [u8; 65],
        package: Vec<u8>,
        wrapped_ca_key: Vec<u8>,
    ) -> Result<Self, OfficeError> {
        if organization_id.is_empty()
            || organization_id.len() > 255
            || org_proto::ulid_time_ms(&operation_id).is_none()
            || public_key[0] != 4
            || package.is_empty()
            || package.len() > 4475
            || wrapped_ca_key.len() < MIN_WRAPPED_CA_KEY
            || wrapped_ca_key.len() > MAX_WRAPPED_CA_KEY
        {
            return Err(invalid());
        }
        Ok(Self {
            organization_id,
            operation_id,
            public_key,
            package,
            wrapped_ca_key,
        })
    }
    /// The wrapped CA private key. The worker unwraps it with the office secrets key.
    pub fn wrapped_ca_key(&self) -> &[u8] {
        &self.wrapped_ca_key
    }
}
/// Bounds of a wrapped record: version, nonce and tag around 1..=4096 key bytes.
pub(crate) const MIN_WRAPPED_CA_KEY: usize = 1 + 12 + 1 + 16;
pub(crate) const MAX_WRAPPED_CA_KEY: usize = 1 + 12 + 4096 + 16;
fn invalid() -> OfficeError {
    OfficeError::Invalid("HQ provisioning requires recovery")
}

impl Office {
    /// Read exact committed public bytes, verifying the canonical copied record.
    /// This is the source for protected package export, not an authorization API.
    pub fn hq_creation_public_record(&self) -> Result<Option<PublicRecord>, OfficeError> {
        let binding = crate::office::hq_creation::verify_intent(
            self.folder(),
            self.info(),
            self.local_store().meta("instance_id")?.as_deref(),
        )?;
        let Some(binding) = binding else {
            return Ok(None);
        };
        storage::verify_binding(self.local_store(), &binding)?;
        let record = self
            .local_store()
            .read(storage::read_public)?
            .ok_or_else(invalid)?;
        if record.organization_id != self.info().org_id {
            return Err(invalid());
        }
        crate::office::hq_creation::verify_public(&binding, &record)?;
        Ok(Some(record))
    }
    pub fn hq_creation_record(&self) -> Result<Option<LocalRecord>, OfficeError> {
        let record = self.local_store().read(storage::read)?;
        let operation = self.local_store().meta("hq_creation_operation")?;
        // Pre-provisioning offices (including legacy satellites) have no HQ
        // receipt. A present creation receipt still requires current HQ authority.
        if record.is_none() {
            return if operation.is_none() {
                Ok(None)
            } else {
                Err(invalid())
            };
        }
        if !self.is_replication_hq()?
            || record.as_ref().is_some_and(|r| {
                r.organization_id != self.info().org_id
                    || operation.as_ref() != Some(&r.operation_id)
            })
        {
            return Err(invalid());
        }
        Ok(record)
    }
    pub fn commit_hq_creation(&self, record: LocalRecord) -> Result<(), OfficeError> {
        if record.organization_id != self.info().org_id || !self.is_replication_hq()? {
            return Err(invalid());
        }
        storage::initialize(self.local_store(), record)
    }
    pub fn verify_hq_creation_binding(&self, binding: &Binding) -> Result<(), OfficeError> {
        binding.validate()?;
        let operation = self.local_store().meta("hq_creation_operation")?;
        let owner = self.local_store().meta("hq_creation_owner")?;
        if operation.as_deref() != Some(&binding.operation_id)
            || owner.as_deref() != Some(&binding.owner_member_id)
        {
            return Err(invalid());
        }
        Ok(())
    }
}
