//! The registered start against a temp account base: setup's record and reservation are made
//! directly, the host uses explicit development custody, and no worker binary is needed.
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;

use org_host_control::listen::ListenConfig;

use super::*;
use crate::instance::setup::{Input, Record};
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
    let temp = tempfile::tempdir_in("/private/tmp").unwrap();
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
            root: self.base.join("instance"),
            listen,
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
    assert!(!db.administrator_exists().unwrap());
    let created = fs::metadata(f.registry_db()).unwrap().modified().unwrap();
    // The installed admin is bound to the frozen setup operation and the setup origin.
    let admin = host.service.admin().unwrap();
    assert!(admin.issues_for(&record.setup_operation, "http://127.0.0.1:7420"));
    drop(db);
    host.shutdown();

    // A later start reopens the same registry rather than creating another.
    let host = f.start().unwrap();
    assert_eq!(
        fs::metadata(f.registry_db()).unwrap().modified().unwrap(),
        created
    );
    assert!(host.service.admin().is_some());
    host.shutdown();
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
    f.record(listen(7420), Some(crate::ids::generate().unwrap()));
    assert!(f.start().is_err());
    assert!(!f.base.join("instance").exists(), "no reservation made");

    // A record naming another ID than the reservation.
    let f = fixture();
    let listen_config = listen(7420);
    f.reserve(Some(&listen_config));
    f.record(listen_config, Some(crate::ids::generate().unwrap()));
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
