//! Reading an archive: detecting and opening the envelope, inflating under a byte budget,
//! and extracting into a fresh folder only what the manifest lists, in its order, with its
//! lengths and digests. Links, devices, unlisted, duplicated, reordered or missing members,
//! `..` and absolute paths, and anything past the budget are refused. A refusal leaves a
//! partly written folder the caller discards; nothing is ever written outside it.

use std::cell::Cell;
use std::fs::{File, OpenOptions};
use std::io::{self, BufRead, BufReader, Read, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;
use std::rc::Rc;

use flate2::bufread::GzDecoder;
use sha2::{Digest, Sha256};
use tar::{Archive, EntryType};

use crate::collect::{hex, open_regular};
use crate::envelope::{detect, ArchiveKind, OpenReader, Passphrase};
use crate::error::{BackupError, OverBudget};
use crate::manifest::{Limits, Manifest, Member};
use crate::paths::{shown, RelPath, MANIFEST_MEMBER};
use crate::tarfmt::{is_ustar, parse_pax_path, MAX_PAX_BYTES};

/// What the first bytes of the archive at `path` say it is.
pub fn archive_kind(path: &Path) -> Result<ArchiveKind, BackupError> {
    let mut file = open_regular(path)?;
    let mut prefix = [0u8; 8];
    let mut read = 0;
    while read < prefix.len() {
        let n = file
            .read(&mut prefix[read..])
            .map_err(BackupError::io(format!("reading {}", path.display())))?;
        if n == 0 {
            break;
        }
        read += n;
    }
    detect(&prefix[..read])
}

/// Inflated bytes with a limit that can be raised once the manifest is known.
struct Budget<R> {
    inner: R,
    used: u64,
    limit: Rc<Cell<u64>>,
}

impl<R: Read> Read for Budget<R> {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        let room = self.limit.get().saturating_sub(self.used).saturating_add(1);
        let want = (out.len() as u64).min(room) as usize;
        let n = self.inner.read(&mut out[..want])?;
        self.used += n as u64;
        if self.used > self.limit.get() {
            return Err(io::Error::new(io::ErrorKind::InvalidData, OverBudget));
        }
        Ok(n)
    }
}

type Inflated = Budget<GzDecoder<BufReader<Box<dyn Read>>>>;

fn open_stream(
    path: &Path,
    passphrase: Option<&Passphrase>,
    limit: &Rc<Cell<u64>>,
) -> Result<Inflated, BackupError> {
    let kind = archive_kind(path)?;
    let file = BufReader::with_capacity(256 * 1024, open_regular(path)?);
    let source: Box<dyn Read> = match (kind, passphrase) {
        (ArchiveKind::Plain, _) => Box::new(file),
        (ArchiveKind::Encrypted, Some(passphrase)) => Box::new(OpenReader::new(file, passphrase)?),
        (ArchiveKind::Encrypted, None) => return Err(BackupError::PassphraseRequired),
    };
    Ok(Budget {
        inner: GzDecoder::new(BufReader::with_capacity(256 * 1024, source)),
        used: 0,
        limit: Rc::clone(limit),
    })
}

/// Where extracted members go, and whether each is flushed to the device.
pub(crate) struct Destination<'a> {
    /// A folder that must not exist yet; it is made owner-only.
    pub(crate) durable: &'a Path,
    pub(crate) flush: bool,
}

fn refuse(why: impl Into<String>) -> BackupError {
    BackupError::invalid(why)
}

type Entry<'a> = tar::Entry<'a, Inflated>;

/// Reads and checks the archive at `path`, extracting every member into `to`. Returns the
/// manifest, checked against `limits`.
pub(crate) fn extract(
    path: &Path,
    passphrase: Option<&Passphrase>,
    limits: &Limits,
    to: &Destination<'_>,
) -> Result<Manifest, BackupError> {
    let limit = Rc::new(Cell::new(limits.max_manifest_bytes.saturating_add(1024)));
    let mut archive = Archive::new(open_stream(path, passphrase, &limit)?);
    let context = format!("reading {}", path.display());
    let mut entries = archive
        .entries()
        .map_err(BackupError::io(context.clone()))?
        .raw(true);
    let (manifest, first_size) = read_manifest(entries.next(), limits, &context)?;
    // From here the stream may hold exactly what the manifest accounts for: the manifest
    // member already read, then every member and the end blocks.
    let manifest_member = 512 + first_size.div_ceil(512) * 512;
    limit.set(manifest_member.saturating_add(manifest.tar_bytes()));
    org_platform::fs::create_owner_only_dir(to.durable)
        .map_err(BackupError::io(format!("making {}", to.durable.display())))?;
    let members = manifest.members();
    let mut extraction = Extraction {
        members: &members,
        next: 0,
        long_path: None,
        to,
        context: &context,
    };
    for entry in entries {
        let mut entry = entry.map_err(BackupError::io(context.clone()))?;
        extraction.entry(&mut entry)?;
    }
    extraction.done()?;
    drop(members);
    finish(archive.into_inner(), &context)?;
    if to.flush {
        crate::output::sync_dir(to.durable)?;
    }
    Ok(manifest)
}

/// The first member: `manifest.json`, within its limit, parsed and checked. Returns it with
/// the member's size.
fn read_manifest(
    first: Option<io::Result<Entry<'_>>>,
    limits: &Limits,
    context: &str,
) -> Result<(Manifest, u64), BackupError> {
    let mut first = first
        .ok_or_else(|| refuse("it is empty"))?
        .map_err(BackupError::io(context.to_owned()))?;
    let header = first.header();
    if !is_ustar(header)
        || header.entry_type() != EntryType::Regular
        || first.path_bytes().as_ref() != MANIFEST_MEMBER.as_bytes()
    {
        return Err(refuse("its first member is not manifest.json"));
    }
    let size = first.size();
    if size > limits.max_manifest_bytes {
        return Err(refuse("its manifest is larger than the limit"));
    }
    let mut bytes = Vec::new();
    first
        .read_to_end(&mut bytes)
        .map_err(BackupError::io(context.to_owned()))?;
    let manifest: Manifest = serde_json::from_slice(&bytes)
        .map_err(|e| refuse(format!("its manifest does not parse: {e}")))?;
    manifest.validate(limits)?;
    Ok((manifest, size))
}

/// The kind of a member, refusing every kind this dialect never writes.
fn member_kind(header: &tar::Header) -> Result<EntryType, BackupError> {
    if !is_ustar(header) {
        return Err(refuse("a member is not a ustar member"));
    }
    let kind = header.entry_type();
    if matches!(kind, EntryType::Symlink | EntryType::Link) || header.link_name_bytes().is_some() {
        return Err(refuse(
            "a link member (symbolic or hard); links are never extracted",
        ));
    }
    if !matches!(
        kind,
        EntryType::Regular | EntryType::Directory | EntryType::XHeader
    ) {
        return Err(refuse(format!(
            "a special member (type {:?}); only files and folders are extracted",
            kind.as_byte() as char
        )));
    }
    Ok(kind)
}

/// Where an extraction stands: the members still expected, in order, and a long path an
/// extended header carried for the next member.
struct Extraction<'m, 'd> {
    members: &'m [Member<'m>],
    next: usize,
    long_path: Option<String>,
    to: &'d Destination<'d>,
    context: &'d str,
}

impl<'m> Extraction<'m, '_> {
    fn io(&self) -> impl FnOnce(io::Error) -> BackupError {
        BackupError::io(self.context.to_owned())
    }

    fn entry(&mut self, entry: &mut Entry<'_>) -> Result<(), BackupError> {
        let kind = member_kind(entry.header())?;
        if kind == EntryType::XHeader {
            return self.extended(entry);
        }
        let path = self.member_path(entry)?;
        let next = self.expect(&path)?;
        self.place(next, kind, &path, entry)?;
        let mut extra = [0u8; 1];
        if entry.read(&mut extra).map_err(self.io())? != 0 {
            return Err(refuse("a member holds more than its size"));
        }
        Ok(())
    }

    /// One PAX header carrying the next member's path, and nothing else.
    fn extended(&mut self, entry: &mut Entry<'_>) -> Result<(), BackupError> {
        if self.long_path.is_some() || entry.size() > MAX_PAX_BYTES {
            return Err(refuse("an extended header that is repeated or too large"));
        }
        let mut data = Vec::new();
        entry.read_to_end(&mut data).map_err(self.io())?;
        self.long_path = Some(parse_pax_path(&data).map_err(refuse)?);
        Ok(())
    }

    fn member_path(&mut self, entry: &Entry<'_>) -> Result<RelPath, BackupError> {
        let name = match self.long_path.take() {
            Some(name) => name,
            None => String::from_utf8(entry.path_bytes().into_owned())
                .map_err(|_| refuse("a member name that is not UTF-8"))?,
        };
        RelPath::from_member(&name).map_err(|why| refuse(format!("a member with {why}")))
    }

    /// The next member the manifest lists, which must be `path`.
    fn expect(&mut self, path: &RelPath) -> Result<&'m Member<'m>, BackupError> {
        let members: &'m [Member<'m>] = self.members;
        let Some(next) = members.get(self.next) else {
            return Err(refuse(format!(
                "{} is not listed in its manifest",
                path.as_str()
            )));
        };
        if next.path() != path {
            let listed = members.iter().any(|m| m.path() == path);
            let what = if listed {
                "duplicated or out of order"
            } else {
                "not listed in its manifest"
            };
            return Err(refuse(format!(
                "{} is {what} (expected {})",
                shown(path.as_str()),
                next.path().as_str()
            )));
        }
        self.next += 1;
        Ok(next)
    }

    /// Makes the folder or writes the file `next` lists, checking its size and digest.
    fn place(
        &self,
        next: &Member<'_>,
        kind: EntryType,
        path: &RelPath,
        entry: &mut Entry<'_>,
    ) -> Result<(), BackupError> {
        let target = path.under(self.to.durable);
        match next {
            Member::Directory(_) if kind == EntryType::Directory && entry.size() == 0 => {
                make_dir(&target)
            }
            Member::File(mark) if kind == EntryType::Regular => {
                if entry.size() != mark.length {
                    return Err(refuse(format!(
                        "{} is {} bytes but its manifest says {}",
                        path.as_str(),
                        entry.size(),
                        mark.length
                    )));
                }
                let written = write_file(&target, entry, self.to.flush)?;
                if written != (mark.length, mark.sha256.clone()) {
                    return Err(refuse(format!(
                        "{} does not match its manifest digest",
                        path.as_str()
                    )));
                }
                Ok(())
            }
            _ => Err(refuse(format!(
                "{} is not the kind its manifest lists",
                path.as_str()
            ))),
        }
    }

    /// After the last member: no dangling extended header and nothing listed missing.
    fn done(&self) -> Result<(), BackupError> {
        if self.long_path.is_some() {
            return Err(refuse("an extended header without its member"));
        }
        if let Some(missing) = self.members.get(self.next) {
            return Err(refuse(format!("{} is missing", missing.path().as_str())));
        }
        Ok(())
    }
}

/// After the last member: only zero blocks to the end of the tar stream, a whole gzip
/// member whose checksum holds, and nothing after it.
fn finish(mut stream: Inflated, context: &str) -> Result<(), BackupError> {
    let mut buffer = vec![0u8; 64 * 1024];
    loop {
        let n = stream
            .read(&mut buffer)
            .map_err(BackupError::io(context.to_owned()))?;
        if n == 0 {
            break;
        }
        if buffer[..n].iter().any(|b| *b != 0) {
            return Err(refuse("data after its last member"));
        }
    }
    let mut rest = stream.inner.into_inner();
    let left = rest
        .fill_buf()
        .map_err(BackupError::io(context.to_owned()))?;
    if !left.is_empty() {
        return Err(refuse("data after its compressed stream"));
    }
    Ok(())
}

fn make_dir(path: &Path) -> Result<(), BackupError> {
    org_platform::fs::create_owner_only_dir(path)
        .map_err(BackupError::io(format!("making {}", path.display())))
}

/// Writes a new owner-only file at `path` (never following a link, never replacing), from
/// `data`; returns its length and digest.
fn write_file(path: &Path, data: &mut dyn Read, flush: bool) -> Result<(u64, String), BackupError> {
    let mut file: File = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .custom_flags(org_platform::fs::O_NOFOLLOW | org_platform::fs::O_CLOEXEC)
        .open(path)
        .map_err(BackupError::io(format!("creating {}", path.display())))?;
    let mut hasher = Sha256::new();
    let mut length = 0u64;
    let mut buffer = vec![0u8; 64 * 1024];
    let context = format!("writing {}", path.display());
    loop {
        let n = data
            .read(&mut buffer)
            .map_err(BackupError::io(context.clone()))?;
        if n == 0 {
            break;
        }
        hasher.update(&buffer[..n]);
        length += n as u64;
        file.write_all(&buffer[..n])
            .map_err(BackupError::io(context.clone()))?;
    }
    if flush {
        org_platform::fs::full_sync(&file).map_err(BackupError::io(context))?;
    }
    Ok((length, hex(&hasher.finalize())))
}
