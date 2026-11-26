//! A satellite office's writer reservation: its data roots and the writer device id it is
//! bound to.

use std::{
    fs, io,
    path::{Path, PathBuf},
};

use org_office_launch::writer::{self as wire, Admission, Identity};

use crate::{
    host_secrets::{HostSecrets, Name, Value},
    sync::lock,
};

use super::{private, refused, Record, Writers, SCHEMA_VERSION};

/// Office-isolated data roots for a satellite office on this instance so same-instance HQ and
/// satellite offices never share a mutable `durable/` or `runtime/` directory (`TASK-52` #2).
pub(super) fn satellite_data_roots(
    root: &Path,
    organization: &str,
    office_id: &str,
) -> io::Result<(PathBuf, PathBuf)> {
    if !org_proto::is_uuid_v7(organization) || !org_proto::is_uuid_v7(office_id) {
        return Err(refused());
    }
    let base = root.join(organization).join("offices").join(office_id);
    Ok((base.join("durable"), base.join("runtime")))
}

/// Reserves (or reuses existing) writer custody for a joining/re-enrolling satellite office
/// (`TASK-52` H-1), creating private office-isolated data roots if needed and installing the marker.
pub(crate) fn reserve_satellite(
    writers: &Writers,
    operation: &str,
    organization_id: &str,
    office_id: &str,
    actor_account: &str,
) -> io::Result<Admission> {
    use std::os::unix::fs::DirBuilderExt;
    let _gate = lock(&writers.gate);
    writers.root.verify()?;
    let (durable, runtime) = satellite_data_roots(&writers.root.path, organization_id, office_id)?;
    if !org_proto::is_uuid_v7(operation)
        || !org_proto::is_uuid_v7(office_id)
        || actor_account.is_empty()
    {
        return Err(refused());
    }
    if let Some(record) = writers.load(office_id)? {
        writers.verify(&record, office_id)?;
        if record.admission.identity.organization_id != organization_id {
            return Err(refused());
        }
        writers.install_marker(office_id)?;
        return Ok(record.admission);
    }
    let marker = writers.marker(office_id)?;
    for path in [marker.parent().ok_or_else(refused)?, &durable, &runtime] {
        fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(path)?;
    }
    let machine_id = ensure_writer_device_id(&*writers.custody)?;
    let owner_account = if org_proto::is_uuid_v7(actor_account)
        || org_proto::ulid_time_ms(actor_account).is_some()
    {
        actor_account.to_owned()
    } else {
        office_id.to_owned()
    };
    let admission = Admission {
        version: wire::VERSION,
        identity: Identity {
            instance_id: writers.instance.clone(),
            organization_id: organization_id.into(),
            office_id: office_id.into(),
            journal_incarnation: crate::ids::uuid_v7()?,
            writer_epoch: 1,
            machine_id,
        },
        durable_root: durable.to_str().ok_or_else(refused)?.into(),
        runtime_root: runtime.to_str().ok_or_else(refused)?.into(),
        owner_account,
        owner_member: office_id.into(),
    };
    let record = Record {
        schema_version: SCHEMA_VERSION,
        operation_id: operation.into(),
        root: writers.root.clone(),
        admission: admission.clone(),
        head: None,
    };
    writers.verify(&record, office_id)?;
    writers.save(office_id, &record)?;
    writers.install_marker(office_id)?;
    Ok(admission)
}

fn ensure_writer_device_id(custody: &dyn HostSecrets) -> io::Result<String> {
    let device_name = Name::parse("writer-device").map_err(private)?;
    if custody.load(&device_name).map_err(private)?.is_none() {
        let random = crate::secrets::generate_office_secrets_key()?;
        let value: String = random
            .expose_bytes()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        custody
            .put(
                &device_name,
                &Value::new(value.into_bytes()).map_err(private)?,
            )
            .map_err(private)?;
    }
    let device = custody
        .load(&device_name)
        .map_err(private)?
        .ok_or_else(refused)?;
    String::from_utf8(device.expose().to_vec()).map_err(private)
}
