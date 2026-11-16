//! REC-ESC-1 composition: crypto remains in the worker; the host receives no
//! usable CA key. Creation context is authenticated by the launch admission.
use org_office::{
    hq_creation::{Binding, LocalRecord},
    Key, NewOffice, Office, OfficeError,
};
use org_proto::RecoveryCode;
use std::path::Path;
use zeroize::Zeroizing;
fn invalid() -> OfficeError {
    OfficeError::Invalid("HQ provisioning requires recovery")
}

pub(crate) fn prepare(
    folder: &Path,
    new: &NewOffice,
    key: Key,
    instance: &str,
    binding: &Binding,
    recovery: RecoveryCode,
) -> Result<Office, OfficeError> {
    let code = org_pki::Code::parse(recovery.expose()).map_err(|_| invalid())?;
    // The original wrapper and parsed KDF bytes both drop before serving.
    drop(recovery);
    Office::provision_hq_for_instance(folder, new, key, instance, binding, move |prior| {
        if let Some(prior) = prior {
            if prior.organization_id != new.org_id || prior.operation_id != binding.operation_id {
                return Err(invalid());
            }
            let recovered =
                org_pki::recover(&prior.package, &new.org_id, &code).map_err(|_| invalid())?;
            let local =
                org_pki::SigningKey::from_local_der(prior.local_der()).map_err(|_| invalid())?;
            if recovered.public_key() != prior.public_key || local.public_key() != prior.public_key
            {
                return Err(invalid());
            }
            return Ok(prior);
        }
        let ca = org_pki::SigningKey::generate().map_err(|_| invalid())?;
        let package = ca.seal(&new.org_id, &code).map_err(|_| invalid())?;
        LocalRecord::new(
            new.org_id.clone(),
            binding.operation_id.clone(),
            ca.public_key(),
            package,
            Zeroizing::new(ca.local_der().to_vec()),
        )
    })
}

/// Acknowledged healthy HQ starts without a pending delivery code. A retained
/// creation binding with absent/inconsistent material is recovery-required.
pub(crate) fn verify_existing(office: &Office) -> Result<(), OfficeError> {
    let Some(record) = office.hq_creation_record()? else {
        return Ok(());
    };
    let key = org_pki::SigningKey::from_local_der(record.local_der()).map_err(|_| invalid())?;
    let envelope =
        org_pki::Envelope::parse(&record.package, &office.info().org_id).map_err(|_| invalid())?;
    if key.public_key() != record.public_key || envelope.public_key() != &record.public_key {
        return Err(invalid());
    }
    Ok(())
}
#[cfg(test)]
mod tests;
