use super::*;
fn request() -> Request {
    Request {
        version: 1,
        instance_id: "0190a5d2-7c3e-7a01-8b2c-3d4e5f607182".into(),
        office_id: "019a9999-0000-7000-8000-000000000001".into(),
        organization_id: "019a9999-0000-7000-8000-000000000002".into(),
        operation_id: "019a9999-0000-7000-8000-000000000006".into(),
    }
}
fn record(org: String, length: usize) -> PublicRecord {
    let mut binding = request();
    binding.organization_id = org;
    let mut bytes = b"AGORGCA1".to_vec();
    bytes.extend((binding.organization_id.len() as u16).to_be_bytes());
    bytes.extend(binding.organization_id.as_bytes());
    bytes.push(1);
    bytes.extend([0; 28]);
    let mut key = [7; 65];
    key[0] = 4;
    bytes.extend(key);
    bytes.extend((length as u32).to_be_bytes());
    bytes.extend(vec![0; length]);
    PublicRecord {
        version: 1,
        instance_id: binding.instance_id,
        office_id: binding.office_id,
        organization_id: binding.organization_id,
        operation_id: binding.operation_id,
        escrow_version: 1,
        public_key: hex(&key),
        package_sha256: "a".repeat(64),
        package_hex: hex(&bytes),
    }
}
#[test]
fn exact_roundtrip_and_strict_whole_frames() {
    let req = request();
    let mut bytes = Vec::new();
    write_request(&mut bytes, &req).unwrap();
    assert_eq!(read_request(&mut bytes.as_slice()).unwrap(), req);
    let public = record(req.organization_id.clone(), 17);
    bytes.clear();
    write_record(&mut bytes, &public).unwrap();
    assert_eq!(read_record(&mut bytes.as_slice()).unwrap(), public);
    for bad in [
        bytes[..bytes.len() - 1].to_vec(),
        [bytes.as_slice(), b"\n"].concat(),
        [bytes.as_slice(), bytes.as_slice()].concat(),
        vec![b' '; MAX_FRAME_BYTES + 1],
        b"[]\n".to_vec(),
        b"null\n".to_vec(),
    ] {
        assert!(read_record(&mut bad.as_slice()).is_err());
    }
    let raw = serde_json::to_string(&req).unwrap();
    for injection in [
        r#""version":1,"#,
        r#""\u0076ersion":1,"#,
        r#""unknown":true,"#,
    ] {
        let bad = format!("{{{}{}\n", injection, &raw[1..]);
        assert!(read_request(&mut bad.as_bytes()).is_err(), "{bad}");
    }
}
#[test]
fn envelope_bounds_identity_lengths_and_unknown_versions() {
    let p = record(request().organization_id, 4112);
    assert!(p.package().is_ok());
    for n in [0, 16, 4113] {
        assert!(record(request().organization_id, n).package().is_err());
    }
    let mut p = record(request().organization_id, 17);
    for field in [
        "version",
        "escrow_version",
        "organization_id",
        "public_key",
        "package_sha256",
        "package_hex",
    ] {
        let mut value = serde_json::to_value(&p).unwrap();
        value[field] = match field {
            "version" | "escrow_version" => serde_json::json!(2),
            "public_key" => serde_json::json!("04".repeat(65)),
            "package_hex" => serde_json::json!(format!("{}00", p.package_hex)),
            "package_sha256" => serde_json::json!("A".repeat(64)),
            _ => serde_json::json!("other"),
        };
        assert!(
            serde_json::from_value::<PublicRecord>(value)
                .unwrap()
                .package()
                .is_err(),
            "{field}"
        );
    }
    p.package_hex = "00".repeat(MAX_PACKAGE_BYTES + 1);
    assert!(p.package().is_err());
    let mut r = request();
    r.instance_id = "a".repeat(256);
    assert!(r.check().is_err());
}

#[test]
fn one_shot_unix_roundtrip_requires_write_half_close_and_rejects_an_unfinished_sender() {
    use std::{
        io::{BufReader, Write},
        net::Shutdown,
        os::unix::net::UnixStream,
        time::Duration,
    };
    let (mut caller, worker) = UnixStream::pair().unwrap();
    for stream in [&caller, &worker] {
        stream
            .set_read_timeout(Some(Duration::from_secs(1)))
            .unwrap();
        stream
            .set_write_timeout(Some(Duration::from_secs(1)))
            .unwrap();
    }
    let req = request();
    let expected = record(req.organization_id.clone(), 17);
    let response = expected.clone();
    let binding = req.clone();
    let server = std::thread::spawn(move || {
        let mut worker = BufReader::new(worker);
        assert_eq!(read_request(&mut worker).unwrap(), binding);
        write_record(worker.get_mut(), &response).unwrap();
        worker.get_mut().shutdown(Shutdown::Write).unwrap();
    });
    write_request(&mut caller, &req).unwrap();
    caller.shutdown(Shutdown::Write).unwrap();
    assert_eq!(read_record(&mut BufReader::new(caller)).unwrap(), expected);
    server.join().unwrap();
    let (mut caller, worker) = UnixStream::pair().unwrap();
    worker
        .set_read_timeout(Some(Duration::from_millis(20)))
        .unwrap();
    write_request(&mut caller, &req).unwrap(); // No half-close: never accept a prefix.
    assert!(read_request(&mut BufReader::new(worker)).is_err());
    let raw = serde_json::to_string(&expected).unwrap();
    for injection in [
        r#""version":1,"#,
        r#""\u0076ersion":1,"#,
        r#""unknown":true,"#,
    ] {
        let bytes = format!("{{{}{}\n", injection, &raw[1..]);
        assert!(read_record(&mut bytes.as_bytes()).is_err());
    }
    let mut bytes = serde_json::to_vec(&expected).unwrap();
    bytes.push(b'\n');
    bytes.extend(b"{}");
    assert!(read_record(&mut bytes.as_slice()).is_err());
    caller.write_all(b" ").ok();
}
#[test]
fn transfer_recovery_version_is_checked_in_both_directions() {
    use crate::{decode_transfer_record, encode_transfer_record, TransferRecord};
    let valid = TransferRecord::RecoveryPublicRead {
        version: 1,
        office_id: request().office_id,
        channel_id: request().operation_id,
    };
    assert!(decode_transfer_record(&encode_transfer_record(&valid).unwrap()).is_ok());
    let invalid = TransferRecord::RecoveryPublicRead {
        version: 2,
        office_id: request().office_id,
        channel_id: request().operation_id,
    };
    assert!(encode_transfer_record(&invalid).is_err());
    assert!(decode_transfer_record(&serde_json::to_vec(&invalid).unwrap()).is_err());
}

/// The instance id is canonical lowercase UUIDv7 text: a ULID and uppercase UUIDv7 are refused.
#[test]
fn request_refuses_a_ulid_or_uppercase_instance() {
    assert!(request().check().is_ok());
    for instance in [
        "01ARZ3NDEKTSV4RRFFQ69G5FAV",
        "0190A5D2-7C3E-7A01-8B2C-3D4E5F607182",
    ] {
        let mut r = request();
        r.instance_id = instance.into();
        assert!(r.check().is_err(), "{instance}");
    }
}
