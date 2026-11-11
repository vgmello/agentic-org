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
    // It is never sent: writing it fails rather than sending "unknown".
    assert!(serde_json::to_value(&later).is_err());
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
    // It is never sent: writing it fails rather than inventing a state.
    assert!(serde_json::to_value(RecoveryState::Unknown).is_err());
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
/// other params is `operation_reused`, and so is an `account.create` whose password doesn't match
/// the row's create-time verifier (a copy of the create's Argon2id hash, which the host checks;
/// `password_verifier` stands for that check).
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
        // Only account.create carries a password, and its rows say whether it matches.
        let verifier_matches = match case.get("password_verifier") {
            None => {
                assert_ne!(method, "account.create", "{name}: no password_verifier");
                true
            }
            Some(v) => {
                assert_eq!(method, "account.create", "{name}");
                let want = v == "match";
                assert!(want || v == "mismatch", "{name}: {v}");
                // The password never reaches the fingerprint, so the verifier is what tells two
                // passwords apart.
                let same_password =
                    case["stored"]["params"]["password"] == case["retry"]["params"]["password"];
                assert_eq!(want, same_password, "{name}");
                if !want {
                    // Only the password differs, and the fingerprint doesn't see it.
                    assert_eq!(stored, retry, "{name}");
                }
                want
            }
        };
        let outcome = if stored == retry && same_account && verifier_matches {
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

/// A vector's pair, as `bootstrap` carries it.
fn tls_pair(cert: &Value, key: &Value) -> TlsPair {
    serde_json::from_value(json!({"cert": cert, "key": key})).unwrap()
}

/// A refusal's wire `code`.
fn code<T: serde::Serialize>(refusal: &T) -> String {
    serde_json::to_value(refusal).unwrap()["code"]
        .as_str()
        .unwrap()
        .to_owned()
}

/// X-02 §6.3: `bootstrap`'s fingerprint is `login` and `hq_operation_id`; a replay checks the
/// sign-in first, then the fingerprint, then the pair by [`TlsPair::fingerprint`]: another pair
/// is `tls_mismatch`, and one with no fingerprint is `tls_invalid`, unstored.
#[test]
fn bootstrap_vectors_fingerprint_login_and_hq_operation_id_only() {
    let doc: Value = serde_json::from_str(include_str!("../test-vectors/journal.json")).unwrap();
    let vectors = &doc["bootstrap"];
    let stored_params = &vectors["stored"];
    let (stored_id, stored) = journaled::<BootstrapParams>("bootstrap", stored_params);
    let stored_pair = tls_pair(&stored_params["tls"]["cert"], &stored_params["tls"]["key"])
        .fingerprint()
        .unwrap();
    assert_eq!(
        String::from_utf8(stored.clone()).unwrap(),
        vectors["fingerprint_input"].as_str().unwrap()
    );
    let mut seen = Vec::new();
    for retry in vectors["retries"].as_array().unwrap() {
        let name = retry["name"].as_str().unwrap();
        let params = &retry["params"];
        let (retry_id, fingerprint) = journaled::<BootstrapParams>("bootstrap", params);
        assert_eq!(retry_id, stored_id, "{name}");
        // Never the password, the pair or the operation id.
        let text = String::from_utf8(fingerprint.clone()).unwrap();
        for secret in ["password", "MIG", "CERTIFICATE", &retry_id] {
            assert!(!text.contains(secret), "{name}: {text}");
        }
        let same = fingerprint == stored;
        assert_eq!(same, retry["same_fingerprint"], "{name}");
        // The replay's order: sign-in, fingerprint, pair.
        let signs_in = params["login"] == stored_params["login"]
            && params["password"] == stored_params["password"];
        let pair = params
            .get("tls")
            .map(|pair| tls_pair(&pair["cert"], &pair["key"]).fingerprint());
        let outcome = if !signs_in {
            "not_authorized".to_owned()
        } else if !same {
            "operation_reused".to_owned()
        } else {
            match pair {
                // No fingerprint: tls_invalid, never stored.
                Some(Err(invalid)) => {
                    let refusal = BootstrapRefusal::Tls(TlsRefusal::from(invalid));
                    assert!(!refusal.is_stored(), "{name}");
                    code(&refusal)
                }
                Some(Ok(pair)) if pair != stored_pair => code(&BootstrapRefusal::TlsMismatch),
                _ => "replay".to_owned(),
            }
        };
        assert_eq!(outcome, retry["expect"], "{name}");
        seen.push((retry["same_fingerprint"].as_bool().unwrap(), outcome));
    }
    let seen: Vec<(bool, &str)> = seen.iter().map(|(same, o)| (*same, o.as_str())).collect();
    // Both sides of the fingerprint, and every outcome of the order.
    for want in [
        (true, "replay"),
        (true, "tls_mismatch"),
        (true, "tls_invalid"),
        (true, "not_authorized"),
        (false, "operation_reused"),
        (false, "not_authorized"),
    ] {
        assert!(seen.contains(&want), "{want:?}");
    }
}

/// X-02 §6.2, §6.3: `tls.set`'s fingerprint fields are the SHA-256 of the certificate's and the
/// key's DER, as lowercase hex, never the pair; the expected values come from openssl. A pair
/// with no block to hash has no fingerprint.
#[test]
fn tls_set_vectors_fingerprint_the_der_hashes_only() {
    let doc: Value = serde_json::from_str(include_str!("../test-vectors/journal.json")).unwrap();
    let vectors = &doc["tls_set"];
    let cases = vectors["cases"].as_array().unwrap();
    let mut fields = Vec::new();
    for case in cases {
        let name = case["name"].as_str().unwrap();
        let params = &case["params"];
        let (operation_id, input) = journaled::<TlsSetParams>("tls.set", params);
        let (cert, key) = (&case["cert_der_sha256"], &case["key_der_sha256"]);
        for hash in [cert, key] {
            let hash = hash.as_str().unwrap();
            assert!(
                hash.len() == 64 && hash.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f')),
                "{name}: {hash}"
            );
        }
        // Exactly these two fields, in this order.
        let want = format!(r#"{{"cert_der_sha256":{cert},"key_der_sha256":{key}}}"#);
        assert_eq!(
            String::from_utf8(input.clone()).unwrap(),
            format!("tls.set\u{0}{want}"),
            "{name}"
        );
        // Never the pair, the session or the operation id.
        let text = String::from_utf8(input).unwrap();
        for secret in ["-----", "session-handle", &operation_id] {
            assert!(!text.contains(secret), "{name}: {text}");
        }
        let key_body: String = params["key"]
            .as_str()
            .unwrap()
            .lines()
            .filter(|line| !line.starts_with("-----") && !line.is_empty())
            .collect();
        assert!(!text.contains(&key_body[..16]), "{name}");
        fields.push((params, cert, key));
    }
    // The same text always hashes the same, and another key with the same certificate changes
    // key_der_sha256 only.
    let mut another_key = false;
    for (a, a_cert, a_key) in &fields {
        for (b, b_cert, b_key) in &fields {
            if a["cert"] == b["cert"] {
                assert_eq!(a_cert, b_cert);
                if a["key"] != b["key"] && a_key != b_key {
                    another_key = true;
                }
            }
            if a["key"] == b["key"] {
                assert_eq!(a_key, b_key);
            }
        }
    }
    assert!(another_key);
    // RFC 7468's line ends: LF, CRLF and a lone CR frame the same DER, so the same fingerprint.
    let mut eols = Vec::new();
    for case in vectors["line_ends"].as_array().unwrap() {
        let name = case["name"].as_str().unwrap();
        let eol = case["eol"].as_str().unwrap();
        let params = &case["params"];
        for text in [&params["cert"], &params["key"]] {
            let text = text.as_str().unwrap();
            let others = text.replace(eol, "");
            assert!(
                !others.contains(['\r', '\n']),
                "{name}: only {eol:?} ends a line"
            );
        }
        let (_, input) = journaled::<TlsSetParams>("tls.set", params);
        let (cert, key) = (&case["cert_der_sha256"], &case["key_der_sha256"]);
        assert_eq!(
            String::from_utf8(input).unwrap(),
            format!(
                r#"tls.set{}{{"cert_der_sha256":{cert},"key_der_sha256":{key}}}"#,
                '\0'
            ),
            "{name}"
        );
        assert_eq!((cert, key), (fields[0].1, fields[0].2), "{name}");
        eols.push(eol);
    }
    assert_eq!(eols, ["\n", "\r\n", "\r"]);
    // A pair the PEM reader refuses has no fingerprint: tls_invalid, never stored.
    for case in vectors["invalid"].as_array().unwrap() {
        let name = case["name"].as_str().unwrap();
        let params: TlsSetParams = serde_json::from_value(case["params"].clone()).unwrap();
        let error = params.fingerprint_fields().unwrap_err().to_string();
        assert!(fingerprint_input(&params).is_err(), "{name}");
        // The error never quotes the pair.
        assert!(
            !error.contains("MII") && !error.contains("-----"),
            "{name}: {error}"
        );
        assert_eq!(case["fingerprint"], "error", "{name}");
        let pair = tls_pair(&case["params"]["cert"], &case["params"]["key"]);
        let refusal = TlsRefusal::from(pair.fingerprint().unwrap_err());
        assert_eq!(code(&refusal), case["expect"].as_str().unwrap(), "{name}");
        assert!(!refusal.is_stored(TlsInstallOp::TlsSet), "{name}");
    }
}

/// X-02 §6.3: `tls.set`'s journal lookup needs the pair's fingerprint. A known `operation_id`
/// with a pair that has none is `tls_invalid`, unstored, never a replay or `operation_reused`.
#[test]
fn tls_set_lookup_without_a_fingerprint_is_tls_invalid() {
    let doc: Value = serde_json::from_str(include_str!("../test-vectors/journal.json")).unwrap();
    let vectors = &doc["tls_set"]["lookup"];
    let (stored_id, stored) = journaled::<TlsSetParams>("tls.set", &vectors["stored"]["params"]);
    let mut seen = Vec::new();
    for retry in vectors["retries"].as_array().unwrap() {
        let name = retry["name"].as_str().unwrap();
        let params: TlsSetParams = serde_json::from_value(retry["params"].clone()).unwrap();
        // The lookup is by operation_id: every retry here names a known one.
        assert_eq!(params.operation_id(), stored_id, "{name}");
        let (fingerprint, outcome) = match fingerprint_input(&params) {
            Err(_) => {
                let pair = tls_pair(&retry["params"]["cert"], &retry["params"]["key"]);
                let refusal = TlsRefusal::from(pair.fingerprint().unwrap_err());
                assert!(!refusal.is_stored(TlsInstallOp::TlsSet), "{name}");
                ("error", code(&refusal))
            }
            Ok(input) if input == stored => ("same", "replay".to_owned()),
            Ok(_) => ("other", "operation_reused".to_owned()),
        };
        assert_eq!(fingerprint, retry["fingerprint"], "{name}");
        assert_eq!(outcome, retry["expect"].as_str().unwrap(), "{name}");
        seen.push(outcome);
    }
    for want in ["replay", "operation_reused", "tls_invalid"] {
        assert!(seen.iter().any(|o| o == want), "{want}");
    }
}

/// X-02 §6.2, §6.3: `bootstrap`'s pair and `tls.set`'s pair fingerprint identically: one
/// implementation, [`TlsPair::fingerprint`], is `tls.set`'s journal fields too.
#[test]
fn bootstrap_and_tls_set_fingerprint_a_pair_identically() {
    let doc: Value = serde_json::from_str(include_str!("../test-vectors/journal.json")).unwrap();
    let vectors = &doc["tls_set"];
    let mut checked = 0;
    for group in ["cases", "line_ends", "invalid"] {
        for case in vectors[group].as_array().unwrap() {
            let name = case["name"].as_str().unwrap();
            let params: TlsSetParams = serde_json::from_value(case["params"].clone()).unwrap();
            let pair = tls_pair(&case["params"]["cert"], &case["params"]["key"]);
            match (pair.fingerprint(), params.fingerprint_fields()) {
                (Ok(pair), Ok(fields)) => {
                    assert_eq!(serde_json::to_string(&pair).unwrap(), fields, "{name}");
                    assert_eq!(pair.cert_der_sha256(), case["cert_der_sha256"], "{name}");
                    assert_eq!(pair.key_der_sha256(), case["key_der_sha256"], "{name}");
                    checked += 1;
                }
                (Err(_), Err(_)) => assert_eq!(group, "invalid", "{name}"),
                (pair, fields) => panic!("{name}: {pair:?} / {fields:?}"),
            }
        }
    }
    assert!(checked >= 9, "{checked}");
}

/// X-02 §5.2: the byte before the descriptor on `host.sock`.
#[test]
fn the_host_sock_marker_is_zero() {
    assert_eq!(HOST_SOCK_MARKER, 0x00);
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
