//! The registered start against a temp account base: setup's record and reservation are made
//! directly, the host uses explicit development custody, and no worker binary is needed.
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;

use org_host_control::listen::ListenConfig;

use super::*;
use crate::hostdb::HostStore;
use crate::instance::setup::{Input, Record};
use crate::links::{LinkKind, Subject};
use crate::release::ReleasePin;

fn listen(port: u16) -> ListenConfig {
    serde_json::from_value(serde_json::json!({
        "address": "127.0.0.1", "port": port, "scheme": "http_loopback",
        "origin": format!("http://127.0.0.1:{port}"), "hsts": "off"
    }))
    .unwrap()
}

struct Fixture {
    paths: Paths,
    base: PathBuf,
    _temp: tempfile::TempDir,
}

fn fixture() -> Fixture {
    let temp =
        tempfile::tempdir_in(org_platform::dirs::testing::disk_backed_temp().unwrap()).unwrap();
    let base = temp.path().canonicalize().unwrap();
    fs::set_permissions(&base, fs::Permissions::from_mode(0o700)).unwrap();
    for name in ["temp", "keys"] {
        fs::create_dir(base.join(name)).unwrap();
        fs::set_permissions(base.join(name), fs::Permissions::from_mode(0o700)).unwrap();
    }
    Fixture {
        paths: Paths {
            base: base.clone(),
            locator: base.join("locator"),
            user_temp: base.join("temp"),
        },
        base,
        _temp: temp,
    }
}

impl Fixture {
    /// Setup's frozen record, with `instance_id` as given.
    fn record(&self, listen: ListenConfig, instance_id: Option<String>) -> Record {
        let journal = Journal::open(&self.base, None).unwrap();
        let input = Input {
            version: 1,
            service: crate::instance::setup::Service::Registered,
            root: self.base.join("instance"),
            credentials: None,
            listen,
            federation: None,
        };
        let pin = ReleasePin {
            release_id: "fixture-1".into(),
            sequence: 1,
            manifest_sha256: "a".repeat(64),
        };
        let record = journal.freeze(input, true, pin).unwrap();
        if instance_id.is_none() {
            return record;
        }
        let next = Record {
            instance_id,
            ..record
        };
        journal.write(&next).unwrap();
        next
    }

    /// Reserves the instance as setup does, with `browser` as its listener. Returns its ID;
    /// the reservation's root lock is released before the host starts.
    fn reserve(&self, browser: Option<&ListenConfig>) -> String {
        let registry = Registry::open(&self.paths.locator, &self.paths.user_temp).unwrap();
        let instance = registry.reserve(None, &self.base.join("instance")).unwrap();
        if let Some(browser) = browser {
            instance.set_browser(browser).unwrap();
        }
        instance.id().to_owned()
    }

    fn config(&self) -> Config {
        let mut config = Config::new(
            self.base.join("unused-home"),
            self.base.join("no-such-worker"),
        );
        config.dev_key_dir = Some(self.base.join("keys"));
        config
    }

    fn start(&self) -> Result<Host, HostError> {
        Host::start_registered_at(self.config(), &self.paths, None)
    }

    fn registry_db(&self) -> PathBuf {
        self.base.join("instance/office-state/host-registry.db")
    }

    fn marker(&self) -> PathBuf {
        self.base.join("instance/host-registry.created")
    }

    /// Every custody file and its bytes, to show a refused start changed none of them.
    fn custody(&self) -> Vec<(std::ffi::OsString, Vec<u8>)> {
        let mut files: Vec<_> = walk(&self.base.join("keys"));
        files.sort();
        files
    }
}

fn walk(dir: &std::path::Path) -> Vec<(std::ffi::OsString, Vec<u8>)> {
    let mut out = Vec::new();
    for entry in fs::read_dir(dir).unwrap() {
        let entry = entry.unwrap();
        let path = entry.path();
        if entry.file_type().unwrap().is_dir() {
            out.extend(walk(&path));
        } else {
            out.push((path.into_os_string(), fs::read(entry.path()).unwrap()));
        }
    }
    out
}

fn registry(host: &Host) -> std::sync::Arc<dyn HostStore> {
    host.service
        .authentication
        .as_ref()
        .unwrap()
        .provisioning_storage()
        .0
}

fn administrator() -> Subject {
    Subject::new("administrator").unwrap()
}

#[test]
fn the_first_start_creates_the_registry_once_and_installs_admin() {
    let f = fixture();
    let listen = listen(7420);
    let id = f.reserve(Some(&listen));
    let record = f.record(listen, Some(id.clone()));
    let host = f.start().unwrap();
    assert!(host.service.admin().is_some(), "admin installed");
    assert_eq!(host.instance.as_ref().unwrap().id(), id);
    let db = host
        .service
        .authentication
        .as_ref()
        .unwrap()
        .provisioning_storage()
        .0;
    assert!(!db.has_administrator().unwrap());
    // The installed admin is bound to the frozen setup operation and the setup origin.
    let admin = host.service.admin().unwrap();
    assert!(admin.issues_for(&record.setup_operation, "http://127.0.0.1:7420"));
    assert_eq!(
        fs::read(f.marker()).unwrap(),
        id.as_bytes(),
        "marker written"
    );
    // A sentinel only this registry holds: the setup link, issued before the restart.
    let sentinel = db
        .issue_first(LinkKind::Registration, &administrator(), 1)
        .unwrap()
        .expect("first link");
    drop(db);
    host.shutdown();

    // A later start reopens the same registry rather than creating another.
    let host = f.start().unwrap();
    let db = registry(&host);
    let pending = db
        .pending(LinkKind::Registration, &administrator(), 2)
        .unwrap()
        .expect("the sentinel link survived");
    assert_eq!(pending.link_id, sentinel.link_id);
    assert!(db
        .issue_first(LinkKind::Registration, &administrator(), 3)
        .unwrap()
        .is_none());
    assert!(host.service.admin().is_some());
    drop(db);
    host.shutdown();
}

#[test]
fn a_start_whose_created_registry_is_missing_refuses_and_issues_no_link() {
    let f = fixture();
    let listen = listen(7420);
    let id = f.reserve(Some(&listen));
    f.record(listen, Some(id));
    f.start().unwrap().shutdown();
    assert!(f.marker().exists());
    let custody = f.custody();
    fs::remove_file(f.registry_db()).unwrap();

    let err = f.start().err().expect("refused");
    assert!(err.to_string().contains(REGISTRY_MISSING), "{err}");
    assert!(
        !f.registry_db().exists(),
        "no registry recreated, so no link"
    );
    assert_eq!(f.custody(), custody, "custody untouched");
    assert!(f.marker().exists(), "the marker stays");
}

#[test]
fn a_registry_without_its_marker_is_reopened_and_the_marker_repaired() {
    // A start that died after publishing the registry but before writing the marker.
    let f = fixture();
    let listen = listen(7420);
    let id = f.reserve(Some(&listen));
    f.record(listen, Some(id.clone()));
    let host = f.start().unwrap();
    let sentinel = registry(&host)
        .issue_first(LinkKind::Registration, &administrator(), 1)
        .unwrap()
        .expect("first link");
    host.shutdown();
    fs::remove_file(f.marker()).unwrap();

    let host = f.start().unwrap();
    assert_eq!(fs::read(f.marker()).unwrap(), id.as_bytes());
    let pending = registry(&host)
        .pending(LinkKind::Registration, &administrator(), 2)
        .unwrap()
        .expect("same registry");
    assert_eq!(pending.link_id, sentinel.link_id);
    host.shutdown();
}

#[test]
fn a_marker_naming_another_instance_is_refused() {
    let f = fixture();
    let listen = listen(7420);
    let id = f.reserve(Some(&listen));
    f.record(listen, Some(id));
    f.start().unwrap().shutdown();
    fs::remove_file(f.marker()).unwrap();
    fs::write(f.marker(), crate::ids::uuid_v7().unwrap()).unwrap();
    fs::set_permissions(f.marker(), fs::Permissions::from_mode(0o600)).unwrap();
    let err = f.start().err().expect("refused");
    assert!(err.to_string().contains("another instance"), "{err}");
}

#[test]
fn a_start_without_a_complete_setup_reservation_refuses_before_the_registry() {
    // No setup record at all.
    let f = fixture();
    assert!(f.start().is_err());

    // A record without a reserved instance.
    let f = fixture();
    f.record(listen(7420), None);
    assert!(f.start().is_err());
    assert!(!f.registry_db().exists());

    // A record naming an instance the locator never reserved: nothing is reserved here.
    let f = fixture();
    f.record(listen(7420), Some(crate::ids::uuid_v7().unwrap()));
    assert!(f.start().is_err());
    assert!(!f.base.join("instance").exists(), "no reservation made");

    // A record naming another ID than the reservation.
    let f = fixture();
    let listen_config = listen(7420);
    f.reserve(Some(&listen_config));
    f.record(listen_config, Some(crate::ids::uuid_v7().unwrap()));
    assert!(f.start().is_err());
    assert!(!f.registry_db().exists());
}

#[test]
fn a_start_whose_instance_listener_differs_from_setup_refuses() {
    for browser in [None, Some(listen(7421))] {
        let f = fixture();
        let id = f.reserve(browser.as_ref());
        f.record(listen(7420), Some(id));
        assert!(f.start().is_err(), "{browser:?}");
        assert!(!f.registry_db().exists());
    }
}

#[test]
fn a_start_without_admitted_runtime_refuses_before_touching_the_instance() {
    let f = fixture();
    let listen = listen(7420);
    let id = f.reserve(Some(&listen));
    f.record(listen, Some(id));
    let locator = fs::read(&f.paths.locator).ok();
    let mut config = f.config();
    config.dev_key_dir = None;
    let err = Host::start_registered_at(config, &f.paths, None)
        .err()
        .expect("refused");
    assert!(err.to_string().contains("admitting"), "{err}");
    assert!(!f.registry_db().exists());
    assert_eq!(
        fs::read(&f.paths.locator).ok(),
        locator,
        "locator untouched"
    );
    assert_eq!(
        fs::read_dir(f.base.join("keys")).unwrap().count(),
        0,
        "no custody"
    );
}

impl Fixture {
    /// Writes setup's record for a reserved instance whose credential directory is
    /// `credentials`, in the service mode `service`.
    fn record_with_credentials(
        &self,
        id: String,
        listen: ListenConfig,
        service: crate::instance::setup::Service,
        credentials: &std::path::Path,
    ) {
        let journal = Journal::open(&self.base, None).unwrap();
        let input = Input {
            version: 1,
            service,
            root: self.base.join("instance"),
            credentials: Some(credentials.to_owned()),
            listen,
            federation: None,
        };
        let pin = ReleasePin {
            release_id: "fixture-1".into(),
            sequence: 1,
            manifest_sha256: "a".repeat(64),
        };
        let record = journal.freeze(input, true, pin).unwrap();
        journal
            .write(&Record {
                instance_id: Some(id),
                credentials: Some(credentials.to_owned()),
                ..record
            })
            .unwrap();
    }
}

/// A credential directory beside the fixture, outside the account's records (`base`), as
/// setup makes one: `<another temporary directory>/secrets`, mode 700.
fn credential_directory() -> (tempfile::TempDir, PathBuf) {
    let temp =
        tempfile::tempdir_in(org_platform::dirs::testing::disk_backed_temp().unwrap()).unwrap();
    let base = temp.path().canonicalize().unwrap();
    fs::set_permissions(&base, fs::Permissions::from_mode(0o700)).unwrap();
    let secrets = base.join("secrets");
    fs::create_dir(&secrets).unwrap();
    fs::set_permissions(&secrets, fs::Permissions::from_mode(0o700)).unwrap();
    (temp, secrets)
}

/// Whether a refusal is this machine's, not the rule's: the temporary directory is on a
/// filesystem that cannot hold a credential directory.
fn this_machine_cannot_hold_one(refused: &str) -> bool {
    refused.contains("must be on ext4")
}

/// Runs `f` with this thread's host starts walking to their credential directory from
/// `base` (`credential_store_root::WALK_FROM`): every check below `base` is the host's.
fn walking_from<R>(base: &std::path::Path, f: impl FnOnce() -> R) -> R {
    crate::credential_store_root::WALK_FROM.with(|from| *from.borrow_mut() = Some(base.to_owned()));
    let result = f();
    crate::credential_store_root::WALK_FROM.with(|from| *from.borrow_mut() = None);
    result
}

/// X5 review, fourth round (Sol P1-1, P1-2, P1-5): where this system keeps a host's secrets
/// in a directory and the setup record names one, a development build keeps every secret
/// there, as a release build does, and needs no development key folder. The directory is
/// judged at every start with the release rules: an instance whose records say keys exist
/// does not start on an empty one (a lost secrets volume), and nothing is made in it.
#[test]
fn a_recorded_credential_directory_holds_a_development_builds_custody() {
    use org_platform_host::credentials::{protected_store, ProtectedStore};
    if protected_store() != Ok(ProtectedStore::ReleaseDirectory) {
        println!("skipped: this system records no credential directory");
        return;
    }
    let f = fixture();
    let (_secrets_temp, secrets) = credential_directory();
    let listen = listen(7420);
    let id = f.reserve(Some(&listen));
    f.record_with_credentials(
        id.clone(),
        listen,
        crate::instance::setup::Service::Registered,
        &secrets,
    );
    // No development key folder: the recorded directory is the custody.
    let walk_base = secrets.parent().unwrap().to_owned();
    let start = || {
        let mut config = f.config();
        config.dev_key_dir = None;
        walking_from(&walk_base, || {
            Host::start_registered_at(config, &f.paths, None)
        })
    };
    let host = match start() {
        Ok(host) => host,
        Err(error) if this_machine_cannot_hold_one(&error.to_string()) => {
            println!("skipped: {error}");
            return;
        }
        Err(error) => panic!("{error}"),
    };
    assert!(f.registry_db().exists());
    host.shutdown();
    // Both views were opened in the directory (each made its item store there, with its
    // lock), and nothing in the development folder: there is none.
    for store in ["host-secrets", "office-keys"] {
        assert!(
            secrets.join(store).join(".lock").is_file(),
            "{store}: {:?}",
            walk(&secrets)
        );
    }
    assert!(f.custody().is_empty(), "the development folder was used");

    // An office exists, and the directory is replaced by an empty one: the start refuses
    // before any key or secret is made, and the directory stays empty.
    let offices = f
        .base
        .join("instance/office-state")
        .join(org_office_launch::OFFICES_DIR_NAME);
    fs::create_dir_all(offices.join("01AAAAAAAAAAAAAAAAAAAAAAAA")).unwrap();
    fs::remove_dir_all(&secrets).unwrap();
    fs::create_dir(&secrets).unwrap();
    fs::set_permissions(&secrets, fs::Permissions::from_mode(0o700)).unwrap();
    let refused = start().err().expect("refused").to_string();
    assert!(
        refused.contains("credentials missing") && refused.contains(&*secrets.to_string_lossy()),
        "{refused}"
    );
    assert_eq!(fs::read_dir(&secrets).unwrap().count(), 0);
    assert!(f.custody().is_empty());
}

#[test]
fn a_host_refuses_a_credential_directory_bound_to_another_instance() {
    use org_platform_host::credentials::{protected_store, ProtectedStore};
    if protected_store() != Ok(ProtectedStore::ReleaseDirectory) {
        return;
    }
    let (_temp, secrets) = credential_directory();
    let owner = fixture();
    let other = fixture();
    let listen = listen(7420);
    let owner_id = owner.reserve(Some(&listen));
    let other_id = other.reserve(Some(&listen));
    assert_ne!(owner_id, other_id);
    for (fixture, id) in [(&owner, &owner_id), (&other, &other_id)] {
        fixture.record_with_credentials(
            id.clone(),
            listen.clone(),
            crate::instance::setup::Service::Registered,
            &secrets,
        );
    }
    let start = |fixture: &Fixture| walking_from(secrets.parent().unwrap(), || fixture.start());
    match start(&owner) {
        Ok(host) => host.shutdown(),
        Err(error) if this_machine_cannot_hold_one(&error.to_string()) => return,
        Err(error) => panic!("{error}"),
    }

    // A first start on the other data root must not open custody. Creating a host
    // registry would clean up this writer item.
    let writer = secrets.join("host-secrets/writer-device");
    fs::write(&writer, b"writer of the original instance").unwrap();
    fs::set_permissions(&writer, fs::Permissions::from_mode(0o600)).unwrap();
    let refused = match start(&other) {
        Err(error) => error.to_string(),
        Ok(host) => {
            host.shutdown();
            panic!("started with another instance's credential directory");
        }
    };
    assert!(refused.contains("bound to another instance"), "{refused}");
    assert!(!other.registry_db().exists());
    assert_eq!(
        fs::read(&writer).unwrap(),
        b"writer of the original instance"
    );
    let marker = secrets.join(".instance");
    assert_eq!(fs::read_to_string(&marker).unwrap(), owner_id);
    assert_eq!(
        fs::metadata(marker).unwrap().permissions().mode() & 0o7777,
        0o600
    );
}

/// X5 review, fourth round (Sol P1-3): the credential directory is kept apart from the
/// account's records as it is from the data folder: one inside the records' directory is
/// refused, and nothing is made in it.
#[test]
fn a_credential_directory_among_the_accounts_records_is_refused() {
    use org_platform_host::credentials::{protected_store, ProtectedStore};
    if protected_store() != Ok(ProtectedStore::ReleaseDirectory) {
        println!("skipped: this system records no credential directory");
        return;
    }
    let f = fixture();
    // The fixture's records are in `base` itself.
    let secrets = f.base.join("secrets");
    fs::create_dir(&secrets).unwrap();
    fs::set_permissions(&secrets, fs::Permissions::from_mode(0o700)).unwrap();
    let listen = listen(7420);
    let id = f.reserve(Some(&listen));
    f.record_with_credentials(
        id,
        listen,
        crate::instance::setup::Service::Registered,
        &secrets,
    );
    let refused = walking_from(&f.base, || f.start())
        .err()
        .expect("refused")
        .to_string();
    if this_machine_cannot_hold_one(&refused) {
        println!("skipped: {refused}");
        return;
    }
    assert!(refused.contains("account's records"), "{refused}");
    assert_eq!(fs::read_dir(&secrets).unwrap().count(), 0);
    assert!(!f.registry_db().exists());
}

/// A development build whose record names no credential directory makes the development
/// key folder its environment names when it is missing (mode 700), never its parent.
#[test]
fn a_missing_development_key_folder_is_made_but_never_its_parent() {
    let f = fixture();
    let listen = listen(7420);
    let id = f.reserve(Some(&listen));
    f.record(listen, Some(id));
    fs::remove_dir(f.base.join("keys")).unwrap();
    f.start().unwrap().shutdown();
    assert_eq!(
        fs::metadata(f.base.join("keys"))
            .unwrap()
            .permissions()
            .mode()
            & 0o7777,
        0o700
    );
    let mut config = f.config();
    config.dev_key_dir = Some(f.base.join("absent/keys"));
    let refused = Host::start_registered_at(config, &f.paths, None)
        .err()
        .expect("refused")
        .to_string();
    assert!(
        refused.contains("making the development key folder"),
        "{refused}"
    );
    assert!(!f.base.join("absent").exists());
}

/// X5 review, second round: on a system that keeps a host's secrets in a directory, a
/// foreground record that names no secrets volume (setup refuses such a file; this one is
/// written by hand) does not start, so neither the volume's checks nor the development key
/// folder's rule can be skipped. Nothing is written.
#[test]
fn a_foreground_record_without_its_secrets_volume_does_not_start_where_one_is_required() {
    use org_platform_host::credentials::{protected_store, ProtectedStore};
    if protected_store() != Ok(ProtectedStore::ReleaseDirectory) {
        println!("skipped: this system keeps no host secrets in a directory");
        return;
    }
    let f = fixture();
    let listen = listen(7420);
    let id = f.reserve(Some(&listen));
    let journal = Journal::open(&f.base, None).unwrap();
    let input = Input {
        version: 1,
        service: crate::instance::setup::Service::Foreground,
        root: f.base.join("instance"),
        credentials: None,
        listen,
        federation: None,
    };
    let pin = ReleasePin {
        release_id: "fixture-1".into(),
        sequence: 1,
        manifest_sha256: "a".repeat(64),
    };
    let record = journal.freeze(input, true, pin).unwrap();
    journal
        .write(&Record {
            instance_id: Some(id),
            ..record
        })
        .unwrap();
    drop(journal);
    let refused = f.start().err().expect("refused").to_string();
    assert!(refused.contains(FOREGROUND_WITHOUT_VOLUME), "{refused}");
    assert!(f.custody().is_empty(), "custody was written");
    assert!(!f.registry_db().exists());
}

#[test]
fn registered_start_and_cli_origin_accept_opt_in_https_upgrade_and_refuse_tamper_or_downgrade() {
    let f = fixture();
    let http_listen = listen(7420);
    let id = f.reserve(Some(&http_listen));
    f.record(http_listen.clone(), Some(id.clone()));

    // First start over default plain HTTP succeeds.
    let host = f.start().expect("initial HTTP registered start");
    host.shutdown();

    // Upgrade instance.json to HTTPS on the exact same address/port/host (ADR-0033).
    let https_listen: ListenConfig = serde_json::from_value(serde_json::json!({
        "address": "127.0.0.1",
        "port": 7420,
        "scheme": "https",
        "origin": "https://127.0.0.1:7420",
        "hsts": "off"
    }))
    .unwrap();
    {
        let registry = Registry::open(&f.paths.locator, &f.paths.user_temp).unwrap();
        let instance = registry.reserve(None, &f.base.join("instance")).unwrap();
        instance.upgrade_browser_to_https(&https_listen).unwrap();
    }

    // CLI origin resolution (`admin tls set` / `admin reset`) now sees the upgraded https:// origin.
    let (_rec, resolved_id, _sock, resolved_origin) =
        crate::setup_flow::registered_with_origin(&f.paths, None).unwrap();
    assert_eq!(resolved_id, id);
    assert_eq!(resolved_origin.as_str(), "https://127.0.0.1:7420");

    // Registered host restarts cleanly on HTTPS (also persists HSTS updates).
    let upgraded_host = f
        .start()
        .expect("registered start after HTTP->HTTPS upgrade");
    assert!(
        upgraded_host.service.web_tls_for_test().is_some(),
        "upgraded registered host initialized WebTls"
    );
    upgraded_host.shutdown();

    // HSTS change on the same HTTPS listener is also accepted on registered restart.
    let hsts_on: ListenConfig = serde_json::from_value(serde_json::json!({
        "address": "127.0.0.1",
        "port": 7420,
        "scheme": "https",
        "origin": "https://127.0.0.1:7420",
        "hsts": "on"
    }))
    .unwrap();
    {
        let registry = Registry::open(&f.paths.locator, &f.paths.user_temp).unwrap();
        let instance = registry.reserve(None, &f.base.join("instance")).unwrap();
        instance.upgrade_browser_to_https(&hsts_on).unwrap();
    }
    let hsts_host = f.start().expect("registered start after HSTS update");
    hsts_host.shutdown();

    // Tampered port is still refused on registered start.
    let wrong_port: ListenConfig = serde_json::from_value(serde_json::json!({
        "address": "127.0.0.1",
        "port": 7499,
        "scheme": "https",
        "origin": "https://127.0.0.1:7499",
        "hsts": "off"
    }))
    .unwrap();
    assert!(!browser_matches_setup(&wrong_port, &http_listen));
    // HTTPS setup record refuses downgrade to plain HTTP.
    assert!(!browser_matches_setup(&http_listen, &https_listen));
    {
        let registry = Registry::open(&f.paths.locator, &f.paths.user_temp).unwrap();
        let instance = registry.reserve(None, &f.base.join("instance")).unwrap();
        assert!(
            instance.upgrade_browser_to_https(&wrong_port).is_err(),
            "upgrade_browser_to_https refuses port change"
        );
    }
    // Even if instance.json is tampered with directly on disk, start_registered_at refuses it.
    let cfg_path = f.base.join("instance/instance.json");
    let tampered = fs::read_to_string(&cfg_path)
        .unwrap()
        .replace("7420", "7499");
    fs::write(&cfg_path, tampered).unwrap();
    let refused = f.start().err().expect("tampered port refused").to_string();
    assert!(
        refused.contains("differs from setup"),
        "unexpected refusal: {refused}"
    );
}
