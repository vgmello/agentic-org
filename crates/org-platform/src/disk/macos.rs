//! FileVault through `diskutil info -plist` for the volume that holds the path.
//!
//! APFS encrypts per volume, so the boot volume's `fdesetup isactive` would misreport a
//! root on another volume. When `diskutil` gives no answer the state is `Unknown`, never
//! guessed.
use super::{DiskEncryption, EncryptionState};
use crate::Refused;
use std::{
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

const DISKUTIL: &str = "/usr/sbin/diskutil";

pub(super) fn disk_encryption() -> Result<Box<dyn DiskEncryption>, Refused> {
    Ok(Box::new(FileVault))
}

struct FileVault;

impl DiskEncryption for FileVault {
    fn state(&self, path: &Path) -> EncryptionState {
        let Some(target) = nearest_existing(path) else {
            return EncryptionState::Unknown(format!("no part of {} exists", path.display()));
        };
        let output = Command::new(DISKUTIL)
            .args(["info", "-plist"])
            .arg(&target)
            .env_clear()
            .env("PATH", "/usr/bin:/bin:/usr/sbin:/sbin")
            .stdin(Stdio::null())
            .stderr(Stdio::null())
            .output();
        match output {
            Ok(out) if out.status.success() => classify(&String::from_utf8_lossy(&out.stdout)),
            Ok(out) => EncryptionState::Unknown(format!("diskutil exited with {}", out.status)),
            Err(e) => EncryptionState::Unknown(format!("diskutil could not run: {e}")),
        }
    }
}

fn nearest_existing(path: &Path) -> Option<PathBuf> {
    path.ancestors()
        .find(|p| !p.as_os_str().is_empty() && p.symlink_metadata().is_ok())
        .map(Path::to_path_buf)
}

/// `FileVault` is the volume's own FileVault state on APFS and Core Storage.
fn classify(plist: &str) -> EncryptionState {
    match plist_bool(plist, "FileVault") {
        Some(true) => EncryptionState::On,
        Some(false) => EncryptionState::Off,
        None => EncryptionState::Unknown("diskutil reported no FileVault state".into()),
    }
}

/// The boolean value of a top-level `<key>` in an XML property list. Only the exact
/// `<true/>` and `<false/>` forms count; anything else is no answer.
fn plist_bool(plist: &str, key: &str) -> Option<bool> {
    let marker = format!("<key>{key}</key>");
    let mut found = None;
    for (at, _) in plist.match_indices(&marker) {
        let rest = plist[at + marker.len()..].trim_start();
        let value = if rest.starts_with("<true/>") {
            true
        } else if rest.starts_with("<false/>") {
            false
        } else {
            return None;
        };
        // A repeated key with a different value is ambiguous.
        if found.is_some_and(|f| f != value) {
            return None;
        }
        found = Some(value);
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_only_exact_filevault_values() {
        let on = "<dict>\n\t<key>Encryption</key>\n\t<true/>\n\t<key>FileVault</key>\n\t<true/>\n</dict>";
        assert_eq!(classify(on), EncryptionState::On);
        let off = "<dict><key>FileVault</key>\n<false/></dict>";
        assert_eq!(classify(off), EncryptionState::Off);
        for unknown in [
            "<dict><key>Encryption</key><true/></dict>",
            "<dict><key>FileVault</key><string>true</string></dict>",
            "<dict><key>FileVault</key><true/><key>FileVault</key><false/></dict>",
        ] {
            assert!(matches!(classify(unknown), EncryptionState::Unknown(_)));
        }
    }

    #[test]
    fn a_missing_path_is_probed_through_its_nearest_existing_ancestor() {
        let temp = tempfile::tempdir().unwrap();
        let missing = temp.path().join("a/b/c");
        assert_eq!(nearest_existing(&missing).unwrap(), temp.path());
    }

    /// Opt-in: `AGENTIC_ORG_REAL_DISK_ENCRYPTION=1 cargo test -p org-platform -- --ignored real_disk_encryption`.
    /// Read-only; checks that this machine's `diskutil` answers with the key we parse.
    #[test]
    #[ignore = "reads this machine's real disk state"]
    fn real_disk_encryption() {
        if std::env::var_os("AGENTIC_ORG_REAL_DISK_ENCRYPTION").is_none() {
            return;
        }
        let state = FileVault.state(&std::env::temp_dir());
        assert!(!matches!(state, EncryptionState::Unknown(_)), "{state:?}");
    }
}
