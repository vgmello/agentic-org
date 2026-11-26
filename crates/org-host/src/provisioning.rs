//! Protected selected HQ orchestration. The office owns canonical data; this
//! adapter owns reservations/custody and uses the existing admitted supervisor.
#[cfg(test)]
use crate::hostdb::accounts::Account;
use crate::hostdb::pending_recovery::{unavailable, Pending};
use crate::{
    broadcast::Broadcast,
    host_secrets::HostSecrets,
    hostdb::{provisioning::Reservation, HostDb},
    keys::OfficeSecretsKeys,
    offices::Offices,
    writer::Writers,
};
use org_proto::{ApiError, ErrorCode, RecoveryCode};
use sha2::{Digest, Sha256};
use std::{
    os::unix::fs::{DirBuilderExt, MetadataExt},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

mod admission;
mod drive;
mod requests;
use drive::RESUME_ATTEMPTS;
use requests::already_exists;

pub(crate) struct Provisioning {
    db: Arc<HostDb>,
    custody: Arc<dyn HostSecrets>,
    keys: Arc<dyn OfficeSecretsKeys>,
    writers: Arc<Writers>,
    instance: String,
    office_home: PathBuf,
    /// Held for admission only, never while an organization is being created.
    gate: Mutex<()>,
    hq: std::sync::OnceLock<Arc<dyn Hq>>,
    /// The admitted web connections. `account.changed` goes to these and to nothing else,
    /// in order with the answers that report a create's state.
    web: Broadcast,
    /// The operations a creation is running for right now.
    driving: Arc<Mutex<std::collections::BTreeSet<String>>>,
    /// Set when the host stops: no attempt starts and no link is recorded afterwards.
    stopping: std::sync::atomic::AtomicBool,
    #[cfg(test)]
    drives_started: std::sync::atomic::AtomicUsize,
    /// While set, a creation thread holds its claim and creates nothing until the host
    /// stops.
    #[cfg(test)]
    hold_drives: std::sync::atomic::AtomicBool,
    /// 1: the next reservation commit fails; 2: the host is lost between staging the code
    /// and the commit, so the staged entry stays; 3: the account's administrator authority
    /// is revoked after every earlier check, just before the reservation commit.
    #[cfg(test)]
    reservation_fault: std::sync::atomic::AtomicU8,
}
fn generate_recovery_code() -> Result<RecoveryCode, ApiError> {
    use std::fmt::Write as _;
    let random = crate::secrets::generate_office_secrets_key().map_err(storage)?;
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
    RecoveryCode::new(formatted).map_err(storage)
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
    /// The organizations' data roots come from `writers` (its instance root), never from a
    /// caller.
    pub(crate) fn new(
        db: Arc<HostDb>,
        custody: Arc<dyn HostSecrets>,
        keys: Arc<dyn OfficeSecretsKeys>,
        writers: Arc<Writers>,
        instance: &str,
        office_home: &Path,
    ) -> Self {
        Self {
            db,
            custody,
            keys,
            writers,
            instance: instance.into(),
            office_home: office_home.into(),
            gate: Mutex::new(()),
            hq: std::sync::OnceLock::new(),
            driving: Arc::default(),
            stopping: std::sync::atomic::AtomicBool::new(false),
            #[cfg(test)]
            drives_started: std::sync::atomic::AtomicUsize::new(0),
            #[cfg(test)]
            hold_drives: std::sync::atomic::AtomicBool::new(false),
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
                    if self.drive_on_thread(&r, RESUME_ATTEMPTS).is_err() {
                        crate::host_log::log("a reserved organization create could not be resumed");
                    }
                }
            }
            Err(_) => crate::host_log::log("reserved organization creates could not be read"),
        }
    }

    /// The host is stopping: no creation attempt starts from now on, a waiting retry ends
    /// and a creation whose worker answers later records no link (the next start continues
    /// it). Call before the offices stop.
    pub(crate) fn halt(&self) {
        self.stopping
            .store(true, std::sync::atomic::Ordering::Release);
    }

    /// [`Self::halt`], then waits until no creation thread is left, so nothing writes
    /// custody or the registry after the host stopped. The wait is bounded: it ends
    /// [`STOP_MARGIN`] after `deadline`, where the host's supervisor kills the servers it
    /// stops (the end of their grace period, fixed when the host was asked to stop), so a
    /// creation whose server is killed then has ended before the wait does. A thread still
    /// running when it ends is logged, records no link (see [`Self::drive`]), but may still
    /// finish a custody or file write it had begun. Call after the offices stopped: a
    /// creation waiting for its worker ends when the supervisor stops it, and the
    /// supervisor's shutdown returns only after that worker is gone.
    pub(crate) fn stop_by(&self, deadline: std::time::Instant) {
        self.halt();
        let until = deadline + STOP_MARGIN;
        while self.driving.lock().is_ok_and(|driving| !driving.is_empty()) {
            if std::time::Instant::now() >= until {
                crate::host_log::log("an organization create did not end with the host");
                return;
            }
            std::thread::sleep(STOP_POLL);
        }
    }

    fn stopping(&self) -> bool {
        self.stopping.load(std::sync::atomic::Ordering::Acquire)
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

    #[cfg(test)]
    pub(crate) fn drives_started(&self) -> usize {
        self.drives_started
            .load(std::sync::atomic::Ordering::Acquire)
    }
    /// Creation threads started from now on wait, holding their claim, until the host stops.
    #[cfg(test)]
    pub(crate) fn hold_drives(&self) {
        self.hold_drives
            .store(true, std::sync::atomic::Ordering::Release);
    }
    /// The account a browser signed in as `login` acts as, for a test that creates an
    /// organization in the host's process where no web process serves.
    #[cfg(test)]
    pub(crate) fn account_for_test(&self, login: &str) -> Account {
        let credentials = self
            .db
            .account_credentials(&crate::accounts::identity::LoginKey::new(login).unwrap())
            .unwrap()
            .expect("the account exists");
        let id = crate::accounts::identity::AccountId::parse(credentials.account().account_id())
            .unwrap();
        self.db.account_authority(&id).unwrap().unwrap()
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
    /// The next admission's account is revoked between its last check and its commit.
    #[cfg(test)]
    pub(crate) fn revoke_before_next_reservation(&self) {
        self.reservation_fault
            .store(3, std::sync::atomic::Ordering::Release);
    }
    /// Starts a creation from `r` as a poll that read `r` earlier would.
    #[cfg(test)]
    pub(crate) fn continue_from(self: &Arc<Self>, r: &Reservation) {
        self.ensure_driving(r).unwrap();
    }
    /// What a poll's attempt to start a creation answers.
    #[cfg(test)]
    pub(crate) fn try_continue_from(self: &Arc<Self>, r: &Reservation) -> Result<(), ApiError> {
        self.ensure_driving(r)
    }
    #[cfg(test)]
    pub(crate) fn sweep_for_test(&self) {
        self.sweep_unreserved();
    }
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
    /// Clones and validates an existing organization's Git state branch read-only (`TASK-39` / R1).
    fn inspect_import(
        &self,
        _repository_remote_url: &str,
        _state_branch: &str,
    ) -> Result<org_office_launch::InspectImportResponse, ApiError> {
        Err(unavailable())
    }
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
    fn inspect_import(
        &self,
        repository_remote_url: &str,
        state_branch: &str,
    ) -> Result<org_office_launch::InspectImportResponse, ApiError> {
        Offices::inspect_import(self, repository_remote_url, state_branch)
    }
}

/// How long past the supervisor's grace period a stopping host waits for its creation
/// threads to end: what a thread needs after its server was stopped or killed. With the
/// host's other waits it ends inside the host's stop bound of 2.5 s (`Timings`); a thread
/// that waits longer (for a write lock of the registry, say) is left, as
/// [`Provisioning::stop_by`] says.
const STOP_MARGIN: std::time::Duration = std::time::Duration::from_millis(500);
const STOP_POLL: std::time::Duration = std::time::Duration::from_millis(10);

#[path = "provisioning/import.rs"]
mod import;

#[cfg(test)]
mod tests;
