//! X-02 rev 14 vectors: `ListenOrigin`'s form and fit (`test-vectors/listen-origin.json`), and
//! `tls.fetch`, `ListenFailure`, the `tls.set` refusals and the invalid-configuration
//! rule (`test-vectors/tls.json`).

use std::net::{IpAddr, Ipv6Addr};

use org_proto::Call;
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
            // On the wire (serde) only the serialization itself is accepted; `parse` also takes
            // operator input with one trailing `/`.
            if input == want {
                assert_eq!(from_serde.unwrap(), origin, "{input:?}");
            } else {
                assert!(from_serde.is_err(), "{input:?}");
            }
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
        (ListenMode::HttpPublished, "http_published", "http"),
    ] {
        assert_eq!(serde_json::to_value(mode).unwrap(), wire);
        assert_eq!(
            serde_json::from_value::<ListenMode>(json!(wire)).unwrap(),
            mode
        );
        assert_eq!(mode.as_str(), wire);
        assert_eq!(mode.scheme().as_str(), scheme);
    }
    // No spelling near a mode's is one: a mode is chosen by its exact name.
    for bad in [
        "http",
        "http-published",
        "HTTP_PUBLISHED",
        "http_public",
        "published",
        "",
    ] {
        assert!(
            serde_json::from_value::<ListenMode>(json!(bad)).is_err(),
            "{bad}"
        );
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
            Ok(TlsFetchResult::HttpPublished { .. }) => "http_published",
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
        assert_eq!(
            refusal.is_stored(),
            row["stored"][TlsSet::METHOD].as_bool().unwrap(),
            "{name}"
        );
        // tls.set answers it as a rejected HostOpResult, and operation.get reads it back.
        let wire = json!({"status": "rejected", "refusal": row["wire"]});
        let result: HostOpResult<Empty, TlsRefusal> = serde_json::from_value(wire.clone()).unwrap();
        assert_eq!(result, HostOpResult::Rejected { refusal });
        assert_eq!(serde_json::to_value(&result).unwrap(), wire);
        let looked_up: <OperationGet as org_proto::Call>::Result =
            serde_json::from_value(wire).unwrap();
        assert!(matches!(looked_up, HostOpResult::Rejected { .. }), "{name}");
    }
    // A row for every refusal, and only tls.set's tls_key_mismatch is stored.
    for code in [
        "tls_name_mismatch",
        "tls_invalid",
        "tls_key_mismatch",
        "tls_not_valid",
        "origin_invalid",
        "hsts_invalid",
    ] {
        assert!(
            rows(&doc, "refusals")
                .iter()
                .any(|row| row["wire"]["code"] == code),
            "no vector for {code}"
        );
    }
    assert!(!TlsRefusal::TlsInvalid.is_stored());
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
    assert!(!later.is_stored());
    // It is never sent: writing it fails, alone or as a refusal.
    assert!(serde_json::to_value(&later).is_err());
    let rejected: HostOpResult<Empty, TlsRefusal> = HostOpResult::Rejected { refusal: later };
    assert!(serde_json::to_value(&rejected).is_err());
    assert_eq!(TlsSet::METHOD, "tls.set");
}

#[test]
fn timestamp_vectors() {
    let doc = doc(include_str!("../../test-vectors/tls.json"));
    let refusal = |text: &str| json!({"code": "tls_not_valid", "not_before": text, "not_after": "2027-12-01T00:00:00Z"});
    for row in rows(&doc, "timestamps") {
        let name = text(&row["name"]);
        let input = text(&row["text"]);
        let valid = row["valid"].as_bool().unwrap();
        let parsed = UtcTimestamp::parse(input);
        assert_eq!(parsed.is_ok(), valid, "{name}");
        let decoded = serde_json::from_value::<TlsRefusal>(refusal(input));
        assert_eq!(decoded.is_ok(), valid, "{name}");
        if let Ok(stamp) = parsed {
            // Never rewritten: it serializes back as given.
            assert_eq!(stamp.as_str(), input);
            assert_eq!(serde_json::to_value(&stamp).unwrap(), json!(input));
            assert_eq!(
                serde_json::to_value(decoded.unwrap()).unwrap(),
                refusal(input)
            );
        }
    }
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
                // The one rule: the host reports it as that ListenFailure, and tls.set
                // refuses with that code, which it does not store.
                assert_eq!(
                    serde_json::to_value(invalid.listen_failure()).unwrap(),
                    json!({ "reason": code })
                );
                let refusal = TlsRefusal::from(invalid);
                assert_eq!(
                    serde_json::to_value(&refusal).unwrap(),
                    json!({ "code": code })
                );
                assert!(!refusal.is_stored());
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

/// The whole rule of plain HTTP off loopback, every combination: the mode, the listen
/// address, the origin's host and the instance's service mode. `http_published` is accepted
/// only on an address that is not a loopback address, only with a loopback name as the
/// origin's host, and only for a foreground instance. Nothing else gains anything from it:
/// `http_loopback` stays on exactly `127.0.0.1` and `::1` in either service mode, and
/// `https` is the same in both.
#[test]
fn plain_http_off_loopback_is_one_mode_for_a_foreground_instance_only() {
    use crate::listen::ForegroundOnly;
    let addresses = [
        "127.0.0.1",
        "::1",
        "127.0.0.2",
        "0.0.0.0",
        "::",
        "172.17.0.2",
        "192.168.1.20",
        "fd00::2",
    ];
    let hosts = [
        "localhost",
        "127.0.0.1",
        "[::1]",
        "127.0.0.2",
        "0.0.0.0",
        "192.168.1.20",
        "office.example",
        "sub.localhost",
        "[fd00::2]",
    ];
    let mut accepted = Vec::new();
    for mode in [
        ListenMode::Https,
        ListenMode::HttpLoopback,
        ListenMode::HttpPublished,
    ] {
        for address in addresses {
            let address: IpAddr = address.parse().unwrap();
            for host in hosts {
                let origin =
                    ListenOrigin::parse(&format!("{}://{host}:8443", mode.scheme().as_str()))
                        .unwrap();
                let fit = origin.check(mode, address, 8443);
                for foreground in [false, true] {
                    let allowed = mode.fits_service(foreground);
                    let expected_fit = match mode {
                        ListenMode::Https => true,
                        // Exactly the bound literal, on exactly the two loopback addresses.
                        ListenMode::HttpLoopback => {
                            (address.to_string() == "127.0.0.1" && host == "127.0.0.1")
                                || (address.to_string() == "::1" && host == "[::1]")
                        }
                        ListenMode::HttpPublished => {
                            !address.is_loopback()
                                && matches!(host, "localhost" | "127.0.0.1" | "[::1]")
                        }
                    };
                    let expected_allowed = mode != ListenMode::HttpPublished || foreground;
                    assert_eq!(
                        fit.is_ok(),
                        expected_fit,
                        "{mode:?} {address} {host}: {fit:?}"
                    );
                    assert_eq!(
                        allowed.is_ok(),
                        expected_allowed,
                        "{mode:?} foreground={foreground}"
                    );
                    if mode == ListenMode::HttpPublished && fit.is_ok() && allowed.is_ok() {
                        accepted.push((address, host));
                    }
                }
            }
        }
    }
    // What is accepted: five addresses that are not loopback, three loopback names each.
    assert_eq!(accepted.len(), 5 * 3, "{accepted:?}");
    // The two refusals of the mode itself have their own reasons.
    let origin = ListenOrigin::parse("http://localhost:8443").unwrap();
    assert_eq!(
        origin.check(
            ListenMode::HttpPublished,
            "127.0.0.1".parse().unwrap(),
            8443
        ),
        Err(OriginError::PublishedAddress)
    );
    let named = ListenOrigin::parse("http://office.example:8443").unwrap();
    assert_eq!(
        named.check(ListenMode::HttpPublished, "0.0.0.0".parse().unwrap(), 8443),
        Err(OriginError::PublishedHost)
    );
    // Plain HTTP with a non-loopback address is never `http_loopback`, whoever asks.
    assert_eq!(
        origin.check(ListenMode::HttpLoopback, "0.0.0.0".parse().unwrap(), 8443),
        Err(OriginError::LoopbackAddress)
    );
    assert_eq!(
        ListenMode::HttpPublished.fits_service(false),
        Err(ForegroundOnly)
    );
    let why = ForegroundOnly.to_string();
    assert!(
        why.contains("http_published") && why.contains("foreground"),
        "{why}"
    );
    // A stored configuration is judged by the same rule, and decodes only when it holds.
    let stored = |address: &str, origin: &str| {
        serde_json::from_value::<ListenConfig>(json!({
            "address": address, "port": 8443, "scheme": "http_published",
            "origin": origin, "hsts": "off"
        }))
    };
    let config = stored("0.0.0.0", "http://localhost:8443").unwrap();
    assert_eq!(config.scheme(), ListenMode::HttpPublished);
    assert_eq!(
        serde_json::to_value(&config).unwrap()["scheme"],
        "http_published"
    );
    assert!(stored("127.0.0.1", "http://localhost:8443").is_err());
    assert!(stored("0.0.0.0", "http://office.example:8443").is_err());
    assert!(stored("0.0.0.0", "https://localhost:8443").is_err());
}
