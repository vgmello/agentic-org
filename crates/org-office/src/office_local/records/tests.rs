use org_store::{testkit::id, DurablePath, Sha256};

use super::*;

fn record() -> CaKey {
    let mut public_key = [0xab; 65];
    public_key[0] = 4;
    CaKey {
        organization_id: id(0xA2),
        office_id: id(0xA3),
        operation_id: id(0x01),
        public_key,
        package_sha256: Sha256::of(b"package"),
        wrapped_ca_key: vec![0x0f; 31],
    }
}

/// The record's exact bytes, written out by hand from document 01's form.
fn golden() -> String {
    let public = format!("04{}", "ab".repeat(64));
    let digest = Sha256::of(b"package").to_hex();
    format!(
        "{{\"schema_version\":1,\
\"organization_id\":\"01900000-0000-7000-8000-0000000000a2\",\
\"office_id\":\"01900000-0000-7000-8000-0000000000a3\",\
\"operation_id\":\"01900000-0000-7000-8000-000000000001\",\
\"escrow_version\":1,\
\"public_key\":\"{public}\",\
\"package_sha256\":\"{digest}\",\
\"wrapped_ca_key\":\"{}\"}}\n",
        "0f".repeat(31)
    )
}

#[test]
fn the_ca_key_record_has_one_exact_form() {
    let bytes = encode_ca_key(&record());
    assert_eq!(String::from_utf8(bytes.clone()).unwrap(), golden());
    assert_eq!(decode_ca_key(&bytes), Ok(record()));
    assert_eq!(golden().matches("\"public_key\":\"04").count(), 1);
    assert_eq!(public_hex_len(&golden()), 130);
}

fn public_hex_len(text: &str) -> usize {
    let start = text.find("\"public_key\":\"").unwrap() + "\"public_key\":\"".len();
    text[start..].find('"').unwrap()
}

#[test]
fn a_ca_key_record_in_any_other_form_is_refused() {
    let golden = golden();
    let upper_hex = golden.replace(&"ab".repeat(64), &"AB".repeat(64));
    let moved = golden.replacen("\"schema_version\":1,", "", 1).replacen(
        "\"escrow_version\":1,",
        "\"escrow_version\":1,\"schema_version\":1,",
        1,
    );
    let forms = [
        // No final newline; a space; pretty-printed; another key order.
        golden.trim_end().to_owned(),
        golden.replacen(":1,", ": 1,", 1),
        golden.replace(',', ",\n"),
        moved,
        upper_hex,
        // Another version of either kind; a field more; a field less.
        golden.replacen("\"schema_version\":1", "\"schema_version\":2", 1),
        golden.replacen("\"escrow_version\":1", "\"escrow_version\":2", 1),
        golden.replacen("{", "{\"extra\":1,", 1),
        golden.replacen("\"escrow_version\":1,", "", 1),
        // The operation's wire ULID, not its store UUIDv7.
        golden.replacen(
            "01900000-0000-7000-8000-000000000001",
            "01ARZ3NDEKTSV4RRFFQ69G5FAV",
            1,
        ),
        // A compressed key; a key of 64 bytes; a hash of 31 bytes.
        golden.replacen("\"public_key\":\"04", "\"public_key\":\"02", 1),
        golden.replacen(
            &format!("04{}", "ab".repeat(64)),
            &format!("04{}", "ab".repeat(63)),
            1,
        ),
        golden.replacen(&Sha256::of(b"package").to_hex(), &"00".repeat(31), 1),
        // A wrapped key too short to be one; odd hex.
        golden.replacen(&"0f".repeat(31), &"0f".repeat(29), 1),
        golden.replacen(&"0f".repeat(31), &format!("{}0", "0f".repeat(31)), 1),
        String::new(),
    ];
    for form in forms {
        assert!(decode_ca_key(form.as_bytes()).is_err(), "{form}");
    }
}

#[test]
fn a_record_that_breaks_a_rule_is_caught_before_it_is_written() {
    let mut compressed = record();
    compressed.public_key[0] = 2;
    assert!(compressed.check().is_err());
    let mut ulid = record();
    ulid.operation_id = org_store::Id::parse("01arz3ndektsv4rrffq69g5fav").unwrap();
    assert!(ulid.check().is_err());
    for length in [MIN_WRAPPED_CA_KEY - 1, MAX_WRAPPED_CA_KEY + 1] {
        let mut wrapped = record();
        wrapped.wrapped_ca_key = vec![1; length];
        assert!(wrapped.check().is_err(), "{length}");
    }
    let mut longest = record();
    longest.wrapped_ca_key = vec![1; MAX_WRAPPED_CA_KEY];
    assert_eq!(decode_ca_key(&encode_ca_key(&longest)), Ok(longest));
}

#[test]
fn the_debug_form_never_shows_key_bytes() {
    let shown = format!("{:?}", record());
    assert!(
        !shown.contains("15, 15") && !shown.contains("0f0f"),
        "{shown}"
    );
    assert!(!shown.contains("171"), "{shown}");
}

#[test]
fn the_clean_stop_marker_has_one_exact_form() {
    let marker = CleanStop {
        journal_incarnation: id(0xA4),
        head_ordinal: 7,
    };
    let text =
        "{\"journal_incarnation\":\"01900000-0000-7000-8000-0000000000a4\",\"head_ordinal\":7}\n";
    assert_eq!(encode_clean_stop(&marker), text.as_bytes());
    assert_eq!(decode_clean_stop(text.as_bytes()), Ok(marker));
    for other in [
        text.trim_end(),
        "{\"head_ordinal\":7,\"journal_incarnation\":\"01900000-0000-7000-8000-0000000000a4\"}\n",
        "{\"journal_incarnation\":\"01900000-0000-7000-8000-0000000000a4\",\"head_ordinal\":-1}\n",
        "{\"journal_incarnation\":\"../x\",\"head_ordinal\":7}\n",
        "{\"journal_incarnation\":\"01900000-0000-7000-8000-0000000000a4\",\"head_ordinal\":7,\"x\":1}\n",
        "{\"journal_incarnation\":\"01900000-0000-7000-8000-0000000000a4\",\"head_ordinal\":7}\n\n",
        "",
    ] {
        assert!(decode_clean_stop(other.as_bytes()).is_err(), "{other:?}");
    }
}

#[test]
fn the_family_owns_its_two_files_of_its_own_office_and_nothing_else() {
    let office = id(0xA3);
    let at = |text: &str| DurablePath::parse(text).unwrap();
    let local = format!("local/offices/{office}");
    assert_eq!(
        kind_of(&at(&format!("{local}/security/ca-key.v1.json")), &office),
        Some(Kind::CaKey)
    );
    assert_eq!(
        kind_of(&at(&format!("{local}/clean-stop")), &office),
        Some(Kind::CleanStop)
    );
    assert_eq!(
        ca_key_path(&office),
        at(&format!("{local}/security/ca-key.v1.json"))
    );
    assert_eq!(clean_stop_path(&office), at(&format!("{local}/clean-stop")));
    let other = id(0xB3);
    for path in [
        format!("local/offices/{other}/security/ca-key.v1.json"),
        format!("local/offices/{other}/clean-stop"),
        format!("{local}/security/ca-key.v2.json"),
        format!("{local}/security"),
        format!("{local}/probe/x"),
        format!("{local}/clean-stop/x"),
        "state/security/ca-escrow.v1.bin".to_owned(),
    ] {
        assert_eq!(kind_of(&at(&path), &office), None, "{path}");
    }
    assert_eq!(folder(&office), at(&local));
}
