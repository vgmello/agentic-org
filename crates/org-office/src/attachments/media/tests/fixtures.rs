//! Files for the attachment tests, built in memory: images through the same pure-Rust codecs,
//! and PDFs and ZIP containers written by hand so every byte is known.

use std::io::{Cursor, Write};

use image::codecs::gif::{GifEncoder, Repeat};
use image::codecs::jpeg::JpegEncoder;
use image::codecs::png::PngEncoder;
use image::codecs::webp::WebPEncoder;
use image::{Delay, DynamicImage, Frame, ImageEncoder, Rgb, RgbImage, Rgba, RgbaImage};

/// The bytes no sanitized image may still hold.
pub(crate) const GPS_MARKER: &str = "GPS-SECRET-48.8584N-2.2945E";

/// A `width` x `height` image whose left half is red and right half blue.
pub(crate) fn halves(width: u32, height: u32) -> RgbImage {
    RgbImage::from_fn(width, height, |x, _| {
        if x < width / 2 {
            Rgb([220, 20, 20])
        } else {
            Rgb([20, 20, 220])
        }
    })
}

pub(crate) fn png(width: u32, height: u32) -> Vec<u8> {
    let mut out = Vec::new();
    DynamicImage::ImageRgb8(halves(width, height))
        .write_with_encoder(PngEncoder::new(&mut out))
        .unwrap();
    out
}

pub(crate) fn chunk(kind: &[u8; 4], data: &[u8]) -> Vec<u8> {
    let mut crc = flate2::Crc::new();
    crc.update(kind);
    crc.update(data);
    let mut out = (data.len() as u32).to_be_bytes().to_vec();
    out.extend_from_slice(kind);
    out.extend_from_slice(data);
    out.extend_from_slice(&crc.sum().to_be_bytes());
    out
}

/// A PNG with a `tEXt` chunk holding `marker`, before `IEND`.
pub(crate) fn png_with_text(width: u32, height: u32, marker: &str) -> Vec<u8> {
    let mut bytes = png(width, height);
    let iend = bytes.len() - 12;
    let mut text = b"Comment\0".to_vec();
    text.extend_from_slice(marker.as_bytes());
    let inserted = chunk(b"tEXt", &text);
    bytes.splice(iend..iend, inserted);
    bytes
}

/// A PNG that declares `width` x `height` and holds no real pixel data.
pub(crate) fn png_header_only(width: u32, height: u32) -> Vec<u8> {
    let mut out = b"\x89PNG\r\n\x1a\n".to_vec();
    let mut ihdr = width.to_be_bytes().to_vec();
    ihdr.extend_from_slice(&height.to_be_bytes());
    ihdr.extend_from_slice(&[8, 2, 0, 0, 0]);
    out.extend(chunk(b"IHDR", &ihdr));
    out.extend(chunk(
        b"IDAT",
        &[0x78, 0x9c, 0x03, 0x00, 0x00, 0x00, 0x00, 0x01],
    ));
    out.extend(chunk(b"IEND", &[]));
    out
}

/// The EXIF APP1 payload: `Make` = `marker`, `Orientation` = `orientation`.
fn exif(orientation: u16, marker: &str) -> Vec<u8> {
    let mut tiff = b"II*\0".to_vec();
    tiff.extend_from_slice(&8u32.to_le_bytes());
    tiff.extend_from_slice(&2u16.to_le_bytes());
    let data_at = 8 + 2 + 2 * 12 + 4;
    let make = format!("{marker}\0");
    // Make (0x010f), ASCII, at `data_at`.
    tiff.extend_from_slice(&0x010fu16.to_le_bytes());
    tiff.extend_from_slice(&2u16.to_le_bytes());
    tiff.extend_from_slice(&(make.len() as u32).to_le_bytes());
    tiff.extend_from_slice(&(data_at as u32).to_le_bytes());
    // Orientation (0x0112), SHORT, inline.
    tiff.extend_from_slice(&0x0112u16.to_le_bytes());
    tiff.extend_from_slice(&3u16.to_le_bytes());
    tiff.extend_from_slice(&1u32.to_le_bytes());
    tiff.extend_from_slice(&orientation.to_le_bytes());
    tiff.extend_from_slice(&[0, 0]);
    tiff.extend_from_slice(&0u32.to_le_bytes());
    tiff.extend_from_slice(make.as_bytes());
    let mut payload = b"Exif\0\0".to_vec();
    payload.extend(tiff);
    payload
}

/// A JPEG of [`halves`] with an EXIF block carrying `orientation` and `marker`.
pub(crate) fn jpeg_with_exif(width: u32, height: u32, orientation: u16, marker: &str) -> Vec<u8> {
    let mut plain = Vec::new();
    JpegEncoder::new_with_quality(&mut plain, 95)
        .write_image(
            halves(width, height).as_raw(),
            width,
            height,
            image::ExtendedColorType::Rgb8,
        )
        .unwrap();
    let payload = exif(orientation, marker);
    let mut app1 = vec![0xff, 0xe1];
    app1.extend_from_slice(&((payload.len() + 2) as u16).to_be_bytes());
    app1.extend(payload);
    let mut out = plain[..2].to_vec();
    out.extend(app1);
    out.extend_from_slice(&plain[2..]);
    out
}

/// A GIF of `frames` frames, each a different shade.
pub(crate) fn gif(width: u32, height: u32, frames: usize) -> Vec<u8> {
    let mut out = Vec::new();
    {
        let mut encoder = GifEncoder::new_with_speed(&mut out, 30);
        encoder.set_repeat(Repeat::Infinite).unwrap();
        for i in 0..frames {
            let shade = (i * 37 % 256) as u8;
            let buffer = RgbaImage::from_pixel(width, height, Rgba([shade, 0, 255 - shade, 255]));
            let delay = Delay::from_numer_denom_ms(50, 1);
            encoder
                .encode_frame(Frame::from_parts(buffer, 0, 0, delay))
                .unwrap();
        }
    }
    out
}

pub(crate) fn webp(width: u32, height: u32) -> Vec<u8> {
    let mut out = Vec::new();
    DynamicImage::ImageRgb8(halves(width, height))
        .write_with_encoder(WebPEncoder::new_lossless(&mut out))
        .unwrap();
    out
}

/// How many frames an encoded GIF holds.
pub(crate) fn gif_frame_count(bytes: &[u8]) -> usize {
    use image::AnimationDecoder;
    image::codecs::gif::GifDecoder::new(Cursor::new(bytes))
        .unwrap()
        .into_frames()
        .count()
}

/// A PDF of `pages` pages with a correct `startxref`.
pub(crate) fn pdf(pages: usize) -> Vec<u8> {
    let kids: Vec<String> = (0..pages).map(|i| format!("{} 0 R", i + 3)).collect();
    let mut body = String::from("%PDF-1.7\n%\u{e2}\u{e3}\u{cf}\u{d3}\n");
    body.push_str("1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n");
    body.push_str(&format!(
        "2 0 obj\n<< /Type /Pages /Kids [{}] /Count {pages} >>\nendobj\n",
        kids.join(" ")
    ));
    for i in 0..pages {
        body.push_str(&format!(
            "{} 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] >>\nendobj\n",
            i + 3
        ));
    }
    let xref = body.len();
    body.push_str(&format!(
        "xref\n0 {}\ntrailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
        pages + 3,
        pages + 3
    ));
    body.into_bytes()
}

/// A PDF whose page tree is only inside a compressed object stream.
pub(crate) fn pdf_with_object_stream(pages: usize) -> Vec<u8> {
    pdf_with_object_stream_named(pages, 0, 0)
}

/// [`pdf_with_object_stream`] whose stream's dictionary names `/ObjStm` `extra` more times,
/// and whose inflated objects carry `padding` more bytes of white space.
pub(crate) fn pdf_with_object_stream_named(pages: usize, extra: usize, padding: usize) -> Vec<u8> {
    let inner = format!(
        "2 0 << /Type /Pages /Kids [] /Count {pages} >>{}",
        " ".repeat(padding)
    );
    let names = "/ObjStm ".repeat(extra);
    let mut encoder = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
    encoder.write_all(inner.as_bytes()).unwrap();
    let compressed = encoder.finish().unwrap();
    let mut body = b"%PDF-1.7\n1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n".to_vec();
    body.extend_from_slice(
        format!(
            "5 0 obj\n<< {names}/Type /ObjStm /N 1 /First 4 /Filter /FlateDecode /Length {} >>\nstream\n",
            compressed.len()
        )
        .as_bytes(),
    );
    body.extend_from_slice(&compressed);
    body.extend_from_slice(b"\nendstream\nendobj\n");
    let xref = body.len();
    body.extend_from_slice(format!("startxref\n{xref}\n%%EOF").as_bytes());
    body
}

/// One entry of [`zip`]; `declared` overrides the uncompressed size the headers claim.
pub(crate) struct ZipEntry<'a> {
    pub name: &'a str,
    pub data: &'a [u8],
    pub declared: Option<u32>,
}

pub(crate) fn entry<'a>(name: &'a str, data: &'a [u8]) -> ZipEntry<'a> {
    ZipEntry {
        name,
        data,
        declared: None,
    }
}

/// A ZIP of stored entries, written field by field.
pub(crate) fn zip(entries: &[ZipEntry<'_>]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut central = Vec::new();
    for e in entries {
        let mut crc = flate2::Crc::new();
        crc.update(e.data);
        let size = e.data.len() as u32;
        let declared = e.declared.unwrap_or(size);
        let offset = out.len() as u32;
        let common = |buf: &mut Vec<u8>| {
            buf.extend_from_slice(&20u16.to_le_bytes());
            buf.extend_from_slice(&0u16.to_le_bytes());
            buf.extend_from_slice(&0u16.to_le_bytes());
            buf.extend_from_slice(&0u32.to_le_bytes());
            buf.extend_from_slice(&crc.sum().to_le_bytes());
            buf.extend_from_slice(&size.to_le_bytes());
            buf.extend_from_slice(&declared.to_le_bytes());
            buf.extend_from_slice(&(e.name.len() as u16).to_le_bytes());
            buf.extend_from_slice(&0u16.to_le_bytes());
        };
        out.extend_from_slice(b"PK\x03\x04");
        common(&mut out);
        out.extend_from_slice(e.name.as_bytes());
        out.extend_from_slice(e.data);
        central.extend_from_slice(b"PK\x01\x02");
        central.extend_from_slice(&20u16.to_le_bytes());
        common(&mut central);
        // Comment length, disk, internal and external attributes: all zero.
        central.extend_from_slice(&[0; 10]);
        central.extend_from_slice(&offset.to_le_bytes());
        central.extend_from_slice(e.name.as_bytes());
    }
    let cd_offset = out.len() as u32;
    let cd_size = central.len() as u32;
    out.extend(central);
    out.extend_from_slice(b"PK\x05\x06");
    out.extend_from_slice(&[0; 4]);
    out.extend_from_slice(&(entries.len() as u16).to_le_bytes());
    out.extend_from_slice(&(entries.len() as u16).to_le_bytes());
    out.extend_from_slice(&cd_size.to_le_bytes());
    out.extend_from_slice(&cd_offset.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out
}

pub(crate) fn content_types(main: &str) -> String {
    format!(
        "<?xml version=\"1.0\"?><Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\">\
         <Override PartName=\"/main.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.{main}.main+xml\"/></Types>"
    )
}

pub(crate) fn docx() -> Vec<u8> {
    let types = content_types("wordprocessingml.document");
    zip(&[
        entry("[Content_Types].xml", types.as_bytes()),
        entry("_rels/.rels", b"<Relationships/>"),
        entry("word/document.xml", b"<w:document/>"),
    ])
}

pub(crate) fn xlsx() -> Vec<u8> {
    let types = content_types("spreadsheetml.sheet");
    zip(&[
        entry("[Content_Types].xml", types.as_bytes()),
        entry("xl/workbook.xml", b"<workbook/>"),
    ])
}

/// Where one frame of a hand-made GIF sits on its canvas, and its size.
pub(crate) struct GifRect {
    pub left: u16,
    pub top: u16,
    pub width: u16,
    pub height: u16,
}

/// LZW codes packed least significant bit first, as GIF stores them.
#[derive(Default)]
struct GifBits {
    bytes: Vec<u8>,
    pending: u32,
    count: u32,
}

impl GifBits {
    fn push(&mut self, code: u16, width: u32) {
        self.pending |= u32::from(code) << self.count;
        self.count += width;
        while self.count >= 8 {
            self.bytes.push(self.pending as u8);
            self.pending >>= 8;
            self.count -= 8;
        }
    }

    fn finish(mut self) -> Vec<u8> {
        if self.count > 0 {
            self.bytes.push(self.pending as u8);
        }
        self.bytes
    }
}

/// A GIF89a of a `width` x `height` canvas with a four-colour global table and `frames`, each
/// one solid colour. The LZW data is written uncompressed (a clear code before every two
/// pixels keeps every code 3 bits wide). Without `pixels`, each frame's data is only a clear
/// code and the end code: a decoder that tried to fill the frame from it would fail.
pub(crate) fn gif_frames(width: u16, height: u16, frames: &[GifRect], pixels: bool) -> Vec<u8> {
    const CLEAR: u16 = 4;
    const END: u16 = 5;
    let mut out = b"GIF89a".to_vec();
    out.extend(width.to_le_bytes());
    out.extend(height.to_le_bytes());
    out.extend([0b1000_0001, 0, 0]);
    out.extend([0, 0, 0, 255, 0, 0, 0, 255, 0, 0, 0, 255]);
    for (i, frame) in frames.iter().enumerate() {
        out.extend([0x21, 0xf9, 4, 0, 5, 0, 0, 0]);
        out.push(0x2c);
        for field in [frame.left, frame.top, frame.width, frame.height] {
            out.extend(field.to_le_bytes());
        }
        out.extend([0, 2]);
        let colour = (i % 3 + 1) as u16;
        let mut bits = GifBits::default();
        let mut left = if pixels {
            u32::from(frame.width) * u32::from(frame.height)
        } else {
            0
        };
        bits.push(CLEAR, 3);
        while left > 0 {
            bits.push(CLEAR, 3);
            for _ in 0..left.min(2) {
                bits.push(colour, 3);
                left -= 1;
            }
        }
        bits.push(END, 3);
        for block in bits.finish().chunks(255) {
            out.push(block.len() as u8);
            out.extend_from_slice(block);
        }
        out.push(0);
    }
    out.push(0x3b);
    out
}
