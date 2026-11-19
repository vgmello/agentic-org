use super::*;
use serde_json::Value;

fn vectors() -> Value {
    serde_json::from_str(include_str!(
        "../../../../crates/org-proto/test-vectors/v2/proto-grant-mac.json"
    ))
    .unwrap()
}
fn key() -> GrantKey {
    GrantKey::from_bytes(std::array::from_fn(|n| n as u8))
}
fn grant(value: &Value) -> Grant {
    serde_json::from_value(value.clone()).unwrap()
}

#[test]
fn known_answers_and_every_claim_key_and_tag_bit_are_authenticated() {
    for case in vectors()["cases"].as_array().unwrap() {
        let original = grant(&case["grant"]);
        assert!(verify(&key(), &original), "{}", case["name"]);
        for field in [
            "account",
            "member",
            "org_id",
            "office_id",
            "channel_id",
            "session_id",
            "exp",
        ] {
            let mut changed = case["grant"].clone();
            if field == "exp" {
                changed[field] = (changed[field].as_u64().unwrap() ^ 1).into();
            } else {
                changed[field] = format!("{}x", changed[field].as_str().unwrap()).into();
            }
            assert!(
                !verify(&key(), &grant(&changed)),
                "unauthenticated claim {field}"
            );
        }
        assert!(!verify(&GrantKey::from_bytes([9; 32]), &original));
        for bit in 0..256 {
            let mut bytes = *original.mac.expose_bytes();
            bytes[bit / 8] ^= 1 << (bit % 8);
            let changed = Grant::new(original.claims.clone(), GrantMac::from_bytes(bytes));
            bytes.fill(0);
            assert!(!verify(&key(), &changed), "unauthenticated tag bit {bit}");
        }
    }
}

#[test]
fn missing_encoding_refuses_and_new_boundary_does_not_format_secret_bytes() {
    let value = &vectors()["cases"][0]["grant"];
    let grant = grant(value);
    assert!(!verify_input(&key(), None, &grant.mac));
    let debug = format!("{:?} {:?} {:?}", key(), grant.mac, grant);
    assert!(!debug.contains(value["mac"].as_str().unwrap()));
    assert!(!debug.contains(vectors()["key_hex"].as_str().unwrap()));
    assert!(debug.contains("redacted"));
    let source = include_str!("../grant.rs");
    assert!(source.contains("mac.verify_slice(tag.expose_bytes())"));
    assert!(
        !source.contains("=="),
        "grant verifier must not compare tag bytes itself"
    );
}

#[test]
fn principal_parser_refuses_every_frozen_malformed_mac_or_shape() {
    let v = vectors();
    for case in v["refused_principals"]["cases"].as_array().unwrap() {
        let request: org_proto::Request = serde_json::from_value(serde_json::json!({
            "id": 7, "method":"org.snapshot", "params":{}, "principal":case["principal"]
        }))
        .unwrap();
        assert!(request.principal().is_err(), "{}", case["name"]);
    }
}
