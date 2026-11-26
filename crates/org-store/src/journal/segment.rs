//! Verifying segment frames and publishing the journal's small files.

use super::*;

/// A change opening makes to the drive, held back until the opening is known to pass.
pub(super) enum Repair {
    Unlink(PathBuf),
    Truncate {
        path: PathBuf,
        file: Arc<dyn DiskFile>,
        length: u64,
    },
}

pub(super) enum Bad {
    /// A bad length or hash: what a cut write leaves.
    Torn {
        offset: usize,
        ordinal: u64,
        reason: String,
    },
    /// A whole entry, correctly hashed, that does not continue the chain.
    Chain { ordinal: u64, reason: String },
}

/// Verifies the entries of one segment from `point`, advancing it. Returns the length of
/// the good prefix.
pub(super) fn scan(
    bytes: &[u8],
    point: &mut JournalPoint,
    identity: &OfficeIdentity,
    entries: &mut Vec<Arc<Entry>>,
) -> Result<usize, Bad> {
    let mut offset = 0;
    while offset < bytes.len() {
        let ordinal = point.ordinal + 1;
        let torn = |reason: &str| Bad::Torn {
            offset,
            ordinal,
            reason: reason.to_owned(),
        };
        let (frame, sha256) = whole_frame(&bytes[offset..]).map_err(torn)?;
        let chain = |reason: String| Bad::Chain { ordinal, reason };
        let entry = Entry::parse(frame, sha256).map_err(chain)?;
        *point = point.after(&entry, identity).map_err(chain)?;
        entries.push(Arc::new(entry));
        offset += frame.len();
    }
    Ok(offset)
}

/// The first frame of `rest`, if it is whole and its hash matches, or what a cut left.
pub(super) fn whole_frame(rest: &[u8]) -> Result<(&[u8], Sha256), &'static str> {
    let Some(length) = rest.first_chunk::<4>() else {
        return Err("a cut length");
    };
    let length = u32::from_be_bytes(*length) as usize;
    let total = 4 + length + TRAILER;
    if length == 0 || total > MAX_ENTRY_BYTES {
        return Err("an impossible length");
    }
    if rest.len() < total {
        return Err("a cut entry");
    }
    let sha256 = Sha256::of(&rest[4..4 + length]);
    if sha256.as_bytes()[..] != rest[4 + length..total] {
        return Err("its hash does not match its bytes");
    }
    Ok((&rest[..total], sha256))
}

/// Whether a segment named `first` holds only whole entries, numbered from `first`
/// without a gap, the last of them at or below `base`: a segment a release had passed.
/// Its chain cannot be verified (its predecessor is gone) and need not be: nothing in it
/// is read again. The one entry that can be compared is: a last entry at `base`'s own
/// ordinal is the entry the checkpoint was written from, so another hash there is
/// corruption, not a segment to drop.
pub(super) fn ends_at_or_below(
    bytes: &[u8],
    first: u64,
    base: &JournalPoint,
) -> Result<bool, JournalError> {
    let (mut offset, mut next) = (0, first);
    let mut last = None;
    while offset < bytes.len() {
        let Ok((frame, sha256)) = whole_frame(&bytes[offset..]) else {
            return Ok(false);
        };
        if next > base.ordinal || Entry::parse(frame, sha256).map(|e| e.ordinal()) != Ok(next) {
            return Ok(false);
        }
        last = Some((next, sha256));
        next += 1;
        offset += frame.len();
    }
    match last {
        Some((ordinal, sha256)) if ordinal == base.ordinal && sha256 != base.sha256 => {
            Err(JournalError::Corrupt {
                ordinal,
                reason: "the entry at the checkpoint is not the one the checkpoint names".into(),
            })
        }
        _ => Ok(true),
    }
}

pub(super) fn read_small(path: &Path) -> Result<Option<Vec<u8>>, JournalError> {
    match fs::read(path) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(io_error(path, "read", &e)),
    }
}

/// Publishes one small file of the journal: written and fully flushed under a temporary
/// name, then linked (`exclusive`) or renamed into place, then the directory and one more
/// full flush.
pub(super) fn write_small(
    disk: &dyn Disk,
    dir: &Path,
    name: &str,
    bytes: &[u8],
    exclusive: bool,
) -> Result<(), JournalError> {
    let path = dir.join(name);
    let temp = dir.join(format!("{TEMP_PREFIX}{name}"));
    let file = match disk.create_new(&temp) {
        Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {
            disk.unlink(&temp)
                .map_err(|e| io_error(&temp, "remove", &e))?;
            disk.create_new(&temp)
        }
        other => other,
    }
    .map_err(|e| io_error(&temp, "create", &e))?;
    write_all(file.as_ref(), bytes).map_err(|e| io_error(&temp, "write", &e))?;
    file.flush_full().map_err(|e| flush_error(&temp, &e))?;
    if exclusive {
        disk.link(&temp, &path).map_err(|e| match e.kind() {
            io::ErrorKind::AlreadyExists => JournalError::Exists,
            _ => io_error(&path, "link", &e),
        })?;
        disk.unlink(&temp)
            .map_err(|e| io_error(&temp, "remove", &e))?;
    } else {
        disk.rename(&temp, &path)
            .map_err(|e| io_error(&path, "rename", &e))?;
    }
    disk.sync_dir(dir).map_err(|e| flush_error(dir, &e))?;
    file.flush_full().map_err(|e| flush_error(&path, &e))
}
