//! Protected selected HQ orchestration. The office owns canonical data; this
//! adapter owns reservations/custody and uses the existing admitted supervisor.
use crate::{
    host_secrets::{HostSecrets, Name, Value},
    hostdb::{accounts::Account, provisioning::Reservation, HostDb},
    keys::OfficeSecretsKeys,
    offices::Offices,
    writer::Writers,
};
use org_host_control::{
    journal,
    provision::{Params, Provisioned, RemoteReadiness},
    HostOpResult,
};
use org_office_launch::writer::Admission;
use org_proto::{
    selected_bootstrap::PreparedV1, work::WriterIdentity, ApiError, ErrorCode, RecoveryCode,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    os::unix::fs::{DirBuilderExt, MetadataExt},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

pub(crate) struct Provisioning {
    db: Arc<HostDb>,
    custody: Arc<dyn HostSecrets>,
    keys: Arc<dyn OfficeSecretsKeys>,
    writers: Arc<Writers>,
    instance: String,
    root: PathBuf,
    office_home: PathBuf,
    gate: Mutex<()>,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Pending {
    version: u32,
    reservation: Reservation,
    code: RecoveryCode,
}
pub(crate) fn pending_recovery(
    custody: &dyn HostSecrets,
    r: &Reservation,
) -> Result<RecoveryCode, ApiError> {
    let bytes = custody
        .load(&Name::parse(&r.recovery_handle).map_err(storage)?)
        .map_err(storage)?
        .ok_or_else(unavailable)?;
    let p: Pending = serde_json::from_slice(bytes.expose()).map_err(storage)?;
    if p.version != 1
        || p.reservation.operation_id != r.operation_id
        || p.reservation.account != r.account
        || p.reservation.fingerprint != r.fingerprint
        || p.reservation.prepared != r.prepared
        || p.reservation.owner_member != r.owner_member
        || p.reservation.recovery_handle != r.recovery_handle
        || p.reservation.key_fingerprint.is_some()
        || p.reservation.linked
    {
        return Err(unavailable());
    }
    let code = p.code.expose().as_bytes();
    if code.len() != 49
        || code.iter().enumerate().any(|(i, b)| {
            if i % 5 == 4 {
                *b != b'-'
            } else {
                !b.is_ascii_digit() && !(b'A'..=b'F').contains(b)
            }
        })
    {
        return Err(unavailable());
    }
    Ok(p.code)
}
fn unavailable() -> ApiError {
    ApiError::new(
        ErrorCode::Unavailable,
        "HQ preparation is unresolved; retry the original operation",
    )
}
fn storage<T>(_: T) -> ApiError {
    unavailable()
}
fn lookup_error(error: crate::hostdb::Error) -> ApiError {
    use crate::hostdb::Error;
    let code = match error {
        Error::OperationReused => ErrorCode::OperationReused,
        Error::Expired => ErrorCode::Expired,
        Error::ClockSkew => ErrorCode::ClockSkew,
        Error::InvalidId => ErrorCode::InvalidRequest,
        _ => return unavailable(),
    };
    ApiError::new(code, "provisioning operation not admissible")
}
fn fingerprint(key: &org_proto::OfficeSecretsKey) -> String {
    Sha256::digest(key.expose_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}
fn private(path: &Path) -> Result<(), ApiError> {
    match std::fs::DirBuilder::new().mode(0o700).create(path) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(e) => return Err(storage(e)),
    }
    let m = std::fs::symlink_metadata(path).map_err(storage)?;
    if !m.is_dir()
        || m.uid() != crate::home::euid()
        || m.mode() & 0o7777 != 0o700
        || path.canonicalize().map_err(storage)? != path
    {
        return Err(unavailable());
    }
    Ok(())
}
impl Provisioning {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(
        db: Arc<HostDb>,
        custody: Arc<dyn HostSecrets>,
        keys: Arc<dyn OfficeSecretsKeys>,
        writers: Arc<Writers>,
        instance: &str,
        root: &Path,
        office_home: &Path,
    ) -> Self {
        Self {
            db,
            custody,
            keys,
            writers,
            instance: instance.into(),
            root: root.into(),
            office_home: office_home.into(),
            gate: Mutex::new(()),
        }
    }
    pub(crate) fn run(
        &self,
        account: &Account,
        p: &Params,
        offices: &Offices,
    ) -> Result<HostOpResult<Provisioned>, ApiError> {
        if !account.admin() || account.revoked_at_ms().is_some() {
            return Err(ApiError::new(
                ErrorCode::NotAuthorized,
                "instance administrator required",
            ));
        }
        let _gate = self.gate.lock().map_err(storage)?;
        let fp = journal::fingerprint(p).map_err(storage)?;
        let wall = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(storage)?
            .as_millis();
        let prior = self
            .db
            .provisioning(
                account,
                &p.operation_id,
                &fp,
                i64::try_from(wall).map_err(storage)?,
            )
            .map_err(lookup_error)?;
        if prior.is_none() && !p.plan.valid() {
            return Err(ApiError::new(
                ErrorCode::InvalidRequest,
                "invalid selected provisioning plan",
            ));
        }
        let handle = Name::parse(&format!("recovery.{}", p.operation_id)).map_err(storage)?;
        // After acknowledged completion, normal retry never depends on a code
        // which protected collection may already have erased.
        if let Some(r) = prior.as_ref().filter(|r| r.linked) {
            self.db
                .verify_selected_owner(&r.prepared.organization_id, &r.account, &r.owner_member)
                .map_err(storage)?;
            return Ok(HostOpResult::Completed { result: result(r) });
        }
        let pending = match self.custody.load(&handle).map_err(storage)? {
            Some(bytes) => serde_json::from_slice::<Pending>(bytes.expose()).map_err(storage)?,
            None if prior.is_none() => self.new_pending(account, p, fp, &handle)?,
            None => return Err(unavailable()),
        };
        validate_pending_binding(&pending, account, p, &fp)?;
        let r = match prior {
            Some(r) => {
                if r.prepared != pending.reservation.prepared
                    || r.owner_member != pending.reservation.owner_member
                {
                    return Err(unavailable());
                }
                r
            }
            None => self
                .db
                .reserve_provisioning(account, &pending.reservation)
                .map_err(storage)?,
        };
        let folder = org_office_launch::office_dir(&self.office_home, &r.prepared.office_id)
            .ok_or_else(unavailable)?;
        let public = self.reserve_writer(account, &r, &folder)?;
        let first = org_office_launch::CreateLine {
            office: org_office_launch::OfficeLine {
                id_profile: Some(org_office_launch::IdProfile::SelectedV1),
                selected_writer_required: true,
                instance_id: self.instance.clone(),
                office_id: r.prepared.office_id.clone(),
                rendezvous: String::new(),
            },
            org_id: r.prepared.organization_id.clone(),
            org_name: r.prepared.plan.name.clone(),
            office_name: "HQ".into(),
            role: org_proto::OfficeRole::Hq,
            owner_member_id: r.owner_member.clone(),
            hq_creation: Some(org_office_launch::HqCreationV1 {
                version: 1,
                operation_id: r.operation_id.clone(),
                escrow_profile: 1,
                public_root: public.to_str().ok_or_else(unavailable)?.into(),
                bootstrap: Some(Box::new(r.prepared.clone())),
            }),
        };
        if !offices.provision_hq(first, pending.code)? {
            return Ok(HostOpResult::Accepted);
        }
        self.db.provision_linked(&r).map_err(storage)?;
        Ok(HostOpResult::Completed { result: result(&r) })
    }
    fn reserve_writer(
        &self,
        account: &Account,
        r: &Reservation,
        folder: &Path,
    ) -> Result<PathBuf, ApiError> {
        let key = match self.keys.load(&r.prepared.office_id).map_err(storage)? {
            Some(key) => key,
            None if r.key_fingerprint.is_none()
                && matches!(std::fs::symlink_metadata(folder),Err(e) if e.kind()==std::io::ErrorKind::NotFound) =>
            {
                let key = crate::secrets::generate_office_secrets_key().map_err(storage)?;
                self.keys
                    .create(&r.prepared.office_id, &key)
                    .map_err(storage)?;
                self.keys
                    .load(&r.prepared.office_id)
                    .map_err(storage)?
                    .ok_or_else(unavailable)?
            }
            None => return Err(unavailable()),
        };
        let key_fp = fingerprint(&key);
        drop(key);
        if r.key_fingerprint.as_ref().is_some_and(|f| f != &key_fp) {
            return Err(unavailable());
        }
        self.db.provision_key_stored(r, &key_fp).map_err(storage)?;
        // All paths derive from protected instance/identity facts. No caller
        // or browser chooses a local path, and every parent is checked.
        private(&self.root.join("durable"))?;
        private(&self.root.join("durable/state"))?;
        private(&self.root.join("durable/authority"))?;
        let public = self
            .root
            .join("durable/state")
            .join(&r.prepared.organization_id);
        private(&public)?;
        let protected = self
            .root
            .join("durable/authority")
            .join(&r.prepared.office_id);
        private(&protected)?;
        private(folder)?;
        self.db
            .establish_organization_owner(account, &r.prepared.organization_id, &r.owner_member)
            .map_err(storage)?;
        self.writers
            .reserve(
                &r.operation_id,
                Admission {
                    version: org_office_launch::writer::VERSION,
                    identity: WriterIdentity {
                        instance_id: self.instance.clone(),
                        organization_id: r.prepared.organization_id.clone(),
                        office_id: r.prepared.office_id.clone(),
                        machine_id: String::new(),
                        writer_epoch: 1,
                        journal_incarnation: String::new(),
                    },
                    protected_root: protected.to_str().ok_or_else(unavailable)?.into(),
                    public_root: public.to_str().ok_or_else(unavailable)?.into(),
                    owner_account: r.account.clone(),
                    owner_member: r.owner_member.clone(),
                },
            )
            .map_err(storage)?;
        Ok(public)
    }

    fn new_pending(
        &self,
        account: &Account,
        p: &Params,
        fp: [u8; 32],
        handle: &Name,
    ) -> Result<Pending, ApiError> {
        let prepared = PreparedV1 {
            plan: p.plan.clone(),
            organization_id: crate::ids::uuid_v7().map_err(storage)?,
            office_id: crate::ids::uuid_v7().map_err(storage)?,
            repository_id: crate::ids::uuid_v7().map_err(storage)?,
            home_team_id: crate::ids::uuid_v7().map_err(storage)?,
            hr_employee_id: crate::ids::uuid_v7().map_err(storage)?,
        };
        if self
            .keys
            .load(&prepared.office_id)
            .map_err(storage)?
            .is_some()
        {
            return Err(unavailable());
        }
        let reservation = Reservation {
            operation_id: p.operation_id.clone(),
            account: account.account_id().into(),
            fingerprint: fp,
            prepared,
            owner_member: crate::ids::generate().map_err(storage)?,
            recovery_handle: handle.as_str().into(),
            key_fingerprint: None,
            linked: false,
        };
        let random = crate::secrets::generate_office_secrets_key().map_err(storage)?;
        use std::fmt::Write as _;
        let mut formatted = String::with_capacity(49);
        for (index, bytes) in random.expose_bytes()[..20]
            .as_chunks::<2>()
            .0
            .iter()
            .enumerate()
        {
            if index != 0 {
                formatted.push('-');
            }
            write!(&mut formatted, "{:02X}{:02X}", bytes[0], bytes[1]).map_err(storage)?;
        }
        let code = RecoveryCode::new(formatted).map_err(storage)?;
        let pending = Pending {
            version: 1,
            reservation,
            code,
        };
        let bytes = Value::new(serde_json::to_vec(&pending).map_err(storage)?).map_err(storage)?;
        self.custody.put(handle, &bytes).map_err(storage)?;
        // Re-read after a possibly durable publication; no worker
        // starts from an unverified in-memory credential copy.
        let actual = self
            .custody
            .load(handle)
            .map_err(storage)?
            .ok_or_else(unavailable)?;
        if actual.expose() != bytes.expose() {
            return Err(unavailable());
        }
        Ok(pending)
    }
}
fn validate_pending_binding(
    pending: &Pending,
    account: &Account,
    p: &Params,
    fp: &[u8; 32],
) -> Result<(), ApiError> {
    if pending.version != 1
        || pending.reservation.operation_id != p.operation_id
        || pending.reservation.account != account.account_id()
        || &pending.reservation.fingerprint != fp
        || pending.reservation.prepared.plan != p.plan
    {
        return Err(ApiError::new(
            ErrorCode::OperationReused,
            "operation binding differs",
        ));
    }
    Ok(())
}

fn result(r: &Reservation) -> Provisioned {
    Provisioned {
        organization_id: r.prepared.organization_id.clone(),
        office_id: r.prepared.office_id.clone(),
        home_team_id: r.prepared.home_team_id.clone(),
        remote_readiness: RemoteReadiness::Unverified,
    }
}

#[cfg(test)]
mod tests;
