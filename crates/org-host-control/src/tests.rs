use serde_json::{json, Value};

use crate::gateway::*;
use crate::journal::{fingerprint_input, Journaled};
use crate::listen::*;
use crate::ops::*;
use crate::*;

const MAC: &str = "8637d563af0edf21100827a4a0a1f79f0053b6d974f0a9afb30465412e4b95a5";

#[test]
fn the_contract_has_its_own_version_space() {
    assert_eq!(API_VERSION, 1);
    assert_eq!(SUPPORTED_API, ApiRange { min: 1, max: 1 });
    assert_eq!(Hello::METHOD, "hello");
    // `hello` answers with the instance.
    let hello =
        HelloResult::host("0.0.0", "This Mac").with_instance_id("01M72N8MG00000000000000I01");
    assert_eq!(
        serde_json::to_value(&hello).unwrap()["instance_id"],
        "01M72N8MG00000000000000I01"
    );
}

#[test]
fn op_methods() {
    for (method, want) in [
        (AccountVerify::METHOD, "account.verify"),
        (SessionRenew::METHOD, "session.renew"),
        (SessionEnd::METHOD, "session.end"),
        (SessionGrant::METHOD, "session.grant"),
        (OrganizationCreate::METHOD, "organization.create"),
        (OrganizationCancel::METHOD, "organization.cancel"),
        (OrganizationList::METHOD, "organization.list"),
        (OfficeList::METHOD, "office.list"),
        (OfficeRestart::METHOD, "office.restart"),
        (OfficeOpenChannel::METHOD, "office.open_channel"),
        (OperationGet::METHOD, "operation.get"),
        (TlsFetch::METHOD, "tls.fetch"),
        (TlsSet::METHOD, "tls.set"),
        (AccountSetPassword::METHOD, "account.set_password"),
        (AccountResetPassword::METHOD, "account.reset_password"),
        (AccountCreate::METHOD, "account.create"),
        (RecoveryRelease::METHOD, "recovery.release"),
        (RecoveryAcknowledge::METHOD, "recovery.acknowledge"),
        (Subscribe::METHOD, "subscribe"),
        (Bootstrap::METHOD, "bootstrap"),
        (Exit::METHOD, "exit"),
    ] {
        assert_eq!(method, want);
    }
    for (name, want) in [
        (event::ORGANIZATION_CREATED, "organization.created"),
        (event::ORGANIZATION_CHANGED, "organization.changed"),
        (event::OFFICE_STATUS_CHANGED, "office.status_changed"),
        (event::ACCOUNT_CHANGED, "account.changed"),
        (event::TLS_RENEW, "tls.renew"),
    ] {
        assert_eq!(name, want);
    }
}

#[test]
fn sign_in_has_its_wire_shape_and_hides_its_secrets() {
    let params: AccountVerifyParams =
        serde_json::from_value(json!({"login": "ada", "password": "correct horse"})).unwrap();
    assert_eq!(params.password.expose(), "correct horse");
    assert!(!format!("{params:?}").contains("correct horse"));

    let wire = json!({
        "session": "an-opaque-handle",
        "account": "01M72N8MG00000000000000A01",
        "admin": true,
        "orgs": [{"org_id": "01M72N8MG00000000000000G01", "member_id": "01M72N8MG00000000000000M01",
                  "office_ids": ["01M72N8MG00000000000000F01"]}]
    });
    let result: AccountVerifyResult = serde_json::from_value(wire.clone()).unwrap();
    assert_eq!(result.session.expose(), "an-opaque-handle");
    assert_eq!(result.orgs[0].office_ids.len(), 1);
    assert!(!format!("{result:?}").contains("an-opaque-handle"));
    assert_eq!(serde_json::to_value(&result).unwrap(), wire);

    // Secrets are non-empty strings, and a wrong type never echoes the value.
    assert!(
        serde_json::from_value::<AccountVerifyParams>(json!({"login": "ada", "password": ""}))
            .is_err()
    );
    let error =
        serde_json::from_value::<AccountVerifyParams>(json!({"login": "ada", "password": 12345}))
            .unwrap_err()
            .to_string();
    assert!(!error.contains("12345"), "{error}");
    assert!(serde_json::from_value::<SessionParams>(json!({})).is_err());
}

#[test]
fn a_grant_comes_back_as_org_proto_writes_it() {
    let grant = json!({
        "account": "01M72N8MG00000000000000A01", "member": "01M72N8MG00000000000000M01",
        "org_id": "01M72N8MG00000000000000G01", "office_id": "01M72N8MG00000000000000F01",
        "channel_id": "01M72N8MG00000000000000C01", "session_id": "01M72N8MG00000000000000S01",
        "exp": 1762766100000u64, "mac": MAC
    });
    let result: SessionGrantResult = serde_json::from_value(json!({"grant": grant})).unwrap();
    assert_eq!(result.grant.claims.exp, 1_762_766_100_000);
    assert!(!format!("{result:?}").contains(MAC));
    assert_eq!(
        serde_json::to_value(&result).unwrap(),
        json!({"grant": grant})
    );
    let params: SessionGrantParams = serde_json::from_value(json!({
        "session": "h", "office_id": "01M72N8MG00000000000000F01", "channel_id": "01M72N8MG00000000000000C01"
    }))
    .unwrap();
    assert_eq!(params.channel_id, "01M72N8MG00000000000000C01");
}

#[test]
fn host_op_results_have_three_states() {
    let org = json!({
        "id": "01M72N8MG00000000000000G01", "name": "Acme",
        "created_at": "2026-11-10T12:00:00.000Z", "offices": []
    });
    for wire in [
        json!({"status": "accepted"}),
        json!({"status": "completed", "result": org}),
        json!({"status": "rejected", "refusal": {"code": "already_exists", "message": "Taken."}}),
    ] {
        let result: HostOpResult<org_proto::Organization> =
            serde_json::from_value(wire.clone()).unwrap();
        assert_ne!(result, HostOpResult::Unknown);
        assert_eq!(serde_json::to_value(&result).unwrap(), wire);
    }
    let later: HostOpResult<Empty> =
        serde_json::from_value(json!({"status": "abandoned"})).unwrap();
    assert_eq!(later, HostOpResult::Unknown);
    let restarted: HostOpResult<Empty> =
        serde_json::from_value(json!({"status": "completed", "result": {}})).unwrap();
    assert_eq!(restarted, HostOpResult::Completed { result: Empty {} });
}

#[test]
fn organization_list_carries_the_recovery_state() {
    let wire = json!({"organizations": [{
        "id": "01M72N8MG00000000000000G01", "name": "Acme",
        "created_at": "2026-11-10T12:00:00.000Z", "offices": [], "recovery": "delivered"
    }]});
    let list: OrganizationListResult = serde_json::from_value(wire.clone()).unwrap();
    assert_eq!(list.organizations[0].recovery, RecoveryState::Delivered);
    assert_eq!(list.organizations[0].organization.name, "Acme");
    assert_eq!(serde_json::to_value(&list).unwrap(), wire);
    for (state, name) in [
        (RecoveryState::Pending, "pending"),
        (RecoveryState::Delivered, "delivered"),
        (RecoveryState::Acknowledged, "acknowledged"),
    ] {
        assert_eq!(serde_json::to_value(state).unwrap(), name);
    }
    assert_eq!(
        serde_json::from_value::<RecoveryState>(json!("shown")).unwrap(),
        RecoveryState::Unknown
    );
}

#[test]
fn channels_and_lists_have_their_wire_shapes() {
    let open: OfficeOpenChannelParams =
        serde_json::from_value(json!({"session": "h", "office_id": "o"})).unwrap();
    assert_eq!(open.office_id, "o");
    assert_eq!(
        serde_json::to_value(OfficeOpenChannelResult {
            channel_id: "01M72N8MG00000000000000C01".into()
        })
        .unwrap(),
        json!({"channel_id": "01M72N8MG00000000000000C01"})
    );
    let all: OfficeListParams = serde_json::from_value(json!({"session": "h"})).unwrap();
    assert!(all.org_id.is_none());
    assert_eq!(serde_json::to_value(&all).unwrap(), json!({"session": "h"}));
}

#[test]
fn tls_fetch_has_two_modes_and_unavailable() {
    let origin = ListenOrigin::parse("https://mac.local:8443").unwrap();
    let https: TlsFetchResult = serde_json::from_value(json!({"ok": {
        "mode": "https", "cert": "-----BEGIN CERTIFICATE-----", "key": "-----BEGIN PRIVATE KEY-----x",
        "origin": "https://mac.local:8443", "hsts": "on"
    }}))
    .unwrap();
    let TlsFetchResult::Https {
        cert,
        key,
        origin: served,
        hsts,
    } = &https
    else {
        panic!("{https:?}");
    };
    assert!(cert.starts_with("-----BEGIN CERTIFICATE"));
    assert_eq!(key.expose(), "-----BEGIN PRIVATE KEY-----x");
    assert_eq!((served, *hsts), (&origin, Hsts::On));
    assert_eq!(https.listen(), Some((ListenMode::Https, &origin, Hsts::On)));
    assert!(!format!("{https:?}").contains("PRIVATE KEY"));

    let wire =
        json!({"ok": {"mode": "http_loopback", "origin": "http://127.0.0.1:8080", "hsts": "off"}});
    let loopback: TlsFetchResult = serde_json::from_value(wire.clone()).unwrap();
    assert!(matches!(
        &loopback,
        TlsFetchResult::HttpLoopback {
            hsts: Hsts::Off,
            ..
        }
    ));
    assert_eq!(serde_json::to_value(&loopback).unwrap(), wire);

    let unavailable: TlsFetchResult =
        serde_json::from_value(json!({"unavailable": {"reason": "hsts_invalid"}})).unwrap();
    assert!(matches!(
        unavailable,
        TlsFetchResult::Unavailable(ListenFailure::HstsInvalid)
    ));
    assert_eq!(unavailable.listen(), None);
}

#[test]
fn tls_fetch_refuses_a_key_where_the_mode_has_none() {
    let ok = |extra: Value| {
        let mut body = json!({"origin": "http://127.0.0.1:8080", "hsts": "off"});
        body.as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        json!({ "ok": body })
    };
    for bad in [
        ok(json!({"mode": "http_loopback", "key": "k"})),
        ok(json!({"mode": "http_loopback", "cert": "c"})),
        ok(json!({"mode": "https", "key": "k"})),
    ] {
        let error = serde_json::from_value::<TlsFetchResult>(bad)
            .unwrap_err()
            .to_string();
        assert!(!error.contains("\"k\""), "{error}");
    }
    let https = TlsFetchResult::Https {
        cert: "c".into(),
        key: TlsPrivateKey::new("k".into()).unwrap(),
        origin: ListenOrigin::parse("https://mac.local").unwrap(),
        hsts: Hsts::Clear,
    };
    assert_eq!(
        serde_json::to_value(&https).unwrap(),
        json!({"ok": {"mode": "https", "cert": "c", "key": "k", "origin": "https://mac.local", "hsts": "clear"}})
    );
}

fn journaled<P: serde::de::DeserializeOwned + Journaled>(
    method: &str,
    params: &Value,
) -> (String, Vec<u8>) {
    assert_eq!(method, P::METHOD);
    let typed: P = serde_json::from_value(params.clone()).unwrap();
    (
        typed.operation_id().to_owned(),
        fingerprint_input(&typed).unwrap(),
    )
}

/// X-02 §6.3: the same op after a new sign-in by the same account replays; another account or
/// other params is `operation_reused`.
#[test]
fn journal_vectors_replay_only_the_same_op_for_the_same_account() {
    let doc: Value = serde_json::from_str(include_str!("../test-vectors/journal.json")).unwrap();
    for case in doc["cases"].as_array().unwrap() {
        let name = case["name"].as_str().unwrap();
        let method = case["method"].as_str().unwrap();
        let fingerprint = |side: &Value| match method {
            "organization.create" => journaled::<OrganizationCreateParams>(method, side),
            "office.restart" => journaled::<OfficeRestartParams>(method, side),
            "account.create" => journaled::<AccountCreateParams>(method, side),
            other => panic!("{other}"),
        };
        let (stored_id, stored) = fingerprint(&case["stored"]["params"]);
        let (retry_id, retry) = fingerprint(&case["retry"]["params"]);
        assert_eq!(stored_id, retry_id, "{name}");
        assert_eq!(
            String::from_utf8(stored.clone()).unwrap(),
            case["fingerprint_input"].as_str().unwrap(),
            "{name}"
        );
        // Neither the session nor the operation id is part of the fingerprint.
        let text = String::from_utf8(retry.clone()).unwrap();
        assert!(
            !text.contains("session-handle") && !text.contains(&retry_id),
            "{name}"
        );
        assert!(!text.contains("password"), "{name}: {text}");
        let same_account = case["stored"]["account"] == case["retry"]["account"];
        let outcome = if stored == retry && same_account {
            "replay"
        } else {
            "operation_reused"
        };
        assert_eq!(outcome, case["expect"], "{name}");
        if outcome == "operation_reused" {
            assert_eq!(
                serde_json::from_value::<org_proto::ErrorCode>(json!(outcome)).unwrap(),
                org_proto::ErrorCode::OperationReused
            );
        }
    }
}

#[test]
fn gateway_rendezvous_lines_have_their_wire_shapes() {
    fn clone_only<T: Clone>() {}
    clone_only::<GatewayFirstLine>();
    let first = GatewayFirstLine {
        rendezvous: "/run/user/501/agentic-org/r/0123456789abcdef".into(),
        instance_id: "01M72N8MG00000000000000I01".into(),
    };
    let wire = json!({
        "v": 1, "rendezvous": "/run/user/501/agentic-org/r/0123456789abcdef",
        "instance_id": "01M72N8MG00000000000000I01"
    });
    assert_eq!(serde_json::to_value(&first).unwrap(), wire);
    assert_eq!(
        serde_json::from_value::<GatewayFirstLine>(wire).unwrap(),
        first
    );
    for bad in [
        json!({"v": 2, "rendezvous": "/r", "instance_id": "i"}),
        json!({"v": 1, "rendezvous": "r", "instance_id": "i"}),
        json!({"v": 1, "rendezvous": "/r"}),
        json!({"v": 1, "rendezvous": "/r", "instance_id": "../x"}),
        json!({"v": 1, "rendezvous": "/r", "instance_id": ""}),
    ] {
        assert!(serde_json::from_value::<GatewayFirstLine>(bad).is_err());
    }

    let mac = GatewayReadyForSecrets { landlock: None };
    assert_eq!(
        serde_json::to_value(mac).unwrap(),
        json!({"v": 1, "event": "ready_for_secrets"})
    );
    let linux = GatewayReadyForSecrets {
        landlock: Some(LandlockStatus {
            abi: 4,
            enforcement: LandlockEnforcement::FullyEnforced,
        }),
    };
    let wire = json!({"v": 1, "event": "ready_for_secrets",
                      "landlock": {"abi": 4, "enforcement": "fully_enforced"}});
    assert_eq!(serde_json::to_value(linux).unwrap(), wire);
    assert_eq!(
        serde_json::from_value::<GatewayReadyForSecrets>(wire).unwrap(),
        linux
    );
    assert!(
        serde_json::from_value::<GatewayReadyForSecrets>(json!({"v": 1, "event": "ready"}))
            .is_err()
    );

    // The handoff names the transfer socket, then the listener, and carries no key.
    let wire = json!({"v": 1, "fds": ["transfer", "listener"]});
    assert_eq!(serde_json::to_value(GatewayHandoff).unwrap(), wire);
    assert_eq!(
        serde_json::from_value::<GatewayHandoff>(wire).unwrap(),
        GatewayHandoff
    );
    for bad in [
        json!({"v": 1, "fds": ["listener", "transfer"]}),
        json!({"v": 1, "fds": ["transfer"]}),
        json!({"v": 1, "fds": []}),
        json!({"v": 2, "fds": ["transfer", "listener"]}),
    ] {
        assert!(serde_json::from_value::<GatewayHandoff>(bad).is_err());
    }
}

#[test]
fn passwords_and_keys_stay_redacted() {
    let params: AccountSetPasswordParams = serde_json::from_value(json!({
        "session": "the-handle", "current": "old secret", "new": "new secret"
    }))
    .unwrap();
    let printed = format!("{params:?}");
    for secret in ["the-handle", "old secret", "new secret"] {
        assert!(!printed.contains(secret), "{printed}");
    }
    let reset: AccountResetPasswordParams =
        serde_json::from_value(json!({"login": "ada", "new": "fresh"})).unwrap();
    assert_eq!(reset.new.expose(), "fresh");
    let set: TlsSetParams = serde_json::from_value(json!({
        "session": "h", "operation_id": "01M72N8MG00000000000000H03", "cert": "c", "key": "k-secret"
    }))
    .unwrap();
    assert!(!format!("{set:?}").contains("k-secret"));
    assert_eq!(format!("{:?}", set.key), "TlsPrivateKey(<redacted>)");
}

#[test]
fn account_recovery_and_bootstrap_ops_have_their_wire_shapes() {
    let create: AccountCreateParams = serde_json::from_value(json!({
        "session": "h", "operation_id": "01M72N8MG00000000000000H04", "login": "sam",
        "password": "pw-secret", "admin": false, "owner_of": "01M72N8MG00000000000000G01"
    }))
    .unwrap();
    assert_eq!(
        create.owner_of.as_deref(),
        Some("01M72N8MG00000000000000G01")
    );
    assert!(!format!("{create:?}").contains("pw-secret"));
    let done: HostOpResult<AccountCreated> = serde_json::from_value(json!({
        "status": "completed", "result": {"account": "01M72N8MG00000000000000A02"}
    }))
    .unwrap();
    assert!(matches!(done, HostOpResult::Completed { .. }));

    let release: RecoveryReleaseParams =
        serde_json::from_value(json!({"session": "h", "org_id": "o"})).unwrap();
    assert!(!release.redeliver);
    assert_eq!(
        serde_json::to_value(&release).unwrap(),
        json!({"session": "h", "org_id": "o"})
    );
    let again: RecoveryReleaseParams =
        serde_json::from_value(json!({"session": "h", "org_id": "o", "redeliver": true})).unwrap();
    assert!(again.redeliver);
    let code: RecoveryReleaseResult =
        serde_json::from_value(json!({"code": "WXYZ-1234-SECRET"})).unwrap();
    assert_eq!(code.code.expose(), "WXYZ-1234-SECRET");
    assert!(!format!("{code:?}").contains("SECRET"));
    let ack: RecoveryAcknowledgeParams =
        serde_json::from_value(json!({"session": "h", "org_id": "o"})).unwrap();
    assert_eq!(ack.org_id, "o");

    let bootstrap: BootstrapParams = serde_json::from_value(json!({
        "operation_id": "01M72N8MG00000000000000H05", "hq_operation_id": "01M72N8MG00000000000000H06",
        "login": "admin", "password": "first-pw", "tls": {"cert": "c", "key": "tls-key-secret"}
    }))
    .unwrap();
    let printed = format!("{bootstrap:?}");
    assert!(!printed.contains("first-pw") && !printed.contains("tls-key-secret"));
    assert_eq!(
        bootstrap.tls.as_ref().unwrap().key.expose(),
        "tls-key-secret"
    );
    let no_tls: BootstrapParams = serde_json::from_value(json!({
        "operation_id": "a", "hq_operation_id": "b", "login": "admin", "password": "p"
    }))
    .unwrap();
    assert!(no_tls.tls.is_none());
    assert!(serde_json::to_value(&no_tls).unwrap().get("tls").is_none());
    let session: BootstrapResult = serde_json::from_value(json!({"session": "sess"})).unwrap();
    assert_eq!(
        format!("{session:?}"),
        "BootstrapResult { session: SessionHandle(<redacted>) }"
    );
}

mod listen_vectors;
