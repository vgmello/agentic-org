//! X-02 rev 14 vectors: `ListenOrigin`'s form and fit (`test-vectors/listen-origin.json`), and
//! `tls.fetch`, `ListenFailure`, the `tls.set`/`bootstrap` refusals and the invalid-configuration
//! rule (`test-vectors/tls.json`).

use std::net::{IpAddr, Ipv6Addr};

use serde_json::{json, Value};

use crate::listen::*;
use crate::ops::*;
use crate::{Empty, HostOpResult};

fn doc(text: &str) -> Value {
    serde_json::from_str(text).unwrap()
}

fn rows<'a>(doc: &'a Value, key: &str) -> &'a [Value] {
    doc[key].as_array().unwrap()
}

fn text(v: &Value) -> &str {
    v.as_str().unwrap()
}

fn mode(v: &Value) -> ListenMode {
    serde_json::from_value(v.clone()).unwrap()
}

#[test]
fn listen_origin_form_vectors() {
    let doc = doc(include_str!("../../test-vectors/listen-origin.json"));
    let form = rows(&doc, "form");
    assert!(form.len() > 80);
    for row in form {
        let input = text(&row["input"]);
        let result = text(&row["result"]);
        let parsed = ListenOrigin::parse(input);
        let from_serde = serde_json::from_value::<ListenOrigin>(json!(input));
        if result == "accepted" {
            let origin = parsed.unwrap_or_else(|e| panic!("{input:?}: {e}"));
            let want = text(&row["origin"]);
            assert_eq!(origin.as_str(), want, "{input:?}");
            assert_eq!(origin.to_string(), want, "{input:?}");
            assert_eq!(serde_json::to_value(&origin).unwrap(), json!(want));
            assert_eq!(from_serde.unwrap(), origin, "{input:?}");
            // The serialization is canonical: it parses back to itself.
            assert_eq!(ListenOrigin::parse(want).unwrap(), origin);
            // Every accepted origin is what a WHATWG parser gives.
            assert_eq!(row["whatwg"], json!(want), "{input:?}");
        } else {
            let error = parsed.expect_err(input);
            assert_eq!(error.code(), result, "{input:?}");
            assert!(from_serde.is_err(), "{input:?}");
            // A refusal of what a browser would send unchanged is a deliberate strictness, and
            // the row says why; no other row claims it.
            let unchanged =
                row["whatwg"].as_str() == Some(input.strip_suffix('/').unwrap_or(input));
            assert_eq!(
                unchanged,
                row.get("stricter_than_browsers").is_some(),
                "{input:?}"
            );
        }
    }
}

#[test]
fn listen_origin_fit_vectors() {
    let doc = doc(include_str!("../../test-vectors/listen-origin.json"));
    for row in rows(&doc, "fit") {
        let origin = ListenOrigin::parse(text(&row["origin"])).unwrap();
        let address: IpAddr = text(&row["address"]).parse().unwrap();
        let port = u16::try_from(row["port"].as_u64().unwrap()).unwrap();
        let got = match origin.check(mode(&row["scheme"]), address, port) {
            Ok(()) => "accepted",
            Err(e) => e.code(),
        };
        assert_eq!(got, text(&row["result"]), "{row}");
    }
}

/// The IPv6 serializer against WHATWG's (Node 26's `new URL`), and never through `Display`.
#[test]
fn ipv6_is_serialised_as_whatwg_does() {
    for (addr, want) in [
        ("::1", "::1"),
        ("::", "::"),
        ("1::", "1::"),
        ("::ffff:127.0.0.1", "::ffff:7f00:1"),
        ("2001:db8:0:0:1:0:0:1", "2001:db8::1:0:0:1"),
        ("1:0:0:2:0:0:0:3", "1:0:0:2::3"),
        ("1:0:2:3:4:5:6:7", "1:0:2:3:4:5:6:7"),
        ("1:0:0:0:0:0:0:0", "1::"),
        ("0:0:1:0:0:0:0:0", "0:0:1::"),
        (
            "abcd:ef01:2345:6789:abcd:ef01:2345:6789",
            "abcd:ef01:2345:6789:abcd:ef01:2345:6789",
        ),
        ("fe80::a:b", "fe80::a:b"),
        ("::127.0.0.1", "::7f00:1"),
    ] {
        let addr: Ipv6Addr = addr.parse().unwrap();
        assert_eq!(ipv6_whatwg(&addr), want, "{addr}");
    }
    // Rust's Display differs for an IPv4-mapped address, so it is never used.
    let mapped: Ipv6Addr = "::ffff:7f00:1".parse().unwrap();
    assert_eq!(mapped.to_string(), "::ffff:127.0.0.1");

    // Every pattern of zero and non-zero pieces round-trips through the origin's own parser.
    for mask in 0u16..256 {
        let mut pieces = [0u16; 8];
        for (i, piece) in pieces.iter_mut().enumerate() {
            if mask & (1 << i) != 0 {
                *piece = 0x00a0 + i as u16;
            }
        }
        let addr = Ipv6Addr::from(pieces);
        let origin =
            ListenOrigin::parse(&format!("https://[{}]:8443", ipv6_whatwg(&addr))).unwrap();
        assert_eq!(origin.host(), &OriginHost::Ipv6(addr));
    }
}

#[test]
fn listen_mode_and_hsts_have_their_wire_spellings() {
    for (mode, wire, scheme) in [
        (ListenMode::Https, "https", "https"),
        (ListenMode::HttpLoopback, "http_loopback", "http"),
    ] {
        assert_eq!(serde_json::to_value(mode).unwrap(), wire);
        assert_eq!(mode.scheme().as_str(), scheme);
    }
    for (hsts, wire) in [(Hsts::Off, "off"), (Hsts::On, "on"), (Hsts::Clear, "clear")] {
        assert_eq!(serde_json::to_value(hsts).unwrap(), wire);
        assert_eq!(serde_json::from_value::<Hsts>(json!(wire)).unwrap(), hsts);
        assert_eq!(wire.parse::<Hsts>().unwrap(), hsts);
        assert_eq!(hsts.to_string(), wire);
    }
    for bad in ["Off", "ON", "", "true", "max-age=0"] {
        assert_eq!(bad.parse::<Hsts>(), Err(InvalidListenConfig::Hsts));
        assert!(serde_json::from_value::<Hsts>(json!(bad)).is_err());
    }
}

#[test]
fn tls_fetch_vectors() {
    let doc = doc(include_str!("../../test-vectors/tls.json"));
    for row in rows(&doc, "fetch") {
        let name = text(&row["name"]);
        let decoded = serde_json::from_value::<TlsFetchResult>(row["wire"].clone());
        let got = match &decoded {
            Ok(TlsFetchResult::Https { .. }) => "https",
            Ok(TlsFetchResult::HttpLoopback { .. }) => "http_loopback",
            Ok(TlsFetchResult::Unavailable(_)) => "unavailable",
            Err(e) => {
                // No decode error quotes the key.
                assert!(
                    !e.to_string().contains("secret-key-material"),
                    "{name}: {e}"
                );
                "refused"
            }
        };
        assert_eq!(got, text(&row["expect"]), "{name}");
        if let Ok(result) = decoded {
            assert_eq!(
                serde_json::to_value(&result).unwrap(),
                row["wire"],
                "{name}"
            );
            assert!(
                !format!("{result:?}").contains("secret-key-material"),
                "{name}"
            );
        }
    }
}

#[test]
fn listen_failure_vectors() {
    let doc = doc(include_str!("../../test-vectors/tls.json"));
    for row in rows(&doc, "listen_failure") {
        let decoded = serde_json::from_value::<ListenFailure>(row["wire"].clone());
        assert_eq!(decoded.is_ok(), row["valid"].as_bool().unwrap(), "{row}");
        if let Ok(failure) = decoded {
            assert_eq!(serde_json::to_value(&failure).unwrap(), row["wire"]);
        }
    }
    let origin = ListenOrigin::parse("https://mac.local:8443").unwrap();
    assert_eq!(
        serde_json::to_value(ListenFailure::TlsNameMismatch {
            uncovered: Uncovered::one(origin.clone())
        })
        .unwrap(),
        json!({"reason": "tls_name_mismatch", "uncovered": ["https://mac.local:8443"]})
    );
    assert!(Uncovered::new(vec![]).is_err());
    assert!(Uncovered::new(vec![origin.clone(), origin]).is_err());
}

#[test]
fn tls_refusal_vectors() {
    let doc = doc(include_str!("../../test-vectors/tls.json"));
    for row in rows(&doc, "refusals") {
        let name = text(&row["name"]);
        let refusal: TlsRefusal = serde_json::from_value(row["wire"].clone()).unwrap();
        assert_ne!(refusal, TlsRefusal::Unknown, "{name}");
        assert_eq!(
            serde_json::to_value(&refusal).unwrap(),
            row["wire"],
            "{name}"
        );
        for op in [TlsInstallOp::TlsSet, TlsInstallOp::Bootstrap] {
            assert_eq!(
                refusal.is_stored(op),
                row["stored"][op.method()].as_bool().unwrap(),
                "{name}: {}",
                op.method()
            );
        }
        // tls.set answers it as a rejected HostOpResult, and operation.get reads it back.
        let wire = json!({"status": "rejected", "refusal": row["wire"]});
        let result: HostOpResult<Empty, TlsRefusal> = serde_json::from_value(wire.clone()).unwrap();
        assert_eq!(result, HostOpResult::Rejected { refusal });
        assert_eq!(serde_json::to_value(&result).unwrap(), wire);
        let looked_up: <OperationGet as org_proto::Call>::Result =
            serde_json::from_value(wire).unwrap();
        assert!(matches!(looked_up, HostOpResult::Rejected { .. }), "{name}");
    }
    for row in rows(&doc, "bad_refusals") {
        assert!(
            serde_json::from_value::<TlsRefusal>(row["wire"].clone()).is_err(),
            "{}",
            row["name"]
        );
    }
    // A code this build doesn't know is kept apart, and never stored.
    let later: TlsRefusal = serde_json::from_value(json!({"code": "tls_future"})).unwrap();
    assert_eq!(later, TlsRefusal::Unknown);
    assert!(!later.is_stored(TlsInstallOp::TlsSet));
    // It is never sent: writing it fails, alone or as a refusal.
    assert!(serde_json::to_value(&later).is_err());
    let rejected: HostOpResult<Empty, TlsRefusal> = HostOpResult::Rejected { refusal: later };
    assert!(serde_json::to_value(&rejected).is_err());
    assert_eq!(TlsInstallOp::TlsSet.method(), "tls.set");
    assert_eq!(TlsInstallOp::Bootstrap.method(), "bootstrap");
}

#[test]
fn listen_config_vectors() {
    let doc = doc(include_str!("../../test-vectors/tls.json"));
    for row in rows(&doc, "listen_config") {
        let name = text(&row["name"]);
        let stored: StoredListenConfig =
            serde_json::from_value(row["stored"].clone()).unwrap_or_else(|e| panic!("{name}: {e}"));
        let strict = serde_json::from_value::<ListenConfig>(row["stored"].clone());
        match (stored.validate(), text(&row["result"])) {
            (Ok(config), "valid") => {
                assert_eq!(
                    serde_json::to_value(&config).unwrap(),
                    row["stored"],
                    "{name}"
                );
                assert_eq!(strict.unwrap(), config, "{name}");
                assert_eq!(config.origin().as_str(), text(&row["stored"]["origin"]));
            }
            (Err(invalid), want) => {
                let code = match invalid {
                    InvalidListenConfig::Listen(field) => {
                        assert_eq!(field.as_str(), text(&row["field"]), "{name}");
                        "origin_invalid"
                    }
                    InvalidListenConfig::Origin(why) => {
                        assert_eq!(why.code(), text(&row["origin_error"]), "{name}");
                        "origin_invalid"
                    }
                    InvalidListenConfig::Hsts => "hsts_invalid",
                };
                assert_eq!(code, want, "{name}");
                // Setup names the field: a listener field, or the one the code names.
                let field = row["field"]
                    .as_str()
                    .unwrap_or(&code[..code.find('_').unwrap()]);
                assert_eq!(invalid.field(), field, "{name}");
                // The one rule: the host reports it as that ListenFailure, and bootstrap and
                // tls.set refuse with that code, which neither op stores.
                assert_eq!(
                    serde_json::to_value(invalid.listen_failure()).unwrap(),
                    json!({ "reason": code })
                );
                let refusal = TlsRefusal::from(invalid);
                assert_eq!(
                    serde_json::to_value(&refusal).unwrap(),
                    json!({ "code": code })
                );
                assert!(!refusal.is_stored(TlsInstallOp::TlsSet));
                assert!(!refusal.is_stored(TlsInstallOp::Bootstrap));
                assert!(strict.is_err(), "{name}");
            }
            (Ok(_), want) => panic!("{name}: valid, expected {want}"),
        }
    }
    // A value that isn't a string is kept apart from an empty one, and is never written back.
    let stored: StoredListenConfig = serde_json::from_value(json!({
        "address": "0.0.0.0", "port": 8443, "scheme": "https", "origin": 8443, "hsts": ""
    }))
    .unwrap();
    assert_eq!(stored.origin, Some(Stored::Malformed));
    assert_eq!(stored.hsts, Some(Stored::Value(String::new())));
    assert!(serde_json::to_value(&stored).is_err());
    // Each listener field is checked before the origin, which can't be checked against it.
    let both: StoredListenConfig = serde_json::from_value(json!({
        "address": "mac.local", "port": 8443, "scheme": "https", "hsts": "on"
    }))
    .unwrap();
    assert_eq!(both.address, Some(Stored::Malformed));
    assert_eq!(
        both.validate(),
        Err(InvalidListenConfig::Listen(ListenField::Address))
    );
}

#[test]
fn bootstrap_result_vectors() {
    let doc = doc(include_str!("../../test-vectors/tls.json"));
    let mut seen = Vec::new();
    for row in rows(&doc, "bootstrap") {
        let name = text(&row["name"]);
        let decoded =
            serde_json::from_value::<<Bootstrap as org_proto::Call>::Result>(row["wire"].clone());
        let got = match &decoded {
            Ok(BootstrapOutcome::Completed { result }) => {
                assert_eq!(result.session.expose(), "bootstrap-session-handle");
                assert!(!format!("{result:?}").contains("bootstrap-session-handle"));
                "completed".to_owned()
            }
            Ok(BootstrapOutcome::Rejected { refusal }) => {
                assert!(!refusal.is_stored(), "{name}");
                assert_ne!(
                    refusal,
                    &BootstrapRefusal::Tls(TlsRefusal::Unknown),
                    "{name}"
                );
                text(&row["wire"]["refusal"]["code"]).to_owned()
            }
            Err(_) => "refused".to_owned(),
        };
        assert_eq!(got, text(&row["expect"]), "{name}");
        if let Ok(result) = decoded {
            assert_eq!(
                serde_json::to_value(&result).unwrap(),
                row["wire"],
                "{name}"
            );
        }
        seen.push(got);
    }
    // A vector per refusal (X-02 §6.3), bootstrap's own four and the TlsRefusals alike.
    for code in [
        "completed",
        "tls_required",
        "tls_mismatch",
        "already_bootstrapped",
        "bootstrap_finished",
        "tls_name_mismatch",
        "tls_key_mismatch",
        "tls_not_valid",
        "origin_invalid",
        "hsts_invalid",
    ] {
        assert!(seen.iter().any(|got| got == code), "no vector for {code}");
    }
    // `accepted` is refused at decode, though a HostOpResult would take it.
    let accepted = json!({"status": "accepted"});
    assert!(rows(&doc, "bootstrap")
        .iter()
        .any(|row| row["wire"] == accepted && row["expect"] == "refused"));
    assert!(serde_json::from_value::<BootstrapOutcome>(accepted.clone()).is_err());
    assert_eq!(
        serde_json::from_value::<HostOpResult<Empty>>(accepted).unwrap(),
        HostOpResult::Accepted
    );
    for (refusal, code) in [
        (BootstrapRefusal::TlsRequired, "tls_required"),
        (BootstrapRefusal::TlsMismatch, "tls_mismatch"),
        (
            BootstrapRefusal::AlreadyBootstrapped,
            "already_bootstrapped",
        ),
        (BootstrapRefusal::BootstrapFinished, "bootstrap_finished"),
        (
            BootstrapRefusal::Tls(TlsRefusal::TlsKeyMismatch),
            "tls_key_mismatch",
        ),
    ] {
        assert_eq!(
            serde_json::to_value(&refusal).unwrap(),
            json!({ "code": code })
        );
        assert!(!refusal.is_stored());
    }
    // A code neither type knows is kept apart, and never stored.
    let later: BootstrapRefusal =
        serde_json::from_value(json!({"code": "bootstrap_future"})).unwrap();
    assert_eq!(later, BootstrapRefusal::Tls(TlsRefusal::Unknown));
    assert!(serde_json::to_value(&later).is_err());
}
