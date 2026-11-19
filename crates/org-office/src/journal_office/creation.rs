//! Creating the HQ: the organization's first journal entry, with its CA records
//! (ADR-0007, `docs/design/office-store.md` §4, data contract document 16).
//!
//! 1. Check the request before touching anything.
//! 2. Take the office lock; freeze the intent (or find the same one, from an earlier
//!    attempt); refuse another request or a folder the old databases made.
//! 3. Open the store: a new journal incarnation when its anchor is not there yet, else
//!    the existing one (an attempt may have died at any point after the anchor).
//! 4. Read the CA records an earlier attempt committed, and hand them to `material`, which
//!    returns them as they are or, when there are none, new ones.
//! 5. Commit the creation (entry 1): durable, so flushed and confirmed by the host.
//! 6. Wait until the entry's files are written, then write `office.json` last.
//!
//! A failure leaves the intent and whatever the journal holds; the same request resumes
//! from there and never makes a second organization or a second CA. Nothing under the data
//! roots is ever removed.

use std::path::PathBuf;

use org_store::{journal_dir, AnchorFacts, Files, Id, Opening, Outcome, StoreError};

use super::{
    check_roots, intent, intent::Intent, recovery, refuse_older_build, storage, Custody, DataRoots,
    Office, Options,
};
use crate::{
    error::OfficeError,
    hq_creation::{Binding, LocalRecord},
    key::Key,
    model::{NewOffice, OfficeInfo, OfficeRole},
    office::{
        check_named_by, check_not_a_link, exists, lock_folder, plain_folder,
        remove_the_empty_folder, LOCK_FILE, LOCK_WAIT,
    },
    office_file,
    office_local::{read_ca, CaMaterial},
    organization::{
        features::{CreateOrganization, Creator},
        operation_id, records,
    },
    private_fs::{private_dir, tighten_dir},
    store::open::{open_store, CreateOrganizationOp, OfficeStanding},
};

/// The prefix of a publication's temporary files (`org_store::publish_one`).
const TEMPORARY: &str = ".store-tmp-";

/// What a creation folder may hold besides its lock.
const CREATION_ENTRIES: [&str; 2] = [intent::FILE, office_file::FILE_NAME];

/// The request, checked as a whole: the binding's bootstrap names this office and
/// organization, its creator is the owner, every ID is a UUIDv7 but the operation, which
/// is the wire's ULID, and the custody is this office's.
fn intent_of(
    new: &NewOffice,
    binding: &Binding,
    roots: &DataRoots,
    custody: &Custody,
    options: &Options,
) -> Result<Intent, OfficeError> {
    check_roots(roots)?;
    let Some(prepared) = binding.bootstrap.as_ref() else {
        return Err(recovery());
    };
    let here = &custody.identity;
    let uuid = org_proto::is_uuid_v7;
    let ok = prepared.valid()
        && prepared.creator.employee_id == binding.owner_member_id
        && uuid(&binding.owner_member_id)
        && operation_id::from_wire_ulid(&binding.operation_id).is_ok()
        && new.role == OfficeRole::Hq
        && uuid(&new.office_id)
        && uuid(&new.org_id)
        && prepared.office_id == new.office_id
        && prepared.organization_id == new.org_id
        && prepared.plan.name == new.org_name
        && uuid(here.instance_id.as_str())
        && here.organization_id.as_str() == new.org_id
        && here.office_id.as_str() == new.office_id;
    if !ok {
        return Err(recovery());
    }
    let intent = Intent {
        version: intent::VERSION,
        instance: here.instance_id.to_string(),
        journal_incarnation: here.journal_incarnation.to_string(),
        writer_epoch: here.writer_epoch,
        machine_id: here.machine_id.to_string(),
        operation: binding.operation_id.clone(),
        owner: binding.owner_member_id.clone(),
        office_id: new.office_id.clone(),
        office_name: new.office_name.trim().to_owned(),
        org_id: new.org_id.clone(),
        org_name: new.org_name.trim().to_owned(),
        created_at: records::time_text(options.clock.now()),
        durable_root: roots.durable.clone(),
        runtime_root: roots.runtime.clone(),
        bootstrap: prepared.clone(),
    };
    // The checks `office.json` will make, before anything is touched.
    office_file::encode(&intent.info())?;
    Ok(intent)
}

/// The organization's creation command, from the frozen request.
fn command(intent: &Intent) -> Result<CreateOrganization, OfficeError> {
    let prepared = &intent.bootstrap;
    let id = |text: &str| Id::parse(text).map_err(|_| recovery());
    Ok(CreateOrganization {
        organization_id: id(&prepared.organization_id)?,
        office_id: id(&prepared.office_id)?,
        repository_id: id(&prepared.repository_id)?,
        team_id: id(&prepared.home_team_id)?,
        creator: Creator {
            employee_id: id(&prepared.creator.employee_id)?,
            name: prepared.creator.name.clone(),
            handle: prepared.creator.handle.clone(),
        },
        hr_employee_id: id(&prepared.hr_employee_id)?,
        name: prepared.plan.name.clone(),
        slug: prepared.plan.slug.clone(),
        repository_remote_url: prepared.plan.repository_remote_url.clone(),
        state_branch: prepared.plan.state_branch.clone(),
        office_name: intent.office_name.clone(),
    })
}

impl Office {
    /// Creates the HQ office and its organization, or completes an earlier attempt of the
    /// same request. `material` runs at most once, outside any store retry: with `None` it
    /// generates the CA; with the records an earlier attempt committed, it must verify and
    /// return them unchanged. Returns once the creation is durable and its files are
    /// written, with `office.json` written last.
    #[allow(clippy::too_many_arguments)]
    pub fn provision_hq(
        folder: impl Into<PathBuf>,
        new: &NewOffice,
        key: Key,
        binding: &Binding,
        roots: DataRoots,
        custody: Custody,
        material: impl FnOnce(Option<LocalRecord>) -> Result<LocalRecord, OfficeError>,
    ) -> Result<Office, OfficeError> {
        let options = Options::production();
        Self::provision_with(
            folder.into(),
            new,
            Some(key),
            binding,
            roots,
            custody,
            material,
            options,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn provision_with(
        folder: PathBuf,
        new: &NewOffice,
        key: Option<Key>,
        binding: &Binding,
        roots: DataRoots,
        custody: Custody,
        material: impl FnOnce(Option<LocalRecord>) -> Result<LocalRecord, OfficeError>,
        options: Options,
    ) -> Result<Office, OfficeError> {
        let request = intent_of(new, binding, &roots, &custody, &options)?;
        let folder = plain_folder(folder)?;
        check_named_by(&folder, &new.office_id)?;
        check_not_a_link(&folder)?;
        private_dir(&folder).map_err(OfficeError::storage)?;
        tighten_dir(&folder).map_err(OfficeError::storage)?;
        let lock = lock_folder(&folder, LOCK_WAIT)?;
        refuse_older_build(&folder)?;

        // Under the lock: the request frozen once, then only resumed.
        let intent = match intent::read(&folder)? {
            Some(frozen) if frozen.same_request(&request) => frozen,
            Some(_) => return Err(OfficeError::OfficeExists),
            None => {
                if exists(&folder.join(office_file::FILE_NAME))? {
                    return Err(OfficeError::OfficeExists);
                }
                // A fresh create finds nothing but its lock and unfinished publications.
                if !holds_only_lock_and_temporaries(&folder)? {
                    return Err(OfficeError::Invalid(
                        "the folder holds something an office does not make",
                    ));
                }
                intent::write(&folder, &request)?;
                request
            }
        };
        let info: OfficeInfo = intent.info();
        if office_file::read(&folder)?.is_some_and(|done| done != info) {
            return Err(recovery());
        }
        let creation = operation_id::from_wire_ulid(&intent.operation).map_err(|_| recovery())?;
        let owner = Id::parse(&intent.owner).map_err(|_| recovery())?;

        // A new journal incarnation until its anchor is written; after that, the one there
        // (an attempt that died after its folders and before its anchor makes them again).
        let anchor = journal_dir(&roots.durable, &custody.identity).join("anchor.json");
        let mut config = options.config(&roots, &custody);
        if !exists(&anchor)? {
            refuse_another_beginning(&roots, &custody)?;
            config.opening = Opening::Create(AnchorFacts::default());
        }
        let store = open_store(&config).map_err(storage)?;

        // The CA an earlier attempt committed: never generated again.
        let here = custody.identity.clone();
        let files = Files::open(&roots.durable).map_err(OfficeError::storage)?;
        let prior = read_ca(&files, &here.office_id).map_err(|_| recovery())?;
        if let Some(records) = &prior {
            records
                .check(&here.organization_id, &here.office_id, &creation)
                .map_err(|_| recovery())?;
        }
        let standing = OfficeStanding {
            office: here.office_id.clone(),
        };
        let (standing, _) = store.read(standing).map_err(storage)?;
        if prior.is_none() && standing.organization.is_some() {
            return Err(recovery());
        }
        let prior = match prior {
            Some(records) => {
                let key = records.key;
                let record = LocalRecord::new(
                    key.organization_id.to_string(),
                    key.operation_id.to_string(),
                    key.public_key,
                    records.package,
                    key.wrapped_ca_key,
                )?;
                Some(record)
            }
            None => None,
        };
        let frozen = material(prior)?;
        if frozen.organization_id != info.org_id || frozen.operation_id != creation.as_str() {
            return Err(recovery());
        }
        let ca = CaMaterial {
            public_key: frozen.public_key,
            package: frozen.package.clone(),
            wrapped_ca_key: frozen.wrapped_ca_key().to_vec(),
        };
        let reply = store.run(CreateOrganizationOp {
            operation_id: creation.clone(),
            here: here.clone(),
            command: command(&intent)?,
            ca,
        });
        match reply {
            Ok(reply) if reply.outcome == Outcome::Completed => {}
            // Refused, or the operation ID holds another creation: never a second one.
            Ok(_) | Err(StoreError::Invalid(_) | StoreError::Conflict) => return Err(recovery()),
            Err(error) => return Err(storage(error)),
        }
        // H1: nobody is told the office exists before its files do.
        store.sync().map_err(storage)?;
        let bytes = office_file::encode(&info)?;
        let office = Office {
            folder,
            info,
            durable: roots.durable,
            identity: here,
            owner,
            creation,
            secrets_key: key,
            store,
            _lock: lock,
            #[cfg(any(test, feature = "testkit"))]
            _test_root: None,
        };
        office.check_standing()?;
        office.ca()?;
        org_store::publish_one(&office.folder, office_file::FILE_NAME, &bytes, true)
            .map_err(OfficeError::storage)?;
        Ok(office)
    }
}

/// Whether `folder` holds nothing but its lock and temporary files of an unfinished
/// publication.
fn holds_only_lock_and_temporaries(folder: &std::path::Path) -> Result<bool, OfficeError> {
    for entry in std::fs::read_dir(folder).map_err(OfficeError::storage)? {
        let entry = entry.map_err(OfficeError::storage)?;
        let name = entry.file_name().to_string_lossy().into_owned();
        let file = entry.file_type().map_err(OfficeError::storage)?.is_file();
        if !(name == LOCK_FILE || (file && name.starts_with(TEMPORARY))) {
            return Ok(false);
        }
    }
    Ok(true)
}

/// A new journal incarnation begins the organization's data only where nothing else has:
/// no other incarnation of this office's journal, and no organization record. A second
/// anchor beside another could never be removed (nothing under `durable/` is).
fn refuse_another_beginning(roots: &DataRoots, custody: &Custody) -> Result<(), OfficeError> {
    let here = &custody.identity;
    let ours = journal_dir(&roots.durable, here);
    let journals = ours
        .parent()
        .expect("an incarnation is in its office's journals");
    match std::fs::read_dir(journals) {
        Ok(entries) => {
            for entry in entries {
                let entry = entry.map_err(OfficeError::storage)?;
                if entry.file_name().to_str() != Some(here.journal_incarnation.as_str()) {
                    return Err(OfficeError::Invalid(
                        "the data roots hold another journal of this office",
                    ));
                }
            }
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(OfficeError::storage(e)),
    }
    let organization = roots.durable.join(records::organization_path().as_str());
    if exists(&organization)? {
        let there = std::io::Error::new(
            std::io::ErrorKind::AlreadyExists,
            format!(
                "{} is there before the organization's creation",
                organization.display()
            ),
        );
        return Err(OfficeError::storage(there));
    }
    Ok(())
}

/// [`Office::discard`].
pub(super) fn discard(folder: PathBuf, office_id: &str) -> Result<(), OfficeError> {
    if !org_proto::is_uuid_v7(office_id) {
        return Err(OfficeError::Invalid("an office id is a UUIDv7"));
    }
    let folder = plain_folder(folder)?;
    check_named_by(&folder, office_id)?;
    check_not_a_link(&folder)?;
    if !exists(&folder)? {
        return Ok(());
    }
    let _lock = lock_folder(&folder, LOCK_WAIT)?;
    for entry in CREATION_ENTRIES {
        if exists(&folder.join(entry))? {
            return Err(OfficeError::OfficeExists);
        }
    }
    // Only the lock and the temporary files of an unfinished publication may be there.
    let entries = std::fs::read_dir(&folder).map_err(OfficeError::storage)?;
    let mut temporaries = Vec::new();
    for entry in entries {
        let entry = entry.map_err(OfficeError::storage)?;
        let name = entry.file_name().to_string_lossy().into_owned();
        let kind = entry.file_type().map_err(OfficeError::storage)?;
        if name == LOCK_FILE {
            continue;
        }
        if !(name.starts_with(TEMPORARY) && kind.is_file()) {
            return Err(OfficeError::Invalid(
                "the folder holds something an office does not make",
            ));
        }
        temporaries.push(entry.path());
    }
    for temporary in temporaries {
        std::fs::remove_file(temporary).map_err(OfficeError::storage)?;
    }
    remove_the_empty_folder(&folder)
}
