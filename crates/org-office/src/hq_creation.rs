//! The HQ's creation: the request the host binds a creation to, and the CA records it
//! leaves. Private CA bytes never leave the office-local records.
use crate::OfficeError;

/// What the host asked a creation for: its operation (the wire's ULID), the owner, and the
/// prepared organization.
#[derive(Clone)]
pub struct Binding {
    pub operation_id: String,
    pub owner_member_id: String,
    pub bootstrap: org_proto::selected_bootstrap::PreparedV1,
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
    ///
    /// `operation_id` is the store operation that creates the organization, as it is on
    /// disk: a UUIDv7 in lowercase text. The creation request's ULID maps to it through
    /// `organization::operation_id::from_wire_ulid`.
    pub fn new(
        organization_id: String,
        operation_id: String,
        public_key: [u8; 65],
        package: Vec<u8>,
        wrapped_ca_key: Vec<u8>,
    ) -> Result<Self, OfficeError> {
        if !org_proto::is_uuid_v7(&operation_id) {
            return Err(invalid());
        }
        Self::checked(
            organization_id,
            operation_id,
            public_key,
            package,
            wrapped_ca_key,
        )
    }

    fn checked(
        organization_id: String,
        operation_id: String,
        public_key: [u8; 65],
        package: Vec<u8>,
        wrapped_ca_key: Vec<u8>,
    ) -> Result<Self, OfficeError> {
        if organization_id.is_empty()
            || organization_id.len() > 255
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
