//! Protected selected HQ orchestration. The office owns canonical data; this
//! adapter owns reservations/custody and uses the existing admitted supervisor.
use crate::{
    broadcast::Broadcast,
    host_secrets::{HostSecrets, Name, Value},
    hostdb::{
        accounts::Account,
        provisioning::{Reservation, Taken},
        HostDb,
    },
    keys::OfficeSecretsKeys,
    offices::Offices,
    writer::Writers,
};
use org_host_control::{
    ops::RecoveryState,
    organization::{
        AccountChanged, AccountChangedData, CreatingOrganization, ListResult, OrganizationView,
    },
    HostOpResult,
};
use org_office_launch::writer::Admission;
use org_proto::{
    selected_bootstrap::{PlanV1, PreparedV1},
    work::WriterIdentity,
    ApiError, ErrorCode, RecoveryCode,
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
    /// Held for admission only, never while an organization is being created.
    gate: Mutex<()>,
    hq: std::sync::OnceLock<Arc<dyn Hq>>,
    /// The admitted web connections. `account.changed` goes to these and to nothing else,
    /// in order with the answers that report a create's state.
    web: Broadcast,
    /// The operations a creation is running for right now.
    driving: Arc<Mutex<std::collections::BTreeSet<String>>>,
    #[cfg(test)]
    drives_started: std::sync::atomic::AtomicUsize,
    /// 1: the next reservation commit fails; 2: the host is lost between staging the code
    /// and the commit, so the staged entry stays.
    #[cfg(test)]
    reservation_fault: std::sync::atomic::AtomicU8,
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
fn not_administrator() -> ApiError {
    ApiError::new(ErrorCode::NotAuthorized, "instance administrator required")
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
        Error::NotAdministrator => return not_administrator(),
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
            hq: std::sync::OnceLock::new(),
            driving: Arc::default(),
            #[cfg(test)]
            drives_started: std::sync::atomic::AtomicUsize::new(0),
            web: Broadcast::default(),
            #[cfg(test)]
            reservation_fault: std::sync::atomic::AtomicU8::new(0),
        }
    }
    /// Installs the offices this host creates in and continues every create that reserved
    /// its identity before the host last stopped. No client has to ask.
    pub(crate) fn start(self: &Arc<Self>, hq: Arc<dyn Hq>) {
        if self.hq.set(hq).is_err() {
            return;
        }
        self.sweep_unreserved();
        match self.db.provisioning_reserved() {
            Ok(reserved) => {
                for r in reserved {
                    if self.ensure_driving(&r).is_err() {
                        crate::host_log::log("a reserved organization create could not be resumed");
                    }
                }
            }
            Err(_) => crate::host_log::log("reserved organization creates could not be read"),
        }
    }

    /// The admitted web connections: the service adds each one when its hello completes.
    pub(crate) fn web(&self) -> &Broadcast {
        &self.web
    }

    /// Removes every staged recovery code no reservation names: an admission the host lost
    /// between staging the code and committing the reservation. Runs before any admission.
    fn sweep_unreserved(&self) {
        let Ok(_gate) = self.gate.lock() else {
            return;
        };
        let Ok(names) = self.custody.list() else {
            crate::host_log::log("staged recovery codes could not be listed");
            return;
        };
        for name in names {
            let Some(operation) = name.as_str().strip_prefix("recovery.") else {
                continue;
            };
            // Only a successful read that finds no reservation removes an entry.
            if matches!(self.db.provisioning_known(operation), Ok(false))
                && self.custody.delete(&name).is_err()
            {
                crate::host_log::log("a staged recovery code without a reservation remains");
            }
        }
    }

    fn verify_owner(&self, r: &Reservation) -> Result<(), ApiError> {
        self.db
            .verify_selected_owner(&r.prepared.organization_id, &r.account, &r.owner_member)
            .map_err(storage)
    }

    /// Starts the creation on its own thread unless one is already running for this
    /// operation. The check and the claim are one step under a short lock.
    fn ensure_driving(self: &Arc<Self>, r: &Reservation) -> Result<(), ApiError> {
        let hq = Arc::clone(self.hq.get().ok_or_else(unavailable)?);
        let Some(claim) = self.claim(&r.operation_id)? else {
            return Ok(());
        };
        let this = Arc::clone(self);
        let r = r.clone();
        std::thread::Builder::new()
            .name("organization-create".into())
            .spawn(move || {
                let _claim = claim;
                // The caller read its row before this attempt held the claim; an earlier
                // attempt may have linked the organization since. Only the row read under
                // the claim decides, and only it is created from.
                let fresh = match this.db.provisioning_lookup(&r.account, &r.operation_id) {
                    Ok(Some(fresh)) => fresh,
                    _ => {
                        crate::host_log::log("an organization create could not be read");
                        return;
                    }
                };
                if fresh.linked {
                    return;
                }
                let outcome = this.drive(&fresh, &*hq);
                if outcome.is_err() {
                    // The reservation, the key and the code stay; a poll or a restart
                    // continues the same operation.
                    crate::host_log::log("an organization create attempt did not complete");
                }
            })
            .map(drop)
            .map_err(storage)
    }

    fn claim(&self, operation_id: &str) -> Result<Option<Claim>, ApiError> {
        let mut driving = self.driving.lock().map_err(storage)?;
        if !driving.insert(operation_id.to_owned()) {
            return Ok(None);
        }
        #[cfg(test)]
        self.drives_started
            .fetch_add(1, std::sync::atomic::Ordering::AcqRel);
        Ok(Some(Claim {
            driving: Arc::clone(&self.driving),
            operation_id: operation_id.to_owned(),
        }))
    }

    #[cfg(test)]
    pub(crate) fn drives_started(&self) -> usize {
        self.drives_started
            .load(std::sync::atomic::Ordering::Acquire)
    }
    #[cfg(test)]
    pub(crate) fn driving_now(&self) -> usize {
        self.driving.lock().unwrap().len()
    }
    /// Holds the admission gate as an admission in progress does.
    #[cfg(test)]
    pub(crate) fn hold_admission(&self) -> std::sync::MutexGuard<'_, ()> {
        self.gate.lock().unwrap()
    }
    /// The next reservation commit fails after its code was staged.
    #[cfg(test)]
    pub(crate) fn fail_next_reservation(&self) {
        self.reservation_fault
            .store(1, std::sync::atomic::Ordering::Release);
    }
    /// The next admission stops as a lost host does: code staged, no reservation.
    #[cfg(test)]
    pub(crate) fn lose_next_reservation(&self) {
        self.reservation_fault
            .store(2, std::sync::atomic::Ordering::Release);
    }
    /// Starts a creation from `r` as a poll that read `r` earlier would.
    #[cfg(test)]
    pub(crate) fn continue_from(self: &Arc<Self>, r: &Reservation) {
        self.ensure_driving(r).unwrap();
    }
    #[cfg(test)]
    pub(crate) fn sweep_for_test(&self) {
        self.sweep_unreserved();
    }

    /// Admission: the only step that holds the gate. A known operation is read before the
    /// gate, so a poll never waits for another create's admission. A new one is validated,
    /// its name and slug are checked, its recovery code is staged in host custody and its
    /// reservation is committed, in that order; a refusal before the reservation leaves no
    /// custody entry behind.
    fn admit(
        &self,
        account: &Account,
        operation_id: &str,
        plan: &PlanV1,
        fp: [u8; 32],
    ) -> Result<Reservation, ApiError> {
        if !account.admin() || account.revoked_at_ms().is_some() {
            return Err(not_administrator());
        }
        let known = || {
            let wall = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_err(storage)?
                .as_millis();
            self.db
                .provisioning(
                    account,
                    operation_id,
                    &fp,
                    i64::try_from(wall).map_err(storage)?,
                )
                .map_err(lookup_error)
        };
        if let Some(r) = known()? {
            return Ok(r);
        }
        let _gate = self.gate.lock().map_err(storage)?;
        if let Some(r) = known()? {
            return Ok(r);
        }
        if let Some((field, rule)) = plan.invalid_field() {
            let mut error = ApiError::new(
                ErrorCode::InvalidRequest,
                "invalid selected provisioning plan",
            );
            error.field = Some(field.into());
            error.rule = Some(rule.into());
            return Err(error);
        }
        let handle = Name::parse(&format!("recovery.{operation_id}")).map_err(storage)?;
        // A staged code without a reservation is an admission the host lost before its
        // commit. It is continued with its own ids when it is this same request.
        let staged = match self.custody.load(&handle).map_err(storage)? {
            Some(bytes) => {
                let pending: Pending = serde_json::from_slice(bytes.expose()).map_err(storage)?;
                validate_pending_binding(&pending, account, operation_id, plan, &fp)?;
                Some(pending)
            }
            None => None,
        };
        let taken = |fresh: bool| -> Option<ApiError> {
            let taken = self.db.provisioning_taken(&plan.name, &plan.slug);
            if fresh || matches!(taken, Ok(Some(_))) {
                // No reservation will ever name this entry: remove it.
                let _ = self.custody.delete(&handle);
            }
            match taken {
                Ok(Some(taken)) => Some(already_exists(taken)),
                Ok(None) => None,
                Err(_) => Some(unavailable()),
            }
        };
        if let Some(found) = self
            .db
            .provisioning_taken(&plan.name, &plan.slug)
            .map_err(storage)?
        {
            if staged.is_some() {
                self.custody.delete(&handle).map_err(storage)?;
            }
            return Err(already_exists(found));
        }
        let fresh = staged.is_none();
        let pending = match staged {
            Some(pending) => pending,
            None => match self.new_pending(account, operation_id, plan, fp, &handle) {
                Ok(pending) => pending,
                Err(error) => {
                    // The put may have been published before it failed.
                    let _ = self.custody.delete(&handle);
                    return Err(error);
                }
            },
        };
        #[cfg(test)]
        match self
            .reservation_fault
            .swap(0, std::sync::atomic::Ordering::AcqRel)
        {
            1 => return Err(taken(fresh).unwrap_or_else(unavailable)),
            2 => return Err(unavailable()),
            _ => {}
        }
        self.db
            .reserve_provisioning(account, &pending.reservation)
            .map_err(|error| {
                if matches!(error, crate::hostdb::Error::NotAdministrator) {
                    // The account lost its authority after the first check: no reservation
                    // will ever name this entry.
                    let _ = self.custody.delete(&handle);
                    return not_administrator();
                }
                taken(fresh).unwrap_or_else(unavailable)
            })
    }

    /// Creation, outside the admission gate. The account's authority is read again here:
    /// an administrator revoked after admission creates nothing more.
    fn drive(&self, r: &Reservation, hq: &dyn Hq) -> Result<(), ApiError> {
        let id = crate::accounts::identity::AccountId::parse(&r.account).map_err(storage)?;
        let account = self
            .db
            .account_authority(&id)
            .map_err(storage)?
            .filter(|a| a.admin() && a.revoked_at_ms().is_none())
            .ok_or_else(not_administrator)?;
        let account = &account;
        let handle = Name::parse(&r.recovery_handle).map_err(storage)?;
        let pending: Pending = serde_json::from_slice(
            self.custody
                .load(&handle)
                .map_err(storage)?
                .ok_or_else(unavailable)?
                .expose(),
        )
        .map_err(storage)?;
        if pending.version != 1
            || pending.reservation.operation_id != r.operation_id
            || pending.reservation.account != r.account
            || pending.reservation.fingerprint != r.fingerprint
            || pending.reservation.prepared != r.prepared
            || pending.reservation.owner_member != r.owner_member
        {
            return Err(unavailable());
        }
        let folder = org_office_launch::office_dir(&self.office_home, &r.prepared.office_id)
            .ok_or_else(unavailable)?;
        let public = self.reserve_writer(account, r, &folder)?;
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
        hq.provision_hq(first, pending.code)?;
        // The link and its announcement are one critical section of the web connections'
        // broadcast: a reader that sees the organization linked has `account.changed`
        // queued on its connection before its answer.
        self.web.critical(|held| {
            self.db.provision_linked(r).map_err(storage)?;
            held.publish::<AccountChanged>(&AccountChangedData {
                account_id: r.account.clone(),
            });
            Ok(())
        })
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
        operation_id: &str,
        plan: &PlanV1,
        fp: [u8; 32],
        handle: &Name,
    ) -> Result<Pending, ApiError> {
        // The creator's person id is the member id the account is linked to.
        let owner_member = crate::ids::uuid_v7().map_err(storage)?;
        let prepared = PreparedV1 {
            creator: org_proto::selected_bootstrap::CreatorV1::from_login(
                owner_member.clone(),
                account.login(),
            ),
            plan: plan.clone(),
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
            operation_id: operation_id.into(),
            account: account.account_id().into(),
            fingerprint: fp,
            prepared,
            owner_member,
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
/// What a signed-in browser asks: create, look up, list.
impl Provisioning {
    /// `organization.create`: admit a new create or find the one this id names, then report.
    pub(crate) fn create(
        self: &Arc<Self>,
        account: &Account,
        operation_id: &str,
        plan: &PlanV1,
        fp: [u8; 32],
    ) -> Result<HostOpResult<OrganizationView>, ApiError> {
        let r = self.admit(account, operation_id, plan, fp)?;
        self.progress(&r)
    }

    /// `operation.lookup`: one of the account's own creates. Never admits a new one.
    pub(crate) fn lookup(
        self: &Arc<Self>,
        account: &Account,
        operation_id: &str,
    ) -> Result<HostOpResult<OrganizationView>, ApiError> {
        let r = self
            .db
            .provisioning_lookup(account.account_id(), operation_id)
            .map_err(storage)?
            .ok_or_else(|| ApiError::new(ErrorCode::NotFound, "no such operation"))?;
        self.progress(&r)
    }

    /// `organization.list`: the organizations the account is linked to, and its creates
    /// still in flight.
    pub(crate) fn list(&self, account: &Account) -> Result<ListResult, ApiError> {
        let hq = self.hq.get().ok_or_else(unavailable)?;
        self.web.snapshot(|| {
            let mut organizations = Vec::new();
            for link in account.links() {
                // An organization this host did not create through a reservation has no
                // view here; nothing but a reservation links an account today.
                let Some(r) = self
                    .db
                    .provisioning_linked_to(link.org_id())
                    .map_err(storage)?
                else {
                    continue;
                };
                if r.account != account.account_id() || r.owner_member != link.member_id() {
                    continue;
                }
                // One organization this host cannot describe does not hide the others.
                let Some(organization) = hq.organization(&r.prepared.organization_id) else {
                    crate::host_log::log("a linked organization is missing from the catalog");
                    continue;
                };
                organizations.push(self.view(&r, organization)?);
            }
            let creating = self
                .db
                .provisioning_of(account.account_id())
                .map_err(storage)?
                .into_iter()
                .filter(|r| !r.linked)
                .map(|r| CreatingOrganization {
                    operation_id: r.operation_id,
                    name: r.prepared.plan.name,
                    slug: r.prepared.plan.slug,
                })
                .collect();
            Ok(ListResult {
                organizations,
                creating,
            })
        })
    }

    /// Reads the create's state without waiting for any admission or creation. The read is
    /// ordered with the broadcast, so `account.changed` is queued before any `completed`.
    fn progress(
        self: &Arc<Self>,
        r: &Reservation,
    ) -> Result<HostOpResult<OrganizationView>, ApiError> {
        let hq = self.hq.get().ok_or_else(unavailable)?;
        let current = self
            .web
            .snapshot(|| self.db.provisioning_lookup(&r.account, &r.operation_id))
            .map_err(storage)?
            .ok_or_else(unavailable)?;
        if current.linked {
            self.verify_owner(&current)?;
            return Ok(HostOpResult::Completed {
                result: self.view(
                    &current,
                    hq.organization(&current.prepared.organization_id)
                        .ok_or_else(unavailable)?,
                )?,
            });
        }
        self.ensure_driving(&current)?;
        Ok(HostOpResult::Accepted)
    }

    fn view(
        &self,
        r: &Reservation,
        organization: org_proto::Organization,
    ) -> Result<OrganizationView, ApiError> {
        use crate::hostdb::recovery::State;
        let recovery = match self.db.recovery_state(&r.operation_id).map_err(storage)? {
            None | Some(State::Pending) => RecoveryState::Pending,
            Some(State::Delivered | State::AckPending) => RecoveryState::Delivered,
            Some(State::Acknowledged) => RecoveryState::Acknowledged,
        };
        Ok(OrganizationView {
            id: organization.id,
            name: organization.name,
            slug: r.prepared.plan.slug.clone(),
            created_at: organization.created_at,
            employee_id: r.owner_member.clone(),
            offices: organization
                .offices
                .into_iter()
                .map(|mut office| {
                    office.endpoint = None;
                    office
                })
                .collect(),
            recovery,
        })
    }
}

fn validate_pending_binding(
    pending: &Pending,
    account: &Account,
    operation_id: &str,
    plan: &PlanV1,
    fp: &[u8; 32],
) -> Result<(), ApiError> {
    if pending.version != 1
        || pending.reservation.operation_id != operation_id
        || pending.reservation.account != account.account_id()
        || &pending.reservation.fingerprint != fp
        || &pending.reservation.prepared.plan != plan
    {
        return Err(ApiError::new(
            ErrorCode::OperationReused,
            "operation binding differs",
        ));
    }
    Ok(())
}

fn already_exists(taken: Taken) -> ApiError {
    let (field, message) = match taken {
        Taken::Name => ("name", "an organization with that name already exists"),
        Taken::Slug => ("slug", "an organization with that slug already exists"),
    };
    let mut error = ApiError::new(ErrorCode::AlreadyExists, message);
    error.field = Some(field.into());
    error.rule = Some("taken".into());
    error
}

/// The offices a create runs in: what creation needs from the host's supervisor and
/// catalog, owned here so creation can be proven without a worker process.
pub(crate) trait Hq: Send + Sync {
    /// Returns once the HQ worker reported `created` and the host adopted the office.
    fn provision_hq(
        &self,
        first: org_office_launch::CreateLine,
        recovery: RecoveryCode,
    ) -> Result<(), ApiError>;
    /// The organization with its offices on this host, HQ first.
    fn organization(&self, id: &str) -> Option<org_proto::Organization>;
}
impl Hq for Offices {
    fn provision_hq(
        &self,
        first: org_office_launch::CreateLine,
        recovery: RecoveryCode,
    ) -> Result<(), ApiError> {
        Offices::provision_hq(self, first, recovery)
    }
    fn organization(&self, id: &str) -> Option<org_proto::Organization> {
        self.organization_by_id(id)
    }
}

/// One running creation of one operation; dropping it lets the next attempt start.
struct Claim {
    driving: Arc<Mutex<std::collections::BTreeSet<String>>>,
    operation_id: String,
}
impl Drop for Claim {
    fn drop(&mut self) {
        if let Ok(mut driving) = self.driving.lock() {
            driving.remove(&self.operation_id);
        }
    }
}

#[cfg(test)]
mod tests;
