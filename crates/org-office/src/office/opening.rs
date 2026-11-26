//! Opening an office: the open binding, root and custody checks, the companion stores and the
//! journal consumers registered at startup.

use std::{
    path::{Path, PathBuf},
    sync::{atomic::AtomicU64, Arc},
};

use org_store::{Id, OfficeIdentity};

use crate::{
    error::OfficeError,
    key::Key,
    model::OfficeInfo,
    office_file,
    store::open::{open_store, DirectorySnapshot, EngineChoice, OfficeStanding, OfficeStore},
};

use super::folder::LOCK_WAIT;
use super::folder::{
    check_named_by, check_not_a_link, lock_folder, plain_folder, refuse_older_build,
};
use super::{intent, recovery, satellite, storage, Custody, DataRoots, Office, Options};

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

pub(crate) struct CompanionStores {
    pub private_notes: Arc<dyn crate::private_notes::PrivateNotesStore>,
    pub accounting: Arc<dyn crate::accounting::AccountingStore>,
    pub budgets: Arc<dyn crate::budgets::BudgetStore>,
    pub providers: Arc<dyn crate::providers::ProviderPoolStore>,
    pub replication: Arc<dyn crate::replication::ReplicationStore>,
}

pub(crate) struct CompanionOpenSpec<'a> {
    pub engine: EngineChoice,
    pub roots: &'a DataRoots,
    pub info: &'a OfficeInfo,
    pub identity: &'a OfficeIdentity,
    pub hq_office_id: &'a str,
    pub ca_digest: org_sync_proto::Digest,
    pub epoch: u64,
    pub initial_snapshot_required: bool,
}

pub(crate) fn open_companion_stores(
    spec: CompanionOpenSpec<'_>,
) -> Result<CompanionStores, OfficeError> {
    let is_hq = spec.info.role == crate::model::OfficeRole::Hq;
    Ok(CompanionStores {
        private_notes: crate::store::open::open_private_notes(
            spec.engine,
            &spec.roots.durable,
            is_hq,
        )?,
        accounting: crate::store::open::open_accounting(
            spec.engine,
            &spec.roots.durable,
            &spec.roots.runtime,
            &spec.info.office_id,
            &spec.info.org_id,
        )?,
        budgets: crate::store::open::open_budgets(
            spec.engine,
            &spec.roots.durable,
            &spec.roots.runtime,
            &spec.info.office_id,
            &spec.info.org_id,
            is_hq,
        )?,
        providers: crate::store::open::open_providers(
            spec.engine,
            &spec.roots.durable,
            &spec.roots.runtime,
            &spec.info.office_id,
            &spec.info.org_id,
            is_hq,
        )?,
        replication: crate::store::open::open_replication(
            crate::store::open::ReplicationOpenSpec {
                engine: spec.engine,
                durable_root: &spec.roots.durable,
                runtime_root: &spec.roots.runtime,
                org_id: &spec.info.org_id,
                local_office: &spec.info.office_id,
                hq_office: spec.hq_office_id,
                ca_digest: spec.ca_digest,
                epoch: spec.epoch,
                incarnation: spec.identity.journal_incarnation.as_str(),
                initial_snapshot_required: spec.initial_snapshot_required,
            },
        )?,
    })
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

/// Roots that are absolute and plain: no `.` or `..`, and two different folders.
pub(crate) fn check_roots(roots: &DataRoots) -> Result<(), OfficeError> {
    let plain = |path: &Path| {
        path.is_absolute()
            && path.to_str().is_some()
            && path.components().all(|c| {
                matches!(
                    c,
                    std::path::Component::RootDir | std::path::Component::Normal(_)
                )
            })
    };
    if !plain(&roots.durable) || !plain(&roots.runtime) || roots.durable == roots.runtime {
        return Err(OfficeError::Invalid(
            "the data roots are not two plain absolute folders",
        ));
    }
    // The host makes both, owner-only, when it reserves the office's custody.
    for root in [&roots.durable, &roots.runtime] {
        match std::fs::symlink_metadata(root) {
            Ok(meta) if meta.is_dir() => {}
            Ok(_) => return Err(OfficeError::Invalid("a data root is not a folder")),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return Err(OfficeError::Invalid("a data root is not there"));
            }
            Err(e) => return Err(OfficeError::storage(e)),
        }
    }
    Ok(())
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
        let clock_high_ms = AtomicU64::new(options.clock.now().0);
        let office = Office {
            folder,
            info,
            durable: roots.durable,
            runtime_root: roots.runtime,
            identity: custody.identity,
            owner,
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
