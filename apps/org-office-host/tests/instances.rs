//! Private-bootstrap acceptance; every path and key belongs to this disposable fixture.
#![cfg(debug_assertions)]
// The one test starts a real server, which only macOS can spawn (BUG-187).
#![cfg(target_os = "macos")]

mod support;

use std::fs;
use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
use std::path::Path;
use std::time::{Duration, Instant};

use org_office_host::{start_instance_for_test, Config};
use org_proto::OfficeStatus;

const OFFICE: &str = "01AAAAAAAAAAAAAAAAAAAAAAAA";

fn private_dir(path: &Path) {
    fs::DirBuilder::new().mode(0o700).create(path).unwrap();
}

#[test]
fn two_long_root_instances_run_the_same_office_id_with_separate_data_sockets_and_keys() {
    let base = if cfg!(target_os = "linux") {
        "/var/tmp"
    } else {
        "/private/tmp"
    };
    let fixture = tempfile::tempdir_in(base).unwrap();
    let base = fixture.path().canonicalize().unwrap();
    let locator = base.join("locator");
    let temporary = base.join("temp");
    let keys = base.join("keys");
    private_dir(&temporary);
    private_dir(&keys);
    let roots = [base.join("a".repeat(120)), base.join("b".repeat(120))];
    let config = || {
        let mut config = Config::new(base.join("unused"), support::fake_server());
        config.dev_key_dir = Some(keys.clone());
        config.timings = support::fast_timings();
        config
    };
    // Initialize identities before populating identical office IDs in separate catalogs.
    let mut key_files = Vec::new();
    for (index, root) in roots.iter().enumerate() {
        let name = if index == 0 { "alpha" } else { "beta" };
        let host =
            start_instance_for_test(config(), &locator, &temporary, Some(name), root).unwrap();
        host.shutdown();
        let control: serde_json::Value =
            serde_json::from_slice(&fs::read(root.join("instance.json")).unwrap()).unwrap();
        let id = control["binding"]["id"].as_str().unwrap();
        let key_dir = keys.join(id);
        private_dir(&key_dir);
        let key_file = key_dir.join(format!("{OFFICE}.key"));
        fs::write(
            &key_file,
            if index == 0 {
                "1".repeat(64)
            } else {
                "2".repeat(64)
            },
        )
        .unwrap();
        fs::set_permissions(&key_file, fs::Permissions::from_mode(0o600)).unwrap();
        key_files.push(key_file);
        let office_dir = root.join("office-state/offices").join(OFFICE);
        private_dir(&office_dir);
        let office = support::hq(
            OFFICE,
            if index == 0 { "ORGA" } else { "ORGB" },
            name,
            "2026-11-10T00:00:00.000Z",
        );
        fs::write(office_dir.join("office.json"), office.to_vec().unwrap()).unwrap();
        fs::write(root.join("sentinel"), name).unwrap();
    }
    let a =
        start_instance_for_test(config(), &locator, &temporary, Some("alpha"), &roots[0]).unwrap();
    let b =
        start_instance_for_test(config(), &locator, &temporary, Some("beta"), &roots[1]).unwrap();
    assert_ne!(a.socket_path(), b.socket_path());
    assert!(
        start_instance_for_test(config(), &locator, &temporary, Some("alpha"), &roots[0]).is_err()
    );
    let deadline = Instant::now() + Duration::from_secs(15);
    let endpoints = loop {
        let a_info = support::office_info(&a, OFFICE).unwrap();
        let b_info = support::office_info(&b, OFFICE).unwrap();
        if a_info.status == OfficeStatus::Active && b_info.status == OfficeStatus::Active {
            break [a_info.endpoint.unwrap(), b_info.endpoint.unwrap()];
        }
        assert!(
            Instant::now() < deadline,
            "instances did not become active: {a_info:?}, {b_info:?}"
        );
        std::thread::sleep(Duration::from_millis(20));
    };
    assert_ne!(endpoints[0], endpoints[1]);
    for endpoint in &endpoints {
        assert!(endpoint.len() <= org_proto::location::MAX_SOCKET_PATH_BYTES);
        assert!(Path::new(endpoint).exists());
    }
    assert_ne!(
        fs::read(&key_files[0]).unwrap(),
        fs::read(&key_files[1]).unwrap()
    );
    a.shutdown();
    assert_eq!(
        support::office_info(&b, OFFICE).unwrap().status,
        OfficeStatus::Active
    );
    assert_eq!(
        fs::read_to_string(roots[1].join("sentinel")).unwrap(),
        "beta"
    );
    assert!(key_files[1].exists());
    b.shutdown();
}
