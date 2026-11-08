//! Where an office's key lives: the macOS Keychain (account = the office id), or, in a build with
//! the `dev-keys` feature and `ORG_OFFICE_DEV_KEY_DIR` set, plain files, so tests and dev runs
//! never write a real Keychain item. Only `Office::create` makes a key (ADR-0002); a provider is
//! a dumb store that never replaces one.

use org_office::KeyProvider;

/// The provider for this run, or why there is none.
pub fn provider() -> Result<Box<dyn KeyProvider>, String> {
    #[cfg(feature = "dev-keys")]
    if let Some(dir) = std::env::var_os("ORG_OFFICE_DEV_KEY_DIR") {
        return Ok(Box::new(dev::FileKeys::new(dir.into())));
    }
    keychain()
}

#[cfg(target_os = "macos")]
fn keychain() -> Result<Box<dyn KeyProvider>, String> {
    Ok(Box::new(mac::Keychain))
}

#[cfg(not(target_os = "macos"))]
fn keychain() -> Result<Box<dyn KeyProvider>, String> {
    Err("this platform has no Keychain".to_owned())
}

#[cfg(target_os = "macos")]
mod mac {
    use org_office::{Key, KeyError, KeyProvider};
    use security_framework::passwords::{
        delete_generic_password, get_generic_password, set_generic_password,
    };

    /// The Keychain service every office key is filed under; the account is the office id.
    const SERVICE: &str = "agentic.org office key";
    /// `errSecItemNotFound`.
    const NOT_FOUND: i32 = -25300;

    pub(super) struct Keychain;

    impl KeyProvider for Keychain {
        fn load(&self, office_id: &str) -> Result<Option<Key>, KeyError> {
            match get_generic_password(SERVICE, office_id) {
                Ok(bytes) => {
                    let bytes: [u8; 32] = bytes
                        .try_into()
                        .map_err(|_| KeyError::from("the stored key has the wrong length"))?;
                    Ok(Some(Key::from_bytes(bytes)))
                }
                Err(e) if e.code() == NOT_FOUND => Ok(None),
                // The OS error text names no secret.
                Err(e) => Err(e.to_string().into()),
            }
        }

        fn store(&self, office_id: &str, key: &Key) -> Result<(), KeyError> {
            // `set_generic_password` would update an existing item: never replace a key.
            if self.load(office_id)?.is_some() {
                return Err("a key for this office already exists".into());
            }
            set_generic_password(SERVICE, office_id, key.as_bytes())
                .map_err(|e| KeyError::from(e.to_string()))
        }

        fn delete(&self, office_id: &str) -> Result<(), KeyError> {
            match delete_generic_password(SERVICE, office_id) {
                Ok(()) => Ok(()),
                Err(e) if e.code() == NOT_FOUND => Ok(()),
                Err(e) => Err(e.to_string().into()),
            }
        }
    }
}

#[cfg(feature = "dev-keys")]
mod dev {
    use std::fs::{self, OpenOptions};
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    use std::path::PathBuf;

    use org_office::{Key, KeyError, KeyProvider};

    /// Keys as owner-only files `<dir>/<office id>.key` (64 hex digits). **Dev and tests only.**
    pub(super) struct FileKeys {
        dir: PathBuf,
    }

    impl FileKeys {
        pub(super) fn new(dir: PathBuf) -> Self {
            Self { dir }
        }

        fn path(&self, office_id: &str) -> Result<PathBuf, KeyError> {
            // Office ids are plain (the callers check), but this is a path: be sure.
            if office_id.is_empty() || !office_id.bytes().all(|b| b.is_ascii_alphanumeric()) {
                return Err("not a plain office id".into());
            }
            Ok(self.dir.join(format!("{office_id}.key")))
        }
    }

    impl KeyProvider for FileKeys {
        fn load(&self, office_id: &str) -> Result<Option<Key>, KeyError> {
            let path = self.path(office_id)?;
            let text = match fs::read_to_string(&path) {
                Ok(text) => text,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
                Err(e) => return Err(e.into()),
            };
            let text = text.trim();
            if text.len() != 64 || !text.bytes().all(|b| b.is_ascii_hexdigit()) {
                return Err("the stored key is not 64 hex digits".into());
            }
            let mut bytes = [0u8; 32];
            for (i, byte) in bytes.iter_mut().enumerate() {
                *byte = u8::from_str_radix(&text[2 * i..2 * i + 2], 16)?;
            }
            Ok(Some(Key::from_bytes(bytes)))
        }

        fn store(&self, office_id: &str, key: &Key) -> Result<(), KeyError> {
            let path = self.path(office_id)?;
            fs::create_dir_all(&self.dir)?;
            let mut hex = String::with_capacity(64);
            for byte in key.as_bytes() {
                hex.push_str(&format!("{byte:02x}"));
            }
            // `create_new`: never replaces a key.
            let mut file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(path)?;
            file.write_all(hex.as_bytes())?;
            file.sync_all()?;
            Ok(())
        }

        fn delete(&self, office_id: &str) -> Result<(), KeyError> {
            match fs::remove_file(self.path(office_id)?) {
                Ok(()) => Ok(()),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
                Err(e) => Err(e.into()),
            }
        }
    }
}
