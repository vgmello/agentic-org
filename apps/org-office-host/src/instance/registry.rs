use std::collections::BTreeMap;
use std::fs::File;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::directory::{absolute, paths_overlap, ControlFile, Directory, Identity};
use super::temp;
use super::{Error, Name};

const REGISTRY: &str = "instances.json";
const CONFIG: &str = "instance.json";
const LOCK: &str = "registry.lock";
const MAX_BYTES: usize = 64 * 1024;
const MAX_INSTANCES: usize = 256;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Binding {
    name: Name,
    id: String,
    root: PathBuf,
    identity: Identity,
    temp_handle: String,
    temp_folder: PathBuf,
}

impl Binding {
    fn validate(&self) -> Result<(), Error> {
        Name::parse(Some(self.name.as_str()))?;
        if org_proto::ulid_time_ms(&self.id).is_none() {
            return Err(Error::Unsafe("instance ID is not a canonical ULID"));
        }
        if absolute(&self.root)? != self.root {
            return Err(Error::Unsafe("registered root is not normalized"));
        }
        if !temp::valid(&self.temp_handle) {
            return Err(Error::Unsafe("invalid instance temp handle"));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Phase {
    Reserved,
    Ready,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Entry {
    binding: Binding,
    phase: Phase,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Locator {
    version: u32,
    user_temp: PathBuf,
    entries: Vec<Entry>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Config {
    version: u32,
    binding: Binding,
    #[serde(deserialize_with = "office_handles")]
    office_temp_handles: BTreeMap<String, String>,
}

fn office_handles<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<BTreeMap<String, String>, D::Error> {
    struct Handles;
    impl<'de> serde::de::Visitor<'de> for Handles {
        type Value = BTreeMap<String, String>;
        fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            formatter.write_str("distinct office temp handle entries")
        }
        fn visit_map<A: serde::de::MapAccess<'de>>(
            self,
            mut entries: A,
        ) -> Result<Self::Value, A::Error> {
            let mut result = BTreeMap::new();
            while let Some((office, handle)) = entries.next_entry::<String, String>()? {
                if result.insert(office, handle).is_some() {
                    return Err(serde::de::Error::custom("duplicate office temp entry"));
                }
            }
            Ok(result)
        }
    }
    deserializer.deserialize_map(Handles)
}

impl Config {
    fn validate_rebind(&self, binding: &Binding, actual: Identity) -> Result<(), Error> {
        if self.binding.identity != binding.identity && self.binding.identity != actual {
            return Err(Error::Unsafe(
                "rebind found an unexpected configuration identity",
            ));
        }
        let mut expected = binding.clone();
        expected.identity = self.binding.identity;
        self.validate(&expected)
    }

    fn validate(&self, binding: &Binding) -> Result<(), Error> {
        if self.version != 1 || &self.binding != binding {
            return Err(Error::Unsafe(
                "instance configuration no longer matches its reservation",
            ));
        }
        let mut handles = std::collections::BTreeSet::new();
        for (office, handle) in &self.office_temp_handles {
            if !org_office_launch::is_plain_id(office)
                || !temp::valid(handle)
                || !handles.insert(handle)
            {
                return Err(Error::Unsafe("invalid or repeated office temp handle"));
            }
        }
        Ok(())
    }
}

fn decode<T: serde::de::DeserializeOwned>(bytes: &[u8]) -> Result<T, Error> {
    // Never include parser text: it may quote arbitrary control-file contents.
    serde_json::from_slice(bytes)
        .map_err(|_| Error::Unsafe("invalid control file; resume or repair"))
}

fn encode(value: &impl Serialize) -> Result<Vec<u8>, Error> {
    let bytes =
        serde_json::to_vec(value).map_err(|_| Error::Unsafe("control file cannot be encoded"))?;
    if bytes.len() > MAX_BYTES {
        return Err(Error::Invalid(
            "instance locator has reached its size limit",
        ));
    }
    Ok(bytes)
}

/// Per-OS-user instance location only, never account credentials or office data.
pub(crate) struct Registry {
    directory: Directory,
    user_temp: Directory,
}

/// A reserved/opened instance, holding the actual full root's exclusive inode lock.
pub(crate) struct Instance {
    root: Directory,
    binding: Binding,
    user_temp: Directory,
}

/// Managed roots prepared while the instance owns its full-root lock.
pub(crate) struct InstancePaths {
    pub(crate) office_home: PathBuf,
    pub(crate) agents: PathBuf,
}

/// Confirmation is bound to these values. It must use OS presence or an interactive TTY.
pub(crate) struct RebindPreview {
    binding: Binding,
    actual: Identity,
}

impl RebindPreview {
    pub(crate) fn description(&self) -> String {
        format!(
            "Rebind instance {} ({}) at {:?}: {:?} -> {:?}",
            self.binding.name.as_str(),
            self.binding.id,
            self.binding.root,
            self.binding.identity,
            self.actual,
        )
    }
}

struct RebindSnapshot {
    phase: Phase,
    config: ControlFile,
    // Pins the observed directory inode without holding its lock during confirmation.
    _root: Directory,
}

impl Instance {
    pub(crate) fn id(&self) -> &str {
        &self.binding.id
    }
    pub(crate) fn name(&self) -> &str {
        self.binding.name.as_str()
    }
    pub(crate) fn root(&self) -> &Path {
        self.root.path()
    }

    pub(crate) fn prepare_layout(&self) -> Result<InstancePaths, Error> {
        self.config()?;
        let office_home = self.root.child("office-state", true)?;
        office_home.child(org_office_launch::OFFICES_DIR_NAME, true)?;
        office_home.child("logs", true)?;
        let agents = self.root.child("agents", true)?;
        self.verify()?;
        Ok(InstancePaths {
            office_home: office_home.path().to_owned(),
            agents: agents.path().to_owned(),
        })
    }

    fn config(&self) -> Result<(Config, ControlFile), Error> {
        self.root.verify()?;
        let bytes = self
            .root
            .read(CONFIG, MAX_BYTES)?
            .ok_or(Error::Unsafe("ready instance configuration is missing"))?;
        let config: Config = decode(&bytes)?;
        config.validate(&self.binding)?;
        Ok((config, bytes))
    }

    pub(crate) fn verify(&self) -> Result<(), Error> {
        self.config()?;
        self.instance_temp()?.verify()?;
        Ok(())
    }

    pub(crate) fn temp_folder(&self) -> &Path {
        &self.binding.temp_folder
    }

    fn instance_temp(&self) -> Result<Directory, Error> {
        self.user_temp.verify()?;
        self.user_temp
            .child("ao", false)?
            .child(&self.binding.temp_handle, false)
    }

    /// Allocates once under this instance; callers backfill every catalog office before launch.
    pub(crate) fn allocate_office_temp(&mut self, office: &str) -> Result<PathBuf, Error> {
        if !org_office_launch::is_plain_id(office) {
            return Err(Error::Invalid("office ID must be plain"));
        }
        let (mut config, previous) = self.config()?;
        let parent = self.instance_temp()?;
        if let Some(handle) = config.office_temp_handles.get(office) {
            let directory = parent.child(handle, true)?;
            return Ok(directory.path().to_owned());
        }
        let (handle, directory) = temp::allocate(
            &parent,
            config.office_temp_handles.values().map(String::as_str),
        )?;
        config.office_temp_handles.insert(office.to_owned(), handle);
        self.root
            .publish(CONFIG, &encode(&config)?, Some(&previous))?;
        Ok(directory.path().to_owned())
    }
}

impl Registry {
    /// Library use case only until org-setup can expose the signed/interactive CLI.
    /// Preflight and confirmation hold no locks. Revalidation takes registry, then root.
    pub(crate) fn rebind(
        &self,
        name: Option<&str>,
        confirm: impl FnOnce(&RebindPreview) -> Result<(), Error>,
    ) -> Result<(), Error> {
        self.rebind_after_config(name, confirm, || Ok(()))
    }

    fn rebind_after_config(
        &self,
        name: Option<&str>,
        confirm: impl FnOnce(&RebindPreview) -> Result<(), Error>,
        after_config: impl FnOnce() -> Result<(), Error>,
    ) -> Result<(), Error> {
        let name = Name::parse(name)?;
        let (locator, _) = self.load()?;
        let entry = locator
            .entries
            .iter()
            .find(|entry| entry.binding.name == name)
            .ok_or(Error::Invalid("instance is not registered"))?;
        let root = Directory::open_root(&entry.binding.root, false)?;
        let config_file = root.read(CONFIG, MAX_BYTES)?.ok_or(Error::Unsafe(
            "rebind requires the existing in-root configuration",
        ))?;
        let config: Config = decode(&config_file)?;
        config.validate_rebind(&entry.binding, root.identity())?;
        let preview = RebindPreview {
            binding: entry.binding.clone(),
            actual: root.identity(),
        };
        let snapshot = RebindSnapshot {
            phase: entry.phase,
            config: config_file,
            _root: root,
        };
        confirm(&preview)?;

        // Canonical order; no root lock is retained across the human confirmation.
        self.directory.verify()?;
        let lock = self.directory.lock_file(LOCK)?;
        let (mut locator, previous) = self.load()?;
        let changed = || Error::RebindChanged {
            name: name.as_str().to_owned(),
            path: preview.binding.root.clone(),
        };
        let index = locator
            .entries
            .iter()
            .position(|entry| entry.binding.name == name)
            .ok_or_else(changed)?;
        if locator.entries[index].binding != preview.binding
            || locator.entries[index].phase != snapshot.phase
        {
            return Err(changed());
        }
        let mut root = Directory::open_root(&preview.binding.root, false)?;
        root.lock_root()?;
        root.verify()?;
        if root.identity() != preview.actual {
            return Err(changed());
        }
        let current = root.read(CONFIG, MAX_BYTES)?.ok_or_else(changed)?;
        if !snapshot.config.same_version(&current)? {
            return Err(changed());
        }
        let mut config: Config = decode(&current)?;
        config.validate_rebind(&preview.binding, preview.actual)?;
        // D-B22: these observed-identity checks narrow the window, not atomic CAS.
        if config.binding.identity != preview.actual {
            config.binding.identity = preview.actual;
            root.publish(CONFIG, &encode(&config)?, Some(&current))?;
        }
        after_config()?;
        if locator.entries[index].binding.identity != preview.actual {
            locator.entries[index].binding.identity = preview.actual;
            self.save(&locator, &lock, previous.as_ref())?;
        }
        self.directory.verify_file(LOCK, &lock)?;
        root.verify()
    }

    /// Only the final directory may be created; callers resolve the trusted account base.
    pub(crate) fn open(path: &Path, user_temp: &Path) -> Result<Self, Error> {
        let user_temp = Directory::open(user_temp, false)?;
        temp::validate_base(&user_temp)?;
        let path = absolute(path)?;
        if user_temp.contains_path(&path)? {
            return Err(Error::Conflict(
                "locator and user temp trees must be disjoint",
            ));
        }
        let directory = Directory::open(&path, true)?;
        if directory.contains_path(user_temp.path())? {
            return Err(Error::Conflict(
                "locator and user temp trees must be disjoint",
            ));
        }
        Ok(Self {
            directory,
            user_temp,
        })
    }

    /// Startup re-creates all recorded instance folders before sessions build deny lists.
    /// The returned canonical paths are a snapshot taken under the locator lock.
    pub(crate) fn restore_temp_folders(&self) -> Result<Vec<PathBuf>, Error> {
        let lock = self.directory.lock_file(LOCK)?;
        let (locator, _previous) = self.load()?;
        self.user_temp.verify()?;
        let parent = self.user_temp.child("ao", true)?;
        let mut paths = Vec::new();
        for entry in locator.entries {
            let directory = parent.child(&entry.binding.temp_handle, true)?;
            paths.push(directory.path().to_owned());
        }
        self.directory.verify_file(LOCK, &lock)?;
        Ok(paths)
    }

    /// Reserve once or reopen the same identity. Unknown nonempty roots are never adopted.
    ///
    /// The registry lock serializes name/ID publication; the returned instance holds
    /// its root lock independently, so unrelated instances can run concurrently.
    pub(crate) fn reserve(&self, name: Option<&str>, root: &Path) -> Result<Instance, Error> {
        let name = Name::parse(name)?;
        let path = absolute(root)?;
        self.directory.verify()?;
        self.user_temp.verify()?;
        let registry_lock = self.directory.lock_file(LOCK)?;
        let (mut locator, mut previous) = self.load()?;
        let mut root = match self.check_roots(&locator, &name, &path)? {
            Some(root) => root,
            None => Directory::open_root(&path, true)?,
        };
        root.lock_root()?;
        root.verify()?;

        let index = match locator.entries.iter().position(|e| e.binding.name == name) {
            Some(index) => {
                let binding = &locator.entries[index].binding;
                if binding.root != path || binding.identity != root.identity() {
                    return Err(Error::Conflict(
                        "this name is already bound to a different root",
                    ));
                }
                index
            }
            None => {
                if locator.entries.len() >= MAX_INSTANCES {
                    return Err(Error::Invalid(
                        "instance locator has reached its entry limit",
                    ));
                }
                if !root.is_empty()? {
                    return Err(Error::Conflict("unregistered root contains existing state"));
                }
                let id = crate::ids::generate()?;
                if locator.entries.iter().any(|e| e.binding.id == id) {
                    return Err(Error::Conflict(
                        "generated instance ID is already reserved; retry",
                    ));
                }
                let temp_parent = self.user_temp.child("ao", true)?;
                let (temp_handle, temp_folder) = temp::allocate(
                    &temp_parent,
                    locator
                        .entries
                        .iter()
                        .map(|entry| entry.binding.temp_handle.as_str()),
                )?;
                locator.entries.push(Entry {
                    binding: Binding {
                        name,
                        id,
                        root: path,
                        identity: root.identity(),
                        temp_handle,
                        temp_folder: temp_folder.path().to_owned(),
                    },
                    phase: Phase::Reserved,
                });
                previous = Some(self.save(&locator, &registry_lock, previous.as_ref())?);
                locator.entries.len() - 1
            }
        };
        self.finish(locator, index, root, &registry_lock, previous.as_ref())
    }

    fn load(&self) -> Result<(Locator, Option<ControlFile>), Error> {
        let previous = self.directory.read(REGISTRY, MAX_BYTES)?;
        let locator = match previous.as_ref() {
            Some(bytes) => decode::<Locator>(bytes)?,
            None => Locator {
                version: 1,
                user_temp: self.user_temp.path().to_owned(),
                entries: Vec::new(),
            },
        };
        if locator.version != 1 || locator.entries.len() > MAX_INSTANCES {
            return Err(Error::Unsafe("unsupported locator version or entry count"));
        }
        if locator.user_temp != self.user_temp.path() {
            return Err(Error::Unsafe(
                "user temp base differs from the locator's recorded base",
            ));
        }
        for (index, entry) in locator.entries.iter().enumerate() {
            entry.binding.validate()?;
            if entry.binding.temp_folder
                != self
                    .user_temp
                    .path()
                    .join("ao")
                    .join(&entry.binding.temp_handle)
            {
                return Err(Error::Unsafe(
                    "instance temp folder differs from registered user base",
                ));
            }
            if locator.entries[..index].iter().any(|previous| {
                previous.binding.name == entry.binding.name
                    || previous.binding.id == entry.binding.id
                    || previous.binding.identity == entry.binding.identity
                    || previous.binding.temp_handle == entry.binding.temp_handle
            }) {
                return Err(Error::Unsafe(
                    "locator contains duplicate instance bindings",
                ));
            }
        }
        Ok((locator, previous))
    }

    fn check_roots(
        &self,
        locator: &Locator,
        name: &Name,
        path: &Path,
    ) -> Result<Option<Directory>, Error> {
        // Probe an existing root without modifying it; a missing final component is fine.
        let candidate = match Directory::open_root(path, false) {
            Ok(root) => Some(root),
            Err(Error::Io(error)) if error.kind() == std::io::ErrorKind::NotFound => None,
            #[cfg(target_os = "linux")]
            Err(
                error @ (Error::CaseInsensitive(_)
                | Error::UnsupportedFilesystem { .. }
                | Error::FilesystemProbe { .. }),
            ) => {
                return Err(error);
            }
            Err(error) => {
                if let Some(entry) = locator
                    .entries
                    .iter()
                    .find(|entry| &entry.binding.name == name && entry.binding.root == path)
                {
                    return Err(Error::RootUnavailable {
                        name: name.as_str().to_owned(),
                        path: entry.binding.root.clone(),
                    });
                }
                return Err(error);
            }
        };
        if self.directory.contains_path(path)?
            || match &candidate {
                Some(root) => root.contains_path(self.directory.path())?,
                None => false,
            }
        {
            return Err(Error::Conflict(
                "instance and locator roots must not overlap",
            ));
        }
        if self.user_temp.contains_path(path)?
            || match &candidate {
                Some(root) => root.contains_path(self.user_temp.path())?,
                None => false,
            }
        {
            return Err(Error::Conflict(
                "instance control and user temp roots must not overlap",
            ));
        }
        let known_target = locator.entries.iter().any(|entry| {
            &entry.binding.name == name
                && entry.binding.root == path
                && candidate.as_ref().map(Directory::identity) == Some(entry.binding.identity)
        });
        for entry in &locator.entries {
            if &entry.binding.name == name {
                if entry.binding.root != path {
                    return Err(Error::Conflict("name is already bound to another root"));
                }
                if candidate.as_ref().map(Directory::identity) != Some(entry.binding.identity) {
                    return Err(Error::RootUnavailable {
                        name: name.as_str().to_owned(),
                        path: entry.binding.root.clone(),
                    });
                }
                continue;
            }
            // Unavailable peers retain their path and known identity, but do not
            // prevent unrelated instances from starting. Never adopt their moved inode.
            let existing = Directory::open_root(&entry.binding.root, false)
                .ok()
                .filter(|root| root.identity() == entry.binding.identity);
            let lexical = paths_overlap(
                path,
                &entry.binding.root,
                existing.is_none() && !known_target,
            );
            if lexical
                || (existing.is_some() && Directory::has_ancestor(path, entry.binding.identity)?)
                || match &candidate {
                    Some(root) => match &existing {
                        Some(existing) => root.contains_path(existing.path())?,
                        None => false,
                    },
                    None => false,
                }
            {
                return Err(Error::ReservedRoot {
                    name: entry.binding.name.as_str().to_owned(),
                    path: entry.binding.root.clone(),
                });
            }
        }
        Ok(candidate)
    }

    fn finish(
        &self,
        mut locator: Locator,
        index: usize,
        root: Directory,
        lock: &File,
        previous: Option<&ControlFile>,
    ) -> Result<Instance, Error> {
        let entry = &locator.entries[index];
        let expected = Config {
            version: 1,
            binding: entry.binding.clone(),
            office_temp_handles: BTreeMap::new(),
        };
        match root.read(CONFIG, MAX_BYTES)? {
            Some(bytes) => {
                let config: Config = decode(&bytes)?;
                config.validate(&expected.binding)?;
            }
            None if entry.phase == Phase::Reserved => {
                root.publish(CONFIG, &encode(&expected)?, None)?;
            }
            None => return Err(Error::Unsafe("ready instance configuration is missing")),
        }
        if entry.phase == Phase::Reserved {
            locator.entries[index].phase = Phase::Ready;
            self.save(&locator, lock, previous)?;
        }
        self.directory.verify_file(LOCK, lock)?;
        self.user_temp.verify()?;
        self.user_temp
            .child("ao", true)?
            .child(&expected.binding.temp_handle, true)?;
        let instance = Instance {
            root,
            binding: expected.binding,
            user_temp: self.user_temp.duplicate_unlocked()?,
        };
        instance.verify()?;
        Ok(instance)
    }

    fn save(
        &self,
        locator: &Locator,
        lock: &File,
        previous: Option<&ControlFile>,
    ) -> Result<ControlFile, Error> {
        self.directory.verify_file(LOCK, lock)?;
        self.directory
            .publish(REGISTRY, &encode(locator)?, previous)
    }
}

#[cfg(test)]
#[path = "tests.rs"]
mod tests;
