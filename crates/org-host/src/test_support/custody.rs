//! Writer custody for the `agentic-org` real-process tests (`test-support`, debug builds
//! only). Every office needs a valid custody record and its reservation marker to start
//! (`crate::writer`); only protected provisioning reserves them in production, through a
//! browser create. Tests that run stored offices on the fake server have no such create, so
//! [`reserve_office`] makes the reservation provisioning would have made, on the instance's
//! own registry and custody store, while no host runs the instance. What the reservation
//! checks is the production rule, unchanged: canonical UUIDv7 organization and office IDs, an
//! administrator that owns the organization, and an empty office folder.
use std::os::unix::fs::DirBuilderExt;
use std::path::{Path, PathBuf};

use crate::accounts::identity::AccountId;
use crate::home::{HostLock, Layout};
use crate::hostdb::HostDb;
use crate::instance::registry::Registry;
use crate::registered::{create_registry, registry_state, RegistryState};
use crate::writer::{Binding, Writers};

/// Where an instance lives, as its host is started with it.
#[derive(Clone, Debug)]
pub struct InstanceAt {
    locator: PathBuf,
    user_temp: PathBuf,
    name: Option<String>,
    root: PathBuf,
    dev_key_dir: PathBuf,
}

impl InstanceAt {
    /// The instance [`crate::start_instance_for_test`] starts with the same `locator`,
    /// `user_temp`, `name` and `root`, whose host keeps its secrets in `dev_key_dir` (the
    /// config's `dev_key_dir`).
    pub fn new(
        locator: &Path,
        user_temp: &Path,
        name: Option<&str>,
        root: &Path,
        dev_key_dir: &Path,
    ) -> Self {
        Self {
            locator: locator.into(),
            user_temp: user_temp.into(),
            name: name.map(str::to_owned),
            root: root.into(),
            dev_key_dir: dev_key_dir.into(),
        }
    }

    /// The instance `name` that [`crate::setup_flow::fixture::registered_account`] registered
    /// under the application base `base`, served by `agentic-org serve --name` with
    /// `ORG_DEV_ACCOUNT_BASE=<base>` and `ORG_OFFICE_DEV_KEY_DIR=<dev_key_dir>`.
    pub fn account(base: &Path, name: Option<&str>, dev_key_dir: &Path) -> Result<Self, String> {
        let paths = crate::instance::setup::Paths::at(base, base.join("temp"), name)
            .map_err(|e| e.to_string())?;
        let root = paths.instance_root(name).map_err(|e| e.to_string())?;
        Ok(Self {
            locator: paths.locator,
            user_temp: paths.user_temp,
            name: name.map(str::to_owned),
            root,
            dev_key_dir: dev_key_dir.into(),
        })
    }
}

/// The administrator account that owns an organization, and its owner member.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Owner {
    pub account_id: String,
    pub member_id: String,
}

/// The seeded owners' password verifier: hashed once per process (no test signs in as
/// them), so a home with many organizations doesn't pay a full Argon2id hash for each.
fn owner_verifier() -> Result<crate::accounts::verifier::Verifier, String> {
    static PHC: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    let phc = match PHC.get() {
        Some(phc) => phc,
        None => {
            let password = org_host_control::Password::new("synthetic custody password".into())
                .map_err(|_| "an invalid password")?;
            let verifier =
                crate::service::authentication::hash_password(&password).map_err(|e| e.message)?;
            PHC.get_or_init(|| verifier.expose().to_owned())
        }
    };
    crate::accounts::verifier::Verifier::new(phc.clone()).map_err(|_| "an invalid verifier".into())
}

/// Reserves writer custody for `office_id` of `organization_id` (both canonical UUIDv7) on
/// the instance `at`, as protected provisioning does before a create: the record (a fresh
/// journal incarnation, epoch 1, the organization's data roots) and the marker in the
/// office's folder, which is made here if it is missing and must hold nothing yet (the
/// production rule for a first reservation). The instance's registry is created first if
/// its host never started.
///
/// `owner` is the organization's owner from an earlier call (an organization has one); with
/// `None` a new administrator account is seeded and made the owner of `organization_id`.
/// Returns the owner. Call it while no host runs the instance: it takes the host lock.
pub fn reserve_office(
    at: &InstanceAt,
    organization_id: &str,
    office_id: &str,
    owner: Option<&Owner>,
) -> Result<Owner, String> {
    let registry = Registry::open(&at.locator, &at.user_temp).map_err(|e| e.to_string())?;
    let instance = registry
        .reserve(at.name.as_deref(), &at.root)
        .map_err(|e| e.to_string())?;
    let homes = instance.prepare_layout().map_err(|e| e.to_string())?;
    let layout = Layout::with_runtime(&homes.office_home, instance.temp_folder());
    layout.prepare().map_err(|e| e.to_string())?;
    let _lock = HostLock::acquire(&layout).map_err(|e| e.to_string())?;
    match std::fs::DirBuilder::new()
        .mode(0o700)
        .create(&at.dev_key_dir)
    {
        Err(e) if e.kind() != std::io::ErrorKind::AlreadyExists => return Err(e.to_string()),
        _ => {}
    }
    let custody = crate::host_secrets::store_for(
        Some(&at.dev_key_dir),
        instance.root(),
        instance.id(),
        crate::code_signing::own_signing,
        None,
    )
    .map_err(|e| format!("{e:?}"))?;
    if registry_state(&instance, &homes.office_home).map_err(|e| e.to_string())?
        == RegistryState::Absent
    {
        create_registry(&homes.office_home, &*custody, instance.id()).map_err(|e| e.to_string())?;
    }
    instance
        .record_registry_created()
        .map_err(|e| e.to_string())?;
    let db = std::sync::Arc::new(
        HostDb::open(&homes.office_home, &*custody).map_err(|e| format!("{e:?}"))?,
    );
    let owner = match owner {
        Some(owner) => owner.clone(),
        None => {
            let owner = Owner {
                account_id: crate::ids::generate().map_err(|e| e.to_string())?,
                member_id: crate::ids::uuid_v7().map_err(|e| e.to_string())?,
            };
            let verifier = owner_verifier()?;
            // A login of its own per seeded owner (logins are unique).
            let login = format!("owner{}", &owner.account_id[16..]);
            db.seed_account(&owner.account_id, &login, &verifier, true, 1)
                .map_err(|e| format!("{e:?}"))?;
            owner
        }
    };
    let account = AccountId::parse(&owner.account_id).map_err(|_| "an invalid account id")?;
    let administrator = db
        .account_authority(&account)
        .map_err(|e| format!("{e:?}"))?
        .ok_or("the owner account is not in the registry")?;
    db.establish_organization_owner(&administrator, organization_id, &owner.member_id)
        .map_err(|e| format!("{e:?}"))?;
    let writers =
        Writers::open(custody, db, instance.id(), instance.root()).map_err(|e| e.to_string())?;
    let folder = homes.office_home.join("offices").join(office_id);
    std::fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(&folder)
        .map_err(|e| e.to_string())?;
    writers
        .reserve(
            &crate::ids::uuid_v7().map_err(|e| e.to_string())?,
            &Binding {
                organization_id,
                office_id,
                owner_account: &owner.account_id,
                owner_member: &owner.member_id,
            },
        )
        .map_err(|e| format!("reserving writer custody for {office_id}: {e}"))?;
    Ok(owner)
}
