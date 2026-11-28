//! Verification, journal record validation, and directory sealing helpers for [`ReleaseStore`].

use std::{collections::BTreeSet, io::Read, os::unix::fs::MetadataExt, path::Path};

use org_release::{FileEntry, VerifiedManifest};
use serde::Serialize;

use super::{Commit, Intent, ReleasePin, ReleaseStore};
use crate::{files::Directory, invalid, Error};

pub(super) const MAX_CONTROL: usize = 16 * 1024;

impl ReleaseStore {
    pub(crate) fn verify_root(&self) -> Result<(), Error> {
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

    pub(super) fn next_commit_bounds(&self, pin: &ReleasePin) -> Result<(u64, u64), Error> {
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
        Ok((ordinal, highest))
    }

    pub(super) fn ensure_release_id_compatible(
        &self,
        pin: &ReleasePin,
        check: impl Fn(&Directory, &VerifiedManifest) -> Result<(), Error>,
    ) -> Result<(), Error> {
        // An existing ID is never replaced. Even an unpinned orphan must match
        // the exact complete signed bytes before an explicit retry may use it.
        match self.releases.output_child(&pin.release_id, false) {
            Ok(tree) => {
                self.verify_tree(&tree, pin, check)?;
                Ok(())
            }
            Err(Error::Io(std::io::ErrorKind::NotFound)) => Ok(()),
            Err(e) => Err(e),
        }
    }

    pub(super) fn validate_next(&self, commit: &Commit) -> Result<(), Error> {
        commit.pin.validate()?;
        let (ordinal, previous, highest) = match &self.head {
            Some((c, hash)) => (
                c.ordinal.checked_add(1).ok_or(Error::Exhausted)?,
                Some(hash.clone()),
                c.highest_sequence,
            ),
            None => (1, None, 0),
        };
        if commit.version != 1 || commit.ordinal != ordinal || commit.previous != previous {
            return Err(Error::InvalidRelease);
        }
        self.validate_commit_sequence(commit, highest)
    }

    fn validate_commit_sequence(&self, commit: &Commit, highest: u64) -> Result<(), Error> {
        let valid = if commit.rollback {
            commit.highest_sequence == highest
                && commit.pin.sequence <= highest
                && self.prior_pins.contains(&commit.pin)
        } else {
            commit.highest_sequence == commit.pin.sequence.max(highest)
                && commit.pin.sequence >= highest
        };
        if !valid {
            return Err(Error::InvalidRelease);
        }
        Ok(())
    }

    pub(crate) fn manifest_at(&self, root: &Directory) -> Result<VerifiedManifest, Error> {
        let bytes = root.read("release.v1.json", org_release::MAX_MANIFEST_BYTES)?;
        let signature = root.read("release.v1.sig", 64)?;
        org_release::verify_manifest(&bytes, &signature, &self.policy, &self.keys).map_err(invalid)
    }

    pub(super) fn verify_installed(
        &self,
        pin: &ReleasePin,
        check: impl Fn(&Directory, &VerifiedManifest) -> Result<(), Error>,
    ) -> Result<VerifiedManifest, Error> {
        let root = self.releases.output_child(&pin.release_id, false)?;
        self.verify_tree(&root, pin, check)
    }

    pub(super) fn verify_tree(
        &self,
        root: &Directory,
        pin: &ReleasePin,
        check: impl Fn(&Directory, &VerifiedManifest) -> Result<(), Error>,
    ) -> Result<VerifiedManifest, Error> {
        self.verify_tree_with_mode(root, pin, check, 0o500)
    }

    pub(super) fn verify_tree_with_mode(
        &self,
        root: &Directory,
        pin: &ReleasePin,
        check: impl Fn(&Directory, &VerifiedManifest) -> Result<(), Error>,
        root_mode: u32,
    ) -> Result<VerifiedManifest, Error> {
        root.require_mode(root_mode)?;
        let manifest = self.manifest_at(root)?;
        if &ReleasePin::of(&manifest) != pin {
            return Err(Error::InvalidRelease);
        }
        root.verify_inventory(&manifest)?;
        for control in ["release.v1.json", "release.v1.sig"] {
            if root.payload(control)?.metadata()?.mode() & 0o7777 != 0o400 {
                return Err(Error::UnsafeInput);
            }
        }
        for entry in manifest.manifest().files() {
            verify_entry_payload(root, entry)?;
        }
        check(root, &manifest)?;
        Ok(manifest)
    }
}

fn verify_entry_payload(root: &Directory, entry: &FileEntry) -> Result<(), Error> {
    let mut directory = root.duplicate_output()?;
    let mut parts = entry.path().split('/').peekable();
    while let Some(name) = parts.next() {
        if parts.peek().is_some() {
            directory = directory.output_child(name, false)?;
            directory.require_mode(0o500)?;
        }
    }
    let mut file = root.payload(entry.path())?;
    let before = crate::files::identity(&file)?;
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
    if before != crate::files::identity(&file)? {
        return Err(Error::UnsafeInput);
    }
    Ok(())
}

pub(super) fn record_name(ordinal: u64) -> String {
    format!("{ordinal:020}.json")
}

pub(super) fn stage_name(handle: &str) -> String {
    format!(".stage-{handle}")
}

pub(super) fn records(directory: &Directory) -> Result<u64, Error> {
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

pub(super) fn validate_intent(intent: &Intent) -> Result<(), Error> {
    if intent.version != 1 || org_proto::ulid_time_ms(&intent.staging).is_none() {
        return Err(Error::InvalidRelease);
    }
    intent.commit.pin.validate()
}

pub(super) fn payload_parent(
    stage: &Directory,
    path: &str,
    create: bool,
) -> Result<Directory, Error> {
    let mut current = stage.duplicate_output()?;
    let mut parts = path.split('/').peekable();
    while let Some(name) = parts.next() {
        if parts.peek().is_some() {
            current = current.output_child(name, create)?;
        }
    }
    Ok(current)
}

pub(super) fn seal_subdirectories(
    stage: &Directory,
    manifest: &VerifiedManifest,
) -> Result<(), Error> {
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
    Ok(())
}

pub(super) fn encode<T: Serialize>(value: &T) -> Result<Vec<u8>, Error> {
    let bytes = serde_json::to_vec(value).map_err(|_| Error::InvalidRelease)?;
    if bytes.len() > MAX_CONTROL {
        return Err(Error::InvalidRelease);
    }
    Ok(bytes)
}

pub(super) fn decode<T: serde::de::DeserializeOwned>(bytes: &[u8]) -> Result<T, Error> {
    serde_json::from_slice(bytes).map_err(|_| Error::InvalidRelease)
}

pub(super) fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

pub(super) fn digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || matches!(b, b'a'..=b'f'))
}
