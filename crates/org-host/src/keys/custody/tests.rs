use super::*;
use std::os::unix::fs::PermissionsExt;

const A: &str = "01AAAAAAAAAAAAAAAAAAAAAAAA";
const B: &str = "01BBBBBBBBBBBBBBBBBBBBBBBB";

fn key(seed: u8) -> OfficeSecretsKey {
    let mut bytes = [0u8; OFFICE_SECRETS_KEY_BYTES];
    for (i, b) in bytes.iter_mut().enumerate() {
        *b = (i as u8).wrapping_mul(29).wrapping_add(seed);
    }
    OfficeSecretsKey::from_bytes(bytes)
}

/// The development store in `dir`, for a host without a registered instance.
fn store(dir: &Path) -> OfficeKeys {
    OfficeKeys::development(dir, None)
}

/// A temporary folder that is this user's alone, as a key folder must be.
fn private_dir() -> tempfile::TempDir {
    tempfile::Builder::new()
        .permissions(fs::Permissions::from_mode(0o700))
        .tempdir()
        .unwrap()
}

/// Writes `text` as an owner-only file, as a key file must be, replacing any.
fn write(path: &Path, text: &str) {
    let _ = fs::remove_file(path);
    fs::write(path, text).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o600)).unwrap();
}

fn mode(path: &Path) -> u32 {
    fs::metadata(path).unwrap().permissions().mode() & 0o7777
}

#[test]
fn a_key_round_trips_as_an_owner_only_file_of_64_hex_digits() {
    let dir = private_dir();
    let store = store(&dir.path().join("keys"));
    assert!(store.load(A).unwrap().is_none());
    store.create(A, &key(7)).unwrap();
    assert_eq!(store.load(A).unwrap().unwrap(), key(7));

    let path = dir.path().join("keys").join(format!("{A}.key"));
    let text = fs::read_to_string(&path).unwrap();
    assert_eq!(text.len(), 64);
    assert!(text
        .bytes()
        .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)));
    assert_eq!(mode(&path), 0o600);
    assert_eq!(mode(&dir.path().join("keys")), 0o700);
}

#[test]
fn the_folder_is_opened_at_first_use_and_a_missing_one_holds_nothing() {
    let dir = private_dir();
    let keys = dir.path().join("deep").join("keys");
    let store = store(&keys);
    // Nothing is made by choosing the store, or by asking it for what it doesn't have.
    assert!(store.load(A).unwrap().is_none());
    assert_eq!(store.list().unwrap(), Vec::<String>::new());
    store.delete(A).unwrap();
    assert!(!dir.path().join("deep").exists());
    // The first create makes the folder, and every folder above it that is missing.
    store.create(A, &key(1)).unwrap();
    assert_eq!(mode(&keys), 0o700);
    assert_eq!(mode(&dir.path().join("deep")), 0o700);
    assert_eq!(store.list().unwrap(), [A]);
    // A folder made later by someone else is found by a store that had found none.
    let later = dir.path().join("later");
    let other = self::store(&later);
    assert!(other.load(A).unwrap().is_none());
    fs::create_dir(&later).unwrap();
    fs::set_permissions(&later, fs::Permissions::from_mode(0o700)).unwrap();
    write(&later.join(format!("{A}.key")), &"ab".repeat(32));
    assert!(other.load(A).unwrap().is_some());
}

#[test]
fn a_registered_instances_keys_live_in_its_own_folder() {
    let dir = private_dir();
    let base = dir.path().join("keys");
    let instance = "01INSTANCEAAAAAAAAAAAAAAAA";
    let scoped = OfficeKeys::development(&base, Some(instance));
    let plain = store(&base);
    scoped.create(A, &key(1)).unwrap();
    plain.create(A, &key(2)).unwrap();
    assert_eq!(mode(&base.join(instance)), 0o700);
    assert!(base.join(instance).join(format!("{A}.key")).is_file());
    assert!(base.join(format!("{A}.key")).is_file());
    assert_eq!(scoped.load(A).unwrap().unwrap(), key(1));
    assert_eq!(plain.load(A).unwrap().unwrap(), key(2));
    assert_eq!(scoped.list().unwrap(), [A]);
    assert_eq!(plain.list().unwrap(), [A]);
    // An instance id that could leave the folder is refused, and nothing is made for it.
    let escaping = OfficeKeys::development(&base, Some("../escape"));
    assert!(matches!(
        escaping.create(A, &key(3)),
        Err(KeyStoreError::NotPlainId)
    ));
    assert!(!dir.path().join("escape").exists());
}

#[test]
fn several_hosts_may_hold_one_development_folder_and_none_replaces_a_key() {
    let dir = private_dir();
    let keys = dir.path().join("keys");
    let (one, two) = (store(&keys), store(&keys));
    one.create(A, &key(1)).unwrap();
    two.create(B, &key(2)).unwrap();
    assert_eq!(two.load(A).unwrap().unwrap(), key(1));
    assert_eq!(one.load(B).unwrap().unwrap(), key(2));
    assert!(matches!(two.create(A, &key(9)), Err(KeyStoreError::Exists)));
    assert_eq!(one.list().unwrap(), [A, B]);
    two.delete(A).unwrap();
    assert!(one.load(A).unwrap().is_none());
    // Only names: what a host leaves behind while it runs is no key and hides from `ls`.
    let visible: Vec<String> = fs::read_dir(&keys)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().into_string().unwrap())
        .filter(|name| !name.starts_with('.'))
        .collect();
    assert_eq!(visible, [format!("{B}.key")]);
}

#[test]
fn create_never_replaces_a_key() {
    let dir = private_dir();
    let store = store(dir.path());
    store.create(A, &key(1)).unwrap();
    assert!(matches!(
        store.create(A, &key(2)),
        Err(KeyStoreError::Exists)
    ));
    assert_eq!(store.load(A).unwrap().unwrap(), key(1));
}

#[test]
fn list_names_the_offices_with_a_key_and_nothing_else() {
    let dir = private_dir();
    let store = store(&dir.path().join("keys"));
    assert_eq!(store.list().unwrap(), Vec::<String>::new());
    store.create(B, &key(1)).unwrap();
    store.create(A, &key(2)).unwrap();
    for other in ["notes.txt", "a.b.key", ".key"] {
        fs::write(dir.path().join("keys").join(other), b"x").unwrap();
    }
    assert_eq!(store.list().unwrap(), [A, B]);
}

#[test]
fn delete_is_ok_for_a_missing_key() {
    let dir = private_dir();
    let store = store(dir.path());
    store.delete(A).unwrap();
    store.create(A, &key(1)).unwrap();
    store.delete(A).unwrap();
    assert!(store.load(A).unwrap().is_none());
    store.delete(A).unwrap();
}

#[test]
fn a_file_that_isnt_a_key_is_corrupt_and_its_text_isnt_shown() {
    let dir = private_dir();
    let store = store(dir.path());
    let path = dir.path().join(format!("{A}.key"));
    let hex = "0123456789abcdef".repeat(4);
    for bad in [
        String::new(),
        hex[..62].to_owned(),
        format!("{hex}00"),
        format!("{}g", &hex[..63]),
        "z".repeat(4096),
        "z".repeat(64 * 1024),
    ] {
        write(&path, &bad);
        let err = store.load(A).unwrap_err();
        assert!(matches!(err, KeyStoreError::Corrupt), "{bad:?}");
        assert!(err.means_no_key());
        assert!(!err.to_string().contains("0123"), "{err}");
    }
    // The server's format allowed a trailing newline and uppercase digits.
    write(&path, &format!("{}\n", hex.to_uppercase()));
    assert!(store.load(A).unwrap().is_some());
}

#[test]
fn a_key_file_open_to_others_is_refused_and_is_not_a_missing_key() {
    let dir = private_dir();
    let store = store(dir.path());
    let path = dir.path().join(format!("{A}.key"));
    let hex = "0123456789abcdef".repeat(4);
    for open in [0o644, 0o640, 0o400, 0o700] {
        write(&path, &hex);
        fs::set_permissions(&path, fs::Permissions::from_mode(open)).unwrap();
        let err = store.load(A).unwrap_err();
        assert!(matches!(err, KeyStoreError::Io(_)), "{open:o}: {err}");
        assert!(!err.means_no_key());
        let shown = err.to_string();
        assert!(!shown.contains("0123"), "{shown}");
        assert!(!shown.contains(dir.path().to_str().unwrap()), "{shown}");
        // Nothing repairs it.
        assert_eq!(mode(&path), open);
    }
    // A second name for the same file is refused too.
    write(&path, &hex);
    fs::hard_link(&path, dir.path().join("another-name")).unwrap();
    assert!(matches!(store.load(A), Err(KeyStoreError::Io(_))));
}

#[test]
fn a_symlink_or_fifo_isnt_a_key_and_a_folder_open_to_others_is_refused_not_repaired() {
    let dir = private_dir();
    let keys = dir.path().join("keys");
    fs::create_dir(&keys).unwrap();
    fs::set_permissions(&keys, fs::Permissions::from_mode(0o755)).unwrap();
    // The stricter store refuses a folder others can read; it used to be made owner-only.
    let open = store(&keys);
    let err = open.create(A, &key(1)).unwrap_err();
    assert!(matches!(err, KeyStoreError::Io(_)), "{err}");
    assert!(!err.to_string().contains(keys.to_str().unwrap()), "{err}");
    assert!(matches!(open.load(A), Err(KeyStoreError::Io(_))));
    assert_eq!(mode(&keys), 0o755);
    assert_eq!(fs::read_dir(&keys).unwrap().count(), 0);

    fs::set_permissions(&keys, fs::Permissions::from_mode(0o700)).unwrap();
    let store = store(&keys);
    store.create(A, &key(1)).unwrap();
    assert_eq!(mode(&keys), 0o700);

    std::os::unix::fs::symlink(keys.join(format!("{A}.key")), keys.join(format!("{B}.key")))
        .unwrap();
    assert!(matches!(store.load(B), Err(KeyStoreError::Io(_))));
    fs::remove_file(keys.join(format!("{B}.key"))).unwrap();
    const C: &str = "01CCCCCCCCCCCCCCCCCCCCCCCC";
    let status = std::process::Command::new("/usr/bin/mkfifo")
        .args(["-m", "600"])
        .arg(keys.join(format!("{C}.key")))
        .status()
        .unwrap();
    assert!(status.success());
    // Never blocked by a FIFO: only a regular file can be a key. The stricter store reports it
    // as a file that fails its checks, where the former one called it corrupt.
    let err = store.load(C).unwrap_err();
    assert!(matches!(err, KeyStoreError::Io(_)), "{err}");
    assert!(!err.means_no_key());
    // A key folder that's a file isn't mistaken for an existing key.
    let file = dir.path().join("a-file");
    fs::write(&file, b"").unwrap();
    let err = self::store(&file).create(A, &key(1)).unwrap_err();
    assert!(matches!(err, KeyStoreError::Io(_)), "{err}");
    assert!(matches!(
        self::store(&file).load(A),
        Err(KeyStoreError::Io(_))
    ));
}

#[test]
fn ids_that_arent_plain_are_refused() {
    let dir = private_dir();
    let store = store(dir.path());
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

#[test]
fn a_keychain_item_is_a_key_only_at_its_exact_length() {
    assert!(
        raw_key(&Secret::from(vec![7; OFFICE_SECRETS_KEY_BYTES])).unwrap()
            == OfficeSecretsKey::from_bytes([7; 32])
    );
    for length in [
        0,
        OFFICE_SECRETS_KEY_BYTES - 1,
        OFFICE_SECRETS_KEY_BYTES + 1,
        64,
    ] {
        assert!(
            raw_key(&Secret::from(vec![7; length])).is_none(),
            "{length}"
        );
    }
}

#[test]
fn the_platforms_kinds_keep_their_meaning_and_show_no_path() {
    use CredentialError as Kind;
    assert!(matches!(
        KeyStoreError::from(Kind::Exists),
        KeyStoreError::Exists
    ));
    assert!(matches!(
        KeyStoreError::from(Kind::Corrupt),
        KeyStoreError::Corrupt
    ));
    assert!(matches!(
        KeyStoreError::from(Kind::InvalidName),
        KeyStoreError::NotPlainId
    ));
    assert!(matches!(
        KeyStoreError::from(Kind::Os(-25308)),
        KeyStoreError::Os(-25308)
    ));
    // A Keychain reply without data was `errSecParam`, and is not a missing key.
    assert!(matches!(
        KeyStoreError::from(Kind::Malformed),
        KeyStoreError::Os(-50)
    ));
    for kind in [
        Kind::Io(io::ErrorKind::PermissionDenied),
        Kind::TooLarge,
        Kind::Unsafe,
        Kind::InUse,
        Kind::CaseAlias,
    ] {
        let error = KeyStoreError::from(kind);
        assert!(matches!(error, KeyStoreError::Io(_)), "{error}");
        assert!(!error.means_no_key());
        assert!(!error.to_string().contains('/'), "{error}");
    }
}

#[test]
fn every_call_is_refused_with_its_reason_and_nothing_is_kept() {
    let store = UnsupportedKeys;
    let key = OfficeSecretsKey::from_bytes([7; 32]);
    let id = "01JAAAAAAAAAAAAAAAAAAAAAAA";
    let refusals = [
        store.create(id, &key).unwrap_err(),
        store.load(id).unwrap_err(),
        store.delete(id).unwrap_err(),
        store.list().unwrap_err(),
    ];
    for refusal in refusals {
        assert!(matches!(refusal, KeyStoreError::Unsupported), "{refusal:?}");
        assert!(!refusal.means_no_key(), "a refusal is not a missing key");
        assert_eq!(
            refusal.to_string(),
            "this operating system has no office secrets key store"
        );
    }
    // A create that was refused stored nothing: the next load is still refused, not a key.
    assert!(store.load(id).is_err());
}

/// The platform's own refusal (a system with no credential store) is the same reason.
#[test]
fn the_platforms_refusal_is_the_unsupported_reason_and_not_a_missing_key() {
    let refused = org_platform::Refused::Unsupported {
        capability: "credentials",
        os: std::env::consts::OS,
    };
    let error = KeyStoreError::from(CredentialError::Refused(refused));
    assert!(matches!(error, KeyStoreError::Unsupported), "{error:?}");
    assert!(!error.means_no_key());
    assert!(!error.to_string().contains('/'), "{error}");
}
