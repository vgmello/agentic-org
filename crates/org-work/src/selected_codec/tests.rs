use super::*;

fn canon(text: &str) -> Result<String, Error> {
    String::from_utf8(canonical(&parse(text.as_bytes())?)?).map_err(|_| Error::InvalidUtf8)
}

#[test]
fn retained_numbers_and_unicode_have_pinned_canonical_bytes() {
    let vectors: serde_json::Value =
        serde_json::from_str(include_str!("../../test-vectors/selected-codec-2.json")).unwrap();
    for case in vectors["valid"].as_array().unwrap() {
        let input = case["input"].as_str().unwrap();
        let expected = case["canonical"].as_str().unwrap();
        assert_eq!(canon(input).unwrap(), expected);
        assert_eq!(canon(expected).unwrap(), expected);
        assert_eq!(
            sha256(expected.as_bytes()),
            case["sha256"].as_str().unwrap()
        );
    }
    for case in vectors["invalid"].as_array().unwrap() {
        assert!(canon(case.as_str().unwrap()).is_err());
    }
}

#[test]
fn numeric_identity_is_lexical_but_header_integers_are_strict() {
    let tokens = ["1", "1.0", "1e0", "1E+000", "-0", "-0.00"];
    let hashes: std::collections::BTreeSet<_> = tokens
        .iter()
        .map(|n| payload_sha256(&parse(format!("{{\"n\":{n}}}").as_bytes()).unwrap()).unwrap())
        .collect();
    assert_eq!(hashes.len(), tokens.len());
    for token in ["0", "-0", "-1", "1.0", "1e0", "9223372036854775808"] {
        assert_eq!(NumberToken::new(token).unwrap().positive_i64(), None);
    }
    for token in ["1", "9223372036854775807"] {
        assert_eq!(
            NumberToken::new(token).unwrap().positive_i64(),
            token.parse().ok()
        );
    }
    for token in ["01", "+1", "1.", "1e", "--1", "", " 1", "1 "] {
        assert!(NumberToken::new(token).is_err());
    }
}

#[test]
fn checksum_omission_is_top_level_only() {
    let mut object =
        parse(br#"{"payload_sha256":"old","nested":{"payload_sha256":"included"}}"#).unwrap();
    let expected = sha256(br#"{"nested":{"payload_sha256":"included"}}"#);
    assert_eq!(payload_sha256(&object).unwrap(), expected);
    if let Json::Object(map) = &mut object {
        map.insert("payload_sha256".into(), Json::String("new".into()));
    }
    assert_eq!(payload_sha256(&object).unwrap(), expected);
    assert_eq!(payload_sha256(&Json::Null), Err(Error::NotAnObject));
}

#[test]
fn budgets_apply_to_parse_and_emission_without_magnitude_bans() {
    let number = format!("1{}", "0".repeat(MAX_TOKEN_BYTES - 1));
    assert_eq!(canon(&number).unwrap(), number);
    assert_eq!(canon(&(number + "0")), Err(Error::TokenLimit));

    let value = Json::String("🦀".repeat(MAX_TOKEN_BYTES / 4));
    assert!(parse(&canonical(&value).unwrap()).is_ok());
    assert_eq!(
        canonical(&Json::String("x".repeat(MAX_TOKEN_BYTES + 1))),
        Err(Error::TokenLimit)
    );
    assert_eq!(
        parse(&vec![b' '; MAX_JSON_BYTES + 1]),
        Err(Error::InputLimit)
    );
    assert_eq!(
        canonical(&Json::String("\0".repeat(MAX_TOKEN_BYTES))),
        Err(Error::OutputLimit)
    );

    let accepted = format!("{}null{}", "[".repeat(MAX_DEPTH), "]".repeat(MAX_DEPTH));
    assert!(canon(&accepted).is_ok());
    assert_eq!(canon(&format!("[{accepted}]")), Err(Error::DepthLimit));
    let too_deep = (0..MAX_DEPTH + 1).fold(Json::Null, |v, _| Json::Array(vec![v]));
    assert_eq!(canonical(&too_deep), Err(Error::DepthLimit));

    assert!(canonical(&Json::Array(vec![Json::Null; MAX_NODES - 1])).is_ok());
    assert_eq!(
        canonical(&Json::Array(vec![Json::Null; MAX_NODES])),
        Err(Error::NodeLimit)
    );
    let many = format!("[{}null]", "null,".repeat(MAX_NODES - 1));
    assert_eq!(parse(many.as_bytes()), Err(Error::NodeLimit));
}

#[test]
fn invalid_utf8_and_decoded_duplicates_never_reach_a_map() {
    assert_eq!(parse(&[b'"', 0xff, b'"']), Err(Error::InvalidUtf8));
    for text in [r#"{"a":1,"\u0061":2}"#, r#"[{}, {"x":{"y":0,"\u0079":1}}]"#] {
        assert_eq!(canon(text), Err(Error::DuplicateKey));
    }
    // No Unicode normalization: these are distinct keys.
    assert!(canon("{\"é\":1,\"e\\u0301\":2}").is_ok());
}

#[test]
fn aggregate_blob_limits_count_references_including_repetitions() {
    assert_eq!(check_blob_lengths([MAX_BLOB_BYTES]), Ok(()));
    assert_eq!(check_blob_lengths([MAX_BLOB_BYTES / 2; 2]), Ok(()));
    assert_eq!(
        check_blob_lengths([MAX_BLOB_BYTES / 2; 3]),
        Err(Error::BlobLimit)
    );
    assert_eq!(
        check_blob_lengths([MAX_BLOB_BYTES + 1]),
        Err(Error::BlobLimit)
    );
    assert_eq!(check_blob_lengths(vec![0; MAX_BLOB_REFS]), Ok(()));
    assert_eq!(
        check_blob_lengths(vec![0; MAX_BLOB_REFS + 1]),
        Err(Error::BlobLimit)
    );
}

#[test]
fn bounded_malformed_byte_corpus_never_panics_or_accepts_non_json() {
    // Deterministic malformed and valid fragments exercise every ASCII byte and
    // invalid UTF-8 under several structural prefixes, without a new fuzz crate.
    for byte in 0..=255u8 {
        for prefix in [b"".as_slice(), b"[", b"{\"key\":", b"\"", b"\"\\u"] {
            let mut input = prefix.to_vec();
            input.push(byte);
            if let Ok(value) = parse(&input) {
                let encoded = canonical(&value).unwrap();
                assert_eq!(parse(&encoded).unwrap(), value);
            }
        }
    }
}
