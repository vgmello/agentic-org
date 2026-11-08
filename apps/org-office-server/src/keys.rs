//! Where an office's key lives: the macOS Keychain (account = the office id), or, in a build with
//! the `dev-keys` feature and `ORG_OFFICE_DEV_KEY_DIR` set, plain files, so tests and dev runs
//! never write a real Keychain item. Only `Office::create` makes a key (ADR-0002); a provider is
//! a dumb store that never replaces one.

use org_office::KeyProvider;

/// The provider for this run, or why there is none.
///
/// A build with the `dev-keys` feature NEVER reaches the Keychain: it uses files in
/// `ORG_OFFICE_DEV_KEY_DIR`, and refuses to start without that variable, so a forgotten variable
/// can't write a real item.
pub fn provider() -> Result<Box<dyn KeyProvider>, String> {
    #[cfg(feature = "dev-keys")]
    {
        match std::env::var_os("ORG_OFFICE_DEV_KEY_DIR") {
            Some(dir) => Ok(Box::new(dev::FileKeys::new(dir.into()))),
            None => Err("a dev-keys build needs ORG_OFFICE_DEV_KEY_DIR".to_owned()),
        }
    }
    #[cfg(not(feature = "dev-keys"))]
    keychain()
}

#[cfg(all(target_os = "macos", not(feature = "dev-keys")))]
fn keychain() -> Result<Box<dyn KeyProvider>, String> {
    Ok(Box::new(mac::Keychain))
}

#[cfg(all(not(target_os = "macos"), not(feature = "dev-keys")))]
fn keychain() -> Result<Box<dyn KeyProvider>, String> {
    Err("this platform has no Keychain".to_owned())
}

#[cfg(target_os = "macos")]
#[cfg_attr(feature = "dev-keys", allow(dead_code))]
mod mac {
    use core_foundation::data::CFData;
    use org_office::{Key, KeyError, KeyProvider};
    use security_framework::item::{ItemAddOptions, ItemAddValue, ItemClass};
    use security_framework::passwords::{delete_generic_password, get_generic_password};

    /// The Keychain service every office key is filed under (ADR-0002); the account is the
    /// office id. It is persisted state: changing it later would orphan every office's key.
    pub(super) const SERVICE: &str = "org.agentic.office";
    /// `errSecItemNotFound`.
    const NOT_FOUND: i32 = -25300;
    /// `errSecDuplicateItem`.
    const DUPLICATE: i32 = -25299;

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
            // `SecItemAdd` only: unlike `set_generic_password` it never updates an existing
            // item, so a key is never replaced, even by a racing process.
            let mut item = ItemAddOptions::new(ItemAddValue::Data {
                class: ItemClass::generic_password(),
                data: CFData::from_buffer(key.as_bytes()),
            });
            item.set_service(SERVICE).set_account_name(office_id);
            match item.add() {
                Ok(()) => Ok(()),
                Err(e) if e.code() == DUPLICATE => {
                    Err("a key for this office already exists".into())
                }
                Err(e) => Err(e.to_string().into()),
            }
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

#[cfg(all(test, target_os = "macos"))]
mod tests {
    #[test]
    fn the_keychain_names_are_the_ones_the_adr_fixes() {
        // Persisted state: an office's key is filed under this service and its id. Changing
        // either orphans every key, so a change must be deliberate (ADR-0002).
        assert_eq!(super::mac::SERVICE, "org.agentic.office");
    }
}
