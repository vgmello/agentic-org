//! Complete-container archive input. Maintained parsers read entries only;
//! archive names, modes and metadata never address filesystem output directly.
use super::{files, invalid, policy, Error};
use flate2::bufread::GzDecoder;
use org_release::{FileEntry, TrustedKeys, TrustedPolicy, VerifiedManifest};
use sha2::{Digest, Sha256};
use std::{
    cell::Cell,
    collections::{BTreeMap, BTreeSet},
    fs::File,
    io::{self, BufRead, BufReader, Read, Seek},
    path::Path,
    rc::Rc,
};

const MAX_COMPRESSED: u64 = org_release::MAX_PAYLOAD_BYTES;
// Two controls, per-member header+round-up allowance, and bounded zero padding.
const MAX_DECOMPRESSED: u64 = org_release::MAX_PAYLOAD_BYTES
    + org_release::MAX_MANIFEST_BYTES as u64
    + 64
    + (org_release::MAX_FILES as u64 + 2) * 1024
    + 1024 * 1024;
const MAX_ZERO_TAIL: u64 = 1024 * 1024;

/// Publisher-authenticated, privately frozen archive. This verifies input only;
/// it is not a platform-admitted executable or durable release selection.
pub struct ArchiveBundle {
    input: File,
    manifest: VerifiedManifest,
    signature: [u8; 64],
}
struct Fact {
    size: u64,
    sha256: [u8; 32],
}
impl ArchiveBundle {
    /// Freeze a complete local tar.gz under an already-private prepared folder,
    /// then authenticate using embedded build policy only. No trust override.
    /// Failed/uncommitted source is retained privately for explicit recovery.
    pub fn open(source: &Path, prepared: &Path) -> Result<Self, Error> {
        let (policy, keys) = policy()?;
        let folder = files::Directory::open(prepared)?;
        Self::with_policy(
            folder.freeze_archive(source, MAX_COMPRESSED)?,
            &policy,
            &keys,
        )
    }
    pub(super) fn with_policy(
        mut input: File,
        policy: &TrustedPolicy,
        keys: &TrustedKeys,
    ) -> Result<Self, Error> {
        let before = files::identity(&input)?;
        let mut manifest_bytes = None;
        let mut signature = None;
        let mut inventory = BTreeMap::new();
        visit(&mut input, &mut |name, size, reader| {
            if matches!(name, "release.v1.json" | "release.v1.sig") {
                let max = if name == "release.v1.json" {
                    org_release::MAX_MANIFEST_BYTES
                } else {
                    64
                };
                if size > max as u64 {
                    return Err(Error::InvalidRelease);
                }
                let mut bytes = Vec::with_capacity(size as usize);
                reader.take(max as u64 + 1).read_to_end(&mut bytes)?;
                if bytes.len() != size as usize {
                    return Err(Error::InvalidRelease);
                }
                if name == "release.v1.json" {
                    manifest_bytes = Some(bytes);
                } else {
                    signature = Some(bytes);
                }
            } else {
                let mut digest = Sha256::new();
                let mut count = 0_u64;
                let mut buffer = [0_u8; 64 * 1024];
                loop {
                    let n = reader.read(&mut buffer)?;
                    if n == 0 {
                        break;
                    }
                    count = count.checked_add(n as u64).ok_or(Error::UnsafeInput)?;
                    digest.update(&buffer[..n]);
                }
                if count != size {
                    return Err(Error::InvalidRelease);
                }
                inventory.insert(
                    name.to_owned(),
                    Fact {
                        size,
                        sha256: digest.finalize().into(),
                    },
                );
            }
            Ok(())
        })?;
        if before != files::identity(&input)? {
            return Err(Error::UnsafeInput);
        }
        let bytes = manifest_bytes.ok_or(Error::InvalidRelease)?;
        let signature: [u8; 64] = signature
            .ok_or(Error::InvalidRelease)?
            .try_into()
            .map_err(|_| Error::InvalidRelease)?;
        let manifest =
            org_release::verify_manifest(&bytes, &signature, policy, keys).map_err(invalid)?;
        manifest
            .verify_inventory(inventory.keys().map(String::as_str))
            .map_err(invalid)?;
        for entry in manifest.manifest().files() {
            let actual = inventory.get(entry.path()).ok_or(Error::InvalidRelease)?;
            if actual.size != entry.size() || &actual.sha256 != entry.sha256() {
                return Err(Error::InvalidRelease);
            }
        }
        Ok(Self {
            input,
            manifest,
            signature,
        })
    }
    /// Exact authenticated expectations, with no mutable parsed tree.
    pub fn manifest(&self) -> &VerifiedManifest {
        &self.manifest
    }
    /// Exact raw64 publisher signature for immutable publication.
    pub fn signature(&self) -> &[u8; 64] {
        &self.signature
    }
    /// Reparse the same held private input. A staging sink must consume each
    /// complete authenticated payload; final publication remains the caller's
    /// separate durability/platform/pin responsibility. No unpack API is used.
    pub fn copy_payloads(
        &mut self,
        mut sink: impl FnMut(&FileEntry, &mut dyn Read) -> Result<(), Error>,
    ) -> Result<(), Error> {
        let before = files::identity(&self.input)?;
        let mut seen = Vec::new();
        visit(&mut self.input, &mut |name, size, reader| {
            if matches!(name, "release.v1.json" | "release.v1.sig") {
                let expected = if name == "release.v1.json" {
                    self.manifest.manifest_bytes()
                } else {
                    &self.signature
                };
                if size != expected.len() as u64 {
                    return Err(Error::UnsafeInput);
                }
                let mut bytes = Vec::with_capacity(expected.len());
                reader
                    .take(expected.len() as u64 + 1)
                    .read_to_end(&mut bytes)?;
                if bytes != expected {
                    return Err(Error::UnsafeInput);
                }
            } else {
                let expected = self.manifest.file(name).ok_or(Error::InvalidRelease)?;
                if expected.size() != size {
                    return Err(Error::InvalidRelease);
                }
                let mut checked = Checked {
                    reader,
                    verifier: expected.payload_verifier(),
                };
                sink(expected, &mut checked)?;
                checked.verifier.finish().map_err(invalid)?;
                seen.push(name.to_owned());
            }
            Ok(())
        })?;
        self.manifest
            .verify_inventory(seen.iter().map(String::as_str))
            .map_err(invalid)?;
        if before != files::identity(&self.input)? {
            return Err(Error::UnsafeInput);
        }
        Ok(())
    }
}
struct Checked<'a> {
    reader: &'a mut dyn Read,
    verifier: org_release::PayloadVerifier,
}
impl Read for Checked<'_> {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        let n = self.reader.read(out)?;
        self.verifier
            .update(&out[..n])
            .map_err(|_| io::Error::other("release input verification failed"))?;
        Ok(n)
    }
}
struct Budget<R> {
    input: R,
    used: u64,
    padding: Rc<Cell<Option<(u64, u64)>>>,
}
impl<R: Read> Read for Budget<R> {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        if out.is_empty() {
            return Ok(0);
        }
        let available = MAX_DECOMPRESSED
            .saturating_sub(self.used)
            .saturating_add(1)
            .min(out.len() as u64) as usize;
        let n = self.input.read(&mut out[..available])?;
        let end = self
            .used
            .checked_add(n as u64)
            .filter(|n| *n <= MAX_DECOMPRESSED)
            .ok_or_else(|| io::Error::other("release container exceeds profile"))?;
        if let Some((start, stop)) = self.padding.get() {
            let from = start.max(self.used);
            let to = stop.min(end);
            if from < to
                && out[(from - self.used) as usize..(to - self.used) as usize]
                    .iter()
                    .any(|b| *b != 0)
            {
                return Err(io::Error::other("invalid release padding"));
            }
            if end >= stop {
                self.padding.set(None);
            }
        }
        self.used = end;
        Ok(n)
    }
}
type MemberVisitor<'a> = dyn FnMut(&str, u64, &mut dyn Read) -> Result<(), Error> + 'a;
fn visit(input: &mut File, visitor: &mut MemberVisitor<'_>) -> Result<(), Error> {
    input.rewind()?;
    let padding = Rc::new(Cell::new(None));
    let mut archive = tar::Archive::new(Budget {
        input: GzDecoder::new(BufReader::new(input)),
        used: 0,
        padding: Rc::clone(&padding),
    });
    let mut names = BTreeSet::new();
    for entry in archive.entries()?.raw(true) {
        let mut entry = entry?;
        let header = entry.header();
        let raw = header.as_bytes();
        if !header.entry_type().is_file()
            || header.as_ustar().is_none()
            || &raw[257..263] != b"ustar\0"
            || &raw[263..265] != b"00"
            || header.link_name_bytes().is_some()
        {
            return Err(Error::InvalidRelease);
        }
        let path = entry.path_bytes();
        if path.len() > org_release::MAX_PATH_BYTES {
            return Err(Error::InvalidRelease);
        }
        let name = std::str::from_utf8(&path)
            .map_err(|_| Error::InvalidRelease)?
            .to_owned();
        if names.len() >= org_release::MAX_FILES + 2 || !names.insert(name.clone()) {
            return Err(Error::InvalidRelease);
        }
        let size = entry.size();
        if size > org_release::MAX_PAYLOAD_BYTES {
            return Err(Error::InvalidRelease);
        }
        let end = entry
            .raw_file_position()
            .checked_add(size)
            .ok_or(Error::InvalidRelease)?;
        let stop = end
            .checked_add((512 - size % 512) % 512)
            .ok_or(Error::InvalidRelease)?;
        padding.set((end < stop).then_some((end, stop)));
        visitor(&name, size, &mut entry)?;
        // Visitors must consume each entry completely; do not let next() silently
        // skip unverified bytes on behalf of an incomplete staging sink.
        let mut extra = [0; 1];
        if entry.read(&mut extra)? != 0 {
            return Err(Error::InvalidRelease);
        }
    }
    verify_archive_tail(archive.into_inner())
}

fn verify_archive_tail(mut bounded: Budget<GzDecoder<BufReader<&mut File>>>) -> Result<(), Error> {
    // tar stops after its first zero header (and accepts bare EOF). Require a
    // second complete zero block, bounded zero padding and physical gzip EOF.
    let mut zeros = 0_u64;
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let n = bounded.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        zeros = zeros
            .checked_add(n as u64)
            .filter(|n| *n <= MAX_ZERO_TAIL)
            .ok_or(Error::InvalidRelease)?;
        if buffer[..n].iter().any(|b| *b != 0) {
            return Err(Error::InvalidRelease);
        }
    }
    if zeros < 512 || !zeros.is_multiple_of(512) {
        return Err(Error::InvalidRelease);
    }
    // GzDecoder verifies CRC/ISIZE at EOF and returns the buffered compressed
    // reader without consuming a second member or ignored trailing bytes.
    let mut compressed = bounded.input.into_inner();
    if !compressed.fill_buf()?.is_empty() {
        return Err(Error::InvalidRelease);
    }
    Ok(())
}
