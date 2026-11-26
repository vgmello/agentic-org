//! Blob frames and chunked transfer (`TASK-98.6`): every frame round-trips, the largest chunk
//! fits one federation frame, and the assembler refuses anything but the declared bytes in
//! order, verified by SHA-256, for an image and a full 25 MiB document alike.

use std::io::Cursor;

use super::*;
use crate::frame::{decode_sync_frame, encode_sync_frame, read_sync_frame, write_sync_frame};
use crate::MAX_ENCODED_FRAME_BYTES;

const ID: &str = "0190a5d2-0000-7000-8000-0000000000aa";

fn pattern(len: usize, seed: u8) -> Vec<u8> {
    (0..len)
        .map(|i| (i as u8).wrapping_mul(31).wrapping_add(seed))
        .collect()
}

fn reassemble(bytes: &[u8], max: u64) -> Result<Vec<u8>, BlobTransferError> {
    let digest = blob_digest(bytes);
    let mut assembler = BlobAssembler::new(&digest, bytes.len() as u64, max)?;
    let mut done = false;
    for frame in blob_chunk_frames("req", &digest, bytes) {
        let encoded = encode_sync_frame(&frame).expect("every chunk fits a frame");
        assert!(encoded.len() <= MAX_ENCODED_FRAME_BYTES);
        let SyncFrame::BlobChunk {
            content_sha256,
            offset,
            total_bytes,
            data_b64,
            ..
        } = decode_sync_frame(&encoded).unwrap()
        else {
            panic!("a blob chunk");
        };
        assert!(!done, "no chunk after the last");
        done = assembler.push(&content_sha256, offset, total_bytes, &data_b64)?;
    }
    assert!(done);
    Ok(assembler.into_verified_bytes().expect("verified"))
}

#[test]
fn every_blob_frame_round_trips_and_optional_fields_stay_off_the_wire() {
    let frames = [
        SyncFrame::BlobHasRequest {
            request_id: "r1".into(),
            org_id: "org".into(),
            attachment_id: ID.into(),
            content_sha256: "a".repeat(64),
            byte_count: 7,
            record_b64: "eWFtbA==".into(),
        },
        SyncFrame::BlobHasResponse {
            request_id: "r1".into(),
            content_sha256: "a".repeat(64),
            present: false,
        },
        SyncFrame::BlobChunk {
            request_id: "r1".into(),
            content_sha256: "a".repeat(64),
            offset: 0,
            total_bytes: 7,
            data_b64: "AAAAAAAAAA==".into(),
        },
        SyncFrame::BlobAck {
            request_id: "r1".into(),
            attachment_id: ID.into(),
            content_sha256: "a".repeat(64),
            accepted: true,
            code: None,
            message: None,
        },
        SyncFrame::BlobAck {
            request_id: "r2".into(),
            attachment_id: ID.into(),
            content_sha256: String::new(),
            accepted: false,
            code: Some("not_found".into()),
            message: Some("attachment not found".into()),
        },
        SyncFrame::BlobFetchRequest {
            request_id: "r3".into(),
            org_id: "org".into(),
            attachment_id: ID.into(),
        },
    ];
    let mut wire = Vec::new();
    for frame in &frames {
        write_sync_frame(&mut wire, frame).unwrap();
    }
    let mut cursor = Cursor::new(&wire);
    for frame in &frames {
        assert_eq!(&read_sync_frame(&mut cursor).unwrap().unwrap(), frame);
    }
    let accepted = serde_json::to_string(&frames[3]).unwrap();
    assert!(!accepted.contains("code") && !accepted.contains("message"));
}

#[test]
fn a_fetch_names_an_attachment_and_never_a_hash() {
    // The fetch frame has no field a hash could ride in: a hash-only fetch cannot be said.
    let fetch = serde_json::to_value(SyncFrame::BlobFetchRequest {
        request_id: "r".into(),
        org_id: "org".into(),
        attachment_id: ID.into(),
    })
    .unwrap();
    let mut keys: Vec<&str> = fetch
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    keys.sort_unstable();
    assert_eq!(keys, ["attachment_id", "org_id", "request_id", "type"]);
    let smuggled = format!(
        r#"{{"v":1,"type":"blob_fetch_request","request_id":"r","org_id":"o","content_sha256":"{}"}}"#,
        "a".repeat(64)
    );
    assert!(
        decode_sync_frame(smuggled.as_bytes()).is_err(),
        "a fetch without an attachment id does not decode"
    );
}

#[test]
fn the_largest_chunk_fits_one_frame_with_room_to_spare() {
    let bytes = pattern(BLOB_CHUNK_MAX_BYTES, 7);
    let digest = blob_digest(&bytes);
    let frames: Vec<SyncFrame> = blob_chunk_frames(ID, &digest, &bytes).collect();
    assert_eq!(frames.len(), 1);
    let encoded = encode_sync_frame(&frames[0]).unwrap();
    assert!(
        encoded.len() < MAX_ENCODED_FRAME_BYTES * 3 / 4,
        "a full chunk encodes to {} bytes",
        encoded.len()
    );
}

#[test]
fn an_image_and_a_full_25_mib_document_reassemble_and_verify() {
    let image = pattern(10 * 1024 * 1024, 1);
    assert_eq!(reassemble(&image, 10 * 1024 * 1024).unwrap(), image);
    let document = pattern(BLOB_TRANSFER_MAX_BYTES as usize, 2);
    let chunks = blob_chunk_frames("r", "x", &document).count();
    assert_eq!(chunks, 50, "25 MiB is fifty 512 KiB chunks");
    assert_eq!(
        reassemble(&document, BLOB_TRANSFER_MAX_BYTES).unwrap(),
        document
    );
    // One byte more than the bound is refused before anything is read.
    let digest = blob_digest(b"x");
    assert_eq!(
        BlobAssembler::new(&digest, BLOB_TRANSFER_MAX_BYTES + 1, u64::MAX).unwrap_err(),
        BlobTransferError::InvalidSize
    );
    assert_eq!(
        BlobAssembler::new(&digest, 11, 10).unwrap_err(),
        BlobTransferError::InvalidSize
    );
    assert_eq!(
        BlobAssembler::new(&digest, 0, 10).unwrap_err(),
        BlobTransferError::InvalidSize
    );
    assert_eq!(
        BlobAssembler::new(&digest.to_uppercase(), 1, 10).unwrap_err(),
        BlobTransferError::InvalidDigest
    );
}

#[test]
fn chunks_out_of_order_past_the_end_or_for_another_blob_are_refused() {
    let bytes = pattern(BLOB_CHUNK_MAX_BYTES * 2 + 10, 3);
    let digest = blob_digest(&bytes);
    let frames: Vec<(u64, String)> = blob_chunk_frames("r", &digest, &bytes)
        .map(|f| match f {
            SyncFrame::BlobChunk {
                offset, data_b64, ..
            } => (offset, data_b64),
            _ => unreachable!(),
        })
        .collect();
    let total = bytes.len() as u64;
    let fresh = || BlobAssembler::new(&digest, total, BLOB_TRANSFER_MAX_BYTES).unwrap();

    let mut skipped = fresh();
    assert_eq!(
        skipped.push(&digest, frames[1].0, total, &frames[1].1),
        Err(BlobTransferError::OutOfOrder)
    );
    let mut repeated = fresh();
    assert_eq!(repeated.push(&digest, 0, total, &frames[0].1), Ok(false));
    assert_eq!(
        repeated.push(&digest, 0, total, &frames[0].1),
        Err(BlobTransferError::OutOfOrder)
    );
    let mut other = fresh();
    assert_eq!(
        other.push(&"b".repeat(64), 0, total, &frames[0].1),
        Err(BlobTransferError::Mismatch)
    );
    assert_eq!(
        other.push(&digest, 0, total + 1, &frames[0].1),
        Err(BlobTransferError::Mismatch)
    );
    let mut garbled = fresh();
    assert_eq!(
        garbled.push(&digest, 0, total, "not base64!"),
        Err(BlobTransferError::Base64)
    );
    assert_eq!(
        garbled.push(&digest, 0, total, ""),
        Err(BlobTransferError::BadChunk)
    );
    let oversized = STANDARD.encode(pattern(BLOB_CHUNK_MAX_BYTES + 1, 0));
    assert_eq!(
        garbled.push(&digest, 0, total, &oversized),
        Err(BlobTransferError::BadChunk)
    );
    // A last chunk longer than what is left runs past the declared end.
    let mut overrun = BlobAssembler::new(&digest, 5, 10).unwrap();
    assert_eq!(
        overrun.push(&digest, 0, 5, &STANDARD.encode([0u8; 6])),
        Err(BlobTransferError::BadChunk)
    );
    // Nothing is handed out before every byte arrived and verified.
    let mut partial = fresh();
    partial.push(&digest, 0, total, &frames[0].1).unwrap();
    assert!(partial.into_verified_bytes().is_none());
}

#[test]
fn bytes_that_do_not_match_their_digest_are_never_handed_out() {
    let real = pattern(1000, 4);
    let claimed = blob_digest(b"something else entirely");
    let mut assembler = BlobAssembler::new(&claimed, real.len() as u64, 10_000).unwrap();
    assert_eq!(
        assembler.push(&claimed, 0, real.len() as u64, &STANDARD.encode(&real)),
        Err(BlobTransferError::DigestMismatch)
    );
    assert!(assembler.into_verified_bytes().is_none());
    assert!(is_blob_digest(&blob_digest(&real)));
    assert!(!is_blob_digest("A".repeat(64).as_str()));
    assert!(!is_blob_digest("a".repeat(63).as_str()));
}
