//! Debug-only native fixture driver. No host.sock method exposes this pipe.
#[cfg(debug_assertions)]
fn main() {
    use org_office_host::{start_instance_for_test, test_support::Fixture, Config};
    use org_proto::{OrganizationCreate, OrganizationCreateParams, Request};
    use std::io::{BufRead, Write};
    use std::path::PathBuf;
    use std::time::Duration;
    let mut args = std::env::args_os().skip(1);
    let base = PathBuf::from(args.next().expect("fixture base"));
    let server = PathBuf::from(args.next().expect("fake worker"));
    let mut config = Config::new(base.join("i/office-state"), server);
    config.dev_key_dir = Some(base.join("k"));
    let host = start_instance_for_test(
        config,
        &base.join("l"),
        &base.join("t"),
        None,
        &base.join("i"),
    )
    .unwrap();
    let fixture = Fixture::new(&host);
    for (index, line) in std::io::stdin().lock().lines().enumerate() {
        let line = line.unwrap();
        if line == "STOP" {
            break;
        }
        let name: String = serde_json::from_str(&line).unwrap();
        fixture.submit(
            Request::new::<OrganizationCreate>(index as u64, &OrganizationCreateParams { name })
                .unwrap(),
        );
        let response = fixture
            .recv(Duration::from_secs(30))
            .expect("native create answered");
        let mut out = std::io::stdout().lock();
        serde_json::to_writer(&mut out, &response).unwrap();
        out.write_all(b"\n").unwrap();
        out.flush().unwrap();
    }
    host.shutdown();
}
#[cfg(not(debug_assertions))]
fn main() {
    panic!("the native fixture driver is debug-only");
}
