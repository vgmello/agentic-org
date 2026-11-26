//! `manifest.json`, the first member of every archive: what was backed up, from which office
//! and journal head, and the length and SHA-256 of every file. A reader trusts nothing it
//! did not check here first.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use crate::error::BackupError;
use crate::paths::{disposition, shown, RelPath};

/// The manifest's `format`.
pub const FORMAT: &str = "agentic-org-backup";
/// The manifest's `version`; another is an archive this build does not read.
pub const VERSION: u32 = 1;

/// How much an archive may hold, checked before anything is extracted.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Limits {
    /// Files and folders together.
    pub max_entries: usize,
    /// The sum of every file's length.
    pub max_total_bytes: u64,
    /// The manifest member itself.
    pub max_manifest_bytes: u64,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_entries: 1_000_000,
            max_total_bytes: 1 << 40,
            max_manifest_bytes: 256 << 20,
        }
    }
}

/// Where the office's acceptance journal stood.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct JournalMark {
    pub incarnation: String,
    pub head_ordinal: u64,
    pub head_sha256: String,
    /// Bytes past the head that an opening would cut (never acknowledged durable).
    pub torn_tail: bool,
}

/// One file below the durable root.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FileMark {
    pub path: RelPath,
    pub length: u64,
    pub sha256: String,
}

/// The manifest.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    pub format: String,
    pub version: u32,
    /// When the backup was taken: RFC 3339 UTC with milliseconds.
    pub created_at: String,
    pub org_id: String,
    pub org_name: String,
    pub office_id: String,
    /// `hq` or `satellite`.
    pub office_role: String,
    pub journal: JournalMark,
    /// Every folder, sorted.
    pub directories: Vec<RelPath>,
    /// Every file, sorted.
    pub files: Vec<FileMark>,
}

/// One archive member, in the order the archive holds them.
#[derive(Clone, PartialEq, Eq, Debug)]
pub(crate) enum Member<'a> {
    Directory(&'a RelPath),
    File(&'a FileMark),
}

impl Member<'_> {
    pub(crate) fn path(&self) -> &RelPath {
        match self {
            Member::Directory(path) => path,
            Member::File(file) => &file.path,
        }
    }
}

fn hex64(text: &str) -> bool {
    text.len() == 64 && text.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

impl Manifest {
    /// The members after the manifest: folders and files merged in path order, so every
    /// folder comes before what it holds.
    pub(crate) fn members(&self) -> Vec<Member<'_>> {
        let mut members: Vec<Member<'_>> = self
            .directories
            .iter()
            .map(Member::Directory)
            .chain(self.files.iter().map(Member::File))
            .collect();
        members.sort_by(|a, b| a.path().cmp(b.path()));
        members
    }

    /// The sum of every file's length.
    pub fn content_bytes(&self) -> u64 {
        self.files.iter().map(|f| f.length).sum()
    }

    /// The bytes the tar stream holds after the manifest member: a header per member, file
    /// data padded to blocks, an extended header for each long path, and the end blocks.
    pub(crate) fn tar_bytes(&self) -> u64 {
        let blocks = |n: u64| n.div_ceil(512).saturating_mul(512);
        let long = |path: &RelPath| {
            if path.member().len() > 100 {
                512 + blocks(4096)
            } else {
                0
            }
        };
        let dirs = self
            .directories
            .iter()
            .fold(0u64, |sum, d| sum.saturating_add(512 + long(d)));
        let files = self.files.iter().fold(0u64, |sum, f| {
            sum.saturating_add(512)
                .saturating_add(blocks(f.length))
                .saturating_add(long(&f.path))
        });
        dirs.saturating_add(files).saturating_add(1024)
    }

    /// Everything a reader checks before it extracts a byte: the format and version, the
    /// identifiers, sorted unique paths whose folders are all listed, digests and the limits.
    pub fn validate(&self, limits: &Limits) -> Result<(), BackupError> {
        self.check_header()
            .and_then(|()| self.check_limits(limits))
            .and_then(|()| self.check_paths())
            .map_err(|why| BackupError::invalid(format!("its manifest has {why}")))
    }

    /// The format, the version, the identifiers, the role, the head hash and the time.
    fn check_header(&self) -> Result<(), String> {
        if self.format != FORMAT {
            return Err(format!("format {}", shown(&self.format)));
        }
        if self.version != VERSION {
            return Err(format!(
                "version {} (this build reads {VERSION})",
                self.version
            ));
        }
        let ids = [
            ("org_id", &self.org_id),
            ("office_id", &self.office_id),
            ("journal incarnation", &self.journal.incarnation),
        ];
        if let Some((name, _)) = ids
            .iter()
            .find(|(_, id)| !org_office_launch::is_storage_id(id))
        {
            return Err(format!("an invalid {name}"));
        }
        if !matches!(self.office_role.as_str(), "hq" | "satellite") {
            return Err("an unknown office role".into());
        }
        if !hex64(&self.journal.head_sha256) {
            return Err("an invalid journal head hash".into());
        }
        if org_office::parse_rfc3339_ms(&self.created_at).is_none() {
            return Err("an invalid created_at".into());
        }
        Ok(())
    }

    /// The number of entries and the bytes of every file, within `limits`.
    fn check_limits(&self, limits: &Limits) -> Result<(), String> {
        let entries = self.directories.len() + self.files.len();
        if entries > limits.max_entries {
            return Err(format!(
                "{entries} entries (the limit is {})",
                limits.max_entries
            ));
        }
        let total = self
            .files
            .iter()
            .try_fold(0u64, |sum, f| sum.checked_add(f.length));
        if total.is_none_or(|total| total > limits.max_total_bytes) {
            return Err(format!(
                "files over the {}-byte limit",
                limits.max_total_bytes
            ));
        }
        Ok(())
    }

    /// Sorted unique paths a backup carries, valid digests, no path both a file and a
    /// folder, and every path's folder listed.
    fn check_paths(&self) -> Result<(), String> {
        if !self.directories.windows(2).all(|w| w[0] < w[1])
            || !self.files.windows(2).all(|w| w[0].path < w[1].path)
        {
            return Err("paths that are not sorted or not unique".into());
        }
        // A notes sidecar or a disposable folder is never backed up: listed, it would land
        // beside the restored data (a stale `-wal` replayed into the notes database).
        let mut every = self
            .directories
            .iter()
            .chain(self.files.iter().map(|f| &f.path));
        if let Some(path) = every.find(|path| !disposition(path).is_carried()) {
            return Err(format!(
                "{}, a path a backup never carries",
                shown(path.as_str())
            ));
        }
        let folders: BTreeSet<&RelPath> = self.directories.iter().collect();
        if let Some(file) = self.files.iter().find(|f| !hex64(&f.sha256)) {
            return Err(format!("an invalid digest for {}", file.path.as_str()));
        }
        if let Some(file) = self.files.iter().find(|f| folders.contains(&f.path)) {
            return Err(format!("{} as a file and a folder", file.path.as_str()));
        }
        let orphan = self
            .directories
            .iter()
            .chain(self.files.iter().map(|f| &f.path))
            .find(|path| {
                path.parent()
                    .is_some_and(|parent| !folders.contains(&parent))
            });
        match orphan {
            Some(path) => Err(format!("{} without its folder", path.as_str())),
            None => Ok(()),
        }
    }
}
