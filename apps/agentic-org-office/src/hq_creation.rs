//! REC-ESC-1 composition: crypto remains in the worker; the host receives no
//! usable CA key. Creation context is authenticated by the launch admission.
//! The CA private key is stored only as an `org_pki::wrap` record under the
//! office secrets key (ADR-0016 decision 3, ENC-5).
use org_office::{
    hq_creation::{Binding, LocalRecord},
    organization_family::operation_id::from_wire_ulid,
    Custody, DataRoots, Key, NewOffice, Office, OfficeError,
};
use org_proto::RecoveryCode;
use std::path::Path;
fn invalid() -> OfficeError {
    OfficeError::Invalid("HQ provisioning requires recovery")
}
fn ca_binding<'a>(organization_id: &'a str, office_id: &'a str) -> org_pki::wrap::WrapBinding<'a> {
    org_pki::wrap::WrapBinding {
        organization_id,
        office_id,
        purpose: org_pki::wrap::WrapPurpose::OrganizationCa,
    }
}
/// Unwraps the retained CA key and checks it against the recorded public key.
fn unwrap_ca(
    key: &Key,
    record: &LocalRecord,
    office_id: &str,
) -> Result<org_pki::SigningKey, OfficeError> {
    let der = org_pki::wrap::unwrap(
        key.expose_bytes(),
        &ca_binding(&record.organization_id, office_id),
        record.wrapped_ca_key(),
    )
    .map_err(|_| invalid())?;
    let ca = org_pki::SigningKey::from_local_der(&der).map_err(|_| invalid())?;
    if ca.public_key() != record.public_key {
        return Err(invalid());
    }
    Ok(ca)
}

/// Creates the HQ on its store, or retries its creation, under the host's writer custody
/// and in the organization's data roots. Without the office secrets key the CA key can be
/// neither wrapped nor recovered from the retained record: recovery-required, nothing
/// touched (ADR-0016 decision 3). The CA records name the store operation the wire's ULID
/// maps to (`from_wire_ulid`), never the ULID itself.
#[allow(clippy::too_many_arguments)]
pub(crate) fn prepare(
    folder: &Path,
    new: &NewOffice,
    key: Option<Key>,
    binding: &Binding,
    roots: DataRoots,
    custody: Custody,
    recovery: RecoveryCode,
) -> Result<Office, OfficeError> {
    let Some(key) = key else {
        return Err(invalid());
    };
    let operation = from_wire_ulid(&binding.operation_id)
        .map_err(|_| invalid())?
        .to_string();
    let code = org_pki::Code::parse(recovery.expose()).map_err(|_| invalid())?;
    // The original wrapper and parsed KDF bytes both drop before serving.
    drop(recovery);
    let secrets = key.clone();
    Office::provision_hq(folder, new, key, binding, roots, custody, move |prior| {
        if let Some(prior) = prior {
            if prior.organization_id != new.org_id || prior.operation_id != operation {
                return Err(invalid());
            }
            let recovered =
                org_pki::recover(&prior.package, &new.org_id, &code).map_err(|_| invalid())?;
            unwrap_ca(&secrets, &prior, &new.office_id)?;
            if recovered.public_key() != prior.public_key {
                return Err(invalid());
            }
            return Ok(prior);
        }
        let ca = org_pki::SigningKey::generate().map_err(|_| invalid())?;
        let package = ca.seal(&new.org_id, &code).map_err(|_| invalid())?;
        let wrapped = org_pki::wrap::wrap(
            secrets.expose_bytes(),
            &ca_binding(&new.org_id, &new.office_id),
            ca.local_der(),
        )
        .map_err(|_| invalid())?;
        LocalRecord::new(
            new.org_id.clone(),
            operation,
            ca.public_key(),
            package,
            wrapped,
        )
    })
}

/// Whether this run can sign with the organization CA.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum CaStatus {
    /// The office keeps no HQ creation record: it has no CA of its own.
    NotHq,
    /// The wrapped CA key unwraps with the office secrets key to the recorded public key.
    Available,
    /// The office secrets key is absent or does not unwrap the CA key. The office keeps serving
    /// its data; only CA signing is unavailable (ADR-0016 decision 3, ENC-1).
    Unavailable,
}

/// Acknowledged healthy HQ starts without a pending delivery code. A retained creation binding
/// whose material is inconsistent (the package or the unwrapped key does not match the
/// recorded public key) is recovery-required. A missing or unusable secrets key is not: the
/// CA is only [`CaStatus::Unavailable`].
pub(crate) fn verify_existing(office: &Office) -> Result<CaStatus, OfficeError> {
    let Some(record) = office.hq_creation_record()? else {
        return Ok(CaStatus::NotHq);
    };
    let info = office.info();
    let envelope =
        org_pki::Envelope::parse(&record.package, &info.org_id).map_err(|_| invalid())?;
    if envelope.public_key() != &record.public_key {
        return Err(invalid());
    }
    let Some(key) = office.secrets_key() else {
        return Ok(CaStatus::Unavailable);
    };
    let Ok(der) = org_pki::wrap::unwrap(
        key.expose_bytes(),
        &ca_binding(&record.organization_id, &info.office_id),
        record.wrapped_ca_key(),
    ) else {
        return Ok(CaStatus::Unavailable);
    };
    let ca = org_pki::SigningKey::from_local_der(&der).map_err(|_| invalid())?;
    if ca.public_key() != record.public_key {
        return Err(invalid());
    }
    Ok(CaStatus::Available)
}
#[cfg(test)]
pub(crate) mod tests;
