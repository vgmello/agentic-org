//! The host launching the **real** office server with launch v3 (X-02 §5.1): create → created
//! and ready, start → ready, a stop that ends the worker within the grace, and a discard →
//! discarded. The server is the workspace's own `org-server` binary, next to this test's
//! folder (`target/<profile>/org-server`): build it first (`cargo build -p org-server`; a
//! workspace test run builds it). This host may not link the server's library: the two
//! SQLCipher fences stay in separate processes (`hostdb::sqlcipher`'s dependency test).
//! Debug builds on macOS only (dev keys; the spawn).

#![cfg(all(debug_assertions, target_os = "macos"))]

mod support;

use std::path::PathBuf;
use std::time::{Duration, Instant};

use org_proto::{Empty, Hello, HelloParams, OfficeList, OfficeStatus, OrganizationCreate};
use support::{eventually, Client, TestHome};

const WAIT: Duration = Duration::from_secs(30);

/// The workspace's `org-server`, built for this profile.
fn real_server() -> PathBuf {
    let exe = std::env::current_exe().unwrap();
    let server = exe.parent().unwrap().parent().unwrap().join("org-server");
    assert!(
        server.is_file(),
        "{} isn't built: run `cargo build -p org-server` first",
        server.display()
    );
    server
}

fn status(client: &mut Client, office_id: &str) -> OfficeStatus {
    client
        .call::<OfficeList>(&Empty {})
        .unwrap()
        .offices
        .into_iter()
        .find(|o| o.id == office_id)
        .map_or(OfficeStatus::Unknown, |o| o.status)
}

/// The organization name the office's own socket answers `hello` with.
fn office_hello(home: &TestHome, office_id: &str) -> String {
    let socket = org_proto::location::office_socket_in(&home.runtime(), office_id).unwrap();
    let hello = Client::connect(&socket)
        .call::<Hello>(&HelloParams::default())
        .unwrap();
    hello.office.unwrap().organization_name
}

#[test]
fn the_real_server_is_created_started_stopped_and_discarded() {
    let home = TestHome::registered();
    let mut config = home.config();
    config.set_server_program(real_server());
    let grace = config.timings.shutdown_grace;

    // create → created + ready.
    let host = home.start(config.clone()).unwrap();
    let mut client = Client::for_host(&host);
    let org = client
        .call::<OrganizationCreate>(&org_proto::OrganizationCreateParams {
            name: "Acme".into(),
        })
        .unwrap();
    let office_id = org.offices[0].id.clone();
    eventually(WAIT, "the created office to be active", || {
        status(&mut client, &office_id) == OfficeStatus::Active
    });
    assert_eq!(office_hello(&home, &office_id), "Acme");

    // A stop ends the rendezvous: the worker exits by itself, well within the grace (a worker
    // that ignored it would be SIGKILLed only at the grace, leaving its socket behind).
    let socket = org_proto::location::office_socket_in(&home.runtime(), &office_id).unwrap();
    let stopping = Instant::now();
    host.shutdown();
    assert!(stopping.elapsed() < grace, "{:?}", stopping.elapsed());
    assert!(!socket.exists(), "the worker didn't exit cleanly");

    // start → ready, with the instance and key it was created with.
    let host = home.start(config.clone()).unwrap();
    let mut client = Client::for_host(&host);
    eventually(WAIT, "the office to start again", || {
        status(&mut client, &office_id) == OfficeStatus::Active
    });
    assert_eq!(office_hello(&home, &office_id), "Acme");
    host.shutdown();

    // discard → discarded: an unfinished folder, with its key, goes, and so does the key.
    let unfinished = "01UNFINISHEDUNFINISHEDUNFI";
    std::fs::create_dir_all(home.office_dir(unfinished)).unwrap();
    home.write_key(unfinished);
    let host = home.start(config).unwrap();
    eventually(WAIT, "the unfinished office to be discarded", || {
        !home.office_dir(unfinished).exists() && !home.has_key(unfinished)
    });
    host.shutdown();
    home.assert_no_secret_in_home();
}
