//! The clean-stop marker (`docs/design/office-store.md` §13, open point 7):
//! `local/offices/<office-id>/clean-stop`, written by an orderly stop after the store has
//! closed, read before the next opening and removed once the store is open, before
//! anything is submitted. Families that number ahead of their durable mark (the work
//! family's task counter, ADR-0019) jump to the mark when the last stop was not clean.
//!
//! The marker is written in place, not through the journal: losing it only means "not
//! clean", which is always safe. So it is flushed and its folder synced, and nothing more.
//! A marker that is torn, in another form, or for another journal incarnation or head
//! means "not clean" too.

use std::{
    fs::{self, File, OpenOptions},
    io::{self, Write},
    os::unix::fs::{DirBuilderExt, OpenOptionsExt},
    path::{Path, PathBuf},
};

use org_store::{Files, Id};

use super::records::{self, CleanStop};

/// The marker `office`'s last stop left, as read before the store opens. `None` when
/// there is none. Something there that is not a marker in its form, or that cannot be
/// read as one (a link, a folder, a file over the size limit), is `Some(None)`: there,
/// and not clean. Reading it never fails the opening.
pub fn read(files: &Files, office: &Id) -> Option<Option<CleanStop>> {
    match files.read(&records::clean_stop_path(office)) {
        Ok(None) => None,
        Ok(Some(bytes)) => Some(records::decode_clean_stop(&bytes).ok()),
        Err(_) => Some(None),
    }
}

/// Whether the marker `found` says the last stop was clean: there, in its form, for this
/// journal incarnation, at the head the store opened with.
pub fn was_clean(found: Option<&Option<CleanStop>>, incarnation: &Id, head: u64) -> bool {
    let expected = CleanStop {
        journal_incarnation: incarnation.clone(),
        head_ordinal: head,
    };
    found.is_some_and(|marker| marker.as_ref() == Some(&expected))
}

fn file(durable: &Path, office: &Id) -> PathBuf {
    durable.join(records::clean_stop_path(office).as_str())
}

/// Removes whatever stands at `office`'s marker path, and syncs its folder so a crash
/// after the next write cannot bring it back. A link is removed, never followed; an empty
/// folder is removed; a folder with anything in it is refused, never emptied.
pub fn remove(durable: &Path, office: &Id) -> io::Result<()> {
    let path = file(durable, office);
    let folder = path.parent().expect("the marker is in a folder");
    match real_folders(durable, folder) {
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(()),
        checked => checked?,
    }
    let removed = match fs::symlink_metadata(&path) {
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(e) => Err(e),
        Ok(meta) if meta.is_dir() => fs::remove_dir(&path).map_err(|e| {
            let refused = format!(
                "a folder that is not empty stands where the clean-stop marker goes ({}): {e}",
                path.display()
            );
            io::Error::new(e.kind(), refused)
        }),
        Ok(_) => fs::remove_file(&path),
    };
    removed?;
    sync_dir(folder)
}

/// Every folder from `folder` up to the durable root is a folder, never a link.
fn real_folders(durable: &Path, folder: &Path) -> io::Result<()> {
    let mut below = folder;
    while below != durable {
        if !fs::symlink_metadata(below)?.is_dir() {
            let refused = "a folder of the clean-stop marker is not a folder";
            return Err(io::Error::new(io::ErrorKind::InvalidData, refused));
        }
        below = below.parent().expect("under the durable root");
    }
    Ok(())
}

/// Writes `office`'s marker: the store is closed, flushed and materialized through
/// `marker.head_ordinal`. The folders it needs are made owner-only.
pub fn write(durable: &Path, office: &Id, marker: &CleanStop) -> io::Result<()> {
    let path = file(durable, office);
    let folder = path.parent().expect("the marker is in a folder");
    fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(folder)?;
    real_folders(durable, folder)?;
    let mut out = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .custom_flags(org_platform::fs::O_NOFOLLOW)
        .open(&path)?;
    out.write_all(&records::encode_clean_stop(marker))?;
    out.sync_all()?;
    // The marker's folder, and the folders above it that this write may have made.
    let mut synced = folder;
    loop {
        sync_dir(synced)?;
        if synced == durable {
            return Ok(());
        }
        synced = synced.parent().expect("under the durable root");
    }
}

fn sync_dir(folder: &Path) -> io::Result<()> {
    File::open(folder)?.sync_all()
}
