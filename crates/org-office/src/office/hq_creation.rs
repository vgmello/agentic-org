//! Bound HQ preparation. The protected host owns admission; this local intent is
//! only crash continuity, never a substitute for its custody/epoch verifier.
use super::*;
use crate::hq_creation::{Binding, LocalRecord};
use crate::store::{acceptance::durable, hq_creation as storage};
use serde::{Deserialize, Serialize};

pub(super) const INTENT: &str = "hq-creation.v1.json";
const INTENT_LIMIT: usize = 16 * 1024;
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Intent {
    version: u32,
    instance: String,
    operation: String,
    owner: String,
    office_id: String,
    office_name: String,
    org_id: String,
    org_name: String,
    created_at: String,
    public_root: PathBuf,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    bootstrap: Option<org_proto::selected_bootstrap::PreparedV1>,
}
fn invalid() -> OfficeError {
    OfficeError::Invalid("HQ provisioning requires recovery")
}
impl Intent {
    fn info(&self) -> OfficeInfo {
        OfficeInfo {
            office_id: self.office_id.clone(),
            office_name: self.office_name.clone(),
            role: crate::OfficeRole::Hq,
            org_id: self.org_id.clone(),
            org_name: self.org_name.clone(),
            created_at: self.created_at.clone(),
        }
    }
    fn matches(&self, new: &NewOffice, instance: &str, binding: &Binding) -> bool {
        self.version == 1
            && self.instance == instance
            && self.operation == binding.operation_id
            && self.owner == binding.owner_member_id
            && self.public_root == binding.public_root
            && self.bootstrap == binding.bootstrap
            && binding.bootstrap.as_ref().is_none_or(|b| {
                b.organization_id == new.org_id
                    && b.office_id == new.office_id
                    && b.plan.name == new.org_name
            })
            && self.office_id == new.office_id
            && self.office_name == new.office_name
            && self.org_id == new.org_id
            && self.org_name == new.org_name
            && new.role == crate::OfficeRole::Hq
    }
}
impl Office {
    /// Creation-v1 adapter, invoked only by the protected host launch. The factory
    /// runs at most once, outside retryable writer callbacks; on retry it receives
    /// the exact committed material and must verify it, never replace it. This
    /// method does not install or advertise selected-task readiness.
    pub fn provision_hq_for_instance(
        folder: impl Into<PathBuf>,
        new: &NewOffice,
        key: Key,
        instance: &str,
        binding: &Binding,
        material: impl FnOnce(Option<LocalRecord>) -> Result<LocalRecord, OfficeError>,
    ) -> Result<Self, OfficeError> {
        let provisional = validate_creation_request(new, instance, binding)?;
        let folder = plain_folder(folder.into())?;
        validate_public_root(&folder, &binding.public_root)?;
        check_named_by(&folder, &new.office_id)?;
        check_not_a_link(&folder)?;
        private_dir(&folder).map_err(OfficeError::storage)?;
        tighten_dir(&folder).map_err(OfficeError::storage)?;
        let lock = lock_folder(&folder, LOCK_WAIT)?;
        let intent = retained_intent(&folder, new, instance, binding, provisional.created_at)?;
        if !intent.matches(new, instance, binding) {
            return Err(invalid());
        }
        let info = intent.info();
        office_file::encode(&info)?;
        let completed = office_file::read(&folder)?;
        if completed.as_ref().is_some_and(|prior| prior != &info) {
            return Err(invalid());
        }
        verify_completed_history(&folder, completed.is_some())?;
        failpoint(&folder, "hq_after_intent")?;
        let (org, local) =
            storage::open_preparing(&folder, instance, &info.office_id, &info.org_id)?;
        failpoint(&folder, "hq_before_material")?;
        let prior = local.read(storage::read)?;
        // Binding and CA completion were one initial writer transaction. A
        // retained binding/created marker without its material is loss/corruption,
        // never permission to generate a new CA identity.
        if prior.is_none()
            && (completed.is_some()
                || local.meta("hq_creation_operation")?.is_some()
                || org.meta("organization_id")?.is_some())
        {
            return Err(invalid());
        }
        // This may derive/decrypt with Argon2. It is deliberately outside all SQL
        // retry callbacks and before completion publication.
        let frozen = material(prior)?;
        if frozen.organization_id != info.org_id || frozen.operation_id != binding.operation_id {
            return Err(invalid());
        }
        failpoint(&folder, "hq_before_material_commit")?;
        storage::initialize_bound(&local, binding, &info.office_id, frozen)?;
        failpoint(&folder, "hq_after_material")?;
        org_db::prepare_hq_genesis(&org, &info, binding)?;
        failpoint(&folder, "hq_before_public_escrow")?;
        publish_public(&folder, &local, binding)?;
        failpoint(&folder, "hq_after_public_escrow")?;
        let workspace = folder.join(WORKSPACE);
        check_not_a_link(&workspace)?;
        private_dir(&workspace).map_err(OfficeError::storage)?;
        if !fs::symlink_metadata(&workspace)
            .map_err(OfficeError::storage)?
            .is_dir()
        {
            return Err(invalid());
        }
        let stores = stores(org, local, &info.office_id)?;
        failpoint(&folder, "hq_before_created")?;
        durable::publish(
            &folder,
            office_file::FILE_NAME,
            &office_file::encode(&info)?,
            true,
            None,
        )
        .map_err(OfficeError::storage)?;
        Ok(Self {
            folder,
            info,
            stores,
            changes: ChangeSignal::default(),
            secrets_key: Some(key),
            _lock: OfficeLock(lock),
        })
    }
}

/// Created or retained admission proves native history must already exist.
/// Validate before preparation can initialize organization storage again.
fn verify_completed_history(folder: &Path, completed: bool) -> Result<(), OfficeError> {
    if !completed && !storage::has_selected_history(&folder.join(OFFICE_DB))? {
        return Ok(());
    }
    let path = folder.join(ORG_DB);
    let metadata = fs::symlink_metadata(&path).map_err(OfficeError::storage)?;
    if !metadata.is_file() || metadata.file_type().is_symlink() || metadata.len() == 0 {
        return Err(invalid());
    }
    if !Store::inspect(&path, org_db::is_committed)? {
        return Err(invalid());
    }
    Ok(())
}

/// Validate the complete creation request before inspecting or creating files.
/// Retain the provisional timestamp before acquiring the office lock.
fn validate_creation_request(
    new: &NewOffice,
    instance: &str,
    binding: &Binding,
) -> Result<OfficeInfo, OfficeError> {
    binding.validate()?;
    if org_proto::is_uuid_v7(&new.office_id)
        && binding.bootstrap.as_ref().is_none_or(|prepared| {
            prepared.office_id != new.office_id
                || prepared.organization_id != new.org_id
                || prepared.plan.name != new.org_name
        })
    {
        return Err(invalid());
    }
    validate_instance(Some(instance))?;
    let provisional = OfficeInfo {
        office_id: new.office_id.clone(),
        office_name: new.office_name.clone(),
        role: new.role,
        org_id: new.org_id.clone(),
        org_name: new.org_name.clone(),
        created_at: time::now_rfc3339(),
    };
    office_file::encode(&provisional)?;
    if new.role != crate::OfficeRole::Hq {
        return Err(invalid());
    }
    Ok(provisional)
}

fn retained_intent(
    folder: &Path,
    new: &NewOffice,
    instance: &str,
    binding: &Binding,
    created_at: String,
) -> Result<Intent, OfficeError> {
    let intent = match durable::read(folder, INTENT, INTENT_LIMIT) {
        Ok(bytes) => serde_json::from_slice::<Intent>(&bytes).map_err(|_| invalid())?,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            // Never attach a new operation to a legacy, foreign or partially
            // retained office. Its original bytes remain for explicit adoption.
            for entry in OFFICE_ENTRIES {
                if exists(&folder.join(entry))? {
                    return Err(OfficeError::OfficeExists);
                }
            }
            let intent = Intent {
                version: 1,
                instance: instance.into(),
                operation: binding.operation_id.clone(),
                owner: binding.owner_member_id.clone(),
                office_id: new.office_id.clone(),
                office_name: new.office_name.clone(),
                org_id: new.org_id.clone(),
                org_name: new.org_name.clone(),
                created_at,
                public_root: binding.public_root.clone(),
                bootstrap: binding.bootstrap.clone(),
            };
            let bytes = serde_json::to_vec(&intent).map_err(OfficeError::storage)?;
            if bytes.len() > INTENT_LIMIT {
                return Err(invalid());
            }
            durable::publish(folder, INTENT, &bytes, true, None).map_err(OfficeError::storage)?;
            intent
        }
        Err(e) => return Err(OfficeError::storage(e)),
    };
    Ok(intent)
}

pub(crate) fn verify_intent(
    folder: &Path,
    info: &OfficeInfo,
    instance: Option<&str>,
) -> Result<Option<Binding>, OfficeError> {
    let bytes = match durable::read(folder, INTENT, INTENT_LIMIT) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return if org_proto::is_uuid_v7(&info.office_id) {
                Err(invalid())
            } else {
                Ok(None)
            };
        }
        Err(e) => return Err(OfficeError::storage(e)),
    };
    let intent: Intent = serde_json::from_slice(&bytes).map_err(|_| invalid())?;
    if intent.version != 1 || instance != Some(intent.instance.as_str()) || intent.info() != *info {
        return Err(invalid());
    }
    let binding = Binding {
        operation_id: intent.operation,
        owner_member_id: intent.owner,
        public_root: intent.public_root,
        bootstrap: intent.bootstrap,
    };
    binding.validate()?;
    validate_retained_bootstrap(info, &binding)?;
    Ok(Some(binding))
}

fn validate_retained_bootstrap(info: &OfficeInfo, binding: &Binding) -> Result<(), OfficeError> {
    match &binding.bootstrap {
        Some(prepared)
            if prepared.valid()
                && prepared.office_id == info.office_id
                && prepared.organization_id == info.org_id
                && prepared.plan.name == info.org_name =>
        {
            Ok(())
        }
        None if !org_proto::is_uuid_v7(&info.office_id) => Ok(()),
        _ => Err(invalid()),
    }
}

pub(crate) fn verify_retained_binding(
    folder: &Path,
    info: &OfficeInfo,
    instance: Option<&str>,
    store: &Store,
) -> Result<Option<Binding>, OfficeError> {
    let binding = verify_intent(folder, info, instance)?;
    let Some(binding) = binding else {
        let retained: bool = store.read(|db| {
            db.query_row("SELECT EXISTS(SELECT 1 FROM local_hq_creation) OR EXISTS(SELECT 1 FROM meta WHERE key IN ('hq_creation_operation','hq_creation_owner'))", [], |r| r.get(0)).map_err(OfficeError::storage)
        })?;
        return if retained { Err(invalid()) } else { Ok(None) };
    };
    storage::verify_binding(store, &binding)?;
    let record = store.read(storage::read_public)?.ok_or_else(invalid)?;
    if info.role != crate::OfficeRole::Hq
        || record.organization_id != info.org_id
        || record.operation_id != binding.operation_id
    {
        return Err(invalid());
    }
    Ok(Some(binding))
}

impl Office {
    pub(crate) fn retained_hq_creation_binding(&self) -> Result<Option<Binding>, OfficeError> {
        let instance = self.local_store().meta("instance_id")?;
        verify_retained_binding(
            self.folder(),
            self.info(),
            instance.as_deref(),
            self.local_store(),
        )
    }

    pub(crate) fn validate_hq_creation_root(
        &self,
        config: &crate::AcceptanceConfig,
    ) -> Result<Option<Binding>, OfficeError> {
        let binding = self.retained_hq_creation_binding()?;
        if let Some(binding) = &binding {
            if config.public_root != binding.public_root
                || config.identity.office_id != self.info().office_id
                || config.identity.organization_id != self.info().org_id
                || Some(config.identity.instance_id.clone())
                    != self.local_store().meta("instance_id")?
            {
                return Err(invalid());
            }
            let record = self
                .local_store()
                .read(storage::read_public)?
                .ok_or_else(invalid)?;
            verify_public(binding, &record)?;
        }
        Ok(binding)
    }

    /// Reject inconsistent creation context before installing protected acceptance.
    /// This confers no writer or owner authority; live control remains mandatory.
    pub fn validate_hq_creation_admission(
        &self,
        config: &crate::AcceptanceConfig,
        owner_member: &str,
    ) -> Result<(), OfficeError> {
        if self
            .validate_hq_creation_root(config)?
            .is_some_and(|binding| binding.owner_member_id != owner_member)
        {
            return Err(invalid());
        }
        Ok(())
    }
}

const PUBLIC_ESCROW: &str = "security/ca-escrow.v1.bin";
fn validate_public_root(folder: &Path, root: &Path) -> Result<(), OfficeError> {
    let office_path = match fs::canonicalize(folder) {
        Ok(path) => path,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            fs::canonicalize(folder.parent().ok_or_else(invalid)?)
                .map_err(OfficeError::storage)?
                .join(folder.file_name().ok_or_else(invalid)?)
        }
        Err(e) => return Err(OfficeError::storage(e)),
    };
    if !root.is_absolute()
        || root.starts_with(&office_path)
        || office_path.starts_with(root)
        || fs::canonicalize(root).map_err(OfficeError::storage)? != root
        || !fs::metadata(root).map_err(OfficeError::storage)?.is_dir()
    {
        return Err(invalid());
    }
    Ok(())
}
pub(crate) fn publish_public(
    folder: &Path,
    local: &Store,
    binding: &Binding,
) -> Result<(), OfficeError> {
    validate_public_root(folder, &binding.public_root)?;
    let record = local.read(storage::read_public)?.ok_or_else(invalid)?;
    if record.operation_id != binding.operation_id {
        return Err(invalid());
    }
    durable::publish(
        &binding.public_root,
        PUBLIC_ESCROW,
        &record.package,
        true,
        None,
    )
    .map_err(OfficeError::storage)
}
pub(crate) fn verify_public(
    binding: &Binding,
    record: &crate::hq_creation::PublicRecord,
) -> Result<(), OfficeError> {
    if fs::canonicalize(&binding.public_root).map_err(OfficeError::storage)? != binding.public_root
    {
        return Err(invalid());
    }
    let copy =
        durable::read(&binding.public_root, PUBLIC_ESCROW, 4475).map_err(OfficeError::storage)?;
    if record.version != 1 || record.operation_id != binding.operation_id || copy != record.package
    {
        return Err(invalid());
    }
    Ok(())
}

#[cfg(test)]
mod tests;
