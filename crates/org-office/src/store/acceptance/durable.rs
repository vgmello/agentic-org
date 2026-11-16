//! Durable publication primitives. No key custody, no fallback on failed flush.

#[cfg(target_os = "macos")]
use std::os::unix::io::AsRawFd;
use std::{
    fs::{self, File, OpenOptions},
    io::{self, Read, Write},
    os::unix::fs::{DirBuilderExt, OpenOptionsExt},
    path::{Component, Path, PathBuf},
};

#[cfg(test)]
thread_local! {
    static CALLS:std::cell::Cell<(u64,u64,u64)>=const {std::cell::Cell::new((0,0,0))};
    static FAIL_PARENT:std::cell::Cell<usize>=const {std::cell::Cell::new(0)};
    static DIRECTORY_PROBE: std::cell::RefCell<Option<DirectoryProbe>> = const { std::cell::RefCell::new(None) };
}

#[cfg(test)]
struct DirectoryProbe {
    paths: Vec<PathBuf>,
    fail_at: Option<PathBuf>,
}

#[cfg(test)]
pub(super) fn observe_directory_syncs(fail_at: Option<PathBuf>) {
    DIRECTORY_PROBE.with(|probe| {
        *probe.borrow_mut() = Some(DirectoryProbe {
            paths: Vec::new(),
            fail_at,
        });
    });
}

#[cfg(test)]
pub(super) fn take_directory_syncs() -> Vec<PathBuf> {
    DIRECTORY_PROBE.with(|probe| {
        probe
            .borrow_mut()
            .take()
            .expect("directory probe armed")
            .paths
    })
}

#[cfg(test)]
pub(super) fn fail_parent_flush_after(nth: usize) {
    FAIL_PARENT.set(nth);
}

#[cfg(test)]
pub(super) fn calls() -> (u64, u64, u64) {
    CALLS.get()
}

fn sync_directory(path: &Path) -> io::Result<()> {
    #[cfg(test)]
    if DIRECTORY_PROBE.with(|probe| {
        probe
            .borrow()
            .as_ref()
            .is_some_and(|p| p.fail_at.as_deref() == Some(path))
    }) {
        return Err(io::Error::other("injected directory sync failure"));
    }
    File::open(path)?.sync_all()?;
    #[cfg(test)]
    CALLS.set({
        let (a, b, c) = CALLS.get();
        (a, b, c + 1)
    });
    #[cfg(test)]
    DIRECTORY_PROBE.with(|probe| {
        if let Some(probe) = probe.borrow_mut().as_mut() {
            probe.paths.push(path.to_path_buf());
        }
    });
    Ok(())
}

pub(super) fn relative(path: &str) -> io::Result<&Path> {
    let path = Path::new(path);
    if path.as_os_str().is_empty()
        || path.components().collect::<PathBuf>().as_os_str() != path.as_os_str()
        || path
            .components()
            .any(|c| !matches!(c, Component::Normal(_)))
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "invalid artifact path",
        ));
    }
    Ok(path)
}

pub(super) fn component(value: &str) -> io::Result<()> {
    let p = relative(value)?;
    if p.components().count() != 1
        || p.file_name().and_then(|s| s.to_str()) != Some(value)
        || value.len() > 255
        || value.chars().any(char::is_control)
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "unsupported identity path representation",
        ));
    }
    Ok(())
}

pub(super) fn directories(root: &Path, path: &Path) -> io::Result<()> {
    check_directories(root, path, true)
}

fn check_directories(root: &Path, path: &Path, create: bool) -> io::Result<()> {
    if !root.is_absolute() || !path.starts_with(root) {
        return Err(io::ErrorKind::InvalidInput.into());
    }
    let mut current = root.to_path_buf();
    for child in std::iter::once(None).chain(
        path.strip_prefix(root)
            .map_err(|_| io::ErrorKind::InvalidInput)?
            .components()
            .map(Some),
    ) {
        if let Some(child) = child {
            if !matches!(child, Component::Normal(_)) {
                return Err(io::ErrorKind::InvalidInput.into());
            }
            current.push(child);
        }
        match fs::symlink_metadata(&current) {
            Ok(meta) if meta.is_dir() && !meta.file_type().is_symlink() => {}
            Ok(_) => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "artifact parent is not a directory",
                ))
            }
            Err(e) if e.kind() == io::ErrorKind::NotFound && create => {
                fs::DirBuilder::new().mode(0o700).create(&current)?;
                #[cfg(test)]
                super::crash::directory(root, &current);
            }
            Err(e) => return Err(e),
        }
        // A prior process may have died between mkdir and its parent flush.
        // Re-establish every link, including root's, even when it is visible.
        // Reads only validate the chain; publication performs these flushes
        // before the final file full flush on the same filesystem.
        if create {
            if let Some(parent) = current.parent() {
                sync_directory(parent)?;
            }
        }
    }
    Ok(())
}

pub(super) fn flush(file: &File) -> io::Result<()> {
    file.sync_all()?;
    #[cfg(test)]
    CALLS.set({
        let (a, b, c) = CALLS.get();
        (a + 1, b, c)
    });
    #[cfg(target_os = "macos")]
    {
        // SAFETY: a live descriptor and a no-argument fcntl command. Failure is
        // returned, never downgraded to ordinary fsync.
        if unsafe { libc::fcntl(file.as_raw_fd(), libc::F_FULLFSYNC) } == -1 {
            return Err(io::Error::last_os_error());
        }
        #[cfg(test)]
        CALLS.set({
            let (a, b, c) = CALLS.get();
            (a, b + 1, c)
        });
    }
    Ok(())
}

fn parent_flush(parent: &Path, file: &File) -> io::Result<()> {
    #[cfg(test)]
    if FAIL_PARENT.get() != 0 {
        let remaining = FAIL_PARENT.get() - 1;
        FAIL_PARENT.set(remaining);
        if remaining == 0 {
            return Err(io::Error::other("injected publication flush failure"));
        }
    }
    // First issue directory metadata to the device, then fully flush its cache
    // using the file on that same filesystem. Directory F_FULLFSYNC is not used.
    sync_directory(parent)?;
    flush(file)
}

pub(crate) fn read(root: &Path, name: &str, limit: usize) -> io::Result<Vec<u8>> {
    let path = root.join(relative(name)?);
    check_directories(
        root,
        path.parent().ok_or(io::ErrorKind::InvalidInput)?,
        false,
    )?;
    let mut file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW)
        .open(path)?;
    if !file.metadata()?.is_file() || file.metadata()?.len() > limit as u64 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "artifact exceeds profile",
        ));
    }
    let mut bytes = Vec::new();
    (&mut file).take(limit as u64 + 1).read_to_end(&mut bytes)?;
    if bytes.len() > limit {
        return Err(io::ErrorKind::InvalidData.into());
    }
    Ok(bytes)
}

/// Immutable installation uses hard-link publication to avoid replacing an
/// existing identity. `expected` is the old digest for mutable snapshots only.
pub(crate) fn publish(
    root: &Path,
    name: &str,
    bytes: &[u8],
    immutable: bool,
    expected: Option<&str>,
) -> io::Result<()> {
    let path = root.join(relative(name)?);
    let parent = path.parent().ok_or(io::ErrorKind::InvalidInput)?;
    directories(root, parent)?;
    match read(
        root,
        name,
        bytes
            .len()
            .max(org_work::selected_codec::MAX_BLOB_BYTES as usize),
    ) {
        Ok(existing) if existing == bytes => {
            let file = OpenOptions::new()
                .read(true)
                .custom_flags(libc::O_NOFOLLOW)
                .open(&path)?;
            parent_flush(parent, &file)?;
            return Ok(());
        }
        Ok(existing) => {
            if immutable || expected != Some(org_work::selected_codec::sha256(&existing).as_str()) {
                return Err(io::Error::new(
                    io::ErrorKind::AlreadyExists,
                    "artifact identity conflict",
                ));
            }
        }
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            // Frozen recovery can recreate a lost mutable projection from the
            // journal; an expected prior digest is not a second writer lease.
        }
        Err(e) => return Err(e),
    }
    let temp: PathBuf = parent.join(format!(".acceptance-{}.tmp", crate::ulid::generate()?));
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW)
        .open(&temp)?;
    file.write_all(bytes)?;
    flush(&file)?;
    #[cfg(test)]
    super::crash::file(name, true);
    if immutable {
        match fs::hard_link(&temp, &path) {
            Ok(()) => {}
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {
                if read(root, name, bytes.len())? != bytes {
                    return Err(e);
                }
            }
            Err(e) => return Err(e),
        }
    } else {
        fs::rename(&temp, &path)?;
    }
    #[cfg(test)]
    if immutable {
        super::crash::installed(name);
    }
    parent_flush(parent, &file)?;
    if immutable {
        fs::remove_file(&temp)?;
        parent_flush(parent, &file)?;
    }
    #[cfg(test)]
    super::crash::file(name, false);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reused_directory_chain_is_synced_but_reads_do_not_flush() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("recovery");
        let parent = root.join("blobs/sha256");
        // Visible directories do not prove a prior creator flushed their links.
        fs::create_dir_all(&parent).unwrap();
        observe_directory_syncs(None);
        directories(&root, &parent).unwrap();
        assert_eq!(
            take_directory_syncs(),
            [dir.path().to_path_buf(), root.clone(), root.join("blobs")]
        );

        fs::write(parent.join("retained"), b"original").unwrap();
        observe_directory_syncs(None);
        assert_eq!(
            read(&root, "blobs/sha256/retained", 100).unwrap(),
            b"original"
        );
        assert!(take_directory_syncs().is_empty());
    }

    #[test]
    fn failed_existing_ancestor_sync_refuses_new_and_identical_publication() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("recovery");
        let parent = root.join("blobs/sha256");
        fs::create_dir_all(&parent).unwrap();
        fs::write(parent.join("retained"), b"original").unwrap();
        for name in ["blobs/sha256/new", "blobs/sha256/retained"] {
            observe_directory_syncs(Some(root.clone()));
            let error = publish(&root, name, b"original", true, None).unwrap_err();
            assert!(error
                .to_string()
                .contains("injected directory sync failure"));
            assert_eq!(take_directory_syncs(), [dir.path().to_path_buf()]);
            assert!(!parent.join("new").exists());
            assert_eq!(fs::read(parent.join("retained")).unwrap(), b"original");
        }
    }
}
