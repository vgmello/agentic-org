//! Safe maintained descriptor-relative syscalls. No unsafe waiver or host import.
use super::BuildError;
use org_release::{FileEntry, VerifiedManifest};
use rustix::fs::{Dir, Mode, OFlags};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{File, Metadata},
    io::Read,
    os::unix::fs::MetadataExt,
    path::{Component, Path},
};
const FLAGS: OFlags = OFlags::RDONLY
    .union(OFlags::CLOEXEC)
    .union(OFlags::NOFOLLOW)
    .union(OFlags::NONBLOCK);
fn unsafe_entry<T>(_: T) -> BuildError {
    BuildError::UnsafeBuildEntry
}
pub(super) struct Directory(File);
#[derive(PartialEq, Eq)]
struct Identity(u64, u64, u64, u32, u32, u64, (i64, i64), (i64, i64));
fn identity(m: &Metadata) -> Identity {
    Identity(
        m.dev(),
        m.ino(),
        m.len(),
        m.mode(),
        m.uid(),
        m.nlink(),
        (m.mtime(), m.mtime_nsec()),
        (m.ctime(), m.ctime_nsec()),
    )
}
impl Directory {
    pub(super) fn open_root(path: &Path) -> Result<Self, BuildError> {
        if !path.is_absolute() || path.as_os_str().len() > 4096 || path.to_str().is_none() {
            return Err(BuildError::UnsafeBuildEntry);
        }
        let mut current = Self(File::from(
            rustix::fs::open("/", FLAGS | OFlags::DIRECTORY, Mode::empty())
                .map_err(unsafe_entry)?,
        ));
        let mut parts = path.components().peekable();
        while let Some(part) = parts.next() {
            match part {
                Component::RootDir => {}
                Component::Normal(name) => {
                    current = Self(File::from(
                        rustix::fs::openat(
                            &current.0,
                            name,
                            FLAGS | OFlags::DIRECTORY,
                            Mode::empty(),
                        )
                        .map_err(unsafe_entry)?,
                    ));
                    let m = current.0.metadata().map_err(unsafe_entry)?;
                    let final_part = parts.peek().is_none();
                    if !m.is_dir()
                        || (m.uid() != 0 && m.uid() != org_socket::effective_uid())
                        || (m.mode() & 0o022 != 0 && (final_part || m.mode() & 0o1000 == 0))
                    {
                        return Err(BuildError::UnsafeBuildEntry);
                    }
                    if final_part {
                        current.sealed_directory()?;
                    }
                }
                _ => return Err(BuildError::UnsafeBuildEntry),
            }
        }
        current.sealed_directory()?;
        Ok(current)
    }
    fn sealed_directory(&self) -> Result<(), BuildError> {
        let m = self.0.metadata().map_err(unsafe_entry)?;
        if !m.is_dir() || m.uid() != org_socket::effective_uid() || m.mode() & 0o7777 != 0o500 {
            return Err(BuildError::UnsafeBuildEntry);
        }
        Ok(())
    }
    fn child(&self, name: &str) -> Result<Self, BuildError> {
        component(name)?;
        let child = Self(File::from(
            rustix::fs::openat(&self.0, name, FLAGS | OFlags::DIRECTORY, Mode::empty())
                .map_err(unsafe_entry)?,
        ));
        child.sealed_directory()?;
        if child.0.metadata().map_err(unsafe_entry)?.dev()
            != self.0.metadata().map_err(unsafe_entry)?.dev()
        {
            return Err(BuildError::UnsafeBuildEntry);
        }
        Ok(child)
    }
    fn file(&self, name: &str, executable: bool) -> Result<File, BuildError> {
        component(name)?;
        let file = File::from(
            rustix::fs::openat(&self.0, name, FLAGS, Mode::empty()).map_err(unsafe_entry)?,
        );
        let m = file.metadata().map_err(unsafe_entry)?;
        if !m.is_file()
            || m.uid() != org_socket::effective_uid()
            || m.nlink() != 1
            || m.mode() & 0o7777 != if executable { 0o500 } else { 0o400 }
            || m.dev() != self.0.metadata().map_err(unsafe_entry)?.dev()
        {
            return Err(BuildError::UnsafeBuildEntry);
        }
        Ok(file)
    }
    fn payload(&self, path: &str) -> Result<File, BuildError> {
        let mut current = Self(File::from(
            rustix::fs::openat(&self.0, ".", FLAGS | OFlags::DIRECTORY, Mode::empty())
                .map_err(unsafe_entry)?,
        ));
        let mut parts = path.split('/').peekable();
        while let Some(name) = parts.next() {
            if parts.peek().is_none() {
                return current.file(
                    name,
                    !path.starts_with("ui/") && !path.starts_with("release.v1."),
                );
            }
            current = current.child(name)?;
        }
        Err(BuildError::UnsafeBuildEntry)
    }
    pub(super) fn verify_executable(&self, entry: &FileEntry) -> Result<(), BuildError> {
        let mut file = self.payload(entry.path())?;
        let before = file.metadata().map_err(unsafe_entry)?;
        if before.len() != entry.size() {
            return Err(BuildError::ReleaseImageMismatch);
        }
        let mut verifier = entry.payload_verifier();
        let mut buf = [0_u8; 64 * 1024];
        loop {
            let n = file.read(&mut buf).map_err(unsafe_entry)?;
            if n == 0 {
                break;
            }
            verifier
                .update(&buf[..n])
                .map_err(|_| BuildError::ReleaseImageMismatch)?;
        }
        verifier
            .finish()
            .map_err(|_| BuildError::ReleaseImageMismatch)?;
        if identity(&before) != identity(&file.metadata().map_err(unsafe_entry)?) {
            return Err(BuildError::UnsafeBuildEntry);
        }
        Ok(())
    }
    pub(super) fn read_control(&self, name: &str, max: usize) -> Result<Vec<u8>, BuildError> {
        let mut file = self.file(name, false)?;
        let before = file.metadata().map_err(unsafe_entry)?;
        if before.len() > max as u64 {
            return Err(BuildError::TooLarge);
        }
        let mut bytes = Vec::new();
        bytes
            .try_reserve_exact(before.len() as usize)
            .map_err(|_| BuildError::TooLarge)?;
        Read::by_ref(&mut file)
            .take(max as u64 + 1)
            .read_to_end(&mut bytes)
            .map_err(unsafe_entry)?;
        if bytes.len() > max
            || identity(&before) != identity(&file.metadata().map_err(unsafe_entry)?)
        {
            return Err(BuildError::UnsafeBuildEntry);
        }
        Ok(bytes)
    }
    pub(super) fn read_asset(
        &self,
        entry: &FileEntry,
        remaining: &mut u64,
    ) -> Result<Vec<u8>, BuildError> {
        if entry.size() > *remaining {
            return Err(BuildError::TooLarge);
        }
        let mut file = self.payload(entry.path())?;
        let before = file.metadata().map_err(unsafe_entry)?;
        if before.len() != entry.size() {
            return Err(BuildError::UnauthenticatedRelease);
        }
        let count = usize::try_from(entry.size()).map_err(|_| BuildError::TooLarge)?;
        let mut bytes = Vec::new();
        bytes
            .try_reserve_exact(count)
            .map_err(|_| BuildError::TooLarge)?;
        let mut verifier = entry.payload_verifier();
        let mut buffer = [0_u8; 64 * 1024];
        loop {
            let n = file.read(&mut buffer).map_err(unsafe_entry)?;
            if n == 0 {
                break;
            }
            verifier
                .update(&buffer[..n])
                .map_err(|_| BuildError::UnauthenticatedRelease)?;
            *remaining = remaining
                .checked_sub(n as u64)
                .ok_or(BuildError::TooLarge)?;
            bytes.extend_from_slice(&buffer[..n]);
        }
        verifier
            .finish()
            .map_err(|_| BuildError::UnauthenticatedRelease)?;
        if identity(&before) != identity(&file.metadata().map_err(unsafe_entry)?) {
            return Err(BuildError::UnsafeBuildEntry);
        }
        Ok(bytes)
    }
    pub(super) fn verify_inventory(&self, manifest: &VerifiedManifest) -> Result<(), BuildError> {
        let (children, leaves) = manifest_inventory(manifest);
        let duplicate = Self(File::from(
            rustix::fs::openat(&self.0, ".", FLAGS | OFlags::DIRECTORY, Mode::empty())
                .map_err(unsafe_entry)?,
        ));
        let mut pending = vec![(String::new(), duplicate)];
        let mut ui_count = 0_usize;
        while let Some((prefix, current)) = pending.pop() {
            let expected = children.get(&prefix).ok_or(BuildError::UnsafeBuildEntry)?;
            let names = current.inventory_names(&prefix, expected, &mut ui_count)?;
            for name in names {
                let path = if prefix.is_empty() {
                    name.clone()
                } else {
                    format!("{prefix}/{name}")
                };
                if leaves.contains(&path) {
                    current.file(
                        &name,
                        !path.starts_with("ui/") && !path.starts_with("release.v1."),
                    )?;
                } else {
                    pending.push((path, current.child(&name)?));
                }
            }
        }
        Ok(())
    }
    fn inventory_names(
        &self,
        prefix: &str,
        expected: &BTreeSet<String>,
        ui_count: &mut usize,
    ) -> Result<BTreeSet<String>, BuildError> {
        let mut names = BTreeSet::new();
        for entry in Dir::read_from(&self.0).map_err(unsafe_entry)? {
            let entry = entry.map_err(unsafe_entry)?;
            let name = entry.file_name().to_str().map_err(unsafe_entry)?;
            if matches!(name, "." | "..") {
                continue;
            }
            if names.len() >= expected.len() || !names.insert(name.to_owned()) {
                return Err(BuildError::UnsafeBuildEntry);
            }
            if prefix == "ui" || prefix.starts_with("ui/") {
                *ui_count = ui_count
                    .checked_add(1)
                    .filter(|n| *n <= org_release::MAX_UI_ENTRIES)
                    .ok_or(BuildError::TooManyFiles)?;
            }
        }
        if &names != expected {
            return Err(BuildError::UnsafeBuildEntry);
        }
        Ok(names)
    }
}
fn component(name: &str) -> Result<(), BuildError> {
    let mut parts = Path::new(name).components();
    if !matches!(parts.next(),Some(Component::Normal(n)) if n==name) || parts.next().is_some() {
        return Err(BuildError::UnsafeBuildEntry);
    }
    Ok(())
}

fn manifest_inventory(
    manifest: &VerifiedManifest,
) -> (BTreeMap<String, BTreeSet<String>>, BTreeSet<String>) {
    let mut children: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    let mut leaves = BTreeSet::new();
    for path in manifest
        .manifest()
        .files()
        .iter()
        .map(|e| e.path())
        .chain(["release.v1.json", "release.v1.sig"])
    {
        leaves.insert(path.to_owned());
        let mut prefix = String::new();
        let mut parts = path.split('/').peekable();
        while let Some(name) = parts.next() {
            children
                .entry(prefix.clone())
                .or_default()
                .insert(name.into());
            if parts.peek().is_some() {
                if !prefix.is_empty() {
                    prefix.push('/');
                }
                prefix.push_str(name);
            }
        }
    }
    (children, leaves)
}
