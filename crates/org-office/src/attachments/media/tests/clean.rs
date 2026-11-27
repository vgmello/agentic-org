//! An image HQ keeps from a satellite must already be as an office keeps it (`TASK-98.6`):
//! every image the sanitizer writes passes, and any metadata block, unknown block, data hidden
//! after the pixels or after the end, or orientation is refused.

use std::io::Write;

use flate2::write::ZlibEncoder;
use flate2::Compression;
use image::codecs::jpeg::JpegEncoder;
use image::codecs::png::PngEncoder;
use image::codecs::webp::WebPEncoder;
use image::{DynamicImage, ImageBuffer, Luma, LumaA, Rgb, Rgba};
use org_proto::ErrorCode;

use super::super::image::MAX_DIMENSION;
use super::super::{check_clean, validate, MediaType};
use super::fixtures::{chunk, gif, jpeg_with_exif, png, png_with_text, webp, GPS_MARKER};

/// A PNG of `chunks` (each already framed) after its signature.
fn assembled(chunks: &[&[u8]]) -> Vec<u8> {
    [b"\x89PNG\r\n\x1a\n".as_slice()]
        .into_iter()
        .chain(chunks.iter().copied())
        .collect::<Vec<_>>()
        .concat()
}

fn ihdr(width: u32, height: u32, bit_depth: u8, color_type: u8, interlace: u8) -> Vec<u8> {
    let mut data = width.to_be_bytes().to_vec();
    data.extend_from_slice(&height.to_be_bytes());
    data.extend_from_slice(&[bit_depth, color_type, 0, 0, interlace]);
    chunk(b"IHDR", &data)
}

fn zlib(bytes: &[u8]) -> Vec<u8> {
    let mut encoder = ZlibEncoder::new(Vec::new(), Compression::best());
    encoder.write_all(bytes).unwrap();
    encoder.finish().unwrap()
}

fn png_of(image: DynamicImage) -> Vec<u8> {
    let mut out = Vec::new();
    image.write_with_encoder(PngEncoder::new(&mut out)).unwrap();
    out
}

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
    // Every color type and bit depth the PNG encoder writes, odd sizes, grey JPEG, alpha WebP.
    let rgba = ImageBuffer::from_fn(7, 3, |x, y| Rgba([x as u8 * 30, y as u8 * 60, 9, 128]));
    let grey = ImageBuffer::from_fn(33, 17, |x, y| Luma([(x * 7 + y) as u8]));
    let grey_alpha = ImageBuffer::from_fn(5, 5, |x, _| LumaA([x as u8 * 50, 200]));
    let deep = ImageBuffer::from_fn(3, 2, |x, y| Rgb([x as u16 * 9000, y as u16 * 30000, 7]));
    let mut grey_jpeg = Vec::new();
    DynamicImage::ImageLuma8(grey.clone())
        .write_with_encoder(JpegEncoder::new_with_quality(&mut grey_jpeg, 90))
        .unwrap();
    let mut rgba_webp = Vec::new();
    DynamicImage::ImageRgba8(rgba.clone())
        .write_with_encoder(WebPEncoder::new_lossless(&mut rgba_webp))
        .unwrap();
    for (media, raw) in [
        (MediaType::Png, png_of(DynamicImage::ImageRgba8(rgba))),
        (MediaType::Png, png_of(DynamicImage::ImageLuma8(grey))),
        (
            MediaType::Png,
            png_of(DynamicImage::ImageLumaA8(grey_alpha)),
        ),
        (MediaType::Png, png_of(DynamicImage::ImageRgb16(deep))),
        (MediaType::Png, png(1, 1)),
        (MediaType::Jpeg, grey_jpeg),
        (MediaType::Jpeg, jpeg_with_exif(1, 1, 1, "x")),
        (MediaType::Webp, rgba_webp),
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

#[test]
fn a_png_header_the_office_never_writes_is_refused() {
    let pixels = chunk(b"IDAT", &zlib(&[0, 220, 20, 20]));
    let end = chunk(b"IEND", &[]);
    let with = |header: &[u8]| assembled(&[header, &pixels, &end]);
    check_clean(MediaType::Png, &with(&ihdr(1, 1, 8, 2, 0))).unwrap();
    for header in [
        ihdr(1, 1, 8, 2, 1),                                         // interlaced
        ihdr(1, 1, 4, 2, 0),                                         // RGB at 4 bits
        ihdr(1, 1, 8, 5, 0),                                         // no such color type
        ihdr(0, 1, 8, 2, 0),                                         // no pixels
        ihdr(MAX_DIMENSION + 1, 1, 8, 2, 0),                         // too wide
        ihdr(10_000, 10_000, 8, 2, 0),                               // 100 megapixels
        chunk(b"IHDR", &[0, 0, 0, 1, 0, 0, 0, 1, 8, 2, 0, 0, 0, 0]), // 14 bytes
    ] {
        refused(MediaType::Png, &with(&header));
    }
}

#[test]
fn a_png_palette_and_transparency_come_once_each_before_the_pixels() {
    // A 1x1 palette image: one red entry, half transparent.
    let header = ihdr(1, 1, 8, 3, 0);
    let palette = chunk(b"PLTE", &[255, 0, 0]);
    let alpha = chunk(b"tRNS", &[128]);
    let pixels = chunk(b"IDAT", &zlib(&[0, 0]));
    let end = chunk(b"IEND", &[]);
    check_clean(
        MediaType::Png,
        &assembled(&[&header, &palette, &alpha, &pixels, &end]),
    )
    .unwrap();
    // After the pixels, where a decoder no longer reads them: a payload rides along.
    for late in [
        assembled(&[&header, &palette, &pixels, &alpha, &end]),
        assembled(&[&header, &palette, &alpha, &pixels, &palette, &end]),
        assembled(&[
            &header,
            &palette,
            &pixels,
            &chunk(b"PLTE", b"payload!!"),
            &end,
        ]),
    ] {
        assert!(refused(MediaType::Png, &late).contains("after its pixels"));
    }
    // Twice, out of order, missing, the wrong size, or for a color type that has none.
    let rgb = ihdr(1, 1, 8, 2, 0);
    let rgb_pixels = chunk(b"IDAT", &zlib(&[0, 1, 2, 3]));
    for wrong in [
        assembled(&[&header, &palette, &palette, &pixels, &end]),
        assembled(&[&header, &palette, &alpha, &alpha, &pixels, &end]),
        assembled(&[&header, &alpha, &palette, &pixels, &end]),
        assembled(&[&header, &pixels, &end]),
        assembled(&[&header, &chunk(b"PLTE", &[1, 2, 3, 4]), &pixels, &end]),
        assembled(&[&header, &palette, &chunk(b"tRNS", &[1, 2]), &pixels, &end]),
        assembled(&[
            &ihdr(1, 1, 1, 3, 0),
            &chunk(b"PLTE", &[0; 9]),
            &pixels,
            &end,
        ]),
        assembled(&[&rgb, &palette, &rgb_pixels, &end]),
        assembled(&[&rgb, &chunk(b"tRNS", &[0; 2]), &rgb_pixels, &end]),
        assembled(&[
            &ihdr(1, 1, 8, 6, 0),
            &chunk(b"tRNS", &[0; 6]),
            &rgb_pixels,
            &end,
        ]),
    ] {
        refused(MediaType::Png, &wrong);
    }
}

#[test]
fn a_png_ends_with_an_empty_end_block_straight_after_its_pixels() {
    let clean = kept(MediaType::Png, png(8, 8));
    let iend = clean.len() - 12;
    let mut stuffed = clean[..iend].to_vec();
    stuffed.extend(chunk(b"IEND", b"PK\x03\x04 smuggled"));
    assert!(refused(MediaType::Png, &stuffed).contains("inside its end block"));
    // A block the office writes, but between the pixels and the end.
    for late in [
        chunk(b"tRNS", &[0, 0, 0, 0, 0, 0]),
        chunk(b"PLTE", &[1, 2, 3]),
    ] {
        assert!(refused(MediaType::Png, &spliced(&clean, iend, &late)).contains("after its pixels"));
    }
    // The pixels may span several `IDAT` blocks, but only one unbroken run of them.
    let header = ihdr(1, 1, 8, 2, 0);
    let stream = zlib(&[0, 1, 2, 3]);
    let (first, second) = (chunk(b"IDAT", &stream[..4]), chunk(b"IDAT", &stream[4..]));
    let end = chunk(b"IEND", &[]);
    check_clean(
        MediaType::Png,
        &assembled(&[&header, &first, &second, &end]),
    )
    .unwrap();
    let broken = assembled(&[&header, &first, &chunk(b"tEXt", b"a\0b"), &second, &end]);
    refused(MediaType::Png, &broken);
}

#[test]
fn a_png_pixel_stream_that_inflates_past_its_rows_is_refused() {
    // One 1x1 RGB row: a filter byte and three samples, four bytes in all.
    let header = ihdr(1, 1, 8, 2, 0);
    let end = chunk(b"IEND", &[]);
    let with = |stream: &[u8]| assembled(&[&header, &chunk(b"IDAT", stream), &end]);
    check_clean(MediaType::Png, &with(&zlib(&[0, 220, 20, 20]))).unwrap();
    // The row and a payload in one stream that ends exactly: decoders stop reading at the row.
    let smuggled = [[0, 220, 20, 20].as_slice(), b"PK\x03\x04 smuggled"].concat();
    assert!(refused(MediaType::Png, &with(&zlib(&smuggled))).contains("after its pixels"));
    // A zlib bomb, 8 MiB of zeros in a few KiB: refused on its first window of output.
    let bomb = zlib(&vec![0; 8 << 20]);
    assert!(bomb.len() < 64 * 1024);
    assert!(refused(MediaType::Png, &with(&bomb)).contains("after its pixels"));
    // A stream that stops short of its row.
    refused(MediaType::Png, &with(&zlib(&[0, 220])));
}

#[test]
fn a_jpeg_holds_one_baseline_frame_and_one_scan_with_the_end_straight_after() {
    let clean = kept(MediaType::Jpeg, jpeg_with_exif(16, 8, 1, "x"));
    // A quantization table between the scan and the end: no decoder reads it.
    let eoi = clean.len() - 2;
    let table = [[0xFF, 0xDB, 0x00, 0x0B].as_slice(), b"smuggled!"].concat();
    assert!(refused(MediaType::Jpeg, &spliced(&clean, eoi, &table)).contains("after its pixels"));
    // A progressive frame instead of the baseline one, or a second baseline frame.
    let sof = clean
        .windows(2)
        .position(|w| w == [0xFF, 0xC0])
        .expect("a baseline frame");
    let mut progressive = clean.clone();
    progressive[sof + 1] = 0xC2;
    assert!(refused(MediaType::Jpeg, &progressive).contains("frame"));
    let frame_len = usize::from(u16::from_be_bytes([clean[sof + 2], clean[sof + 3]]));
    let twice = spliced(&clean, sof, &clean[sof..sof + 2 + frame_len]);
    assert!(refused(MediaType::Jpeg, &twice).contains("frame"));
}

#[test]
fn a_gif_graphic_control_block_comes_right_before_its_frame() {
    let clean = kept(MediaType::Gif, gif(4, 4, 2));
    let control = [0x21, 0xF9, 0x04, 0x00, 0x0A, 0x00, 0x00, 0x00];
    // A trailing one, describing no frame.
    let trailer = clean.len() - 1;
    let trailing = spliced(&clean, trailer, &control);
    assert!(refused(MediaType::Gif, &trailing).contains("without its frame"));
    // Two before one frame.
    let first = clean
        .windows(2)
        .position(|w| w == [0x21, 0xF9])
        .expect("a graphic control block");
    let doubled = spliced(&clean, first, &control);
    assert!(refused(MediaType::Gif, &doubled).contains("without its frame"));
}
