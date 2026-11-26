//! The encrypted archive envelope, version 1 (`ADR-0016` d6: workspace `argon2` and `ring`
//! only). A plain archive is the `.tar.gz` itself; an encrypted one is this envelope around
//! it, told apart by its first bytes ([`detect`]).
//!
//! ```text
//! header = "AGOBAK01" ‖ version:u8=1 ‖ m_kib:u32 ‖ t:u32 ‖ p:u32 ‖ salt[16] ‖ nonce_prefix[8] ‖ chunk:u32
//! record = final:u8 ‖ length:u32 ‖ AES-256-GCM(chunk plaintext)[length] ‖ tag[16]
//! key    = Argon2id(passphrase, salt, 64 MiB, t=3, p=4) → 32 bytes
//! nonce  = nonce_prefix ‖ index:u32         (big-endian; never reused under one key)
//! aad    = "agentic-org/backup/v1" ‖ header ‖ index:u32 ‖ final:u8 ‖ length:u32
//! ```
//!
//! Every record but the last holds exactly `chunk` bytes; the last is marked final and the
//! file ends right after it, so a cut, reordered, dropped or appended record fails to open.
//! Only the pinned KDF profile is accepted, so a crafted header cannot ask for more memory.

use std::io::{self, Read, Write};

use argon2::{Algorithm, Argon2, Block, Params, Version};
use ring::aead::{Aad, LessSafeKey, Nonce, UnboundKey, AES_256_GCM};
use ring::rand::{SecureRandom, SystemRandom};
use zeroize::Zeroizing;

use crate::error::{AuthFailed, BackupError};

pub(crate) const MAGIC: &[u8; 8] = b"AGOBAK01";
const GZIP_MAGIC: [u8; 2] = [0x1f, 0x8b];
const ENVELOPE_VERSION: u8 = 1;
const AAD_DOMAIN: &[u8] = b"agentic-org/backup/v1";
const M_KIB: u32 = 64 * 1024;
const T_COST: u32 = 3;
const P_LANES: u32 = 4;
const SALT_BYTES: usize = 16;
const PREFIX_BYTES: usize = 8;
const TAG_BYTES: usize = 16;
pub(crate) const CHUNK_BYTES: usize = 1 << 20;
const HEADER_BYTES: usize = 8 + 1 + 12 + SALT_BYTES + PREFIX_BYTES + 4;
/// The longest passphrase read from a file, a descriptor or the terminal.
pub const MAX_PASSPHRASE_BYTES: usize = 1024;
/// The shortest passphrase a new encrypted backup accepts.
pub const MIN_PASSPHRASE_BYTES: usize = 8;

/// A passphrase, zeroed when dropped; never printed.
pub struct Passphrase(Zeroizing<Vec<u8>>);

impl std::fmt::Debug for Passphrase {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Passphrase(<redacted>)")
    }
}

impl Passphrase {
    /// `bytes` less one trailing line end (`\n` or `\r\n`); empty or longer than
    /// [`MAX_PASSPHRASE_BYTES`] is refused.
    pub fn new(bytes: Vec<u8>) -> Result<Self, BackupError> {
        let mut bytes = Zeroizing::new(bytes);
        if bytes.last() == Some(&b'\n') {
            bytes.pop();
            if bytes.last() == Some(&b'\r') {
                bytes.pop();
            }
        }
        if bytes.is_empty() {
            return Err(BackupError::Usage("the passphrase is empty".into()));
        }
        if bytes.len() > MAX_PASSPHRASE_BYTES {
            return Err(BackupError::Usage(format!(
                "the passphrase is longer than {MAX_PASSPHRASE_BYTES} bytes"
            )));
        }
        Ok(Self(bytes))
    }

    /// Reads one passphrase from `reader`, at most [`MAX_PASSPHRASE_BYTES`] and a line end.
    pub fn read_from(reader: &mut dyn Read) -> Result<Self, BackupError> {
        let mut bytes = Zeroizing::new(Vec::with_capacity(MAX_PASSPHRASE_BYTES + 3));
        reader
            .take(MAX_PASSPHRASE_BYTES as u64 + 3)
            .read_to_end(&mut bytes)
            .map_err(BackupError::io("reading the passphrase"))?;
        Self::new(std::mem::take(&mut *bytes))
    }

    /// Reads a passphrase file: a regular file opened without following a link or blocking
    /// on a FIFO, owned by this account and readable by its owner alone (`chmod 600`, no
    /// set-id or sticky bit), at most [`MAX_PASSPHRASE_BYTES`] and a line end.
    pub fn from_file(path: &std::path::Path) -> Result<Self, BackupError> {
        use std::os::unix::fs::MetadataExt;
        let mut file = crate::collect::open_regular(path)?;
        let meta = file
            .metadata()
            .map_err(BackupError::io(format!("reading {}", path.display())))?;
        let uid = org_platform::fs::effective_uid()
            .map_err(BackupError::io("reading this account's user ID"))?;
        if meta.uid() != uid {
            return Err(BackupError::Usage(format!(
                "the passphrase file {} is not owned by this account",
                path.display()
            )));
        }
        if meta.mode() & 0o7077 != 0 {
            return Err(BackupError::Usage(format!(
                "the passphrase file {} is not owner-only; make it so (chmod 600)",
                path.display()
            )));
        }
        Self::read_from(&mut file)
    }

    /// Reads a passphrase from the open file descriptor `fd` (a pipe, `3<file`), through
    /// `/dev/fd/<fd>`, at most [`MAX_PASSPHRASE_BYTES`] and a line end.
    pub fn from_fd(fd: u32) -> Result<Self, BackupError> {
        let path = std::path::PathBuf::from(format!("/dev/fd/{fd}"));
        let mut file = std::fs::File::open(&path)
            .map_err(|e| BackupError::Usage(format!("file descriptor {fd} cannot be read: {e}")))?;
        Self::read_from(&mut file)
    }

    /// Whether a new encrypted backup may use it.
    pub(crate) fn check_strength(&self) -> Result<(), BackupError> {
        if self.0.len() < MIN_PASSPHRASE_BYTES {
            return Err(BackupError::Usage(format!(
                "the passphrase must be at least {MIN_PASSPHRASE_BYTES} bytes"
            )));
        }
        Ok(())
    }

    /// Whether `other` is the same passphrase (a confirmation prompt).
    pub fn same_as(&self, other: &Passphrase) -> bool {
        // Both entries are the person's own; a fold without an early exit still keeps the
        // comparison's time independent of where they differ.
        self.0.len() == other.0.len()
            && self
                .0
                .iter()
                .zip(other.0.iter())
                .fold(0u8, |acc, (a, b)| acc | (a ^ b))
                == 0
    }
}

/// What the first bytes of an archive say it is.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ArchiveKind {
    Plain,
    Encrypted,
}

/// Tells a plain `.tar.gz` from this envelope; anything else is refused.
pub(crate) fn detect(prefix: &[u8]) -> Result<ArchiveKind, BackupError> {
    if prefix.starts_with(MAGIC) {
        Ok(ArchiveKind::Encrypted)
    } else if prefix.starts_with(&GZIP_MAGIC) {
        Ok(ArchiveKind::Plain)
    } else {
        Err(BackupError::invalid(
            "it is neither a backup .tar.gz nor an encrypted backup",
        ))
    }
}

fn derive(passphrase: &Passphrase, salt: &[u8]) -> Result<LessSafeKey, BackupError> {
    let unavailable = || BackupError::Refused("the key derivation is unavailable".into());
    let params = Params::new(M_KIB, T_COST, P_LANES, Some(32)).map_err(|_| unavailable())?;
    let mut blocks = Zeroizing::new(Vec::new());
    blocks
        .try_reserve_exact(params.block_count())
        .map_err(|_| unavailable())?;
    blocks.resize(params.block_count(), Block::default());
    let mut key = Zeroizing::new([0u8; 32]);
    Argon2::new(Algorithm::Argon2id, Version::V0x13, params)
        .hash_password_into_with_memory(&passphrase.0, salt, &mut *key, &mut *blocks)
        .map_err(|_| unavailable())?;
    let unbound = UnboundKey::new(&AES_256_GCM, &key[..]).map_err(|_| unavailable())?;
    Ok(LessSafeKey::new(unbound))
}

fn nonce(prefix: &[u8; PREFIX_BYTES], index: u32) -> Nonce {
    let mut bytes = [0u8; 12];
    bytes[..PREFIX_BYTES].copy_from_slice(prefix);
    bytes[PREFIX_BYTES..].copy_from_slice(&index.to_be_bytes());
    Nonce::assume_unique_for_key(bytes)
}

fn aad(header: &[u8], index: u32, last: bool, length: u32) -> Vec<u8> {
    let mut aad = Vec::with_capacity(AAD_DOMAIN.len() + header.len() + 9);
    aad.extend_from_slice(AAD_DOMAIN);
    aad.extend_from_slice(header);
    aad.extend_from_slice(&index.to_be_bytes());
    aad.push(u8::from(last));
    aad.extend_from_slice(&length.to_be_bytes());
    aad
}

/// Encrypts what is written to it, record by record. [`SealWriter::finish`] writes the
/// final record; a writer dropped without it leaves a file that never opens.
pub(crate) struct SealWriter<W: Write> {
    inner: W,
    key: LessSafeKey,
    header: Vec<u8>,
    prefix: [u8; PREFIX_BYTES],
    index: u32,
    buffer: Zeroizing<Vec<u8>>,
}

impl<W: Write> SealWriter<W> {
    pub(crate) fn new(mut inner: W, passphrase: &Passphrase) -> Result<Self, BackupError> {
        let random = SystemRandom::new();
        let mut salt = [0u8; SALT_BYTES];
        let mut prefix = [0u8; PREFIX_BYTES];
        let unavailable = |_| BackupError::Refused("no system randomness".into());
        random.fill(&mut salt).map_err(unavailable)?;
        random.fill(&mut prefix).map_err(unavailable)?;
        let mut header = Vec::with_capacity(HEADER_BYTES);
        header.extend_from_slice(MAGIC);
        header.push(ENVELOPE_VERSION);
        for value in [M_KIB, T_COST, P_LANES] {
            header.extend_from_slice(&value.to_be_bytes());
        }
        header.extend_from_slice(&salt);
        header.extend_from_slice(&prefix);
        header.extend_from_slice(&(CHUNK_BYTES as u32).to_be_bytes());
        let key = derive(passphrase, &salt)?;
        inner
            .write_all(&header)
            .map_err(BackupError::io("writing the archive"))?;
        Ok(Self {
            inner,
            key,
            header,
            prefix,
            index: 0,
            buffer: Zeroizing::new(Vec::with_capacity(CHUNK_BYTES + TAG_BYTES)),
        })
    }

    fn seal(&mut self, last: bool) -> io::Result<()> {
        let length = self.buffer.len() as u32;
        let aad = aad(&self.header, self.index, last, length);
        let tag = self
            .key
            .seal_in_place_separate_tag(
                nonce(&self.prefix, self.index),
                Aad::from(aad),
                &mut self.buffer,
            )
            .map_err(|_| io::Error::other("sealing a record failed"))?;
        self.inner.write_all(&[u8::from(last)])?;
        self.inner.write_all(&length.to_be_bytes())?;
        self.inner.write_all(&self.buffer)?;
        self.inner.write_all(tag.as_ref())?;
        self.buffer.clear();
        self.index = self
            .index
            .checked_add(1)
            .ok_or_else(|| io::Error::other("the archive has too many records"))?;
        Ok(())
    }

    /// Seals what is buffered as the final record and returns the inner writer.
    pub(crate) fn finish(mut self) -> io::Result<W> {
        self.seal(true)?;
        Ok(self.inner)
    }
}

impl<W: Write> Write for SealWriter<W> {
    fn write(&mut self, data: &[u8]) -> io::Result<usize> {
        let room = CHUNK_BYTES - self.buffer.len();
        let taken = room.min(data.len());
        self.buffer.extend_from_slice(&data[..taken]);
        if self.buffer.len() == CHUNK_BYTES {
            self.seal(false)?;
        }
        Ok(taken)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}

/// Decrypts and authenticates an envelope record by record. A record that fails, a missing
/// final record or bytes after it are errors carrying [`AuthFailed`].
pub(crate) struct OpenReader<R: Read> {
    inner: R,
    key: LessSafeKey,
    header: Vec<u8>,
    prefix: [u8; PREFIX_BYTES],
    index: u32,
    plain: Zeroizing<Vec<u8>>,
    at: usize,
    done: bool,
}

fn auth() -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, AuthFailed)
}

impl<R: Read> OpenReader<R> {
    /// Reads the header, refuses any profile but the pinned one, and derives the key.
    pub(crate) fn new(mut inner: R, passphrase: &Passphrase) -> Result<Self, BackupError> {
        let mut header = vec![0u8; HEADER_BYTES];
        inner
            .read_exact(&mut header)
            .map_err(|_| BackupError::invalid("its encryption header is cut"))?;
        let word = |at: usize| u32::from_be_bytes(header[at..at + 4].try_into().unwrap_or([0; 4]));
        if &header[..8] != MAGIC || header[8] != ENVELOPE_VERSION {
            return Err(BackupError::invalid(
                "an unknown encryption envelope version",
            ));
        }
        let chunk = word(9 + 12 + SALT_BYTES + PREFIX_BYTES);
        if (word(9), word(13), word(17)) != (M_KIB, T_COST, P_LANES)
            || chunk as usize != CHUNK_BYTES
        {
            return Err(BackupError::invalid(
                "an encryption profile this build does not accept",
            ));
        }
        let salt = &header[21..21 + SALT_BYTES];
        let mut prefix = [0u8; PREFIX_BYTES];
        prefix.copy_from_slice(&header[21 + SALT_BYTES..21 + SALT_BYTES + PREFIX_BYTES]);
        let key = derive(passphrase, salt)?;
        Ok(Self {
            inner,
            key,
            header,
            prefix,
            index: 0,
            plain: Zeroizing::new(Vec::with_capacity(CHUNK_BYTES + TAG_BYTES)),
            at: 0,
            done: false,
        })
    }

    fn next_record(&mut self) -> io::Result<()> {
        let mut head = [0u8; 5];
        self.inner.read_exact(&mut head).map_err(|_| auth())?;
        let last = match head[0] {
            0 => false,
            1 => true,
            _ => return Err(auth()),
        };
        let length = u32::from_be_bytes([head[1], head[2], head[3], head[4]]);
        let size = length as usize;
        if size > CHUNK_BYTES || (!last && size != CHUNK_BYTES) {
            return Err(auth());
        }
        self.plain.clear();
        self.plain.resize(size + TAG_BYTES, 0);
        self.inner.read_exact(&mut self.plain).map_err(|_| auth())?;
        let aad = aad(&self.header, self.index, last, length);
        let opened = self
            .key
            .open_in_place(
                nonce(&self.prefix, self.index),
                Aad::from(aad),
                &mut self.plain,
            )
            .map_err(|_| auth())?
            .len();
        self.plain.truncate(opened);
        self.at = 0;
        self.index = self.index.checked_add(1).ok_or_else(auth)?;
        if last {
            let mut extra = [0u8; 1];
            if self.inner.read(&mut extra)? != 0 {
                return Err(auth());
            }
            self.done = true;
        }
        Ok(())
    }
}

impl<R: Read> Read for OpenReader<R> {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        while self.at == self.plain.len() {
            if self.done || out.is_empty() {
                return Ok(0);
            }
            self.next_record()?;
        }
        let n = out.len().min(self.plain.len() - self.at);
        out[..n].copy_from_slice(&self.plain[self.at..self.at + n]);
        self.at += n;
        Ok(n)
    }
}
