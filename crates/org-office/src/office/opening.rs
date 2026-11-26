//! Opening an office: the open binding, custody checks and the journal consumers registered at
//! startup.

use std::{path::PathBuf, sync::atomic::AtomicU64};

use org_store::{Id, OfficeIdentity};

use crate::{
    error::OfficeError,
    key::Key,
    model::OfficeInfo,
    office_file,
    store::open::{open_store, DirectorySnapshot, OfficeStanding, OfficeStore},
};

use super::companions::{open_companion_stores, CompanionOpenSpec};
use super::folder::LOCK_WAIT;
use super::folder::{
    check_named_by, check_not_a_link, lock_folder, plain_folder, refuse_older_build,
};
use super::{
    check_roots, intent, recovery, satellite, storage, Custody, DataRoots, Office, Options,
};

fn validate_open_binding(
    intent: &intent::Intent,
    info: &OfficeInfo,
    instance: &str,
    roots: &DataRoots,
    custody: &Custody,
    active_owner: Option<&str>,
) -> Result<(Id, Id, String), OfficeError> {
    let here = &custody.identity;
    let bound = intent.info() == *info
        && intent.instance == instance
        && intent.roots() == *roots
        && here.instance_id.as_str() == instance
        && here.organization_id.as_str() == info.org_id
        && here.office_id.as_str() == info.office_id;
    if !bound {
        return Err(OfficeError::Invalid(
            "the office was created with other roots or another identity",
        ));
    }
    if !org_proto::is_uuid_v7(&intent.owner) || !org_proto::is_uuid_v7(&intent.operation) {
        return Err(recovery());
    }
    let effective_owner = active_owner.unwrap_or(&intent.owner);
    if !org_proto::is_valid_member_id(effective_owner) {
        return Err(OfficeError::Invalid("active owner id is invalid"));
    }
    let owner = Id::parse(effective_owner).map_err(|_| recovery())?;
    let creation = Id::parse(&intent.operation).map_err(|_| recovery())?;
    Ok((owner, creation, intent.operation.clone()))
}

fn check_standing_on(
    store: &OfficeStore,
    info: &OfficeInfo,
    identity: &OfficeIdentity,
) -> Result<(), OfficeError> {
    let read = OfficeStanding {
        office: identity.office_id.clone(),
    };
    let (standing, _) = store.read(read).map_err(storage)?;
    match info.role {
        crate::model::OfficeRole::Hq => {
            let ours = standing.organization.as_ref() == Some(&identity.organization_id);
            if !ours || !standing.hq {
                return Err(recovery());
            }
        }
        crate::model::OfficeRole::Satellite => {
            if let Some(org) = standing.organization.as_ref() {
                if org != &identity.organization_id || standing.hq {
                    return Err(OfficeError::Invalid(
                        "satellite projection does not match its enrolled organization or role",
                    ));
                }
            }
        }
    }
    Ok(())
}

/// Verifies writer custody against the opened projection BEFORE companion stores are opened so a
/// refused start never bumps `durable/authority/agent-execution/coordinator.json` (`TASK-146`).
fn check_admitted_custody_on(
    store: &OfficeStore,
    info: &OfficeInfo,
    identity: &OfficeIdentity,
    owner: &Id,
    expected_owner: &str,
) -> Result<(), OfficeError> {
    let ok = match info.role {
        crate::model::OfficeRole::Satellite => {
            expected_owner == info.office_id
                && identity.office_id.as_str() == info.office_id
                && identity.organization_id.as_str() == info.org_id
        }
        crate::model::OfficeRole::Hq => {
            let (snapshot, _) = store
                .read(DirectorySnapshot {
                    owner: owner.clone(),
                })
                .map_err(storage)?;
            snapshot
                .members
                .into_iter()
                .find(|member| member.owner)
                .is_some_and(|member| member.id == expected_owner)
        }
    };
    if ok {
        Ok(())
    } else {
        Err(OfficeError::CustodyMismatch)
    }
}

fn verify_store_on_open(
    store: OfficeStore,
    info: &OfficeInfo,
    identity: &OfficeIdentity,
    owner: &Id,
    active_owner: Option<&str>,
) -> Result<OfficeStore, OfficeError> {
    if let Err(err) = check_standing_on(&store, info, identity) {
        let _ = store.close();
        return Err(err);
    }
    if let Some(expected_owner) = active_owner {
        if let Err(err) = check_admitted_custody_on(&store, info, identity, owner, expected_owner) {
            let _ = store.close();
            return Err(err);
        }
    }
    Ok(store)
}

impl Office {
    /// Opens the finished HQ in `folder` on its store: takes the office lock, reads
    /// `office.json` and the creation intent, checks that the roots and the custody are the
    /// ones it was created with, opens the store (replaying or rebuilding the projection),
    /// and checks the organization, this office's role and the CA records. `key` is the
    /// host's office secrets key, held only to unwrap the office's private keys.
    pub fn open(
        folder: impl Into<PathBuf>,
        key: Option<Key>,
        instance: &str,
        roots: DataRoots,
        custody: Custody,
    ) -> Result<Office, OfficeError> {
        Self::open_with_owner(
            folder,
            key,
            instance,
            roots,
            custody,
            None,
            Options::production(),
        )
    }

    /// Opens the finished HQ in `folder` with an explicit host-authoritative owner member ID
    /// (supports organization ownership handover, `TASK-38` / `doc-78`, while keeping the
    /// immutable `creation-intent.json` intact).
    pub fn open_as_owner(
        folder: impl Into<PathBuf>,
        key: Option<Key>,
        instance: &str,
        roots: DataRoots,
        custody: Custody,
        active_owner: &str,
    ) -> Result<Office, OfficeError> {
        Self::open_with_owner(
            folder,
            key,
            instance,
            roots,
            custody,
            Some(active_owner),
            Options::production(),
        )
    }

    #[cfg(test)]
    pub(crate) fn open_with(
        folder: impl Into<PathBuf>,
        key: Option<Key>,
        instance: &str,
        roots: DataRoots,
        custody: Custody,
        options: Options,
    ) -> Result<Office, OfficeError> {
        Self::open_with_owner(folder, key, instance, roots, custody, None, options)
    }

    fn open_with_owner(
        folder: impl Into<PathBuf>,
        key: Option<Key>,
        instance: &str,
        roots: DataRoots,
        custody: Custody,
        active_owner: Option<&str>,
        options: Options,
    ) -> Result<Office, OfficeError> {
        if !org_proto::is_uuid_v7(instance) {
            return Err(OfficeError::Invalid(
                "instance id must be a canonical lowercase UUIDv7",
            ));
        }
        check_roots(&roots)?;
        let folder = plain_folder(folder.into())?;
        // A link or a file where the folder goes is refused here; a missing folder is
        // `NotFound` from `lock_folder`, which creates nothing.
        check_not_a_link(&folder)?;
        let lock = lock_folder(&folder, LOCK_WAIT)?;
        refuse_older_build(&folder)?;
        let info = office_file::read(&folder)?.ok_or(OfficeError::NotFound)?;
        check_named_by(&folder, &info.office_id)?;
        let (owner, creation, creation_command, hq_office_id, epoch, initial_snapshot_required) =
            match info.role {
                crate::model::OfficeRole::Hq => {
                    let intent = intent::read(&folder)?.ok_or_else(intent::older_build)?;
                    let (owner, creation, cmd) = validate_open_binding(
                        &intent,
                        &info,
                        instance,
                        &roots,
                        &custody,
                        active_owner,
                    )?;
                    (owner, creation, cmd, info.office_id.clone(), 1, false)
                }
                crate::model::OfficeRole::Satellite => {
                    let sat_intent = satellite::read_intent(&folder)?
                        .ok_or(OfficeError::Invalid("missing satellite join intent"))?;
                    let (fallback_owner, creation, cmd) =
                        satellite::validate_satellite_open_binding(
                            &sat_intent,
                            &info,
                            instance,
                            &roots,
                            &custody,
                        )?;
                    // Bind satellite open to the admitted owner member when provided so normal
                    // worker restart finds the replicated owner in `org_snapshot()` (`TASK-52` P2-6).
                    let owner = active_owner
                        .and_then(|o| Id::parse(o).ok())
                        .unwrap_or(fallback_owner);
                    // Prefer the single atomic `federation-identity.v1.json` generation if present
                    // so a crash right after D25 re-enrollment reconciles join-intent & checkpoint.
                    let (hq_id, ep) = match satellite::read_active_identity_binding(
                        &roots.durable,
                        &info.office_id,
                    )? {
                        Some(id) => (id.hq_office_id, id.epoch),
                        None => (sat_intent.hq_office_id, sat_intent.epoch),
                    };
                    (owner, creation, cmd, hq_id, ep, true)
                }
            };

        let store = open_store(&options.config(&roots, &custody)).map_err(storage)?;
        let store = verify_store_on_open(store, &info, &custody.identity, &owner, active_owner)?;
        let ca_digest = satellite::read_ca_digest_for_open(&roots.durable, &info, &hq_office_id);
        let companions = open_companion_stores(CompanionOpenSpec {
            engine: options.engine,
            roots: &roots,
            info: &info,
            identity: &custody.identity,
            hq_office_id: &hq_office_id,
            ca_digest,
            epoch,
            initial_snapshot_required,
        })?;
        if info.role == crate::model::OfficeRole::Satellite {
            satellite::reconcile_satellite_replication_on_open(
                companions.replication.as_ref(),
                &hq_office_id,
                ca_digest,
                epoch,
            )?;
        }
        let replicated_owner = if info.role == crate::model::OfficeRole::Satellite {
            let active_hq = companions
                .replication
                .org_checkpoint()?
                .hq_office
                .as_str()
                .to_owned();
            satellite::read_replicated_hq_owner(
                &roots.durable,
                &info.office_id,
                &info.org_id,
                &active_hq,
            )?
            .map(|id| (active_hq, id))
        } else {
            None
        };
        let clock_high_ms = AtomicU64::new(options.clock.now().0);
        let office = Office {
            folder,
            info,
            durable: roots.durable,
            runtime_root: roots.runtime,
            identity: custody.identity,
            owner,
            replicated_owner: std::sync::RwLock::new(replicated_owner),
            creation,
            creation_command,
            secrets_key: key,
            clock: options.clock,
            clock_high_ms,
            runtime: crate::messaging::runtime::OfficeRuntime::new(),
            private_notes: companions.private_notes,
            accounting: companions.accounting,
            budgets: companions.budgets,
            budget_op_lock: std::sync::Mutex::new(()),
            federation_io_lock: std::sync::Mutex::new(()),
            revocation_lock: std::sync::Mutex::new(()),
            snapshot_export_lock: std::sync::Mutex::new(()),
            promoted_removed_offices: std::sync::Mutex::new(std::collections::BTreeSet::new()),
            peer_activity_fences: std::sync::Mutex::new(std::collections::BTreeMap::new()),
            peer_session_stops: std::sync::Mutex::new(std::collections::BTreeMap::new()),
            providers: companions.providers,
            replication: companions.replication,
            store,
            _lock: lock,
            #[cfg(any(test, feature = "testkit"))]
            _test_root: None,
        };
        register_startup_journal_consumers(&office)?;
        // An organization created before the built-in Chief of Staff existed gets her once, at
        // the HQ (`TASK-80.1`). Best effort: a failure leaves the organization as it was, and
        // the next open tries again.
        let _ = office.ensure_chief_of_staff();
        Ok(office)
    }

    pub(super) fn check_standing(&self) -> Result<(), OfficeError> {
        check_standing_on(&self.store, &self.info, &self.identity)
    }
}

/// Eagerly registers durable journal consumers before admitting writes on multi-office HQ and all
/// satellites, while leaving standalone single-office HQ unpinned (`TASK-52` P5-#9 / P6-#7).
fn register_startup_journal_consumers(office: &Office) -> Result<(), OfficeError> {
    match office.info.role {
        crate::model::OfficeRole::Hq => {
            office.ca()?;
            if office.is_multi_office().unwrap_or(false) {
                let _ = office.register_replication_consumer("org-sync-hq")?;
                let _ = office.register_replication_consumer("org-sync-message-relay")?;
            }
        }
        crate::model::OfficeRole::Satellite => {
            let _ = office.register_replication_consumer("org-sync-work-outbox")?;
            let _ = office.register_replication_consumer("org-sync-message-relay")?;
        }
    }
    Ok(())
}
