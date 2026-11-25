//! REC-ESC-1 composition: crypto remains in the worker; the host receives no
//! usable CA key. Creation context is authenticated by the launch admission.
//! The CA private key is stored only as an `org_pki::wrap` record under the
//! office secrets key (ADR-0016 decision 3, ENC-5).
use org_office::{
    hq_creation::{Binding, LocalRecord},
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
/// touched (ADR-0016 decision 3). The CA records name the native UUIDv7 store operation
/// directly from the host's request.
#[expect(
    clippy::too_many_arguments,
    reason = "HQ provisioning coordinates folder, identity, optional key, binding, roots, custody, recovery, and import source"
)]
pub(crate) fn prepare(
    folder: &Path,
    new: &NewOffice,
    key: Option<Key>,
    binding: &Binding,
    roots: DataRoots,
    custody: Custody,
    recovery: RecoveryCode,
    import_source: Option<&org_office_launch::ImportSourceV1>,
) -> Result<Office, OfficeError> {
    let Some(key) = key else {
        return Err(invalid());
    };
    if !org_proto::is_uuid_v7(&binding.operation_id) {
        return Err(invalid());
    }
    let operation = binding.operation_id.clone();
    let code = org_pki::Code::parse(recovery.expose()).map_err(|_| invalid())?;
    // The original wrapper and parsed KDF bytes both drop before serving.
    drop(recovery);
    let secrets = key.clone();
    let org_id_for_ca = new.org_id.clone();
    let office_id_for_ca = new.office_id.clone();
    let material = move |prior: Option<LocalRecord>| -> Result<LocalRecord, OfficeError> {
        if let Some(prior) = prior {
            if prior.organization_id != org_id_for_ca || prior.operation_id != operation {
                return Err(invalid());
            }
            let recovered =
                org_pki::recover(&prior.package, &org_id_for_ca, &code).map_err(|_| invalid())?;
            unwrap_ca(&secrets, &prior, &office_id_for_ca)?;
            if recovered.public_key() != prior.public_key {
                return Err(invalid());
            }
            return Ok(prior);
        }
        let ca = org_pki::SigningKey::generate().map_err(|_| invalid())?;
        let package = ca.seal(&org_id_for_ca, &code).map_err(|_| invalid())?;
        let wrapped = org_pki::wrap::wrap(
            secrets.expose_bytes(),
            &ca_binding(&org_id_for_ca, &office_id_for_ca),
            ca.local_der(),
        )
        .map_err(|_| invalid())?;
        LocalRecord::new(org_id_for_ca, operation, ca.public_key(), package, wrapped)
    };
    if let Some(src) = import_source.cloned() {
        let expected_org_id = new.org_id.clone();
        let load_import = move || {
            let imported = org_office::import::clone_and_validate_state(
                &src.repository_remote_url,
                &src.state_branch,
            )
            .map_err(|_| OfficeError::Invalid("invalid imported organization repository"))?;
            if imported.summary.organization_id.as_str() != expected_org_id {
                return Err(invalid());
            }
            if let Some(pinned) = &src.commit_oid {
                if imported.commit_oid.as_deref() != Some(pinned.as_str()) {
                    return Err(invalid());
                }
            }
            let new_owner = match &src.new_owner_person {
                Some(c) => Some(org_office::organization_family::Creator {
                    employee_id: org_store::Id::parse(&c.employee_id).map_err(|_| invalid())?,
                    name: c.name.clone(),
                    handle: c.handle.clone(),
                }),
                None => None,
            };
            Ok((imported, new_owner))
        };
        return Office::import_hq(
            folder,
            new,
            key,
            binding,
            roots,
            custody,
            load_import,
            material,
        );
    }
    Office::provision_hq(folder, new, key, binding, roots, custody, material)
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
