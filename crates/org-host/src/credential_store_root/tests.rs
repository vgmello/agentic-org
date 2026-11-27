//! The host on a release directory: the opener, the two views below one open directory, and
//! LNX-4 on a real instance root.
//!
//! The store is opened through the platform's test entry, which starts the walk at the
//! test's own directory (a shared temporary directory is writable by others, which the
//! ancestor rule refuses on purpose); every check from there down is the host's. One test
//! opens it as a host does, from `/`, where this machine has a directory that passes.
//! On a system whose protected store is not a release directory these tests assert that
//! none is opened, and stop.
//!
//! A credential directory that is a bind mount of a directory in the data root (the
//! container mistake `-v ./agentic:/data -v ./agentic/secrets:/secrets`) is tested on a real
//! kernel in the platform crate, where the mount calls are (`credentials/linux/release/tests.rs`,
//! as root); the host gives the platform the same held root descriptor in every case here.

use std::fs;
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use org_platform_host::credentials::{protected_store, ProtectedStore, ReleaseRoot};
use org_platform_host::testing::credentials::release_directory_below_with_stores;
use org_proto::OfficeSecretsKey;

use super::*;
use crate::host_secrets::{HostSecrets, Name, SelectionError, Value};
use crate::instance::registry::{Instance, Registry};
use crate::keys::{OfficeSecretsKeys, StoreError};

const OFFICE: &str = "01AAAAAAAAAAAAAAAAAAAAAAAA";

/// A key no other file of a test can hold by accident.
fn key_bytes() -> [u8; 32] {
    std::array::from_fn(|i| 0x80 | (i as u8).wrapping_mul(7))
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn directory(path: &Path) {
    fs::create_dir(path).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o700)).unwrap();
}

/// Whether this system keeps a release build's secrets in a release directory. Where it
/// does not, no directory is opened whatever the record says, and the test has shown that.
fn release_directory_system() -> bool {
    if protected_store() == Ok(ProtectedStore::ReleaseDirectory) {
        return true;
    }
    let anywhere = fs::File::open("/").unwrap();
    let opened = open(
        true,
        protected_store(),
        Facts {
            instance_id: "unused",
            recorded: Some(Path::new("/no-such-credential-directory")),
            data_root: std::os::fd::AsFd::as_fd(&anywhere),
            foreground: false,
            expect_items: true,
            records: None,
        },
    );
    assert!(matches!(opened, Ok(None)));
    false
}

/// A reserved instance with its layout, as setup leaves it, and a credential directory
/// outside its root, as setup makes one.
struct World {
    base: PathBuf,
    instance: Instance,
    office_home: PathBuf,
    credentials: PathBuf,
    _temp: tempfile::TempDir,
}

impl World {
    fn new() -> Self {
        Self::in_dir(&org_platform::dirs::testing::disk_backed_temp().unwrap())
    }

    fn in_dir(shared: &Path) -> Self {
        let temp = tempfile::tempdir_in(shared).unwrap();
        let base = temp.path().canonicalize().unwrap();
        fs::set_permissions(&base, fs::Permissions::from_mode(0o700)).unwrap();
        directory(&base.join("temp"));
        let registry = Registry::open(&base.join("locator"), &base.join("temp")).unwrap();
        let instance = registry.reserve(None, &base.join("instance")).unwrap();
        let office_home = instance.prepare_layout().unwrap().office_home;
        let mut credentials = base.clone();
        for name in ["state", "agentic-org", "credentials", instance.id()] {
            credentials.push(name);
            directory(&credentials);
        }
        Self {
            base,
            instance,
            office_home,
            credentials,
            _temp: temp,
        }
    }

    fn facts(&self, expect_items: bool) -> Facts<'_> {
        Facts {
            instance_id: self.instance.id(),
            recorded: Some(&self.credentials),
            data_root: self.instance.root_descriptor(),
            foreground: false,
            expect_items,
            records: None,
        }
    }

    /// A release host's open, with the walk started at this world's base.
    fn open(&self, expect_items: bool) -> Result<ReleaseRoot, String> {
        open_with(
            true,
            Ok(ProtectedStore::ReleaseDirectory),
            self.facts(expect_items),
            |location| release_directory_below_with_stores(&self.base, location, &ITEM_STORES),
        )
        .map(|root| root.expect("a release host on a release directory system opens one"))
    }

    /// What the host's records say, by the host's own rule.
    fn open_as_the_host_decides(&self) -> Result<ReleaseRoot, String> {
        self.open(records_say_keys_exist(&self.office_home).unwrap())
    }

    fn keys(&self, root: &ReleaseRoot) -> Arc<dyn OfficeSecretsKeys> {
        crate::keys::selected_release_store(
            Ok(ProtectedStore::ReleaseDirectory),
            None,
            Some(self.instance.id()),
            || panic!("a credential directory needs no signing information"),
            Some(root),
        )
        .unwrap()
    }

    fn secrets(&self, root: &ReleaseRoot) -> Arc<dyn HostSecrets> {
        crate::host_secrets::selected_release_store(
            Ok(ProtectedStore::ReleaseDirectory),
            None,
            self.instance.id(),
            || panic!("a credential directory needs no signing information"),
            Some(root),
        )
        .unwrap_or_else(|e| panic!("{e:?}"))
    }

    /// The folder of an office in the office home, as its server makes it after the host
    /// stored the office's key.
    fn office_folder(&self) -> PathBuf {
        self.office_home
            .join(org_office_launch::OFFICES_DIR_NAME)
            .join(OFFICE)
    }
}

/// Every file below `dir` with its bytes; links are reported and never followed.
fn files(dir: &Path) -> Vec<(PathBuf, Vec<u8>)> {
    let mut found = Vec::new();
    let mut pending = vec![dir.to_owned()];
    while let Some(path) = pending.pop() {
        let meta = fs::symlink_metadata(&path).unwrap();
        if meta.is_dir() {
            for entry in fs::read_dir(&path).unwrap() {
                pending.push(entry.unwrap().path());
            }
        } else if meta.is_file() {
            found.push((path.clone(), fs::read(&path).unwrap()));
        } else {
            found.push((path, Vec::new()));
        }
    }
    found.sort();
    found
}

fn holds(bytes: &[u8], needle: &[u8]) -> bool {
    bytes.windows(needle.len()).any(|window| window == needle)
}

#[test]
fn a_development_build_and_a_keychain_system_open_no_release_directory() {
    let anywhere = fs::File::open("/").unwrap();
    let facts = || Facts {
        instance_id: "fixture-instance",
        recorded: Some(Path::new("/no-such-credential-directory")),
        data_root: std::os::fd::AsFd::as_fd(&anywhere),
        foreground: true,
        expect_items: true,
        records: None,
    };
    let refused = org_platform::Refused::Unsupported {
        capability: "credentials",
        os: std::env::consts::OS,
    };
    let never = |_: ReleaseDirectory<'_>| -> Result<ReleaseRoot, CredentialError> {
        panic!("the platform was asked for a release directory")
    };
    for (release_build, store) in [
        // A development build keeps its development stores where the system's store is not
        // a directory.
        (false, Ok(ProtectedStore::Keychain)),
        (false, Err(refused.clone())),
        // The Keychain is the store; and a system without one is answered where each
        // view's store is chosen (the office keys get the refusing view, the host's
        // secrets are refused).
        (true, Ok(ProtectedStore::Keychain)),
        (true, Err(refused)),
    ] {
        let opened = open_with(release_build, store.clone(), facts(), never);
        assert!(matches!(opened, Ok(None)), "{release_build} {store:?}");
    }
    // Where it is a directory, a development build opens the one its record names, with the
    // release build's checks (X5 review, fourth round), and keeps its development stores
    // only when the record names none.
    let asked = std::cell::Cell::new(false);
    let opened = open_with(
        false,
        Ok(ProtectedStore::ReleaseDirectory),
        facts(),
        |location| {
            asked.set(true);
            assert!(location.expect_items && location.mount_point_required);
            Err(CredentialError::InUse)
        },
    );
    assert!(asked.get() && opened.is_err());
    let opened = open_with(
        false,
        Ok(ProtectedStore::ReleaseDirectory),
        Facts {
            recorded: None,
            ..facts()
        },
        never,
    );
    assert!(matches!(opened, Ok(None)));
}

#[test]
fn a_release_host_without_a_recorded_directory_does_not_start() {
    let anywhere = fs::File::open("/").unwrap();
    let refused = open_with(
        true,
        Ok(ProtectedStore::ReleaseDirectory),
        Facts {
            instance_id: "fixture-instance",
            recorded: None,
            data_root: std::os::fd::AsFd::as_fd(&anywhere),
            foreground: false,
            expect_items: false,
            records: None,
        },
        |_| panic!("the platform was asked for a release directory without a path"),
    )
    .err()
    .unwrap();
    assert_eq!(refused, NOT_RECORDED);
    assert!(refused.contains("setup") && refused.contains("nothing was created"));
    // The views refuse the same way when no directory was opened for them.
    let keys = crate::keys::selected_release_store(
        Ok(ProtectedStore::ReleaseDirectory),
        Some("TEAMID1234.org.agentic.host"),
        None,
        || panic!("signing information was read for a credential directory"),
        None,
    );
    assert!(matches!(keys, Err(StoreError::NoCredentialDirectory)));
    let secrets = crate::host_secrets::selected_release_store(
        Ok(ProtectedStore::ReleaseDirectory),
        Some("TEAMID1234.org.agentic.host"),
        "0190a5d2-7c3e-7a01-8b2c-3d4e5f6071e0",
        || panic!("signing information was read for a credential directory"),
        None,
    );
    assert!(matches!(
        secrets,
        Err(SelectionError::NoCredentialDirectory)
    ));
}

/// The early production path at the host: a key added through the host's `OfficeSecretsKeys` view is
/// loaded through a second handle after every handle of the first was dropped.
#[test]
fn a_key_added_through_the_key_store_view_is_loaded_through_a_second_handle() {
    if !release_directory_system() {
        return;
    }
    let world = World::new();
    let key = OfficeSecretsKey::from_bytes(key_bytes());
    {
        let root = world.open_as_the_host_decides().unwrap();
        let keys = world.keys(&root);
        keys.create(OFFICE, &key).unwrap();
        assert!(matches!(
            keys.create(OFFICE, &OfficeSecretsKey::from_bytes([1; 32])),
            Err(crate::keys::KeyStoreError::Exists)
        ));
        assert_eq!(keys.list().unwrap(), [OFFICE]);
    }
    // The office's folder exists from now on: the records say a key exists.
    directory(&world.office_folder());
    assert!(records_say_keys_exist(&world.office_home).unwrap());
    let root = world.open_as_the_host_decides().unwrap();
    let keys = world.keys(&root);
    assert_eq!(keys.load(OFFICE).unwrap().unwrap(), key);
    // The item is the key's 32 bytes, in the office keys' own directory.
    let file = world
        .credentials
        .join("office-keys")
        .join(format!("{OFFICE}.key"));
    assert_eq!(fs::read(&file).unwrap(), key_bytes());
    keys.delete(OFFICE).unwrap();
    assert!(keys.load(OFFICE).unwrap().is_none());
    assert!(!file.exists());
}

/// LNX-4: host secrets are readable only by the service account, and none is in the data
/// root. A registered instance's root is scanned after the host's views stored an office
/// key, the host's own secrets and the host registry.
#[test]
fn host_secrets_are_owner_only_and_none_is_in_the_data_root() {
    if !release_directory_system() {
        return;
    }
    let world = World::new();
    let key = key_bytes();
    let secrets: [(&str, Vec<u8>); 4] = [
        ("registry", b"REGISTRY-SECRET-b7f1".to_vec()),
        ("writer-device", b"WRITER-DEVICE-SECRET-90aa".to_vec()),
        (
            "tls.00112233445566778899aabbccddeeff00112233445566778899aabbccddeeff",
            b"TLS-PRIVATE-KEY-SECRET-3c5d".to_vec(),
        ),
        (
            "recovery.0190a5d2-7c3e-7a01-8b2c-3d4e5f6071b0",
            b"RECOVERY-CODE-SECRET-e2e2".to_vec(),
        ),
    ];
    {
        let root = world.open_as_the_host_decides().unwrap();
        let custody = world.secrets(&root);
        // A regular non-secret file in the data root stands in for the registry data file.
        fs::write(world.office_home.join("host-registry.data"), b"registry").unwrap();
        world
            .keys(&root)
            .create(OFFICE, &OfficeSecretsKey::from_bytes(key))
            .unwrap();
        directory(&world.office_folder());
        for (name, value) in &secrets {
            custody
                .put(
                    &Name::parse(name).unwrap(),
                    &Value::new(value.clone()).unwrap(),
                )
                .unwrap();
        }
        // An add-only secret keeps its first value.
        let tls = Name::parse(secrets[2].0).unwrap();
        custody
            .put(&tls, &Value::new(b"another".to_vec()).unwrap())
            .unwrap();
        assert_eq!(custody.load(&tls).unwrap().unwrap().expose(), secrets[2].1);
    }

    // Readable only by the service account: every directory 0700, every file 0600 with
    // one link, all this user's, the credential directory and everything in it.
    let user = crate::home::euid();
    let mut pending = vec![world.credentials.clone()];
    let mut stored = Vec::new();
    while let Some(path) = pending.pop() {
        let meta = fs::symlink_metadata(&path).unwrap();
        assert_eq!(meta.uid(), user, "{}", path.display());
        if meta.is_dir() {
            assert_eq!(meta.mode() & 0o7777, 0o700, "{}", path.display());
            for entry in fs::read_dir(&path).unwrap() {
                pending.push(entry.unwrap().path());
            }
        } else {
            assert!(meta.is_file(), "{}", path.display());
            assert_eq!(meta.mode() & 0o7777, 0o600, "{}", path.display());
            assert_eq!(meta.nlink(), 1, "{}", path.display());
            stored.push(path.strip_prefix(&world.credentials).unwrap().to_owned());
        }
    }
    stored.sort();
    let mut expected: Vec<PathBuf> = [
        ".instance",
        ".lock",
        "host-secrets/.lock",
        "office-keys/.lock",
    ]
    .iter()
    .map(PathBuf::from)
    .chain(
        secrets
            .iter()
            .map(|(name, _)| Path::new("host-secrets").join(name)),
    )
    .chain([Path::new("office-keys").join(format!("{OFFICE}.key"))])
    .collect();
    expected.sort();
    assert_eq!(stored, expected);

    // None is in the data root: no file there is named like a key or a host secret, and
    // no file there holds a key's bytes (as stored, or as hex in either case) or a
    // secret's value. The registry the host created there is one of the files read.
    let in_data_root = files(world.instance.root());
    assert!(
        in_data_root
            .iter()
            .any(|(path, _)| path.ends_with("office-state/host-registry.data")),
        "the data root fixture file was not read"
    );
    let needles: Vec<Vec<u8>> = [
        key.to_vec(),
        hex(&key).into_bytes(),
        hex(&key).to_uppercase().into_bytes(),
    ]
    .into_iter()
    .chain(secrets.iter().map(|(_, value)| value.clone()))
    .collect();
    for (path, bytes) in &in_data_root {
        let name = path.file_name().unwrap().to_string_lossy();
        assert!(!name.ends_with(".key"), "{}", path.display());
        for (secret, _) in &secrets {
            assert_ne!(name, *secret, "{}", path.display());
        }
        for needle in &needles {
            assert!(!holds(bytes, needle), "{} holds a secret", path.display());
        }
    }
    // And the credential directory is not in the data root, nor the other way round.
    assert!(!world.credentials.starts_with(world.instance.root()));
    assert!(!world.instance.root().starts_with(&world.credentials));

    // The next start finds every one of them.
    let root = world.open_as_the_host_decides().unwrap();
    let custody = world.secrets(&root);
    for (name, value) in &secrets {
        let loaded = custody.load(&Name::parse(name).unwrap()).unwrap().unwrap();
        assert_eq!(loaded.expose(), value, "{name}");
    }
    assert_eq!(
        world.keys(&root).load(OFFICE).unwrap().unwrap(),
        OfficeSecretsKey::from_bytes(key)
    );
}

/// A host whose records say keys exist does not start on a store found empty, and makes no
/// fresh key: the refusal comes before either view exists.
#[test]
fn a_store_found_empty_for_an_instance_with_offices_stops_the_start_and_nothing_is_made() {
    if !release_directory_system() {
        return;
    }
    let world = World::new();
    // A new instance: no office yet, so an empty store is what it must be.
    assert!(!records_say_keys_exist(&world.office_home).unwrap());
    drop(world.open_as_the_host_decides().unwrap());
    fs::remove_dir_all(&world.credentials).unwrap();
    directory(&world.credentials);

    // An office exists, and the store is empty: the volume was lost or replaced.
    directory(&world.office_folder());
    let refused = world.open_as_the_host_decides().err().unwrap();
    assert_eq!(
        refused,
        format!(
            "{}: credentials missing: the host's records say keys exist, and the credential \
             directory holds none; restore the directory, nothing was created",
            world.credentials.display()
        )
    );
    assert_eq!(fs::read_dir(&world.credentials).unwrap().count(), 0);

    // One view's items are enough for the store to be one that was not lost; the office's
    // own key is then missing for that office alone, which its start reports.
    {
        let root = world.open(false).unwrap();
        world
            .secrets(&root)
            .put(
                &Name::parse("registry").unwrap(),
                &Value::new(b"x".to_vec()).unwrap(),
            )
            .unwrap();
    }
    let root = world.open_as_the_host_decides().unwrap();
    assert!(world.keys(&root).load(OFFICE).unwrap().is_none());
}

#[test]
fn a_store_that_breaks_a_rule_stops_the_start_with_the_directory_and_the_rule() {
    if !release_directory_system() {
        return;
    }
    let world = World::new();
    // Another mode.
    fs::set_permissions(&world.credentials, fs::Permissions::from_mode(0o750)).unwrap();
    assert_eq!(
        world.open(false).err().unwrap(),
        format!(
            "{}: the credential directory's mode is not 0700",
            world.credentials.display()
        )
    );
    fs::set_permissions(&world.credentials, fs::Permissions::from_mode(0o700)).unwrap();
    // Inside the data root, with the instance's own held root as the data root.
    let inside = world.office_home.join("credentials");
    directory(&inside);
    let refused = open_with(
        true,
        Ok(ProtectedStore::ReleaseDirectory),
        Facts {
            recorded: Some(&inside),
            ..world.facts(false)
        },
        |location| release_directory_below_with_stores(&world.base, location, &ITEM_STORES),
    )
    .err()
    .unwrap();
    assert_eq!(
        refused,
        format!(
            "{}: the credential directory is inside the data root",
            inside.display()
        )
    );
    assert_eq!(fs::read_dir(&inside).unwrap().count(), 0);
    // The instance root itself.
    let refused = open_with(
        true,
        Ok(ProtectedStore::ReleaseDirectory),
        Facts {
            recorded: Some(world.instance.root()),
            ..world.facts(false)
        },
        |location| release_directory_below_with_stores(&world.base, location, &ITEM_STORES),
    )
    .err()
    .unwrap();
    assert!(
        refused.ends_with("the credential directory is the data root"),
        "{refused}"
    );
    // A second host on the same directory.
    let first = world.open(false).unwrap();
    let refused = world.open(false).err().unwrap();
    assert!(refused.ends_with("in use by another host"), "{refused}");
    // The views keep it when the handle is gone, as the running host does.
    let keys = world.keys(&first);
    drop(first);
    assert!(world.open(false).is_err());
    drop(keys);
    drop(world.open(false).unwrap());
}

/// A memory filesystem is no store: a container's secret mount (`tmpfs`) is refused.
#[test]
fn a_store_on_a_memory_filesystem_stops_the_start() {
    if !release_directory_system() {
        return;
    }
    let Some(memory) = org_platform::dirs::testing::memory_backed_temp().unwrap() else {
        println!("skipped: this system mounts no memory filesystem by default");
        return;
    };
    // Only the credential directory is in memory; the instance is on disk.
    let world = World::new();
    let temp = tempfile::tempdir_in(&memory).unwrap();
    let base = temp.path().canonicalize().unwrap();
    fs::set_permissions(&base, fs::Permissions::from_mode(0o700)).unwrap();
    let store = base.join("secrets");
    directory(&store);
    let refused = open_with(
        true,
        Ok(ProtectedStore::ReleaseDirectory),
        Facts {
            recorded: Some(&store),
            ..world.facts(false)
        },
        |location| release_directory_below_with_stores(&base, location, &ITEM_STORES),
    )
    .err()
    .unwrap();
    if !refused.contains("a memory filesystem") {
        println!(
            "skipped: {} is not a memory filesystem here: {refused}",
            memory.display()
        );
        return;
    }
    assert!(
        refused.starts_with(&format!("{}: ", store.display())),
        "{refused}"
    );
    for class in ["network", "user-space", "memory"] {
        assert!(refused.contains(class), "{refused}");
    }
    assert_eq!(fs::read_dir(&store).unwrap().count(), 0);
}

/// The host's own entry, from `/`: the walk judges every directory of this machine above
/// the store. A store in a shared temporary directory is refused for that directory, which
/// is writable by others; where cargo's test directory passes, a store there opens.
#[test]
fn the_hosts_own_entry_walks_from_the_filesystem_root() {
    if !release_directory_system() {
        return;
    }
    let world = World::new();
    let refused = open(true, protected_store(), world.facts(false))
        .err()
        .unwrap();
    assert!(
        refused.contains("is writable by others"),
        "a store below a shared temporary directory was not refused for it: {refused}"
    );
    assert_eq!(fs::read_dir(&world.credentials).unwrap().count(), 0);

    let Some(tmp) = option_env!("CARGO_TARGET_TMPDIR")
        .map(PathBuf::from)
        .or_else(|| {
            // <target>/<profile>/deps/<test binary>
            let executable = std::env::current_exe().ok()?.canonicalize().ok()?;
            Some(executable.ancestors().nth(3)?.join("tmp"))
        })
    else {
        return;
    };
    if fs::create_dir_all(&tmp).is_err() {
        println!("skipped: {} cannot be made", tmp.display());
        return;
    }
    let Ok(temp) = tempfile::tempdir_in(&tmp) else {
        println!("skipped: nothing can be made in {}", tmp.display());
        return;
    };
    let base = temp.path().canonicalize().unwrap();
    fs::set_permissions(&base, fs::Permissions::from_mode(0o755)).unwrap();
    let store = base.join("credentials");
    directory(&store);
    let opened = open(
        true,
        protected_store(),
        Facts {
            recorded: Some(&store),
            ..world.facts(false)
        },
    );
    match opened {
        Ok(Some(root)) => {
            let keys = world.keys(&root);
            keys.create(OFFICE, &OfficeSecretsKey::from_bytes(key_bytes()))
                .unwrap();
            drop((keys, root));
            let again = open(
                true,
                protected_store(),
                Facts {
                    recorded: Some(&store),
                    ..world.facts(true)
                },
            )
            .unwrap()
            .unwrap();
            assert_eq!(
                world.keys(&again).load(OFFICE).unwrap().unwrap(),
                OfficeSecretsKey::from_bytes(key_bytes())
            );
            println!("opened from / as the host does: {}", store.display());
        }
        Ok(None) => panic!("a release host on a release directory system opened none"),
        // A directory of this machine above cargo's test directory breaks a rule.
        Err(refused) if !refused.starts_with(&format!("{}: ", store.display())) => {
            println!("skipped: no directory here passes the ancestor rule: {refused}")
        }
        Err(refused) => panic!("{refused}"),
    }
}

/// A store restored without its modes: the host is told which item store to fix, not that
/// its credentials are missing; and only what the two views would list counts as a key.
#[test]
fn an_item_store_with_a_wrong_mode_stops_the_start_by_its_own_rule_and_strays_are_no_keys() {
    if !release_directory_system() {
        return;
    }
    let world = World::new();
    {
        let root = world.open(false).unwrap();
        world
            .keys(&root)
            .create(OFFICE, &OfficeSecretsKey::from_bytes(key_bytes()))
            .unwrap();
        drop(world.secrets(&root));
    }
    directory(&world.office_folder());
    drop(world.open_as_the_host_decides().unwrap());
    let keys = world.credentials.join("office-keys");
    fs::set_permissions(&keys, fs::Permissions::from_mode(0o750)).unwrap();
    let refused = world.open_as_the_host_decides().err().unwrap();
    assert!(
        refused.starts_with(&format!("{}: the mode of this item store", keys.display())),
        "{refused}"
    );
    assert!(!refused.contains("credentials missing"), "{refused}");
    assert_eq!(
        fs::symlink_metadata(&keys).unwrap().mode() & 0o7777,
        0o750,
        "the mode was repaired"
    );
    fs::set_permissions(&keys, fs::Permissions::from_mode(0o700)).unwrap();
    // The key itself restored with another mode.
    let key = keys.join(format!("{OFFICE}.key"));
    fs::set_permissions(&key, fs::Permissions::from_mode(0o644)).unwrap();
    let refused = world.open_as_the_host_decides().err().unwrap();
    assert!(
        refused.starts_with(&format!("{}: an item in this item store", keys.display())),
        "{refused}"
    );
    assert!(!refused.contains(OFFICE), "{refused}");
    fs::remove_file(&key).unwrap();
    // No key is left. Files that neither view would list are no keys: a note next to the
    // keys, a name that is no office's, a name that is no host secret's, a directory of
    // someone's backup. The store is one that was lost.
    for stray in [
        "office-keys/notes.txt",
        "office-keys/not an office.key",
        "host-secrets/README",
        "host-secrets/tls.short",
        "stray.key",
    ] {
        let path = world.credentials.join(stray);
        fs::write(&path, b"x").unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
    }
    directory(&world.credentials.join("backup"));
    fs::write(
        world
            .credentials
            .join("backup")
            .join(format!("{OFFICE}.key")),
        b"x",
    )
    .unwrap();
    let refused = world.open_as_the_host_decides().err().unwrap();
    assert!(refused.contains("credentials missing"), "{refused}");
}
