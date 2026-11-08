//! The development key store, debug builds only: keys as owner-only files
//! `<dir>/<office id>.key`, each 64 hex digits, the same format as `org-office-server`'s former
//! `dev-keys` store. So a debug host, and every test, never writes a real Keychain item.
//! **Never** for real data: the files are plain text.

use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::PathBuf;

use org_office_launch::is_plain_id;
use org_proto::{DbKey, DB_KEY_BYTES};

use super::{KeyStore, KeyStoreError};
use crate::secrets::zero;

/// A key's file: 64 hex digits.
const HEX_LEN: usize = DB_KEY_BYTES * 2;

/// The largest key file read: a key with some whitespace around it. A longer file is corrupt.
const MAX_FILE_BYTES: u64 = 128;

/// Keys as files in one folder (see the module docs).
pub(crate) struct DevKeys {
    dir: PathBuf,
}

impl DevKeys {
    /// A store in `dir` (absolute). The first `create` makes it if it's missing, and makes it
    /// owner-only either way (it must be a real folder owned by this user).
    pub(crate) fn new(dir: PathBuf) -> Self {
        Self { dir }
    }

    fn path(&self, office_id: &str) -> Result<PathBuf, KeyStoreError> {
        // Ids are plain (callers check), but this is a path: be sure.
        if !is_plain_id(office_id) {
            return Err(KeyStoreError::NotPlainId);
        }
        Ok(self.dir.join(format!("{office_id}.key")))
    }

    /// Makes the folder if it's missing, and owner-only. It must be a real folder (not a
    /// symlink) owned by this user.
    fn prepare_dir(&self) -> io::Result<()> {
        fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(&self.dir)?;
        let meta = fs::symlink_metadata(&self.dir)?;
        if !meta.is_dir() || meta.uid() != crate::home::euid() {
            return Err(io::Error::other(
                "the key folder isn't a folder owned by this user",
            ));
        }
        if meta.permissions().mode() & 0o077 != 0 {
            fs::set_permissions(&self.dir, fs::Permissions::from_mode(0o700))?;
        }
        Ok(())
    }

    /// Writes a new key file. `Ok(false)`: one already exists (left as it is).
    fn write_new(&self, path: &PathBuf, key: &DbKey) -> io::Result<bool> {
        self.prepare_dir()?;
        // `create_new`: never replaces a key, even one a racing process just made.
        let opened = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(path);
        let mut file = match opened {
            Ok(file) => file,
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => return Ok(false),
            Err(e) => return Err(e),
        };
        let mut hex = [0u8; HEX_LEN];
        encode_hex(key.expose_bytes(), &mut hex);
        let written = file.write_all(&hex).and_then(|()| file.sync_all());
        zero(&mut hex);
        if let Err(e) = written {
            // Ours (`create_new` made it), and not a key: a create leaves nothing on failure.
            let _ = fs::remove_file(path);
            return Err(e);
        }
        // The folder too, so the key outlives a crash right after (best effort).
        if let Ok(dir) = File::open(&self.dir) {
            let _ = dir.sync_all();
        }
        Ok(true)
    }
}

impl KeyStore for DevKeys {
    fn create(&self, office_id: &str, key: &DbKey) -> Result<(), KeyStoreError> {
        let path = self.path(office_id)?;
        match self.write_new(&path, key) {
            Ok(true) => Ok(()),
            Ok(false) => Err(KeyStoreError::Exists),
            Err(e) => Err(e.into()),
        }
    }

    fn load(&self, office_id: &str) -> Result<Option<Box<DbKey>>, KeyStoreError> {
        let path = self.path(office_id)?;
        // No symlink, and never blocked by a FIFO: only a regular file can be a key.
        let opened = OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
            .open(&path);
        let mut file = match opened {
            Ok(file) => file,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(e.into()),
        };
        if !file.metadata()?.is_file() {
            return Err(KeyStoreError::Corrupt);
        }
        // Reserved up front, so reading never reallocates and leaves a copy behind.
        let mut text = Vec::with_capacity(MAX_FILE_BYTES as usize + 1);
        let read = Read::by_ref(&mut file)
            .take(MAX_FILE_BYTES + 1)
            .read_to_end(&mut text);
        let key = match read {
            Ok(_) => decode_hex(text.trim_ascii()).ok_or(KeyStoreError::Corrupt),
            Err(e) => Err(e.into()),
        };
        zero(&mut text);
        key.map(Some)
    }

    fn delete(&self, office_id: &str) -> Result<(), KeyStoreError> {
        match fs::remove_file(self.path(office_id)?) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e.into()),
        }
    }

    fn list(&self) -> Result<Vec<String>, KeyStoreError> {
        let entries = match fs::read_dir(&self.dir) {
            Ok(entries) => entries,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(e.into()),
        };
        let mut ids = Vec::new();
        for entry in entries {
            let name = entry?.file_name();
            if let Some(id) = name.to_str().and_then(|n| n.strip_suffix(".key")) {
                if is_plain_id(id) {
                    ids.push(id.to_owned());
                }
            }
        }
        ids.sort();
        Ok(ids)
    }
}

const HEX: &[u8; 16] = b"0123456789abcdef";

fn encode_hex(bytes: &[u8; DB_KEY_BYTES], out: &mut [u8; HEX_LEN]) {
    for (i, byte) in bytes.iter().enumerate() {
        out[2 * i] = HEX[usize::from(byte >> 4)];
        out[2 * i + 1] = HEX[usize::from(byte & 0x0f)];
    }
}

/// 64 hex digits (either case, as the server's store read them) as a key, or `None`.
fn decode_hex(text: &[u8]) -> Option<Box<DbKey>> {
    if text.len() != HEX_LEN {
        return None;
    }
    let mut bytes = [0u8; DB_KEY_BYTES];
    let mut ok = true;
    for (i, pair) in text.as_chunks::<2>().0.iter().enumerate() {
        match (nibble(pair[0]), nibble(pair[1])) {
            (Some(hi), Some(lo)) => bytes[i] = (hi << 4) | lo,
            _ => ok = false,
        }
    }
    let key = ok.then(|| Box::new(DbKey::from_bytes(bytes)));
    zero(&mut bytes);
    key
}

fn nibble(c: u8) -> Option<u8> {
    match c {
        b'0'..=b'9' => Some(c - b'0'),
        b'a'..=b'f' => Some(c - b'a' + 10),
        b'A'..=b'F' => Some(c - b'A' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    const A: &str = "01AAAAAAAAAAAAAAAAAAAAAAAA";

    fn key(seed: u8) -> DbKey {
        let mut bytes = [0u8; DB_KEY_BYTES];
        for (i, b) in bytes.iter_mut().enumerate() {
            *b = (i as u8).wrapping_mul(29).wrapping_add(seed);
        }
        DbKey::from_bytes(bytes)
    }

    #[test]
    fn a_key_round_trips_as_an_owner_only_file_of_64_hex_digits() {
        let dir = tempfile::tempdir().unwrap();
        let store = DevKeys::new(dir.path().join("keys"));
        assert!(store.load(A).unwrap().is_none());
        store.create(A, &key(7)).unwrap();
        assert_eq!(*store.load(A).unwrap().unwrap(), key(7));

        let path = dir.path().join("keys").join(format!("{A}.key"));
        let text = fs::read_to_string(&path).unwrap();
        assert_eq!(text.len(), 64);
        assert!(text
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)));
        assert_eq!(
            fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        let folder = fs::metadata(dir.path().join("keys")).unwrap();
        assert_eq!(folder.permissions().mode() & 0o777, 0o700);
    }

    #[test]
    fn create_never_replaces_a_key() {
        let dir = tempfile::tempdir().unwrap();
        let store = DevKeys::new(dir.path().to_path_buf());
        store.create(A, &key(1)).unwrap();
        assert!(matches!(
            store.create(A, &key(2)),
            Err(KeyStoreError::Exists)
        ));
        assert_eq!(*store.load(A).unwrap().unwrap(), key(1));
    }

    #[test]
    fn list_names_the_offices_with_a_key_and_nothing_else() {
        let dir = tempfile::tempdir().unwrap();
        let store = DevKeys::new(dir.path().join("keys"));
        assert_eq!(store.list().unwrap(), Vec::<String>::new());
        const B: &str = "01BBBBBBBBBBBBBBBBBBBBBBBB";
        store.create(B, &key(1)).unwrap();
        store.create(A, &key(2)).unwrap();
        for other in ["notes.txt", "a.b.key", ".key"] {
            fs::write(dir.path().join("keys").join(other), b"x").unwrap();
        }
        assert_eq!(store.list().unwrap(), [A, B]);
    }

    #[test]
    fn delete_is_ok_for_a_missing_key() {
        let dir = tempfile::tempdir().unwrap();
        let store = DevKeys::new(dir.path().to_path_buf());
        store.delete(A).unwrap();
        store.create(A, &key(1)).unwrap();
        store.delete(A).unwrap();
        assert!(store.load(A).unwrap().is_none());
        store.delete(A).unwrap();
    }

    #[test]
    fn a_file_that_isnt_a_key_is_corrupt_and_its_text_isnt_shown() {
        let dir = tempfile::tempdir().unwrap();
        let store = DevKeys::new(dir.path().to_path_buf());
        let path = dir.path().join(format!("{A}.key"));
        let hex = "0123456789abcdef".repeat(4);
        for bad in [
            String::new(),
            hex[..62].to_owned(),
            format!("{hex}00"),
            format!("{}g", &hex[..63]),
            "z".repeat(4096),
        ] {
            fs::write(&path, &bad).unwrap();
            let err = store.load(A).unwrap_err();
            assert!(matches!(err, KeyStoreError::Corrupt), "{bad:?}");
            assert!(!err.to_string().contains("0123"), "{err}");
        }
        // The server's format allowed a trailing newline and uppercase digits.
        fs::write(&path, format!("{}\n", hex.to_uppercase())).unwrap();
        assert!(store.load(A).unwrap().is_some());
    }

    #[test]
    fn a_symlink_or_fifo_isnt_a_key_and_an_existing_folder_is_made_owner_only() {
        let dir = tempfile::tempdir().unwrap();
        let keys = dir.path().join("keys");
        fs::create_dir(&keys).unwrap();
        fs::set_permissions(&keys, fs::Permissions::from_mode(0o755)).unwrap();
        let store = DevKeys::new(keys.clone());
        store.create(A, &key(1)).unwrap();
        assert_eq!(
            fs::metadata(&keys).unwrap().permissions().mode() & 0o777,
            0o700
        );

        const B: &str = "01BBBBBBBBBBBBBBBBBBBBBBBB";
        std::os::unix::fs::symlink(keys.join(format!("{A}.key")), keys.join(format!("{B}.key")))
            .unwrap();
        assert!(matches!(store.load(B), Err(KeyStoreError::Io(_))));
        const C: &str = "01CCCCCCCCCCCCCCCCCCCCCCCC";
        let status = std::process::Command::new("/usr/bin/mkfifo")
            .arg(keys.join(format!("{C}.key")))
            .status()
            .unwrap();
        assert!(status.success());
        assert!(matches!(store.load(C), Err(KeyStoreError::Corrupt)));
        // A key folder that's a file isn't mistaken for an existing key.
        let file = dir.path().join("a-file");
        fs::write(&file, b"").unwrap();
        let err = DevKeys::new(file).create(A, &key(1)).unwrap_err();
        assert!(matches!(err, KeyStoreError::Io(_)), "{err}");
    }

    #[test]
    fn ids_that_arent_plain_are_refused() {
        let dir = tempfile::tempdir().unwrap();
        let store = DevKeys::new(dir.path().to_path_buf());
        for id in ["", "../x", "a.b", "a/b"] {
            assert!(matches!(
                store.create(id, &key(1)),
                Err(KeyStoreError::NotPlainId)
            ));
            assert!(matches!(store.load(id), Err(KeyStoreError::NotPlainId)));
            assert!(matches!(store.delete(id), Err(KeyStoreError::NotPlainId)));
        }
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 0);
    }
}
