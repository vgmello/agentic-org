use super::*;
#[test]
fn selected_web_pin_roundtrips_and_rejects_ambiguous_or_invalid_whole_frames() {
    let pin = WebRelease {
        release_id: "release-1".into(),
        manifest_sha256: "ab".repeat(32),
        sequence: u64::MAX,
    };
    let line = GatewayFirstLine {
        rendezvous: "/private/tmp/r".into(),
        instance_id: "INSTANCE".into(),
        release: Some(pin),
    };
    let bytes = serde_json::to_vec(&line).unwrap();
    assert!(bytes.len() < 1024);
    assert_eq!(
        serde_json::from_slice::<GatewayFirstLine>(&bytes).unwrap(),
        line
    );
    let wire = serde_json::to_value(&line).unwrap();
    for (field, value) in [
        ("version", serde_json::json!(2)),
        ("release_id", serde_json::json!("../bad")),
        ("manifest_sha256", serde_json::json!("AB".repeat(32))),
        ("sequence", serde_json::json!(0)),
        ("sequence", serde_json::json!(1.0)),
    ] {
        let mut bad = wire.clone();
        bad["release"][field] = value;
        assert!(
            serde_json::from_value::<GatewayFirstLine>(bad).is_err(),
            "{field}"
        );
    }
    let mut bad = wire.clone();
    bad["release"]["key"] = serde_json::json!("untrusted");
    assert!(serde_json::from_value::<GatewayFirstLine>(bad).is_err());
    let mut missing = wire.clone();
    missing["release"]
        .as_object_mut()
        .unwrap()
        .remove("sequence");
    assert!(serde_json::from_value::<GatewayFirstLine>(missing).is_err());
    for suffix in [r#", "sequence":1"#, r#", "seq\u0075ence":1"#] {
        let raw = format!(
            r#"{{"v":1,"rendezvous":"/r","instance_id":"INSTANCE","release":{{"version":1,"release_id":"a","manifest_sha256":"{}","sequence":1{suffix}}}}}"#,
            "ab".repeat(32)
        );
        assert!(serde_json::from_str::<GatewayFirstLine>(&raw).is_err());
    }
    let encoded = String::from_utf8(bytes).unwrap();
    let raw = format!("{},\"release\":null}}", encoded.strip_suffix('}').unwrap());
    assert!(serde_json::from_str::<GatewayFirstLine>(&raw).is_err());
    // Omission remains decodable for explicit debug fixtures, not production admission.
    assert!(serde_json::from_str::<GatewayFirstLine>(
        r#"{"v":1,"rendezvous":"/r","instance_id":"INSTANCE"}"#
    )
    .unwrap()
    .release
    .is_none());
}
