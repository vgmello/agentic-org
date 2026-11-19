//! The host's writer-custody marker in an office folder (`<home>/offices/<office-id>/`).
//!
//! Before it asks a worker for a create, the host reserves the office's writer custody and
//! installs [`MARKER`] in the office folder: it stages the bytes in a file named
//! [`staging_name`], then renames it into place. A host killed while installing it leaves
//! the staging file. A folder holding either is the reservation's: the host never counts it
//! as an unfinished create, and a worker's discard refuses it. The host writes these files;
//! the worker and the host's catalog only recognise them, through this module.

use std::io;
use std::path::Path;

/// The marker's file name.
pub const MARKER: &str = "selected-writer.required";

/// The prefix and suffix of the files the host stages the marker in.
const STAGING: (&str, &str) = (".selected-writer-", ".tmp");

/// The name of a file the host stages the marker in; `unique` keeps two attempts apart.
pub fn staging_name(unique: &str) -> String {
    let (prefix, suffix) = STAGING;
    format!("{prefix}{unique}{suffix}")
}

/// Whether `name`, an entry of an office folder, is the host's: the marker or a file it
/// stages the marker in.
pub fn is_custody_entry(name: &str) -> bool {
    let (prefix, suffix) = STAGING;
    name == MARKER || (name.starts_with(prefix) && name.ends_with(suffix))
}

/// Whether the host reserved writer custody in `folder`: the marker is there, or a file the
/// host staged it in. An entry that cannot be read counts, so such a folder is kept. An error
/// means the folder itself could not be listed; the caller decides what that means (a
/// folder that is not there holds nothing).
pub fn holds_custody(folder: &Path) -> io::Result<bool> {
    if std::fs::symlink_metadata(folder.join(MARKER)).is_ok() {
        return Ok(true);
    }
    for entry in std::fs::read_dir(folder)? {
        let Ok(entry) = entry else {
            return Ok(true);
        };
        if is_custody_entry(&entry.file_name().to_string_lossy()) {
            return Ok(true);
        }
    }
    Ok(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_marker_and_its_staging_files_are_the_hosts() {
        assert!(is_custody_entry(MARKER));
        assert!(is_custody_entry(&staging_name("01ARZ3NDEK")));
        assert_eq!(staging_name("x"), ".selected-writer-x.tmp");
        for other in [
            "office.lock",
            "office.json",
            "hq-creation.v1.json",
            ".store-tmp-one-1-1",
            "selected-writer.required.tmp",
            ".selected-writer-x",
            "selected-writer-x.tmp",
        ] {
            assert!(!is_custody_entry(other), "{other}");
        }
    }

    #[test]
    fn a_folder_holds_custody_with_the_marker_or_a_staging_file() {
        let temporary = tempfile::tempdir().unwrap();
        let dir = temporary.path();
        assert!(!holds_custody(dir).unwrap());
        std::fs::write(dir.join("office.lock"), b"").unwrap();
        assert!(!holds_custody(dir).unwrap());
        let staged = dir.join(staging_name("01ARZ"));
        std::fs::write(&staged, b"selected-writer/1\n").unwrap();
        assert!(holds_custody(dir).unwrap());
        std::fs::remove_file(&staged).unwrap();
        // A dangling link in the marker's place counts too.
        std::os::unix::fs::symlink(dir.join("gone"), dir.join(MARKER)).unwrap();
        assert!(holds_custody(dir).unwrap());
        let missing = holds_custody(&dir.join("missing")).unwrap_err();
        assert_eq!(missing.kind(), io::ErrorKind::NotFound);
    }
}
