use super::*;

fn fields() -> EventFields {
    EventFields {
        organization_id: "org 🦀".into(), task_id: "opaque-task".into(), revision: 2,
        occurred_at: Some("2026-11-15T12:00:00.000Z".into()), actor_kind: "owner".into(),
        actor_id: None, operation_id: None,
        change: codec::parse(br#"{"type":"assignment_changed","from":{"assigned_team_id":null,"assignee_id":null},"to":{"assigned_team_id":"team","assignee_id":null}}"#).unwrap(),
        source_metadata: BTreeMap::new(), extensions: BTreeMap::new(),
    }
}

fn expected() -> ExpectedIdentity<'static> {
    ExpectedIdentity {
        organization_id: "org 🦀",
        task_id: "opaque-task",
        filename: "2.json",
    }
}

fn replace_field(bytes: &[u8], key: &str, replacement: &str) -> Vec<u8> {
    let mut value = codec::parse(bytes).unwrap();
    if let Json::Object(map) = &mut value {
        map.insert(key.into(), codec::parse(replacement.as_bytes()).unwrap());
    }
    let digest = codec::payload_sha256(&value).unwrap();
    if let Json::Object(map) = &mut value {
        map.insert("payload_sha256".into(), Json::String(digest));
    }
    codec::canonical(&value).unwrap()
}

#[test]
fn actual_hash_header_and_filename_are_verified_before_coverage() {
    let bytes = encode(fields(), Admission::New).unwrap();
    let verified = verify(&bytes, expected(), Admission::New).unwrap();
    let header = verified.coverage_header();
    assert_eq!(header.revision, 2);
    assert_eq!(header.filename_revision, 2);
    assert_eq!(header.task_id, "opaque-task");
    let mut mismatch = expected();
    mismatch.filename = "02.json";
    assert!(matches!(
        verify(&bytes, mismatch, Admission::New),
        Err(Error::IdentityMismatch)
    ));
    let mut mismatch = expected();
    mismatch.organization_id = "other";
    assert!(matches!(
        verify(&bytes, mismatch, Admission::New),
        Err(Error::IdentityMismatch)
    ));
    let changed = String::from_utf8(bytes.clone())
        .unwrap()
        .replace("\"team\"", "\"another-team\"");
    assert!(matches!(
        verify(changed.as_bytes(), expected(), Admission::New),
        Err(Error::ChecksumMismatch)
    ));
}

#[test]
fn schema_one_future_codecs_and_profiles_are_never_reinterpreted() {
    let bytes = encode(fields(), Admission::New).unwrap();
    for schema in ["1", "3", "2.0", "2e0", "\"2\""] {
        let old = replace_field(&bytes, "schema_version", schema);
        let preserved = old.clone();
        assert!(matches!(
            verify(&old, expected(), Admission::Import),
            Err(Error::UnsupportedCodec)
        ));
        assert_eq!(old, preserved);
    }
    let future = replace_field(&bytes, "resource_profile", "\"selected-task-resources/2\"");
    assert!(matches!(
        verify(&future, expected(), Admission::Import),
        Err(Error::UnsupportedProfile)
    ));
}

#[test]
fn typed_headers_and_required_defaults_are_not_silently_repaired() {
    let bytes = encode(fields(), Admission::New).unwrap();
    for (key, value) in [
        ("revision", "0"),
        ("revision", "-1"),
        ("revision", "2.0"),
        ("revision", "2e0"),
        ("revision", "9223372036854775808"),
        ("task_kind", "\"bug\""),
        ("actor_kind", "\"forged\""),
        ("actor_id", "\"owner-is-not-employee\""),
        ("source_metadata", "[]"),
        ("operation_id", "1"),
    ] {
        assert!(verify(
            &replace_field(&bytes, key, value),
            expected(),
            Admission::Import
        )
        .is_err());
    }
    for key in [
        "source_metadata",
        "operation_id",
        "actor_id",
        "occurred_at",
        "resource_profile",
    ] {
        let mut value = codec::parse(&bytes).unwrap();
        if let Json::Object(map) = &mut value {
            map.remove(key);
        }
        let malformed = codec::canonical(&value).unwrap();
        assert!(verify(&malformed, expected(), Admission::Import).is_err());
    }
}

#[test]
fn unknown_changes_extensions_and_original_legacy_numbers_survive() {
    let mut f = fields();
    f.change = codec::parse(br#"{"type":"future_v17","new":{"n":1844674407370955161600001,"f":1.234567890123456789e+0009}}"#).unwrap();
    f.extensions.insert(
        "future_extension".into(),
        codec::parse(b"[-0,1E+000]").unwrap(),
    );
    let legacy = br#"{"auto":false,"cause":null,"org_position":{"epoch":1,"seq":4},"change":{"n":1.234567890123456789},"version":2}"#;
    f.source_metadata
        .insert("legacy_event".into(), codec::parse(legacy).unwrap());
    let encoded = encode(f, Admission::Import).unwrap();
    let verified = verify(&encoded, expected(), Admission::Import).unwrap();
    assert_eq!(codec::canonical(verified.value().get("source_metadata").unwrap().get("legacy_event").unwrap()).unwrap(),
        br#"{"auto":false,"cause":null,"change":{"n":1.234567890123456789},"org_position":{"epoch":1,"seq":4},"version":2}"#);
    let text = std::str::from_utf8(&encoded).unwrap();
    assert!(text.contains("1844674407370955161600001"));
    assert!(text.contains("1.234567890123456789e+0009"));
    assert!(text.contains("[-0,1E+000]"));
    assert_eq!(codec::canonical(verified.value()).unwrap(), encoded);
}

#[test]
fn unknown_import_provenance_is_retained_but_not_used_for_new_writes() {
    let mut f = fields();
    f.actor_kind = "unknown".into();
    f.occurred_at = None;
    let encoded = encode(f, Admission::Import).unwrap();
    assert!(verify(&encoded, expected(), Admission::Import).is_ok());
    assert!(matches!(
        verify(&encoded, expected(), Admission::New),
        Err(Error::InvalidHeader)
    ));
    for timestamp in ["2024-02-29T23:59:59Z", "1960-01-01T00:00:00.00000000001Z"] {
        assert!(utc_timestamp(timestamp));
    }
    for timestamp in [
        "2023-02-29T00:00:00Z",
        "2026-13-01T00:00:00Z",
        "2026-01-01T24:00:00Z",
        "2026-01-01T00:00:00+00:00",
        "2026-01-01T00:00:00.Z",
    ] {
        assert!(!utc_timestamp(timestamp));
    }
}

#[test]
fn complete_pairs_and_exact_public_snapshot_forms_are_validated() {
    let mut f = fields();
    f.change = codec::parse(br#"{"type":"assignment_changed","from":{"assigned_team_id":null},"to":{"assigned_team_id":null,"assignee_id":null}}"#).unwrap();
    assert_eq!(
        encode(f, Admission::New).unwrap_err(),
        Error::InvalidAssignment
    );
    let mut f = fields();
    f.revision = 1;
    f.change = codec::parse(br#"{"type":"created","known_facts":{},"public_snapshot":{"encoding":"utf-8-markdown","text":"---\nexact: 1.234567890123456789\n---\npublic"}}"#).unwrap();
    let bytes = encode(f, Admission::New).unwrap();
    let verified = verify(
        &bytes,
        ExpectedIdentity {
            filename: "1.json",
            ..expected()
        },
        Admission::New,
    )
    .unwrap();
    assert_eq!(
        verified
            .value()
            .get("change")
            .unwrap()
            .get("public_snapshot")
            .unwrap()
            .get("text")
            .unwrap()
            .as_str(),
        Some("---\nexact: 1.234567890123456789\n---\npublic")
    );
    let mut f = fields();
    f.change = codec::parse(format!(r#"{{"type":"history_baseline","known_facts":{{}},"public_snapshot":{{"encoding":"utf-8-markdown","sha256":"{}","byte_length":0,"resource_profile":"{RESOURCE_PROFILE}"}}}}"#, "a".repeat(64)).as_bytes()).unwrap();
    assert!(encode(f, Admission::Import).is_ok());
    let mut f = fields();
    f.change = codec::parse(br#"{"type":"created","known_facts":{},"public_snapshot":{"encoding":"utf-8-markdown","text":"body"}}"#).unwrap();
    assert_eq!(encode(f, Admission::New).unwrap_err(), Error::InvalidHeader);
}
