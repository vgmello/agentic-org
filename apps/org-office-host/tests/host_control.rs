//! The host.sock control boundary; lifecycle setup stays in its own test fixtures.
#![cfg(debug_assertions)]
mod support;

use org_office_host::Host;
use org_proto::{ErrorCode, Outcome};
use support::{Client, TestHome};

#[test]
fn unsupported_host_control_offer_is_refused() {
    let home = TestHome::registered();
    let host = home.start(home.config()).unwrap();
    let mut client = Client::connect(host.socket_path());
    let response = client.raw(r#"{"id":1,"method":"hello","params":{"api":{"min":2,"max":2}}}"#);
    assert!(
        matches!(response.outcome, Outcome::Error(error) if error.code == ErrorCode::UnsupportedVersion)
    );
    host.shutdown();
}

#[test]
fn sessionless_create_is_not_authorized_before_params_are_interpreted() {
    let home = TestHome::registered();
    let host = home.start(home.config()).unwrap();
    let mut client = Client::connect(host.socket_path());
    let response = client.raw(r#"{"id":2,"method":"organization.create","params":{"name":42}}"#);
    assert!(
        matches!(response.outcome, Outcome::Error(error) if error.code == ErrorCode::NotAuthorized)
    );
    assert!(home.office_folders().is_empty());
    assert!(home.key_ids().is_empty());
    host.shutdown();
}

#[test]
fn an_unregistered_host_refuses_hello_status_and_create_without_effects() {
    let home = TestHome::new();
    let host = Host::start(home.config()).unwrap();
    let mut client = Client::connect(host.socket_path());
    for (method, params) in [
        ("hello", "{}"),
        ("hello", "[]"),
        ("hello", r#"{"api":{"min":2,"max":2}}"#),
        ("instance.status", "[]"),
        ("organization.create", r#"{"name":"Blocked"}"#),
        ("future.method", "{}"),
    ] {
        let response = client.raw(&format!(
            r#"{{"id":1,"method":"{method}","params":{params}}}"#
        ));
        let Outcome::Error(error) = response.outcome else {
            panic!("legacy host answered {method}")
        };
        assert_eq!(error.code, ErrorCode::NotAuthorized);
        assert_eq!(
            error.message,
            "this host has no registered instance; run setup"
        );
    }
    assert!(home.office_folders().is_empty());
    assert!(home.key_ids().is_empty());
    host.shutdown();
}
#[test]
fn a_registered_hello_and_strict_status_share_the_immutable_instance_id() {
    use org_host_control::{
        GatewayState, Hello, HelloParams, InstanceMode, InstanceStatus, InstanceStatusParams,
    };
    let home = TestHome::registered();
    let host = home.start(home.config()).unwrap();
    let mut client = Client::connect(host.socket_path());
    let hello = client.call::<Hello>(&HelloParams::default()).unwrap();
    assert_eq!(hello.instance_id.as_deref(), Some(home.instance_id()));
    assert!(hello.capabilities.is_empty());
    let result = client
        .call::<InstanceStatus>(&InstanceStatusParams::default())
        .unwrap();
    assert_eq!(result.instance_id, home.instance_id());
    assert_eq!(result.mode, InstanceMode::Service);
    assert_eq!(result.gateway.state, GatewayState::NotConfigured);
    assert_eq!(
        (
            result.offices.total,
            result.offices.active,
            result.offices.failed
        ),
        (0, 0, 0)
    );
    for params in ["[]", "1", "false", r#"{"session":"x"}"#] {
        let response = client.raw(&format!(
            r#"{{"id":99,"method":"instance.status","params":{params}}}"#
        ));
        assert!(
            matches!(response.outcome,Outcome::Error(error) if error.code==ErrorCode::InvalidRequest)
        );
    }
    host.shutdown();
}
#[test]
fn every_non_open_operation_is_refused_before_valid_or_invalid_params() {
    let home = TestHome::registered();
    let host = home.start(home.config()).unwrap();
    let mut client = Client::connect(host.socket_path());
    for method in [
        "organization.create",
        "organization.list",
        "office.list",
        "subscribe",
        "office.app_token",
        "office.restart",
        "account.verify",
        "bootstrap",
        "exit",
        "future.method",
    ] {
        for params in ["{}", r#"{"name":"Blocked"}"#, "[]"] {
            let response = client.raw(&format!(
                r#"{{"id":9,"method":"{method}","params":{params}}}"#
            ));
            assert!(
                matches!(response.outcome,Outcome::Error(error) if error.code==ErrorCode::NotAuthorized),
                "{method}/{params}"
            );
        }
    }
    assert!(home.office_folders().is_empty());
    assert!(home.key_ids().is_empty());
    host.shutdown();
}

#[test]
fn unsupported_hello_closes_before_a_buffered_status_request() {
    use std::io::{BufRead, BufReader, Write};
    use std::os::unix::net::UnixStream;
    use std::time::Duration;
    let home = TestHome::registered();
    let host = home.start(home.config()).unwrap();
    for (min, max) in [(2, 2), (2, u32::MAX), (u32::MAX, u32::MAX)] {
        let mut out = UnixStream::connect(host.socket_path()).unwrap();
        out.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
        let mut reader = BufReader::new(out.try_clone().unwrap());
        out.write_all(
            format!(
                r#"{{"id":1,"method":"hello","params":{{"api":{{"min":1,"max":{max}}}}}}}
{{"id":2,"method":"hello","params":{{"api":{{"min":{min},"max":{max}}}}}}}
{{"id":3,"method":"instance.status","params":{{}}}}
"#
            )
            .as_bytes(),
        )
        .unwrap();
        let mut line = String::new();
        assert!(reader.read_line(&mut line).unwrap() > 0);
        let response: org_proto::Response = serde_json::from_str(&line).unwrap();
        assert_eq!(response.id, 1);
        let hello = response.into_result::<org_host_control::Hello>().unwrap();
        assert_eq!(hello.api_version, 1);
        line.clear();
        assert!(reader.read_line(&mut line).unwrap() > 0);
        let response: org_proto::Response = serde_json::from_str(&line).unwrap();
        assert_eq!(response.id, 2);
        assert!(
            matches!(response.outcome,Outcome::Error(error) if error.code==ErrorCode::UnsupportedVersion)
        );
        line.clear();
        assert_eq!(
            reader.read_line(&mut line).unwrap(),
            0,
            "buffered status was dispatched: {line}"
        );
    }
    host.shutdown();
}
#[test]
fn status_counts_observed_office_states_without_revealing_ids() {
    use org_host_control::{InstanceStatus, InstanceStatusParams};
    use std::time::Duration;
    let home = TestHome::registered();
    const A: &str = "01AAAAAAAAAAAAAAAAAAAAAAAA";
    const B: &str = "01BBBBBBBBBBBBBBBBBBBBBBBB";
    const C: &str = "01CCCCCCCCCCCCCCCCCCCCCCCC";
    for (id, name) in [(A, "Alpha"), (B, "Beta"), (C, "Missing")] {
        home.write_office(&support::hq(id, id, name, "2026-11-11T00:00:00.000Z"));
    }
    home.remove_key(C);
    let host = home.start(home.config()).unwrap();
    let running_status = if cfg!(target_os = "macos") {
        org_proto::OfficeStatus::Active
    } else {
        org_proto::OfficeStatus::Unavailable
    };
    support::eventually(Duration::from_secs(10), "native offices settled", || {
        let offices = org_office_host::test_support::Fixture::new(&host).offices();
        [A, B].iter().all(|id| {
            offices
                .iter()
                .any(|office| office.id == *id && office.status == running_status)
        }) && offices
            .iter()
            .any(|office| office.id == C && office.status == org_proto::OfficeStatus::Unavailable)
    });
    let mut client = Client::connect(host.socket_path());
    let result = client
        .call::<InstanceStatus>(&InstanceStatusParams::default())
        .unwrap();
    let encoded = serde_json::to_string(&result).unwrap();
    for id in [A, B, C] {
        assert!(!encoded.contains(id));
    }
    assert_eq!(
        (
            result.offices.total,
            result.offices.active,
            result.offices.failed
        ),
        if cfg!(target_os = "macos") {
            (3, 2, 1)
        } else {
            (3, 0, 3)
        }
    );
    host.shutdown();
}

#[test]
fn hello_shape_errors_precede_version_refusal_and_do_not_close() {
    let home = TestHome::registered();
    let host = home.start(home.config()).unwrap();
    let mut client = Client::connect(host.socket_path());
    for params in [
        "[]",
        "false",
        r#"{"api":{"min":2,"max":2},"client":7}"#,
        r#"{"api":{"min":0,"max":2}}"#,
        r#"{"api":{"min":2,"max":1}}"#,
        r#"{"client":null}"#,
    ] {
        let response = client.raw(&format!(r#"{{"id":8,"method":"hello","params":{params}}}"#));
        assert!(
            matches!(response.outcome,Outcome::Error(error) if error.code==ErrorCode::InvalidRequest)
        );
    }
    let response = client.raw(r#"{"id":9,"method":"hello","params":{"future":true,"api":null}}"#);
    assert_eq!(
        response
            .into_result::<org_host_control::Hello>()
            .unwrap()
            .api_version,
        1
    );
    host.shutdown();
}

#[test]
fn even_a_development_app_peer_cannot_use_retired_wire_calls() {
    let home = TestHome::registered();
    const A: &str = "01AAAAAAAAAAAAAAAAAAAAAAAA";
    home.write_office(&support::hq(A, "ORGA", "Acme", "2026-11-11T00:00:00.000Z"));
    let mut config = home.config();
    config.dev_app_path = Some(std::env::current_exe().unwrap());
    let host = home.start(config).unwrap();
    let expected = if cfg!(target_os = "macos") {
        org_proto::OfficeStatus::Active
    } else {
        org_proto::OfficeStatus::Unavailable
    };
    support::eventually(std::time::Duration::from_secs(10), "office active", || {
        support::office_info(&host, A).is_some_and(|o| o.status == expected)
    });
    let before = home.pids(A);
    let keys = home.key_ids();
    let mut client = Client::connect(host.socket_path());
    client
        .call::<org_host_control::Hello>(&org_host_control::HelloParams::default())
        .unwrap();
    assert_eq!(
        client
            .call::<org_proto::OfficeAppToken>(&org_proto::OfficeAppTokenParams {
                office_id: A.into()
            })
            .unwrap_err()
            .code,
        ErrorCode::NotAuthorized
    );
    assert_eq!(
        client
            .call::<org_proto::OfficeRestart>(&org_proto::OfficeRestartParams {
                office_id: A.into()
            })
            .unwrap_err()
            .code,
        ErrorCode::NotAuthorized
    );
    assert_eq!(home.pids(A), before);
    assert_eq!(home.key_ids(), keys);
    host.shutdown();
}
