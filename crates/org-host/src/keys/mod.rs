//! Key custody: where the host keeps every office's secrets key.
//!
//! The host creates each office's key (`secrets::generate_office_secrets_key`), stores it here before the
//! office's first `--create`, loads it before every `--office` and `--discard`, and sends it on
//! the first line of the launch pipe (`org-office-launch`). Its servers never touch a key store.
//! A key is in the host's memory only from its creation or load until that first line is written.
//!
//! [`OfficeSecretsKeys`] is the host's own contract: the supervisor and provisioning depend on it,
//! never on a particular store or system. It is a typed view (`custody`) over one credential
//! store of the platform (`org_platform_host::credentials`): the view keeps the name rule, the
//! add-only rule and what a key looks like; the platform keeps the bytes. Which store a build
//! uses is decided here:
//! - **Release builds:** the system's protected store, whichever kind it is
//!   (`org_platform_host::credentials::protected_store`). The Data Protection keychain, in the
//!   host's access group, so other code gets `errSecItemNotFound`, never a prompt; or the
//!   credential directory setup recorded, which the host opened and checked at its start
//!   (`credential_store_root`). A system without such a store gets the refusing view, which refuses
//!   every call with a reported reason ([`KeyStoreError::Unsupported`]). It is chosen before any
//!   Keychain precondition is checked, so such a host never reports a Keychain-only reason (no
//!   access group, not hardened).
//! - **Debug builds:** plain files in a development folder (the platform's directory store), so
//!   a debug host and the tests never write a real Keychain item. A debug host without that
//!   folder refuses to start.
//! - **Unit tests:** an in-memory store with fault injection (`memory`).
//!
//! The rules every store follows, and the host relies on:
//! - `create` only adds: it never replaces an existing item ([`KeyStoreError::Exists`]), and on
//!   any other error it leaves no item behind.
//! - `delete` of a missing item is `Ok`.
//! - The host deletes an office's key **only** after that office's `--discard` reported
//!   `discarded`: a database left without its key could never be checked or discarded. The one
//!   exception is the orphan-key sweep (`sweep`, release builds only): a key whose office has
//!   left nothing in the home, still so at a start 7 days or more after it was first found so.
//! - No error ever shows key bytes: [`KeyStoreError`] holds only codes and kinds.

mod custody;
#[cfg(test)]
pub(crate) mod memory;
mod sweep;

pub(crate) use custody::RELEASE_ITEM_STORE;
pub(crate) use sweep::sweep_orphans;

use std::fmt;
use std::io;
use std::path::Path;
use std::sync::Arc;

use org_platform::Refused;
use org_platform_host::credentials::{
    credentials, CredentialLocation, KeychainItems, ProtectedStore, ReleaseRoot,
};
use org_proto::OfficeSecretsKey;

use crate::code_signing::{hardened_and_clean, Signing};

/// The Keychain service every office key is filed under (ADR-0002); the account is the office
/// id. It is persisted state: changing it later orphans every office's key. (The server's former
/// items used `org.agentic.office`; this store never reads them.) A registered instance's keys
/// are filed under [`keychain_service`] of its id instead.
pub(crate) const SERVICE: &str = "org.agentic.office-key";

/// The Keychain service of the office keys of instance `instance_id`, or of the host that
/// serves no registered instance.
fn keychain_service(instance_id: Option<&str>) -> String {
    match instance_id {
        Some(id) => format!("{SERVICE}.instance.{id}"),
        None => SERVICE.to_owned(),
    }
}

/// Where the host keeps every office's secrets key (see the module docs). Office ids are plain
/// ids ([`org_office_launch::is_plain_id`]); a store refuses any other
/// ([`KeyStoreError::NotPlainId`]).
pub(crate) trait OfficeSecretsKeys: Send + Sync {
    /// Stores `key` as office `office_id`'s key. Add-only: an existing item is
    /// [`KeyStoreError::Exists`] and is left as it is. On any other error no key is replaced
    /// and no partial item is left. One error can follow a complete store: in the
    /// development store, a directory flush that fails after the key was published. The key
    /// then exists although `create` failed; it belongs to no started office and the orphan
    /// sweep removes it.
    fn create(&self, office_id: &str, key: &OfficeSecretsKey) -> Result<(), KeyStoreError>;

    /// Office `office_id`'s key, or `None` if the store has no item for it. An item that isn't a
    /// key (the wrong length, not hex) is [`KeyStoreError::Corrupt`]. An `OfficeSecretsKey` keeps its bytes
    /// on the heap, so moving it moves a pointer; drop it as soon as its line is written.
    fn load(&self, office_id: &str) -> Result<Option<OfficeSecretsKey>, KeyStoreError>;

    /// Deletes office `office_id`'s key. A missing item is `Ok`.
    fn delete(&self, office_id: &str) -> Result<(), KeyStoreError>;

    /// The office ids the store holds a key for, never the keys themselves. Items whose name
    /// isn't a plain id are left out.
    fn list(&self) -> Result<Vec<String>, KeyStoreError>;
}

/// Why a key store call failed. It never holds key bytes, and neither does its `Display`.
#[derive(Debug)]
pub(crate) enum KeyStoreError {
    /// `create`: the store already has an item for that office (it was left as it is).
    Exists,
    /// `load`: the item isn't a key.
    Corrupt,
    /// The office id isn't a plain id.
    NotPlainId,
    /// The credential store is locked or user interaction is not allowed.
    InteractionNotAllowed,
    /// The Keychain failed with this `OSStatus`.
    Os(i32),
    /// The development store's file I/O failed, or one of its checks refused. The error holds a
    /// kind and a fixed text, never a path.
    Io(io::Error),
    /// This system has no credential store to keep an office secrets key in
    /// (`org_platform_host::credentials` refused).
    Unsupported,
}

impl KeyStoreError {
    /// Whether the office's key is to be treated as missing: only an item that isn't a key.
    /// Every other error, a refused platform included, is a store failure.
    pub(crate) fn means_no_key(&self) -> bool {
        matches!(self, Self::Corrupt)
    }
}

impl fmt::Display for KeyStoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            KeyStoreError::Exists => f.write_str("the key store already has a key for that office"),
            KeyStoreError::Corrupt => f.write_str("the stored item isn't an office secrets key"),
            KeyStoreError::NotPlainId => f.write_str("the office id isn't a plain id"),
            KeyStoreError::InteractionNotAllowed => {
                f.write_str("the key store is locked or user interaction is not allowed")
            }
            KeyStoreError::Os(status) => write!(f, "the Keychain failed (OSStatus {status})"),
            KeyStoreError::Io(e) => write!(f, "the key folder: {e}"),
            KeyStoreError::Unsupported => {
                f.write_str("this operating system has no office secrets key store")
            }
        }
    }
}

impl std::error::Error for KeyStoreError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            KeyStoreError::Io(e) => Some(e),
            _ => None,
        }
    }
}

impl From<io::Error> for KeyStoreError {
    fn from(e: io::Error) -> Self {
        KeyStoreError::Io(e)
    }
}

/// Why no key store could be chosen (the host maps it to its own startup error).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum StoreError {
    /// A debug build has no development key folder (absolute, outside the office home).
    #[cfg_attr(
        not(debug_assertions),
        allow(
            dead_code,
            reason = "release builds never select a development key directory"
        )
    )]
    NoDevKeyDir,
    /// A release build has no Keychain access group compiled in.
    NoKeychainGroup,
    /// A release build's access group isn't in its own `keychain-access-groups` entitlement.
    KeychainGroupNotEntitled { group: String },
    /// A release build's own signature can't be read, or it isn't signed with the hardened
    /// runtime, or is entitled to let other code into it (`code_signing::REJECTED_ENTITLEMENTS`):
    /// any same-user process could then read the keys out of it.
    #[cfg_attr(
        debug_assertions,
        allow(
            dead_code,
            reason = "debug builds do not run release Keychain hardening checks; unit tests cover it"
        )
    )]
    NotHardened,
    /// A release build on a system that keeps its secrets in a credential directory has none
    /// open: it serves no registered instance, and only setup records the directory.
    NoCredentialDirectory,
    /// The office keys' item store could not be opened in the credential directory, for this
    /// reason (a kind, never a path: the directory itself was checked when the host opened it).
    CredentialDirectory(String),
}

/// The key store this host uses. Debug builds: the development folder `dev_key_dir`
/// (absolute, outside the office `home`), and [`StoreError::NoDevKeyDir`] without one, so a
/// debug host never writes a real Keychain item. Release builds on a system with no protected
/// store: the refusing view ([`KeyStoreError::Unsupported`] on every call), before any of
/// the checks below. Release builds whose protected store is the Keychain: the Data Protection
/// keychain in the access group compiled in as `ORG_HOST_KEYCHAIN_GROUP`:
/// [`StoreError::NoKeychainGroup`] without one. `own_signing` (the host's own signing
/// information, read once) must show both that group in its `keychain-access-groups` entitlement
/// ([`StoreError::KeychainGroupNotEntitled`] otherwise, so a misbuilt host fails at startup
/// rather than at its first `SecItemAdd`) and the hardened runtime with none of the
/// entitlements that let other code in ([`StoreError::NotHardened`] otherwise: an injectable
/// host would give every key away). A release build ignores `dev_key_dir`.
///
/// Where a release build's protected store is a credential directory instead, the store is
/// the office keys' item store below `release_directory`, the directory the host opened and
/// checked at its start ([`StoreError::NoCredentialDirectory`] without one). The system says
/// which kind it has; nothing here names a system. A debug build given a `release_directory`
/// (one its setup record names) uses that item store as well, and its development folder
/// only without one.
pub(crate) fn store_for(
    dev_key_dir: Option<&Path>,
    home: &Path,
    instance_id: Option<&str>,
    own_signing: impl FnOnce() -> Option<Signing>,
    release_directory: Option<&ReleaseRoot>,
) -> Result<Arc<dyn OfficeSecretsKeys>, StoreError> {
    #[cfg(debug_assertions)]
    {
        let _ = own_signing;
        // A credential directory the host opened (its setup record names one) holds this
        // build's keys as it holds a release build's; only without one is it the
        // development folder.
        match release_directory {
            Some(root) => selected_release_store(
                Ok(ProtectedStore::ReleaseDirectory),
                None,
                instance_id,
                || None,
                Some(root),
            ),
            None => dev_store(dev_key_dir, home, instance_id),
        }
    }
    #[cfg(not(debug_assertions))]
    {
        let _ = (dev_key_dir, home);
        selected_release_store(
            org_platform_host::credentials::protected_store(),
            option_env!("ORG_HOST_KEYCHAIN_GROUP"),
            instance_id,
            own_signing,
            release_directory,
        )
    }
}

/// A release build's store on a system whose protected store is `store`: the office keys
/// below the open release directory where it is one, and the Keychain store where it is the
/// Keychain. A system without a protected store gets the refusing view, which refuses
/// every call ([`KeyStoreError::Unsupported`]): it is chosen before any Keychain check, so
/// `group` and `own_signing` are not consulted and no Keychain-only reason is reported there.
pub(crate) fn selected_release_store(
    store: Result<ProtectedStore, Refused>,
    group: Option<&str>,
    instance_id: Option<&str>,
    own_signing: impl FnOnce() -> Option<Signing>,
    release_directory: Option<&ReleaseRoot>,
) -> Result<Arc<dyn OfficeSecretsKeys>, StoreError> {
    match store {
        Ok(ProtectedStore::ReleaseDirectory) => {
            let root = release_directory.ok_or(StoreError::NoCredentialDirectory)?;
            match custody::OfficeKeys::release_directory(root) {
                Ok(keys) => Ok(Arc::new(keys)),
                // Kinds only: no item operation or item store names a path.
                Err(e) => Err(StoreError::CredentialDirectory(e.to_string())),
            }
        }
        Ok(ProtectedStore::Keychain) => release_store_scoped(group, instance_id, own_signing),
        Err(_) => Ok(Arc::new(custody::UnsupportedKeys)),
    }
}

/// The development store in `dir`, or for a registered instance in `dir/<instance id>`. That
/// folder must be absolute and outside the office `home`: inside `offices/` it would look like an
/// unfinished office (and be discarded with every key in it), and anywhere in the home it would
/// put plain-text keys there. Nothing is opened or made here: the store opens its folder at
/// its first use.
#[cfg(debug_assertions)]
fn dev_store(
    dir: Option<&Path>,
    home: &Path,
    instance_id: Option<&str>,
) -> Result<Arc<dyn OfficeSecretsKeys>, StoreError> {
    let inside = |dir: &Path| {
        let lexical = dir.starts_with(home);
        let canonical = match (dir.canonicalize(), home.canonicalize()) {
            (Ok(dir), Ok(home)) => dir.starts_with(home),
            _ => false,
        };
        lexical || canonical
    };
    let Some(dir) = dir else {
        return Err(StoreError::NoDevKeyDir);
    };
    let scoped = match instance_id {
        Some(id) => dir.join(id),
        None => dir.to_owned(),
    };
    if scoped.is_absolute() && !inside(&scoped) {
        Ok(Arc::new(custody::OfficeKeys::development(dir, instance_id)))
    } else {
        Err(StoreError::NoDevKeyDir)
    }
}

/// The release store for the access group `group` (`ORG_HOST_KEYCHAIN_GROUP` at build time),
/// once the host's own signing information (`own_signing`) shows it may use it and can be
/// trusted with the keys ([`self_trusted`]).
///
/// The live check can't run in the tests: an unsigned (or ad-hoc signed) test binary has no
/// `keychain-access-groups` and no hardened runtime, so it would always refuse. The decision,
/// [`self_trusted`], is tested on synthetic signing information; the reading is
/// `code_signing`'s, tested the same way. Check it live on the first signed build.
#[cfg(test)]
fn release_store(
    group: Option<&str>,
    own_signing: impl FnOnce() -> Option<Signing>,
) -> Result<Arc<dyn OfficeSecretsKeys>, StoreError> {
    release_store_scoped(group, None, own_signing)
}

#[cfg_attr(
    debug_assertions,
    allow(dead_code, reason = "called in release builds and debug unit tests")
)]
fn release_store_scoped(
    group: Option<&str>,
    instance_id: Option<&str>,
    own_signing: impl FnOnce() -> Option<Signing>,
) -> Result<Arc<dyn OfficeSecretsKeys>, StoreError> {
    let group = match group {
        Some(group) if !group.trim().is_empty() => group,
        _ => return Err(StoreError::NoKeychainGroup),
    };
    self_trusted(group, own_signing().as_ref())?;
    // Opening the Keychain store reads and writes nothing.
    match credentials(CredentialLocation::Keychain(KeychainItems {
        access_group: group,
        service: &keychain_service(instance_id),
    })) {
        Ok(store) => Ok(Arc::new(custody::OfficeKeys::keychain(store))),
        // A system without a protected store has no access group to keep keys in.
        Err(_) => Err(StoreError::NoKeychainGroup),
    }
}

/// The decision, pure: a host whose own signing information is `own` may keep keys in the
/// access group `group` only if that group is in its `keychain-access-groups`, and it runs
/// hardened with none of the entitlements that let other code in
/// (`code_signing::hardened_and_clean`). Information that couldn't be read trusts nothing.
#[cfg_attr(
    debug_assertions,
    allow(dead_code, reason = "called in release builds and debug unit tests")
)]
fn self_trusted(group: &str, own: Option<&Signing>) -> Result<(), StoreError> {
    // Its own signature can't be read: nothing about it can be trusted.
    let Some(own) = own else {
        return Err(StoreError::NotHardened);
    };
    let entitled = own.keychain_access_groups.as_deref();
    if !group_entitled(group, entitled) {
        return Err(StoreError::KeychainGroupNotEntitled {
            group: group.to_owned(),
        });
    }
    if !hardened_and_clean(own) {
        return Err(StoreError::NotHardened);
    }
    Ok(())
}

/// Whether the access group `group` is one of the host's own `entitled` groups (exactly). No
/// entitlement, or one that couldn't be read, entitles nothing.
#[cfg_attr(
    debug_assertions,
    allow(dead_code, reason = "called in release builds and debug unit tests")
)]
fn group_entitled(group: &str, entitled: Option<&[String]>) -> bool {
    entitled.is_some_and(|groups| groups.iter().any(|g| g == group))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(debug_assertions)]
    #[test]
    fn instance_key_namespaces_and_orphan_sweeps_are_independent() {
        use std::time::{Duration, UNIX_EPOCH};
        let fixture = tempfile::tempdir().unwrap();
        let home_a = fixture.path().join("a");
        let home_b = fixture.path().join("b");
        std::fs::create_dir(&home_a).unwrap();
        std::fs::create_dir(&home_b).unwrap();
        let key_base = fixture.path().join("keys");
        let a = store_for(
            Some(&key_base),
            &home_a,
            Some("INSTANCEA"),
            || panic!("debug never signs"),
            None,
        )
        .unwrap();
        let b = store_for(
            Some(&key_base),
            &home_b,
            Some("INSTANCEB"),
            || panic!("debug never signs"),
            None,
        )
        .unwrap();
        let office = "SAMEOFFICE";
        a.create(office, &OfficeSecretsKey::from_bytes([1; 32]))
            .unwrap();
        b.create(office, &OfficeSecretsKey::from_bytes([2; 32]))
            .unwrap();
        assert_eq!(a.list().unwrap(), vec![office]);
        assert_eq!(b.list().unwrap(), vec![office]);
        let before = b.load(office).unwrap().unwrap();
        let now = UNIX_EPOCH + Duration::from_secs(1_800_000_000);
        assert!(sweep_orphans(&*a, &home_a, |ids| Ok(ids.to_vec()), now).is_empty());
        assert_eq!(
            sweep_orphans(
                &*a,
                &home_a,
                |ids| Ok(ids.to_vec()),
                now + Duration::from_secs(8 * 86400)
            ),
            vec![office]
        );
        assert!(a.load(office).unwrap().is_none());
        assert_eq!(b.load(office).unwrap().unwrap(), before);
        assert!(!home_b.join("orphan-keys.json").exists());
        b.delete(office).unwrap();
        assert!(b.list().unwrap().is_empty());
    }

    #[test]
    fn only_a_corrupt_item_means_the_key_is_missing() {
        assert!(KeyStoreError::Corrupt.means_no_key());
        for error in [
            KeyStoreError::Exists,
            KeyStoreError::NotPlainId,
            KeyStoreError::Os(-25308),
            KeyStoreError::Io(io::Error::from(io::ErrorKind::PermissionDenied)),
            KeyStoreError::Unsupported,
        ] {
            assert!(!error.means_no_key(), "{error:?}");
        }
    }

    #[test]
    fn errors_never_show_more_than_codes() {
        let shown = [
            KeyStoreError::Exists,
            KeyStoreError::Corrupt,
            KeyStoreError::NotPlainId,
            KeyStoreError::Os(-25308),
            KeyStoreError::Io(io::Error::from(io::ErrorKind::PermissionDenied)),
            KeyStoreError::Unsupported,
        ]
        .map(|e| e.to_string());
        assert!(shown[3].contains("-25308"), "{}", shown[3]);
        for text in shown {
            assert!(!text.is_empty());
            assert!(!text.bytes().any(|b| b == b'\n'), "{text}");
        }
    }

    const GROUP: &str = "TEAMID1234.org.agentic.host";
    const HARDENED: u32 = crate::code_signing::HARDENED_RUNTIME;

    /// Signing information with these `flags`, access `groups` and boolean `entitlements`.
    fn signing(flags: Option<u32>, groups: &[&str], entitlements: &[(&str, bool)]) -> Signing {
        Signing {
            flags,
            entitlements: Some(
                entitlements
                    .iter()
                    .map(|(key, value)| ((*key).to_owned(), Some(*value)))
                    .collect(),
            ),
            keychain_access_groups: Some(groups.iter().map(|g| (*g).to_owned()).collect()),
        }
    }

    #[test]
    fn a_release_build_without_an_access_group_has_no_store() {
        let mut asked = false;
        for group in [None, Some(""), Some("  ")] {
            let store = release_store(group, || {
                asked = true;
                Some(signing(Some(HARDENED), &[GROUP], &[]))
            });
            assert!(
                matches!(store, Err(StoreError::NoKeychainGroup)),
                "{group:?}"
            );
        }
        assert!(!asked, "the signing information was read without a group");
    }

    #[test]
    fn a_release_build_needs_its_access_group_in_its_own_entitlement() {
        for groups in [
            vec![],
            vec!["TEAMID1234.other"],
            vec![
                "TEAMID1234.org.agentic.host.x",
                "teamid1234.org.agentic.host",
            ],
        ] {
            let own = signing(Some(HARDENED), &groups, &[]);
            let err = self_trusted(GROUP, Some(&own)).unwrap_err();
            assert_eq!(
                err,
                StoreError::KeychainGroupNotEntitled {
                    group: GROUP.to_owned()
                },
                "{groups:?}"
            );
        }
        // Signing information that couldn't be read trusts nothing.
        assert_eq!(self_trusted(GROUP, None), Err(StoreError::NotHardened));
        let entitled = signing(Some(HARDENED), &["TEAMID1234.other", GROUP], &[]);
        assert_eq!(self_trusted(GROUP, Some(&entitled)), Ok(()));
        // Choosing the store touches no Keychain item. A system whose protected store is not
        // the Keychain has none to choose.
        let chosen = release_store(Some(GROUP), || Some(entitled));
        match org_platform_host::credentials::protected_store() {
            Ok(ProtectedStore::Keychain) => assert!(chosen.is_ok()),
            _ => assert!(matches!(chosen, Err(StoreError::NoKeychainGroup))),
        }
    }

    /// The release store this system's protected store decides, for an entitled, hardened host
    /// with its access group compiled in and no release directory open: one outcome per kind
    /// of store. Where it is the Keychain, choosing it touches no item. Where it is a
    /// credential directory, the Keychain preconditions are never consulted. A system with no
    /// protected store gets the refusing view, never a fallback store.
    #[test]
    fn the_release_adapter_for_this_target_is_chosen_without_touching_a_store() {
        let mut asked = false;
        let keys = selected_release_store(
            org_platform_host::credentials::protected_store(),
            Some(GROUP),
            Some("INSTANCEA"),
            || {
                asked = true;
                Some(signing(Some(HARDENED), &[GROUP], &[]))
            },
            None,
        );
        match org_platform_host::credentials::protected_store() {
            Ok(ProtectedStore::Keychain) => {
                assert!(keys.is_ok());
                assert!(
                    asked,
                    "the Keychain was chosen without the host's own signing"
                );
            }
            Ok(ProtectedStore::ReleaseDirectory) => {
                assert!(matches!(keys, Err(StoreError::NoCredentialDirectory)));
                assert!(!asked, "the signing information was read");
            }
            Err(_) => {
                let keys = keys.unwrap_or_else(|e| panic!("{e:?}"));
                assert!(matches!(keys.list(), Err(KeyStoreError::Unsupported)));
                assert!(!asked, "the signing information was read");
            }
        }
    }

    /// Another operating system (one with no protected store) gets the refusing view whatever
    /// the group and signing: no Keychain reason (a missing group, an unsigned host) is ever
    /// reported there, and the signing information is never read. The system is given as the
    /// platform's answer, so this holds on every build target.
    #[test]
    fn another_operating_system_gets_the_refusing_adapter_before_any_keychain_check() {
        let refused = || Refused::Unsupported {
            capability: "credentials",
            os: std::env::consts::OS,
        };
        for group in [None, Some(""), Some(GROUP)] {
            let mut asked = false;
            let keys = selected_release_store(
                Err(refused()),
                group,
                Some("INSTANCEA"),
                || {
                    asked = true;
                    None
                },
                None,
            )
            .unwrap_or_else(|e| panic!("{group:?}: {e:?}"));
            assert!(!asked, "the signing information was read");
            assert!(matches!(keys.list(), Err(KeyStoreError::Unsupported)));
            assert!(matches!(
                keys.load("01JAAAAAAAAAAAAAAAAAAAAAAA"),
                Err(KeyStoreError::Unsupported)
            ));
        }
    }

    #[test]
    fn the_service_is_the_one_the_adr_fixes_and_an_instance_has_its_own() {
        // Persisted state: changing it orphans every office's key (ADR-0002).
        assert_eq!(SERVICE, "org.agentic.office-key");
        assert_eq!(keychain_service(None), "org.agentic.office-key");
        assert_eq!(
            keychain_service(Some("INSTANCEA")),
            "org.agentic.office-key.instance.INSTANCEA"
        );
        assert_ne!(
            keychain_service(Some("INSTANCEA")),
            keychain_service(Some("INSTANCEB"))
        );
    }

    #[test]
    fn ids_that_arent_plain_are_refused_before_the_keychain() {
        let entitled = signing(Some(HARDENED), &[GROUP], &[]);
        // Where the protected store is not the Keychain there is none to keep them from.
        let Ok(store) = release_store(Some(GROUP), || Some(entitled)) else {
            assert_ne!(
                org_platform_host::credentials::protected_store(),
                Ok(ProtectedStore::Keychain)
            );
            return;
        };
        let key = OfficeSecretsKey::from_bytes([1; 32]);
        for id in ["", "../x", "a b"] {
            assert!(matches!(
                store.create(id, &key),
                Err(KeyStoreError::NotPlainId)
            ));
            assert!(matches!(store.load(id), Err(KeyStoreError::NotPlainId)));
            assert!(matches!(store.delete(id), Err(KeyStoreError::NotPlainId)));
        }
    }

    /// M1: an entitled host that isn't hardened, or lets other code in, could give every key
    /// away: refused.
    #[test]
    fn a_release_build_must_itself_be_hardened_and_closed_to_other_code() {
        let entitled =
            |flags, entitlements: &[(&str, bool)]| signing(flags, &[GROUP], entitlements);
        for own in [
            entitled(None, &[]),
            entitled(Some(0x2), &[]),
            entitled(
                Some(HARDENED),
                &[(
                    "com.apple.security.cs.allow-dyld-environment-variables",
                    true,
                )],
            ),
            entitled(
                Some(HARDENED),
                &[("com.apple.security.cs.disable-library-validation", true)],
            ),
            entitled(
                Some(HARDENED),
                &[("com.apple.security.get-task-allow", true)],
            ),
        ] {
            assert_eq!(
                self_trusted(GROUP, Some(&own)),
                Err(StoreError::NotHardened),
                "{own:?}"
            );
            assert!(matches!(
                release_store(Some(GROUP), || Some(own)),
                Err(StoreError::NotHardened)
            ));
        }
        let clean = entitled(
            Some(HARDENED | 0x2),
            &[("com.apple.security.get-task-allow", false)],
        );
        assert_eq!(self_trusted(GROUP, Some(&clean)), Ok(()));
    }

    #[cfg(debug_assertions)]
    #[test]
    fn a_debug_build_needs_an_absolute_dev_key_dir_outside_the_home() {
        use std::path::Path;
        let home = Path::new("/tmp/org-home-never-made");
        for dir in [
            None,
            Some(Path::new("relative/keys")),
            Some(home),
            Some(Path::new("/tmp/org-home-never-made/offices/keys")),
        ] {
            assert!(
                matches!(dev_store(dir, home, None), Err(StoreError::NoDevKeyDir)),
                "{dir:?}"
            );
        }
        // Choosing the store makes and opens nothing.
        assert!(dev_store(Some(Path::new("/tmp/org-keys-never-made")), home, None).is_ok());
        assert!(!Path::new("/tmp/org-keys-never-made").exists());
        // A registered instance's folder is checked, not the folder it is in.
        assert!(dev_store(Some(Path::new("/tmp")), Path::new("/tmp/INSTANCE"), None).is_ok());
        assert!(matches!(
            dev_store(
                Some(Path::new("/tmp")),
                Path::new("/tmp/INSTANCE"),
                Some("INSTANCE")
            ),
            Err(StoreError::NoDevKeyDir)
        ));
    }
}
