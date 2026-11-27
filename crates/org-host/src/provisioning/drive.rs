//! Driving a reserved create to its HQ office, once per operation, with bounded resumes.

use std::{
    path::Path,
    sync::{Arc, Mutex},
};

use org_host_control::organization::{AccountChanged, AccountChangedData};
use org_proto::{ApiError, ErrorCode};

use crate::{
    host_secrets::Name,
    hostdb::{accounts::Account, provisioning::Reservation},
};

use super::{
    fingerprint, not_administrator, private, storage, unavailable, Hq, Pending, Provisioning,
    STOP_POLL,
};

impl Provisioning {
    /// Starts the creation on its own thread unless one is already running for this
    /// operation. The check and the claim are one step under a short lock.
    pub(super) fn ensure_driving(self: &Arc<Self>, r: &Reservation) -> Result<(), ApiError> {
        self.drive_on_thread(r, 1)
    }

    /// [`Self::ensure_driving`] with up to `attempts` tries on the one thread. A client's
    /// poll tries once, because the client asks again. The start of the host has no
    /// client behind it, so it tries again by itself after a short wait, unless the
    /// refusal is one a later try cannot change or the host is stopping.
    pub(super) fn drive_on_thread(
        self: &Arc<Self>,
        r: &Reservation,
        attempts: u32,
    ) -> Result<(), ApiError> {
        if self.stopping() {
            return Err(unavailable());
        }
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
                #[cfg(all(feature = "test-support", debug_assertions))]
                while this.hold_drives.load(std::sync::atomic::Ordering::Acquire)
                    && !this.stopping()
                {
                    std::thread::sleep(STOP_POLL);
                }
                for attempt in 1..=attempts {
                    if this.stopping() {
                        return;
                    }
                    // The caller read its row before this attempt held the claim; an
                    // earlier attempt may have linked the organization since. Only the
                    // row read under the claim decides, and only it is created from.
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
                    let Err(error) = this.drive(&fresh, &*hq) else {
                        return;
                    };
                    // The reservation, the key and the code stay; a poll or a restart
                    // continues the same operation.
                    crate::host_log::log("an organization create attempt did not complete");
                    if !retryable(&error) {
                        return;
                    }
                    if attempt < attempts {
                        let until = std::time::Instant::now() + RESUME_WAIT * attempt;
                        while std::time::Instant::now() < until {
                            if this.stopping() {
                                return;
                            }
                            std::thread::sleep(STOP_POLL);
                        }
                    }
                }
            })
            .map(drop)
            .map_err(storage)
    }

    pub(super) fn claim(&self, operation_id: &str) -> Result<Option<Claim>, ApiError> {
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

    /// Creation, outside the admission gate. The account's authority is read again here:
    /// an administrator revoked after admission creates nothing more.
    pub(super) fn drive(&self, r: &Reservation, hq: &dyn Hq) -> Result<(), ApiError> {
        let account = self
            .administrator(&r.account)?
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
        self.reserve_writer(account, r, &folder)?;
        let first = org_office_launch::CreateLine {
            office: org_office_launch::OfficeLine {
                instance_id: self.instance.clone(),
                office_id: r.prepared.office_id.clone(),
                rendezvous: String::new(),
            },
            org_id: r.prepared.organization_id.clone(),
            org_name: r.prepared.plan.name.clone(),
            office_name: "HQ".into(),
            role: org_proto::OfficeRole::Hq,
            owner_member_id: r.owner_member.clone(),
            hq_creation: org_office_launch::HqCreationV1 {
                version: org_office_launch::HqCreationV1::VERSION,
                operation_id: r.operation_id.clone(),
                escrow_profile: 1,
                bootstrap: Box::new(r.prepared.clone()),
                import_source: pending.import_source.map(Box::new),
            },
        };
        hq.provision_hq(first, pending.code)?;
        // The link and its announcement are one critical section of the web connections'
        // broadcast: a reader that sees the organization linked has `account.changed`
        // queued on its connection before its answer.
        self.web.critical(|held| {
            // A host that is stopping records nothing more; its next start finds the
            // office adopted and links it then.
            if self.stopping() {
                return Err(unavailable());
            }
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
    ) -> Result<(), ApiError> {
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
        // All paths derive from protected instance/identity facts. No caller or browser
        // chooses a local path, and every parent is checked. The organization's data roots
        // are `<instance-root>/<organization-id>/{durable,runtime}`, owner-only; the writer
        // reservation requires them empty unless it already holds this office's record.
        let (durable, runtime) = self
            .writers
            .data_roots(&r.prepared.organization_id)
            .map_err(storage)?;
        private(durable.parent().ok_or_else(unavailable)?)?;
        private(&durable)?;
        private(&runtime)?;
        private(folder)?;
        self.accounts
            .establish_organization_owner(account, &r.prepared.organization_id, &r.owner_member)
            .map_err(storage)?;
        self.writers
            .reserve(
                &r.operation_id,
                &crate::writer::Binding {
                    organization_id: &r.prepared.organization_id,
                    office_id: &r.prepared.office_id,
                    owner_account: &r.account,
                    owner_member: &r.owner_member,
                },
            )
            .map_err(storage)?;
        Ok(())
    }
}

/// How often the starting host tries a reserved create before it leaves it to a poll.
pub(super) const RESUME_ATTEMPTS: u32 = 3;

/// The wait after a failed try at the start of the host, times the number of the try.
const RESUME_WAIT: std::time::Duration = std::time::Duration::from_millis(500);

/// Whether a later try of the same creation can end differently. A refusal that states a
/// decision (the account is no administrator, the request is not admissible) cannot.
fn retryable(error: &ApiError) -> bool {
    matches!(
        error.code,
        ErrorCode::Unavailable
            | ErrorCode::StorageError
            | ErrorCode::Timeout
            // "host starting": the offices are still discarding unfinished folders.
            | ErrorCode::NotReady
    )
}

/// One running creation of one operation; dropping it lets the next attempt start.
pub(super) struct Claim {
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
