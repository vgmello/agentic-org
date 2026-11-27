//! An image HQ keeps from a satellite must already be as an office keeps it (`TASK-98.6`):
//! every image the sanitizer writes passes, and any metadata block, unknown block, data hidden
//! after the pixels or after the end, or orientation is refused.

use org_proto::ErrorCode;

use super::super::{check_clean, validate, MediaType};
use super::fixtures::{chunk, gif, jpeg_with_exif, png, png_with_text, webp, GPS_MARKER};

fn refused(media: MediaType, raw: &[u8]) -> String {
    let error = check_clean(media, raw).expect_err("refused");
    assert_eq!(error.code, ErrorCode::InvalidRequest, "{}", error.message);
    error.message
}

fn kept(media: MediaType, raw: Vec<u8>) -> Vec<u8> {
    let kept = validate(media, raw).unwrap();
    assert_eq!(kept.media, media);
    kept.bytes
}

/// `bytes` with `inserted` spliced in at `at`.
fn spliced(bytes: &[u8], at: usize, inserted: &[u8]) -> Vec<u8> {
    let mut out = bytes.to_vec();
    out.splice(at..at, inserted.iter().copied());
    out
}

#[test]
fn every_image_the_sanitizer_keeps_is_clean() {
    for (media, raw) in [
        (MediaType::Png, png(40, 20)),
        (MediaType::Png, png_with_text(12, 9, GPS_MARKER)),
        (MediaType::Jpeg, jpeg_with_exif(40, 20, 6, GPS_MARKER)),
        (MediaType::Gif, gif(16, 16, 3)),
        (MediaType::Gif, gif(5, 7, 1)),
        (MediaType::Webp, webp(20, 10)),
    ] {
        let kept = kept(media, raw);
        check_clean(media, &kept).unwrap_or_else(|e| panic!("{media:?}: {}", e.message));
    }
    // Documents are checked elsewhere: nothing here refuses them.
    check_clean(MediaType::PlainText, b"plain notes").unwrap();
}

#[test]
fn metadata_comments_and_trailing_bytes_are_refused_in_every_image_format() {
    // Straight from an uploader, never sanitized: EXIF with GPS, a text chunk.
    assert!(refused(MediaType::Jpeg, &jpeg_with_exif(8, 8, 1, GPS_MARKER)).contains("metadata"));
    assert!(refused(MediaType::Png, &png_with_text(8, 8, GPS_MARKER)).contains("tEXt"));

    let clean_png = kept(MediaType::Png, png(8, 8));
    let iend = clean_png.len() - 12;
    for kind in [b"eXIf", b"iCCP", b"iTXt", b"zTXt", b"tIME"] {
        let with = spliced(&clean_png, iend, &chunk(kind, b"payload"));
        refused(MediaType::Png, &with);
    }
    let polyglot = [clean_png.as_slice(), b"PK\x05\x06 a zip after the end"].concat();
    assert!(refused(MediaType::Png, &polyglot).contains("after its end"));

    let clean_jpeg = kept(MediaType::Jpeg, jpeg_with_exif(8, 8, 1, "x"));
    let comment = [0xFF, 0xFE, 0x00, 0x07, b'h', b'e', b'l', b'l', b'o'];
    assert!(refused(MediaType::Jpeg, &spliced(&clean_jpeg, 2, &comment)).contains("comment"));
    let xmp = [0xFF, 0xE1, 0x00, 0x06, b'X', b'M', b'P', b'!'];
    refused(MediaType::Jpeg, &spliced(&clean_jpeg, 2, &xmp));
    let trailing = [clean_jpeg.as_slice(), b"<html>"].concat();
    assert!(refused(MediaType::Jpeg, &trailing).contains("after its end"));

    let clean_gif = kept(MediaType::Gif, gif(6, 6, 2));
    let trailer = clean_gif.len() - 1;
    let comment = [0x21, 0xFE, 0x05, b'h', b'e', b'l', b'l', b'o', 0x00];
    assert!(refused(MediaType::Gif, &spliced(&clean_gif, trailer, &comment)).contains("comment"));
    let mut xmp = vec![0x21, 0xFF, 0x0B];
    xmp.extend_from_slice(b"XMP DataXMP");
    xmp.extend_from_slice(&[0x03, b'<', b'x', b'>', 0x00]);
    refused(MediaType::Gif, &spliced(&clean_gif, trailer, &xmp));
    let trailing = [clean_gif.as_slice(), b"junk"].concat();
    assert!(refused(MediaType::Gif, &trailing).contains("after its end"));

    let clean_webp = kept(MediaType::Webp, webp(6, 6));
    // A RIFF that also holds an EXIF chunk, its size updated to cover it.
    let mut with_exif = clean_webp.clone();
    with_exif.extend_from_slice(b"EXIF");
    with_exif.extend_from_slice(&4u32.to_le_bytes());
    with_exif.extend_from_slice(b"GPS!");
    let riff = (with_exif.len() - 8) as u32;
    with_exif[4..8].copy_from_slice(&riff.to_le_bytes());
    refused(MediaType::Webp, &with_exif);
    let trailing = [clean_webp.as_slice(), b"junk"].concat();
    assert!(refused(MediaType::Webp, &trailing).contains("after its end"));
}

#[test]
fn data_hidden_after_the_pixel_stream_of_a_png_is_refused() {
    let clean = kept(MediaType::Png, png(8, 8));
    // The one IDAT chunk, its zlib stream followed by bytes no decoder reads.
    let start = clean
        .windows(4)
        .position(|w| w == b"IDAT")
        .expect("an IDAT chunk")
        - 4;
    let len = u32::from_be_bytes(clean[start..start + 4].try_into().unwrap()) as usize;
    let stream = &clean[start + 8..start + 8 + len];
    let mut hidden = stream.to_vec();
    hidden.extend_from_slice(b"PK\x03\x04 smuggled");
    let mut forged = clean[..start].to_vec();
    forged.extend(chunk(b"IDAT", &hidden));
    forged.extend_from_slice(&clean[start + 12 + len..]);
    assert!(refused(MediaType::Png, &forged).contains("after its pixels"));
    // The same stream cut short is damaged, not clean.
    let mut cut = clean[..start].to_vec();
    cut.extend(chunk(b"IDAT", &stream[..stream.len() - 6]));
    cut.extend_from_slice(&clean[start + 12 + len..]);
    refused(MediaType::Png, &cut);
}
