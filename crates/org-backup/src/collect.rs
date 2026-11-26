//! Walking a durable root for a backup: every folder and regular file the layout's policy
//! keeps ([`crate::paths::disposition`]), sorted. A link, a socket, a FIFO or a device where
//! the office keeps its data is refused, never followed or skipped: a backup that silently
//! missed data would be worse than none.

use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

use crate::error::BackupError;
use crate::paths::{disposition, shown, Disposition, RelPath, MAX_DEPTH};

/// Where a file's bytes come from.
#[derive(Clone, PartialEq, Eq, Debug)]
pub(crate) enum Source {
    /// The file in the durable root.
    Live(PathBuf),
    /// The live private notes database, to be copied consistently first.
    Notes(PathBuf),
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub(crate) enum Item {
    Directory(RelPath),
    File(RelPath, Source),
}

impl Item {
    pub(crate) fn path(&self) -> &RelPath {
        match self {
            Item::Directory(path) | Item::File(path, _) => path,
        }
    }
}

/// Every item under `durable` the policy keeps, in path order, at most `max_entries`.
pub(crate) fn collect(durable: &Path, max_entries: usize) -> Result<Vec<Item>, BackupError> {
    let meta = fs::symlink_metadata(durable)
        .map_err(BackupError::io(format!("reading {}", durable.display())))?;
    if !meta.is_dir() {
        return Err(BackupError::Refused(format!(
            "{} is not a folder",
            durable.display()
        )));
    }
    let mut items = Vec::new();
    let mut folders: Vec<(Option<RelPath>, usize)> = vec![(None, 0)];
    while let Some((relative, depth)) = folders.pop() {
        if depth > MAX_DEPTH {
            return Err(BackupError::Refused(format!(
                "{} is nested deeper than {MAX_DEPTH} folders",
                durable.display()
            )));
        }
        let absolute = relative
            .as_ref()
            .map_or(durable.to_path_buf(), |r| r.under(durable));
        for name in names(&absolute)? {
            let path = match &relative {
                None => RelPath::top(&name),
                Some(parent) => parent.child(&name),
            }
            .map_err(|why| BackupError::Refused(format!("the durable root holds {why}")))?;
            let Some(item) = classify(durable, path)? else {
                continue;
            };
            if let Item::Directory(folder) = &item {
                folders.push((Some(folder.clone()), depth + 1));
            }
            items.push(item);
            if items.len() > max_entries {
                return Err(BackupError::Refused(format!(
                    "the durable root holds more than {max_entries} entries"
                )));
            }
        }
    }
    items.sort_by(|a, b| a.path().cmp(b.path()));
    Ok(items)
}

/// The names in `folder`, which must all be UTF-8.
fn names(folder: &Path) -> Result<Vec<String>, BackupError> {
    let context = || format!("listing {}", folder.display());
    let mut names = Vec::new();
    for entry in fs::read_dir(folder).map_err(BackupError::io(context()))? {
        let name = entry.map_err(BackupError::io(context()))?.file_name();
        let name = name.into_string().map_err(|_| {
            BackupError::Refused(format!(
                "{} holds a name that is not UTF-8",
                folder.display()
            ))
        })?;
        names.push(name);
    }
    Ok(names)
}

/// What the backup carries for `path`, or `None` when the policy leaves it out. A link or a
/// special file is refused.
fn classify(durable: &Path, path: RelPath) -> Result<Option<Item>, BackupError> {
    let policy = disposition(&path);
    if matches!(policy, Disposition::Skip | Disposition::NotesSidecar) {
        return Ok(None);
    }
    let full = path.under(durable);
    let kind = fs::symlink_metadata(&full)
        .map_err(BackupError::io(format!("reading {}", full.display())))?
        .file_type();
    if kind.is_dir() {
        return Ok(Some(Item::Directory(path)));
    }
    if kind.is_file() {
        let source = if policy == Disposition::NotesDatabase {
            Source::Notes(full)
        } else {
            Source::Live(full)
        };
        return Ok(Some(Item::File(path, source)));
    }
    let what = if kind.is_symlink() {
        "a link"
    } else {
        "a special file"
    };
    Err(BackupError::Refused(format!(
        "the durable root holds {what} at {}; links, sockets, FIFOs and devices are never \
         followed or copied",
        shown(path.as_str())
    )))
}

/// Opens `path` without following a link or blocking on a FIFO, and checks the handle is a
/// regular file.
pub(crate) fn open_regular(path: &Path) -> Result<fs::File, BackupError> {
    let file = org_platform::fs::open_nofollow_ro(path)
        .map_err(BackupError::io(format!("opening {}", path.display())))?;
    let meta = file
        .metadata()
        .map_err(BackupError::io(format!("reading {}", path.display())))?;
    if !meta.is_file() {
        return Err(BackupError::Refused(format!(
            "{} is not a regular file",
            path.display()
        )));
    }
    Ok(file)
}

/// The length and SHA-256 (lowercase hex) of the regular file at `path`.
pub(crate) fn digest(path: &Path) -> Result<(u64, String), BackupError> {
    let mut file = open_regular(path)?;
    let mut hasher = Sha256::new();
    let mut length = 0u64;
    let mut buffer = vec![0u8; 64 * 1024];
    loop {
        let n = file
            .read(&mut buffer)
            .map_err(BackupError::io(format!("reading {}", path.display())))?;
        if n == 0 {
            break;
        }
        hasher.update(&buffer[..n]);
        length += n as u64;
    }
    Ok((length, hex(&hasher.finalize())))
}

/// Lowercase hex.
pub(crate) fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
