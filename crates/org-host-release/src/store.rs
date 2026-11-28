//! Private immutable release publication. The journal freezes its own commit
//! namespace; no office work-origin or selected acceptance allocator is involved.

mod verify;

use super::{
    files::{Directory, Lock},
    invalid, platform, ArchiveBundle, DirectoryBundle, Error,
};
use org_release::{TrustedKeys, TrustedPolicy, VerifiedManifest};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeSet,
    io::Write,
    path::{Path, PathBuf},
};
use verify::{
    decode, digest, encode, hex, payload_parent, record_name, records, seal_subdirectories,
    stage_name, validate_intent, MAX_CONTROL,
};

/// A pin authenticates exact manifest bytes, not just a version label. It grants
/// no account, organization, executable or service authority by itself.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
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

    pub fn validate(&self) -> Result<(), Error> {
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
pub(super) struct Commit {
    version: u32,
    ordinal: u64,
    previous: Option<String>,
    highest_sequence: u64,
    pin: ReleasePin,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    rollback: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Intent {
    version: u32,
    staging: String,
    commit: Commit,
}

/// Exclusive protected store. Opening/recovery never selects an unrelated
/// orphan. No runtime environment or input bundle supplies publisher trust.
pub struct ReleaseStore {
    pub(super) root: Directory,
    pub(super) path: PathBuf,
    pub(super) _lock: Lock,
    pub(super) staging: Directory,
    pub(super) releases: Directory,
    pub(super) intents: Directory,
    pub(super) pins: Directory,
    pub(super) policy: TrustedPolicy,
    pub(super) keys: TrustedKeys,
    pub(super) head: Option<(Commit, String)>,
    pub(super) prior_pins: BTreeSet<ReleasePin>,
    pub(super) history: Vec<ReleasePin>,
}

impl ReleaseStore {
    /// Open a private disk store using only trusted build policy. Recovery and
    /// activation are performed explicitly before setup may open credentials.
    pub fn open(path: &Path) -> Result<Self, Error> {
        let (policy, keys) = super::policy()?;
        Self::with_policy(path, policy, keys)
    }

    /// Open a private disk store with an explicit test policy and key registry.
    #[cfg(any(test, feature = "test-support"))]
    pub fn open_with_policy(
        path: &Path,
        policy: TrustedPolicy,
        keys: TrustedKeys,
    ) -> Result<Self, Error> {
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
            prior_pins: BTreeSet::new(),
            history: Vec::new(),
        };
        result.verify_root()?;
        Ok(result)
    }

    /// The last recovered pin. Readiness still requires complete material and
    /// actual loaded-image admission; callers cannot set this value.
    pub fn pin(&self) -> Option<&ReleasePin> {
        self.head.as_ref().map(|(h, _)| &h.pin)
    }

    /// Highest release sequence ever committed to this store (monotonic across rollbacks).
    pub fn highest_sequence(&self) -> u64 {
        self.head
            .as_ref()
            .map(|(h, _)| h.highest_sequence)
            .unwrap_or(0)
    }

    /// The most recently committed distinct release pin prior to the currently active pin.
    pub fn previous_pin(&self) -> Option<&ReleasePin> {
        let current = self.pin()?;
        self.history.iter().rev().find(|pin| *pin != current)
    }

    /// All distinct release pins ever committed to this store, newest first.
    pub fn committed_pins(&self) -> Vec<ReleasePin> {
        let mut seen = BTreeSet::new();
        let mut out = Vec::new();
        for pin in self.history.iter().rev() {
            if seen.insert(pin.clone()) {
                out.push(pin.clone());
            }
        }
        out
    }

    /// Reconcile an exact frozen pending transaction. This must finish before
    /// any new installation or protected setup work. No domain decisions rerun.
    pub fn recover(&mut self) -> Result<Option<ReleasePin>, Error> {
        self.recover_with(platform::verify, true)
    }

    /// Test-only recovery without host static platform signature admission (used to verify
    /// cross-platform macOS policies on Linux test runners).
    #[cfg(any(test, feature = "test-support"))]
    pub fn recover_without_platform_check(&mut self) -> Result<Option<ReleasePin>, Error> {
        self.recover_with(|_, _| Ok(()), true)
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
        self.prior_pins.clear();
        self.history.clear();
        for ordinal in 1..=pin_count {
            let bytes = self.pins.read(&record_name(ordinal), MAX_CONTROL)?;
            let commit: Commit = decode(&bytes)?;
            self.validate_next(&commit)?;
            let intent: Intent = decode(&self.intents.read(&record_name(ordinal), MAX_CONTROL)?)?;
            validate_intent(&intent)?;
            if encode(&intent.commit)? != bytes {
                return Err(Error::InvalidRelease);
            }
            self.prior_pins.insert(commit.pin.clone());
            self.history.push(commit.pin.clone());
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
        self.install_archive_with(bundle, platform::verify, |_| Ok(()))
    }

    /// Test-only archive install skipping platform code-signature admission (used to verify
    /// cross-platform macOS policies on Linux test runners).
    #[cfg(any(test, feature = "test-support"))]
    pub fn install_archive_without_platform_check(
        &mut self,
        bundle: &mut ArchiveBundle,
    ) -> Result<ReleasePin, Error> {
        self.install_archive_with(bundle, |_, _| Ok(()), |_| Ok(()))
    }

    /// Roll back the active pin to a previously committed release in this store's verified
    /// hash chain without decreasing `highest_sequence`. Re-verifies the entire sealed
    /// release tree (signature, inventory, payload digests, platform admission, and build
    /// freshness floor) before appending the rollback commit.
    pub fn rollback_to(&mut self, target: &ReleasePin) -> Result<ReleasePin, Error> {
        self.rollback_to_with(target, platform::verify, |_| Ok(()))
    }

    /// Roll back to a previously committed release identified by `release_id`.
    pub fn rollback_to_release_id(&mut self, release_id: &str) -> Result<ReleasePin, Error> {
        self.recover_with(platform::verify, false)?;
        let target = self
            .history
            .iter()
            .rev()
            .find(|pin| pin.release_id == release_id)
            .cloned()
            .ok_or(Error::InvalidRelease)?;
        self.rollback_to_with(&target, platform::verify, |_| Ok(()))
    }

    /// Test-only rollback without host static platform signature admission.
    #[cfg(any(test, feature = "test-support"))]
    pub fn rollback_to_without_platform_check(
        &mut self,
        target: &ReleasePin,
    ) -> Result<ReleasePin, Error> {
        self.rollback_to_with(target, |_, _| Ok(()), |_| Ok(()))
    }

    fn rollback_to_with(
        &mut self,
        target: &ReleasePin,
        check: impl Fn(&Directory, &VerifiedManifest) -> Result<(), Error> + Copy,
        mut fault: impl FnMut(Step) -> Result<(), Error>,
    ) -> Result<ReleasePin, Error> {
        target.validate()?;
        self.recover_with(check, false)?;
        if self.pin() == Some(target) {
            self.verify_installed(target, check)?;
            return Ok(target.clone());
        }
        if !self.prior_pins.contains(target) {
            return Err(Error::InvalidRelease);
        }
        let (ordinal, previous, highest) = match &self.head {
            Some((head, hash)) => (
                head.ordinal.checked_add(1).ok_or(Error::Exhausted)?,
                Some(hash.clone()),
                head.highest_sequence,
            ),
            None => return Err(Error::InvalidRelease),
        };
        if target.sequence > highest {
            return Err(Error::InvalidRelease);
        }
        // Re-verify the sealed release tree on disk against build policy and trusted keys.
        self.verify_installed(target, check)?;
        let staging = crate::ids::generate()?;
        let intent = Intent {
            version: 1,
            staging,
            commit: Commit {
                version: 1,
                ordinal,
                previous,
                highest_sequence: highest,
                pin: target.clone(),
                rollback: true,
            },
        };
        self.verify_root()?;
        self.intents
            .publish_control(&record_name(ordinal), &encode(&intent)?)?;
        fault(Step::Intent)?;
        self.finish(&intent, check, fault)?;
        Ok(target.clone())
    }

    fn install_archive_with(
        &mut self,
        bundle: &mut ArchiveBundle,
        check: impl Fn(&Directory, &VerifiedManifest) -> Result<(), Error> + Copy,
        fault: impl FnMut(Step) -> Result<(), Error>,
    ) -> Result<ReleasePin, Error> {
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
            check,
            fault,
        )
    }

    fn stage_release_tree(
        &self,
        manifest: &VerifiedManifest,
        signature: &[u8; 64],
        pin: &ReleasePin,
        copy: impl FnOnce(&Directory, &VerifiedManifest) -> Result<(), Error>,
        check: impl Fn(&Directory, &VerifiedManifest) -> Result<(), Error>,
        fault: &mut impl FnMut(Step) -> Result<(), Error>,
    ) -> Result<String, Error> {
        let staging = crate::ids::generate()?;
        let stage = self.releases.claim_output(&stage_name(&staging))?;
        copy(&stage, manifest)?;
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
        seal_subdirectories(&stage, manifest)?;
        fault(Step::Sealed)?;
        self.verify_tree_with_mode(&stage, pin, check, 0o700)?;
        fault(Step::Staged)?;
        Ok(staging)
    }

    fn install_with(
        &mut self,
        manifest: &VerifiedManifest,
        signature: &[u8; 64],
        copy: impl FnOnce(&Directory, &VerifiedManifest) -> Result<(), Error>,
        check: impl Fn(&Directory, &VerifiedManifest) -> Result<(), Error> + Copy,
        mut fault: impl FnMut(Step) -> Result<(), Error>,
    ) -> Result<ReleasePin, Error> {
        self.recover_with(check, false)?;
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
        let (ordinal, highest) = self.next_commit_bounds(&pin)?;
        self.ensure_release_id_compatible(&pin, check)?;
        let staging =
            self.stage_release_tree(&manifest, signature, &pin, copy, check, &mut fault)?;
        let intent = Intent {
            version: 1,
            staging,
            commit: Commit {
                version: 1,
                ordinal,
                previous: self.head.as_ref().map(|(_, h)| h.clone()),
                highest_sequence: highest.max(pin.sequence),
                pin: pin.clone(),
                rollback: false,
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
                // Recovery after a crash between rename and fchmod may observe the
                // newly published root at 0o700; seal it before final verification.
                if tree.require_mode(0o700).is_ok() {
                    tree.seal_directory()?;
                }
                self.verify_tree(&tree, &intent.commit.pin, check)?;
            }
            Err(Error::Io(std::io::ErrorKind::NotFound)) => {
                let tree = self
                    .releases
                    .output_child(&stage_name(&intent.staging), false)?;
                self.verify_tree_with_mode(&tree, &intent.commit.pin, check, 0o700)?;
                self.releases.publish_directory(
                    &stage_name(&intent.staging),
                    &self.releases,
                    &intent.commit.pin.release_id,
                )?;
                tree.seal_directory()?;
                self.verify_installed(&intent.commit.pin, check)?;
            }
            Err(e) => return Err(e),
        }
        fault(Step::Published)?;
        let bytes = encode(&intent.commit)?;
        self.pins
            .publish_control(&record_name(intent.commit.ordinal), &bytes)?;
        fault(Step::Pinned)?;
        self.prior_pins.insert(intent.commit.pin.clone());
        self.history.push(intent.commit.pin.clone());
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

#[cfg(test)]
mod tests;
