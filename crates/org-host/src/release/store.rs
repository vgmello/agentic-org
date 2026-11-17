//! Private immutable release publication. The journal freezes its own commit
//! namespace; no office work-origin or selected acceptance allocator is involved.
use super::{
    files::{Directory, Lock},
    invalid, platform, ArchiveBundle, DirectoryBundle, Error,
};
use org_release::{TrustedKeys, TrustedPolicy, VerifiedManifest};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeSet,
    io::{Read, Write},
    path::{Path, PathBuf},
};

const MAX_CONTROL: usize = 16 * 1024;

/// A pin authenticates exact manifest bytes, not just a version label. It grants
/// no account, organization, executable or service authority by itself.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReleasePin {
    pub release_id: String,
    pub manifest_sha256: String,
    pub sequence: u64,
}
impl ReleasePin {
    pub(super) fn of(manifest: &VerifiedManifest) -> Self {
        Self {
            release_id: manifest.manifest().release_id().into(),
            manifest_sha256: hex(manifest.manifest_sha256()),
            sequence: manifest.manifest().sequence(),
        }
    }
    pub(crate) fn validate(&self) -> Result<(), Error> {
        let id = self.release_id.as_bytes();
        if id.is_empty()
            || id.len() > 96
            || !id[0].is_ascii_alphanumeric()
            || !id
                .iter()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
            || self.sequence == 0
            || !digest(&self.manifest_sha256)
        {
            return Err(Error::InvalidRelease);
        }
        Ok(())
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Commit {
    version: u32,
    ordinal: u64,
    previous: Option<String>,
    highest_sequence: u64,
    pin: ReleasePin,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Intent {
    version: u32,
    staging: String,
    commit: Commit,
}

/// Exclusive protected store. Opening/recovery never selects an unrelated
/// orphan. No runtime environment or input bundle supplies publisher trust.
pub struct ReleaseStore {
    root: Directory,
    pub(super) path: PathBuf,
    _lock: Lock,
    staging: Directory,
    pub(super) releases: Directory,
    intents: Directory,
    pins: Directory,
    policy: TrustedPolicy,
    keys: TrustedKeys,
    head: Option<(Commit, String)>,
}
impl ReleaseStore {
    /// Open a private disk store using only trusted build policy. Recovery and
    /// activation are performed explicitly before setup may open credentials.
    pub fn open(path: &Path) -> Result<Self, Error> {
        let (policy, keys) = super::policy()?;
        Self::with_policy(path, policy, keys)
    }
    fn with_policy(path: &Path, policy: TrustedPolicy, keys: TrustedKeys) -> Result<Self, Error> {
        let root = Directory::create_store(path)?;
        let lock = root.lock()?;
        let staging = root.output_child("staging", true)?;
        let releases = root.output_child("releases", true)?;
        let intents = root.output_child("intents", true)?;
        let pins = root.output_child("pins", true)?;
        let result = Self {
            root,
            path: path.into(),
            _lock: lock,
            staging,
            releases,
            intents,
            pins,
            policy,
            keys,
            head: None,
        };
        result.verify_root()?;
        Ok(result)
    }
    pub(super) fn verify_root(&self) -> Result<(), Error> {
        self.root.require_mode(0o700)?;
        if !self.root.same_directory(&Directory::open(&self.path)?)? {
            return Err(Error::UnsafeInput);
        }
        self._lock.verify(&self.root)?;
        for (name, held) in [
            ("staging", &self.staging),
            ("releases", &self.releases),
            ("intents", &self.intents),
            ("pins", &self.pins),
        ] {
            held.require_mode(0o700)?;
            if !held.same_directory(&self.root.output_child(name, false)?)? {
                return Err(Error::UnsafeInput);
            }
        }
        self.root.visit_names(|name| {
            if matches!(
                name,
                "store.lock" | "staging" | "releases" | "intents" | "pins"
            ) {
                Ok(())
            } else {
                Err(Error::UnsafeInput)
            }
        })
    }
    /// The last recovered pin. Readiness still requires complete material and
    /// actual loaded-image admission; callers cannot set this value.
    pub fn pin(&self) -> Option<&ReleasePin> {
        self.head.as_ref().map(|(h, _)| &h.pin)
    }

    /// Reconcile an exact frozen pending transaction. This must finish before
    /// any new installation or protected setup work. No domain decisions rerun.
    pub fn recover(&mut self) -> Result<Option<ReleasePin>, Error> {
        self.recover_with(platform::verify, true)
    }
    fn recover_with(
        &mut self,
        check: impl Fn(&Directory, &VerifiedManifest) -> Result<(), Error> + Copy,
        verify_selected: bool,
    ) -> Result<Option<ReleasePin>, Error> {
        self.verify_root()?;
        let pin_count = records(&self.pins)?;
        let intent_count = records(&self.intents)?;
        if intent_count < pin_count || intent_count.checked_sub(pin_count).is_none_or(|n| n > 1) {
            return Err(Error::InvalidRelease);
        }
        self.head = None;
        for ordinal in 1..=pin_count {
            let bytes = self.pins.read(&record_name(ordinal), MAX_CONTROL)?;
            let commit: Commit = decode(&bytes)?;
            self.validate_next(&commit)?;
            let intent: Intent = decode(&self.intents.read(&record_name(ordinal), MAX_CONTROL)?)?;
            validate_intent(&intent)?;
            if encode(&intent.commit)? != bytes {
                return Err(Error::InvalidRelease);
            }
            self.head = Some((commit, hex(&Sha256::digest(&bytes))));
        }
        if intent_count > pin_count {
            let intent: Intent =
                decode(&self.intents.read(&record_name(intent_count), MAX_CONTROL)?)?;
            validate_intent(&intent)?;
            self.validate_next(&intent.commit)?;
            self.finish(&intent, check, |_| Ok(()))?;
        }
        if verify_selected {
            if let Some((head, _)) = &self.head {
                self.verify_installed(&head.pin, check)?;
            }
        }
        Ok(self.pin().cloned())
    }
    fn validate_next(&self, commit: &Commit) -> Result<(), Error> {
        commit.pin.validate()?;
        let (ordinal, previous, highest) = match &self.head {
            Some((c, hash)) => (
                c.ordinal.checked_add(1).ok_or(Error::Exhausted)?,
                Some(hash.clone()),
                c.highest_sequence,
            ),
            None => (1, None, 0),
        };
        if commit.version != 1
            || commit.ordinal != ordinal
            || commit.previous != previous
            || commit.highest_sequence != commit.pin.sequence.max(highest)
            || commit.pin.sequence < highest
        {
            return Err(Error::InvalidRelease);
        }
        Ok(())
    }
    pub(super) fn manifest_at(&self, root: &Directory) -> Result<VerifiedManifest, Error> {
        let bytes = root.read("release.v1.json", org_release::MAX_MANIFEST_BYTES)?;
        let signature = root.read("release.v1.sig", 64)?;
        org_release::verify_manifest(&bytes, &signature, &self.policy, &self.keys).map_err(invalid)
    }
    fn verify_installed(
        &self,
        pin: &ReleasePin,
        check: impl Fn(&Directory, &VerifiedManifest) -> Result<(), Error>,
    ) -> Result<VerifiedManifest, Error> {
        let root = self.releases.output_child(&pin.release_id, false)?;
        self.verify_tree(&root, pin, check)
    }
    fn verify_tree(
        &self,
        root: &Directory,
        pin: &ReleasePin,
        check: impl Fn(&Directory, &VerifiedManifest) -> Result<(), Error>,
    ) -> Result<VerifiedManifest, Error> {
        root.require_mode(0o500)?;
        let manifest = self.manifest_at(root)?;
        if &ReleasePin::of(&manifest) != pin {
            return Err(Error::InvalidRelease);
        }
        root.verify_inventory(&manifest)?;
        for control in ["release.v1.json", "release.v1.sig"] {
            use std::os::unix::fs::MetadataExt;
            if root.payload(control)?.metadata()?.mode() & 0o7777 != 0o400 {
                return Err(Error::UnsafeInput);
            }
        }
        for entry in manifest.manifest().files() {
            let mut directory = root.duplicate_output()?;
            let mut parts = entry.path().split('/').peekable();
            while let Some(name) = parts.next() {
                if parts.peek().is_some() {
                    directory = directory.output_child(name, false)?;
                    directory.require_mode(0o500)?;
                }
            }
            let mut file = root.payload(entry.path())?;
            let before = super::files::identity(&file)?;
            use std::os::unix::fs::MetadataExt;
            let expected = if entry.role() == org_release::Role::Executable {
                0o500
            } else {
                0o400
            };
            if file.metadata()?.mode() & 0o7777 != expected {
                return Err(Error::UnsafeInput);
            }
            let mut verifier = entry.payload_verifier();
            let mut buffer = [0_u8; 64 * 1024];
            loop {
                let n = file.read(&mut buffer)?;
                if n == 0 {
                    break;
                }
                verifier.update(&buffer[..n]).map_err(invalid)?;
            }
            verifier.finish().map_err(invalid)?;
            if before != super::files::identity(&file)? {
                return Err(Error::UnsafeInput);
            }
        }
        check(root, &manifest)?;
        Ok(manifest)
    }

    /// Install and select a complete local directory. Exact same pin is a retry;
    /// a same-ID/different-manifest input or new downgrade cannot be accepted.
    pub fn install_directory(&mut self, bundle: &DirectoryBundle) -> Result<ReleasePin, Error> {
        self.install_directory_with(bundle, platform::verify, |_| Ok(()))
    }
    fn install_directory_with(
        &mut self,
        bundle: &DirectoryBundle,
        check: impl Fn(&Directory, &VerifiedManifest) -> Result<(), Error> + Copy,
        fault: impl FnMut(Step) -> Result<(), Error>,
    ) -> Result<ReleasePin, Error> {
        self.install_with(
            bundle.manifest(),
            bundle.signature(),
            |stage, manifest| {
                bundle.verify_controls()?;
                for entry in manifest.manifest().files() {
                    let parent = payload_parent(stage, entry.path(), true)?;
                    let name = entry
                        .path()
                        .rsplit('/')
                        .next()
                        .ok_or(Error::InvalidRelease)?;
                    let mut output = parent.new_file(name)?;
                    bundle.copy_payload(entry.path(), &mut output)?;
                    parent.seal_file(&output, entry.role() == org_release::Role::Executable)?;
                }
                bundle.verify_controls()?;
                Ok(())
            },
            check,
            fault,
        )
    }
    /// The archive was privately frozen and authenticated on the same held input;
    /// this second pass supplies payload bytes only, never extraction authority.
    pub fn install_archive(&mut self, bundle: &mut ArchiveBundle) -> Result<ReleasePin, Error> {
        let manifest = bundle.manifest().clone();
        let signature = *bundle.signature();
        self.install_with(
            &manifest,
            &signature,
            |stage, _| {
                bundle.copy_payloads(|entry, input| {
                    let parent = payload_parent(stage, entry.path(), true)?;
                    let mut output = parent.new_file(
                        entry
                            .path()
                            .rsplit('/')
                            .next()
                            .ok_or(Error::InvalidRelease)?,
                    )?;
                    let mut verifier = entry.payload_verifier();
                    let mut buffer = [0_u8; 64 * 1024];
                    loop {
                        let n = input.read(&mut buffer)?;
                        if n == 0 {
                            break;
                        }
                        verifier.update(&buffer[..n]).map_err(invalid)?;
                        output.write_all(&buffer[..n])?;
                    }
                    verifier.finish().map_err(invalid)?;
                    parent.seal_file(&output, entry.role() == org_release::Role::Executable)
                })
            },
            platform::verify,
            |_| Ok(()),
        )
    }
    fn install_with(
        &mut self,
        manifest: &VerifiedManifest,
        signature: &[u8; 64],
        copy: impl FnOnce(&Directory, &VerifiedManifest) -> Result<(), Error>,
        check: impl Fn(&Directory, &VerifiedManifest) -> Result<(), Error> + Copy,
        mut fault: impl FnMut(Step) -> Result<(), Error>,
    ) -> Result<ReleasePin, Error> {
        // An explicit new authenticated installation can replace an older pin
        // below this installer's compiled floor. This reads its retained journal
        // metadata, never opens credentials or declares the old image ready.
        self.recover_with(check, false)?;
        // Verify using this store's build policy even if a caller obtained a
        // verified input under a different minimum sequence in another process.
        let manifest = org_release::verify_manifest(
            manifest.manifest_bytes(),
            signature,
            &self.policy,
            &self.keys,
        )
        .map_err(invalid)?;
        let pin = ReleasePin::of(&manifest);
        if self.pin() == Some(&pin) {
            self.verify_installed(&pin, check)?;
            return Ok(pin);
        }
        let ordinal = self
            .head
            .as_ref()
            .map(|(h, _)| h.ordinal)
            .unwrap_or(0)
            .checked_add(1)
            .ok_or(Error::Exhausted)?;
        let highest = self
            .head
            .as_ref()
            .map(|(h, _)| h.highest_sequence)
            .unwrap_or(0);
        if pin.sequence < highest {
            return Err(Error::InvalidRelease);
        }
        // An existing ID is never replaced. Even an unpinned orphan must match
        // the exact complete signed bytes before an explicit retry may use it.
        match self.releases.output_child(&pin.release_id, false) {
            Ok(tree) => {
                self.verify_tree(&tree, &pin, check)?;
            }
            Err(Error::Io(std::io::ErrorKind::NotFound)) => {}
            Err(e) => return Err(e),
        }
        let staging = crate::ids::generate()?;
        let stage = self.releases.claim_output(&stage_name(&staging))?;
        copy(&stage, &manifest)?;
        fault(Step::Payloads)?;
        for (name, bytes) in [
            ("release.v1.json", manifest.manifest_bytes()),
            ("release.v1.sig", signature.as_slice()),
        ] {
            let mut file = stage.new_file(name)?;
            file.write_all(bytes)?;
            stage.seal_file(&file, false)?;
        }
        fault(Step::Controls)?;
        seal_directories(&stage, &manifest)?;
        fault(Step::Sealed)?;
        self.verify_tree(&stage, &pin, check)?;
        fault(Step::Staged)?;
        let intent = Intent {
            version: 1,
            staging,
            commit: Commit {
                version: 1,
                ordinal,
                previous: self.head.as_ref().map(|(_, h)| h.clone()),
                highest_sequence: highest.max(pin.sequence),
                pin: pin.clone(),
            },
        };
        self.verify_root()?;
        self.intents
            .publish_control(&record_name(ordinal), &encode(&intent)?)?;
        fault(Step::Intent)?;
        self.finish(&intent, check, fault)?;
        Ok(pin)
    }
    fn finish(
        &mut self,
        intent: &Intent,
        check: impl Fn(&Directory, &VerifiedManifest) -> Result<(), Error> + Copy,
        mut fault: impl FnMut(Step) -> Result<(), Error>,
    ) -> Result<(), Error> {
        self.verify_root()?;
        self.validate_next(&intent.commit)?;
        match self
            .releases
            .output_child(&intent.commit.pin.release_id, false)
        {
            Ok(tree) => {
                self.verify_tree(&tree, &intent.commit.pin, check)?;
            }
            Err(Error::Io(std::io::ErrorKind::NotFound)) => {
                let tree = self
                    .releases
                    .output_child(&stage_name(&intent.staging), false)?;
                self.verify_tree(&tree, &intent.commit.pin, check)?;
                self.releases.publish_directory(
                    &stage_name(&intent.staging),
                    &self.releases,
                    &intent.commit.pin.release_id,
                )?;
                self.verify_installed(&intent.commit.pin, check)?;
            }
            Err(e) => return Err(e),
        }
        fault(Step::Published)?;
        let bytes = encode(&intent.commit)?;
        self.pins
            .publish_control(&record_name(intent.commit.ordinal), &bytes)?;
        fault(Step::Pinned)?;
        self.head = Some((intent.commit.clone(), hex(&Sha256::digest(&bytes))));
        self.verify_root()?;
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Step {
    Payloads,
    Controls,
    Sealed,
    Staged,
    Intent,
    Published,
    Pinned,
}
fn record_name(ordinal: u64) -> String {
    format!("{ordinal:020}.json")
}
fn stage_name(handle: &str) -> String {
    format!(".stage-{handle}")
}
fn records(directory: &Directory) -> Result<u64, Error> {
    let (mut count, mut max) = (0_u64, 0_u64);
    directory.visit_names(|name| {
        if let Some(handle) = name.strip_prefix(".pending-") {
            if org_proto::ulid_time_ms(handle).is_none() {
                return Err(Error::InvalidRelease);
            }
            directory.read(name, MAX_CONTROL)?;
            return Ok(());
        }
        let number = name
            .strip_suffix(".json")
            .filter(|s| s.len() == 20 && s.bytes().all(|b| b.is_ascii_digit()))
            .and_then(|s| s.parse::<u64>().ok())
            .filter(|n| *n > 0)
            .ok_or(Error::InvalidRelease)?;
        if record_name(number) != name {
            return Err(Error::InvalidRelease);
        }
        count = count.checked_add(1).ok_or(Error::Exhausted)?;
        max = max.max(number);
        Ok(())
    })?;
    if count != max {
        return Err(Error::InvalidRelease);
    }
    Ok(count)
}
fn validate_intent(intent: &Intent) -> Result<(), Error> {
    if intent.version != 1 || org_proto::ulid_time_ms(&intent.staging).is_none() {
        return Err(Error::InvalidRelease);
    }
    intent.commit.pin.validate()
}
fn payload_parent(stage: &Directory, path: &str, create: bool) -> Result<Directory, Error> {
    let mut current = stage.duplicate_output()?;
    let mut parts = path.split('/').peekable();
    while let Some(name) = parts.next() {
        if parts.peek().is_some() {
            current = current.output_child(name, create)?;
        }
    }
    Ok(current)
}
fn seal_directories(stage: &Directory, manifest: &VerifiedManifest) -> Result<(), Error> {
    let mut parents = BTreeSet::new();
    for file in manifest.manifest().files() {
        let mut parent = Path::new(file.path()).parent();
        while let Some(p) = parent {
            if p.as_os_str().is_empty() {
                break;
            }
            parents.insert(p.to_str().ok_or(Error::InvalidRelease)?.to_owned());
            parent = p.parent();
        }
    }
    let mut parents = parents.into_iter().collect::<Vec<_>>();
    parents.sort_by_key(|p| std::cmp::Reverse(p.matches('/').count()));
    for parent in parents {
        let mut directory = stage.duplicate_output()?;
        for name in parent.split('/') {
            directory = directory.output_child(name, false)?;
        }
        directory.seal_directory()?;
    }
    stage.seal_directory()
}
fn encode<T: Serialize>(value: &T) -> Result<Vec<u8>, Error> {
    let bytes = serde_json::to_vec(value).map_err(|_| Error::InvalidRelease)?;
    if bytes.len() > MAX_CONTROL {
        return Err(Error::InvalidRelease);
    }
    Ok(bytes)
}
fn decode<T: serde::de::DeserializeOwned>(bytes: &[u8]) -> Result<T, Error> {
    serde_json::from_slice(bytes).map_err(|_| Error::InvalidRelease)
}
fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
fn digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || matches!(b, b'a'..=b'f'))
}

#[cfg(test)]
mod tests;
