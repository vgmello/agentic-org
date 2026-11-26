//! `backup`: a consistent archive of a stopped office's durable root. The host's and the
//! office's locks are held throughout; every file is hashed into the manifest first and
//! hashed again as it is archived, so a file that changed in between fails the backup.

use std::fs::File;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};

use flate2::{Compression, GzBuilder};
use sha2::{Digest, Sha256};
use tar::{Builder, EntryType};

use crate::collect::{collect, digest, hex, open_regular, Item, Source};
use crate::envelope::{Passphrase, SealWriter};
use crate::error::BackupError;
use crate::journal::journal_mark;
use crate::locate::{locate, take_locks, OfficeTarget};
use crate::manifest::{FileMark, Limits, Manifest, FORMAT, VERSION};
use crate::output::AtomicFile;
use crate::paths::{PrivateDb, RelPath, MANIFEST_MEMBER};
use crate::tarfmt::member_headers;

/// What to back up and where.
#[derive(Debug)]
pub struct BackupRequest<'a> {
    /// The instance's office home (`<instance-root>/office-state`).
    pub office_home: &'a Path,
    /// The organization ID.
    pub org: &'a str,
    /// The office, when the organization has several here.
    pub office: Option<&'a str>,
    /// The archive to write; it must not exist.
    pub out: &'a Path,
    /// Encrypts the archive when given.
    pub passphrase: Option<&'a Passphrase>,
}

/// What a backup wrote.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct BackupReport {
    pub archive: PathBuf,
    pub archive_sha256: String,
    pub archive_bytes: u64,
    pub encrypted: bool,
    pub manifest: Manifest,
}

/// A writer that hashes and counts what passes through it.
struct Counted<W> {
    inner: W,
    hasher: Sha256,
    bytes: u64,
}

impl<W: Write> Write for Counted<W> {
    fn write(&mut self, data: &[u8]) -> io::Result<usize> {
        let n = self.inner.write(data)?;
        self.hasher.update(&data[..n]);
        self.bytes += n as u64;
        Ok(n)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}

/// The archive file, encrypted or not.
enum Sink {
    Plain(Counted<File>),
    Sealed(Box<SealWriter<Counted<File>>>),
}

impl Write for Sink {
    fn write(&mut self, data: &[u8]) -> io::Result<usize> {
        match self {
            Sink::Plain(w) => w.write(data),
            Sink::Sealed(w) => w.write(data),
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        match self {
            Sink::Plain(w) => w.flush(),
            Sink::Sealed(w) => w.flush(),
        }
    }
}

impl Sink {
    fn finish(self) -> io::Result<Counted<File>> {
        match self {
            Sink::Plain(w) => Ok(w),
            Sink::Sealed(w) => w.finish(),
        }
    }
}

/// Backs up the durable root of `request`'s office into `request.out`.
pub fn backup(request: &BackupRequest<'_>) -> Result<BackupReport, BackupError> {
    if let Some(passphrase) = request.passphrase {
        passphrase.check_strength()?;
    }
    let target = locate(request.office_home, request.org, request.office)?;
    refuse_output_inside(&target.durable, request.out)?;
    let _locks = take_locks(&target)?;
    let mut output = AtomicFile::create(request.out)?;
    // The notes copy is as private as the database: it goes in the office's own disposable
    // runtime root, never in a shared temporary folder.
    let scratch = scratch_dir(&target.runtime)?;
    let items = collect(&target.durable, Limits::default().max_entries)?;
    let (items, manifest) = describe(&target, items, scratch.path())?;
    let archive_file = output.take()?;
    let sink = Counted {
        inner: archive_file,
        hasher: Sha256::new(),
        bytes: 0,
    };
    let sink = match request.passphrase {
        Some(passphrase) => Sink::Sealed(Box::new(SealWriter::new(sink, passphrase)?)),
        None => Sink::Plain(sink),
    };
    let counted = write_archive(sink, &manifest, &items).map_err(BackupError::io(format!(
        "writing {}",
        output.destination().display()
    )))??;
    let archive_sha256 = hex(&counted.hasher.finalize());
    let archive_bytes = counted.bytes;
    let archive = output.commit(counted.inner)?;
    Ok(BackupReport {
        archive,
        archive_sha256,
        archive_bytes,
        encrypted: request.passphrase.is_some(),
        manifest,
    })
}

/// An archive written into the durable root would be part of what it backs up.
fn refuse_output_inside(durable: &Path, out: &Path) -> Result<(), BackupError> {
    let out = std::path::absolute(out)
        .map_err(BackupError::io(format!("resolving {}", out.display())))?;
    let Some(folder) = out.parent().and_then(|f| f.canonicalize().ok()) else {
        return Ok(());
    };
    let durable = durable
        .canonicalize()
        .unwrap_or_else(|_| durable.to_path_buf());
    if folder.starts_with(&durable) {
        return Err(BackupError::Usage(format!(
            "--out {} is inside the office's durable folder; write the archive elsewhere",
            out.display()
        )));
    }
    Ok(())
}

/// A private scratch folder in the runtime root, removed when dropped.
pub(crate) fn scratch_dir(runtime: &Path) -> Result<tempfile::TempDir, BackupError> {
    org_platform::fs::create_owner_only_dir_all(runtime)
        .map_err(BackupError::io(format!("making {}", runtime.display())))?;
    tempfile::Builder::new()
        .prefix(".backup-")
        .tempdir_in(runtime)
        .map_err(BackupError::io(format!(
            "making a scratch folder in {}",
            runtime.display()
        )))
}

/// Each member's path and, for a file, where its bytes are read from.
type Members = Vec<(RelPath, Option<PathBuf>)>;

/// Resolves each private database to its consistent copy, hashes every file and builds the
/// manifest.
fn describe(
    target: &OfficeTarget,
    items: Vec<Item>,
    scratch: &Path,
) -> Result<(Members, Manifest), BackupError> {
    let mut directories = Vec::new();
    let mut files = Vec::new();
    let mut members = Vec::new();
    for item in items {
        match item {
            Item::Directory(path) => {
                directories.push(path.clone());
                members.push((path, None));
            }
            Item::File(path, source) => {
                let file = match source {
                    Source::Live(file) => file,
                    Source::Private(db, live) => {
                        let copy = scratch.join(db.file_name());
                        let (snapshot, context): (fn(&Path, &Path) -> _, _) = match db {
                            PrivateDb::TaskNotes => (
                                org_office::backup::snapshot_task_notes,
                                "copying the private task notes database",
                            ),
                            PrivateDb::UserSpace => (
                                org_office::backup::snapshot_user_space,
                                "copying the private file space database",
                            ),
                        };
                        snapshot(&live, &copy).map_err(BackupError::office(context))?;
                        copy
                    }
                };
                let (length, sha256) = digest(&file)?;
                files.push(FileMark {
                    path: path.clone(),
                    length,
                    sha256,
                });
                members.push((path, Some(file)));
            }
        }
    }
    let journal = journal_mark(&target.durable, &target.identity)?;
    let manifest = Manifest {
        format: FORMAT.into(),
        version: VERSION,
        created_at: now(),
        org_id: target.org_id.clone(),
        org_name: target.org_name.clone(),
        office_id: target.office_id.clone(),
        office_role: target.role().into(),
        journal,
        directories,
        files,
    };
    manifest.validate(&Limits::default())?;
    Ok((members, manifest))
}

fn now() -> String {
    let ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX));
    org_office::format_rfc3339_ms(ms)
}

/// A reader that hashes what it reads, to check a file against its manifest entry.
struct Hashing<R> {
    inner: R,
    hasher: Sha256,
    bytes: u64,
}

impl<R: Read> Read for Hashing<R> {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        let n = self.inner.read(out)?;
        self.hasher.update(&out[..n]);
        self.bytes += n as u64;
        Ok(n)
    }
}

/// The tar stream: the manifest, then every member in order. The outer `Result` is the
/// stream's I/O, the inner one a file that changed since it was hashed.
fn write_archive(
    sink: Sink,
    manifest: &Manifest,
    members: &[(RelPath, Option<PathBuf>)],
) -> io::Result<Result<Counted<File>, BackupError>> {
    let gz = GzBuilder::new()
        .mtime(0)
        .operating_system(255)
        .write(sink, Compression::default());
    let mut tar = Builder::new(gz);
    let mut bytes = serde_json::to_vec_pretty(manifest).map_err(io::Error::other)?;
    bytes.push(b'\n');
    append(
        &mut tar,
        MANIFEST_MEMBER,
        EntryType::Regular,
        bytes.len() as u64,
        &bytes[..],
    )?;
    let marks = manifest.files.iter();
    let mut marks = marks.peekable();
    for (path, file) in members {
        let Some(file) = file else {
            append(
                &mut tar,
                &path.member(),
                EntryType::Directory,
                0,
                io::empty(),
            )?;
            continue;
        };
        let Some(mark) = marks.next().filter(|m| &m.path == path) else {
            return Ok(Err(BackupError::Refused("the manifest lost a file".into())));
        };
        let handle = match open_regular(file) {
            Ok(handle) => handle,
            Err(error) => return Ok(Err(error)),
        };
        let mut reader = Hashing {
            inner: handle.take(mark.length),
            hasher: Sha256::new(),
            bytes: 0,
        };
        append(
            &mut tar,
            &path.member(),
            EntryType::Regular,
            mark.length,
            &mut reader,
        )?;
        let extra = reader.inner.into_inner().read(&mut [0u8; 1])?;
        if reader.bytes != mark.length
            || extra != 0
            || hex(&reader.hasher.finalize()) != mark.sha256
        {
            return Ok(Err(BackupError::Refused(format!(
                "{} changed while it was backed up; nothing was written",
                path.as_str()
            ))));
        }
    }
    let gz = tar.into_inner()?;
    let sink = gz.finish()?;
    Ok(Ok(sink.finish()?))
}

fn append<W: Write>(
    tar: &mut Builder<W>,
    path: &str,
    kind: EntryType,
    size: u64,
    data: impl Read,
) -> io::Result<()> {
    let (pax, header) = member_headers(path, kind, size)?;
    if let Some((pax, record)) = pax {
        tar.append(&pax, &record[..])?;
    }
    tar.append(&header, data)
}
