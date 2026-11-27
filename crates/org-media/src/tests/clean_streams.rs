//! The compressed pixel data inside a kept image is walked too (`TASK-241` M-1): JPEG Huffman
//! codes MCU by MCU, GIF LZW codes and WebP lossless codes, so a payload hidden after the last
//! pixel, inside the container, is refused, while every image the sanitizer writes passes.

use image::codecs::gif::{GifEncoder, Repeat};
use image::codecs::jpeg::JpegEncoder;
use image::codecs::png::PngEncoder;
use image::codecs::webp::WebPEncoder;
use image::{Delay, DynamicImage, Frame, GrayImage, Luma, Rgba, RgbaImage};

use super::clean::{kept, refused, spliced};
use crate::testkit::{gif, jpeg_with_exif, webp};
use crate::{check_clean, MediaType};

/// A deterministic picture of flat bands and noise, so encoders write long and short codes,
/// runs, back-references and full LZW tables alike.
fn noisy(width: u32, height: u32, seed: u32) -> RgbaImage {
    let mut state = seed | 1;
    RgbaImage::from_fn(width, height, |x, y| {
        state ^= state << 13;
        state ^= state >> 17;
        state ^= state << 5;
        let [a, b, c, d] = state.to_le_bytes();
        if (x / 7 + y / 5) % 3 == 0 {
            Rgba([200, 30, 30, 255])
        } else {
            Rgba([
                a,
                b,
                (x * 9 + y) as u8,
                if y % 4 == 0 { c | d } else { 255 },
            ])
        }
    })
}

fn encoded(image: DynamicImage, media: MediaType) -> Vec<u8> {
    let mut out = Vec::new();
    match media {
        MediaType::Png => image.write_with_encoder(PngEncoder::new(&mut out)),
        MediaType::Jpeg => image.write_with_encoder(JpegEncoder::new_with_quality(&mut out, 95)),
        _ => image.write_with_encoder(WebPEncoder::new_lossless(&mut out)),
    }
    .unwrap();
    out
}

fn noisy_gif(width: u32, height: u32, frames: u32) -> Vec<u8> {
    let mut out = Vec::new();
    {
        let mut encoder = GifEncoder::new_with_speed(&mut out, 30);
        encoder.set_repeat(Repeat::Infinite).unwrap();
        for seed in 0..frames {
            let delay = Delay::from_numer_denom_ms(40, 1);
            let frame = Frame::from_parts(noisy(width, height, seed + 7), 0, 0, delay);
            encoder.encode_frame(frame).unwrap();
        }
    }
    out
}

#[test]
fn noisy_images_the_sanitizer_keeps_are_clean() {
    let rgba = DynamicImage::ImageRgba8(noisy(97, 61, 3));
    let rgb = DynamicImage::ImageRgb8(rgba.to_rgb8());
    let grey = DynamicImage::ImageLuma8(rgba.to_luma8());
    for (media, raw) in [
        (MediaType::Png, encoded(rgba.clone(), MediaType::Png)),
        (MediaType::Png, encoded(grey.clone(), MediaType::Png)),
        (MediaType::Jpeg, encoded(rgb.clone(), MediaType::Jpeg)),
        (MediaType::Jpeg, encoded(grey, MediaType::Jpeg)),
        (MediaType::Webp, encoded(rgba, MediaType::Webp)),
        (MediaType::Webp, encoded(rgb, MediaType::Webp)),
        // Enough noise to fill the LZW table (and clear it) in every frame.
        (MediaType::Gif, noisy_gif(120, 90, 3)),
        (MediaType::Gif, noisy_gif(33, 17, 1)),
    ] {
        let kept = kept(media, raw);
        check_clean(media, &kept).unwrap_or_else(|e| panic!("{media:?}: {}", e.message));
    }
}

/// Where the scan's entropy-coded data starts in a JPEG, and where its `SOS` marker is.
fn scan_of(jpeg: &[u8]) -> (usize, usize) {
    let sos = jpeg
        .windows(2)
        .position(|w| w == [0xFF, 0xDA])
        .expect("a scan");
    let len = usize::from(u16::from_be_bytes([jpeg[sos + 2], jpeg[sos + 3]]));
    (sos, sos + 2 + len)
}

/// The segment (marker included) that starts with `marker`.
fn segment(jpeg: &[u8], marker: u8) -> Vec<u8> {
    let at = jpeg
        .windows(2)
        .position(|w| w == [0xFF, marker])
        .expect("the segment");
    let len = usize::from(u16::from_be_bytes([jpeg[at + 2], jpeg[at + 3]]));
    jpeg[at..at + 2 + len].to_vec()
}

/// `jpeg` with its scan data replaced by `scan` and `before_scan` inserted before its `SOS`.
fn rescanned(jpeg: &[u8], before_scan: &[u8], scan: &[u8]) -> Vec<u8> {
    let (sos, data) = scan_of(jpeg);
    [
        &jpeg[..sos],
        before_scan,
        &jpeg[sos..data],
        scan,
        &[0xFF, 0xD9],
    ]
    .concat()
}

/// A 16x8 mid-grey JPEG as the sanitizer keeps it: two blocks of nothing but a zero DC
/// difference (`00`) and an end of block (`1010`), padded with ones: `28 AF`.
fn two_blocks() -> Vec<u8> {
    let mut raw = Vec::new();
    DynamicImage::ImageLuma8(GrayImage::from_pixel(16, 8, Luma([128])))
        .write_with_encoder(JpegEncoder::new_with_quality(&mut raw, 90))
        .unwrap();
    let clean = kept(MediaType::Jpeg, raw);
    let (_, data) = scan_of(&clean);
    assert_eq!(clean[data..], [0x28, 0xAF, 0xFF, 0xD9]);
    clean
}

#[test]
fn bytes_stuffed_after_the_last_mcu_of_a_jpeg_are_refused() {
    let payload = b"\xFF\x00SMUGGLED\xFF\x00PAYL";
    assert_eq!(payload.len(), 16);
    for clean in [
        kept(MediaType::Jpeg, jpeg_with_exif(16, 8, 1, "x")),
        kept(MediaType::Jpeg, jpeg_with_exif(23, 41, 1, "x")),
        two_blocks(),
    ] {
        check_clean(MediaType::Jpeg, &clean).unwrap();
        let eoi = clean.len() - 2;
        let smuggled = spliced(&clean, eoi, payload);
        assert!(refused(MediaType::Jpeg, &smuggled).contains("after its pixels"));
        // A scan cut short runs into the end marker: damaged.
        let cut = [&clean[..eoi - 1], &[0xFF, 0xD9][..]].concat();
        assert!(refused(MediaType::Jpeg, &cut).contains("damaged"));
    }
    // The padding to the end marker is ones, never data.
    let clean = two_blocks();
    let (_, data) = scan_of(&clean);
    let mut padded = clean.clone();
    padded[data + 1] = 0xA0;
    assert!(refused(MediaType::Jpeg, &padded).contains("padding"));
}

#[test]
fn jpeg_tables_are_whole_defined_once_and_all_used() {
    let clean = two_blocks();
    let (sos, _) = scan_of(&clean);
    let before_scan = |inserted: &[u8]| spliced(&clean, sos, inserted);
    let quant = segment(&clean, 0xDB);
    let huffman = segment(&clean, 0xC4);
    for twice in [&quant, &huffman] {
        assert!(refused(MediaType::Jpeg, &before_scan(twice)).contains("defined twice"));
    }
    // A quantization or Huffman table the frame and scan never name.
    let mut unused_quant = vec![0xFF, 0xDB, 0x00, 0x43, 0x02];
    unused_quant.extend([1; 64]);
    let mut unused_huffman = huffman.clone();
    unused_huffman[4] = 0x01;
    for unused in [&unused_quant, &unused_huffman] {
        assert!(refused(MediaType::Jpeg, &before_scan(unused)).contains("never use"));
    }
    // Malformed: a byte left over, 16-bit entries, a zero entry, a DC value past 11 bits, more
    // codes than a length holds, the all-ones code, or an empty segment.
    let mut leftover = unused_quant.clone();
    leftover[3] = 0x44;
    leftover.push(0);
    let mut wide = vec![0xFF, 0xDB, 0x00, 0x83, 0x12];
    wide.extend([0, 1].repeat(64));
    let mut zero = unused_quant.clone();
    zero[20] = 0;
    let dht = |table: &[u8]| {
        let mut segment = vec![0xFF, 0xC4, 0x00, (table.len() + 2) as u8];
        segment.extend_from_slice(table);
        segment
    };
    let one_bit = |count: u8, values: &[u8]| {
        let mut table = vec![0x01, count];
        table.extend([0; 15]);
        table.extend_from_slice(values);
        table
    };
    let mut trailing_huffman = unused_huffman.clone();
    trailing_huffman[3] += 1;
    trailing_huffman.push(0);
    for malformed in [
        leftover,
        wide,
        zero,
        dht(&one_bit(1, &[12])),
        dht(&one_bit(3, &[0, 1, 2])),
        dht(&one_bit(2, &[0, 1])),
        dht(&[]),
        trailing_huffman,
    ] {
        let message = refused(MediaType::Jpeg, &before_scan(&malformed));
        assert!(message.contains("damaged"), "{malformed:02X?}: {message}");
    }
}

#[test]
fn a_jpeg_restart_interval_is_exact_once_and_its_markers_in_place() {
    let clean = two_blocks();
    let (sos, _) = scan_of(&clean);
    let no_restart = [0xFF, 0xDD, 0x00, 0x04, 0x00, 0x00];
    check_clean(MediaType::Jpeg, &spliced(&clean, sos, &no_restart)).unwrap();
    let long = [0xFF, 0xDD, 0x00, 0x05, 0x00, 0x00, 0x00];
    assert!(refused(MediaType::Jpeg, &spliced(&clean, sos, &long)).contains("damaged"));
    let twice = [no_restart, no_restart].concat();
    assert!(refused(MediaType::Jpeg, &spliced(&clean, sos, &twice)).contains("second restart"));
    // One MCU an interval: the first block, its ones padding, `RST0`, the second block.
    let every_mcu = [0xFF, 0xDD, 0x00, 0x04, 0x00, 0x01];
    let restarted = |scan: &[u8]| rescanned(&clean, &every_mcu, scan);
    check_clean(MediaType::Jpeg, &restarted(&[0x2B, 0xFF, 0xD0, 0x2B])).unwrap();
    for (scan, why) in [
        (&[0x2B, 0xFF, 0xD1, 0x2B][..], "damaged"), // the wrong marker
        (&[0x2B, 0x2B], "damaged"),                 // no marker
        (&[0x28, 0xAF], "padding"),                 // zero padding where the marker goes
        (&[0x2B, 0xFF, 0xD0, 0x2B, 0xFF, 0xD1], "after its pixels"), // one past the last MCU
    ] {
        let message = refused(MediaType::Jpeg, &restarted(scan));
        assert!(message.contains(why), "{scan:02X?}: {message}");
    }
}

/// A 1x1 GIF of two colors whose one frame holds `lzw` (LZW minimum code size 2) in one
/// sub-block.
fn tiny_gif(lzw: &[u8]) -> Vec<u8> {
    let mut gif = b"GIF89a".to_vec();
    gif.extend_from_slice(&[1, 0, 1, 0, 0x80, 0, 0]);
    gif.extend_from_slice(&[0, 0, 0, 255, 255, 255]);
    gif.extend_from_slice(&[0x2C, 0, 0, 0, 0, 1, 0, 1, 0, 0, 2]);
    gif.push(lzw.len() as u8);
    gif.extend_from_slice(lzw);
    gif.extend_from_slice(&[0, 0x3B]);
    gif
}

#[test]
fn a_sub_block_after_the_end_code_of_a_gif_frame_is_refused() {
    for clean in [
        kept(MediaType::Gif, gif(6, 6, 2)),
        kept(MediaType::Gif, noisy_gif(40, 30, 2)),
    ] {
        // The last frame's terminator sits right before the trailer.
        let terminator = clean.len() - 2;
        assert_eq!(clean[terminator..], [0x00, 0x3B]);
        let evil = spliced(&clean, terminator, &[0x04, b'E', b'V', b'I', b'L']);
        assert!(refused(MediaType::Gif, &evil).contains("after its pixels"));
    }
    // Codes 3 bits wide, least significant bit first: clear (4), pixel 0, end (5).
    let one_pixel = tiny_gif(&[0x44, 0x01]);
    check_clean(MediaType::Gif, &one_pixel).unwrap();
    assert_eq!(image::load_from_memory(&one_pixel).unwrap().width(), 1);
    // A second pixel the 1x1 frame has no room for, or a set padding bit after the end code.
    for hidden in [tiny_gif(&[0x04, 0x0A]), tiny_gif(&[0x44, 0x81])] {
        assert!(refused(MediaType::Gif, &hidden).contains("after its pixels"));
    }
    // No end code before the terminator, or a code the table does not hold yet (7).
    for damaged in [tiny_gif(&[0x44]), tiny_gif(&[0x3C, 0x05])] {
        assert!(refused(MediaType::Gif, &damaged).contains("damaged"));
    }
}

/// Bits written least significant first, as a `VP8L` stream holds them.
#[derive(Default)]
struct Bits {
    bytes: Vec<u8>,
    at: usize,
}

impl Bits {
    fn put(&mut self, value: u32, count: u32) -> &mut Self {
        for shift in 0..count {
            if self.at.is_multiple_of(8) {
                self.bytes.push(0);
            }
            if (value >> shift) & 1 == 1 {
                *self.bytes.last_mut().unwrap() |= 1 << (self.at % 8);
            }
            self.at += 1;
        }
        self
    }

    /// A 1x1 image's header: no transform, no color cache; then whether an entropy image
    /// follows.
    fn header(&mut self, entropy_image: bool) -> &mut Self {
        self.put(0x2F, 8).put(0, 14).put(0, 14).put(0, 1).put(0, 3);
        self.put(0, 1).put(0, 1).put(u32::from(entropy_image), 1)
    }

    /// A simple prefix code of the one symbol 0 or 1.
    fn lone(&mut self, symbol: u32) -> &mut Self {
        self.put(1, 1).put(0, 1).put(0, 1).put(symbol, 1)
    }
}

/// A WebP of one `VP8L` chunk holding `payload`, zero-padded to an even length.
fn riff(payload: &[u8]) -> Vec<u8> {
    let padded = payload.len() + payload.len() % 2;
    let mut out = b"RIFF".to_vec();
    out.extend_from_slice(&(12 + padded as u32).to_le_bytes());
    out.extend_from_slice(b"WEBPVP8L");
    out.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    out.extend_from_slice(payload);
    out.resize(20 + padded, 0);
    out
}

#[test]
fn data_after_the_lossless_bitstream_of_a_webp_is_refused() {
    for clean in [
        kept(MediaType::Webp, webp(6, 6)),
        kept(
            MediaType::Webp,
            encoded(DynamicImage::ImageRgba8(noisy(31, 9, 5)), MediaType::Webp),
        ),
    ] {
        let len = u32::from_le_bytes(clean[16..20].try_into().unwrap()) as usize;
        let payload = &clean[20..20 + len];
        assert_eq!(riff(payload), clean);
        let evil = riff(&[payload, b"EVIL"].concat());
        assert!(refused(MediaType::Webp, &evil).contains("after its pixels"));
        assert!(refused(MediaType::Webp, &riff(&payload[..len - 1])).contains("damaged"));
    }
    // A chunk of odd length: its pad byte is zero.
    let odd = (1..40)
        .map(|side| kept(MediaType::Webp, webp(side, 3)))
        .find(|kept| kept[16] % 2 == 1)
        .expect("an odd-length chunk");
    check_clean(MediaType::Webp, &odd).unwrap();
    let mut padded = odd.clone();
    *padded.last_mut().unwrap() = b'!';
    assert!(refused(MediaType::Webp, &padded).contains("padding"));
}

#[test]
fn a_hand_written_lossless_stream_ends_in_its_last_byte() {
    // 1x1, five one-symbol codes: the pixel reads no bits, so 63 bits in all.
    let mut bits = Bits::default();
    bits.header(false).lone(0).lone(0).lone(0).lone(0).lone(0);
    assert_eq!(bits.at, 63);
    let clean = riff(&bits.bytes);
    check_clean(MediaType::Webp, &clean).unwrap();
    assert_eq!(image::load_from_memory(&clean).unwrap().width(), 1);
    // A set padding bit, or one more byte.
    let mut set = bits.bytes.clone();
    *set.last_mut().unwrap() |= 0x80;
    for hidden in [riff(&set), riff(&[&bits.bytes[..], &[0]].concat())] {
        assert!(refused(MediaType::Webp, &hidden).contains("after its pixels"));
    }
    // An entropy image, which the office never writes.
    let mut meta = Bits::default();
    meta.header(true).put(0, 32);
    assert!(refused(MediaType::Webp, &riff(&meta.bytes)).contains("entropy image"));
    // A green code of two 8-bit symbols read in one bit: in increasing order only.
    let two = |first: u32, second: u32| {
        let mut bits = Bits::default();
        bits.header(false)
            .put(1, 1)
            .put(1, 1)
            .put(1, 1)
            .put(first, 8)
            .put(second, 8);
        bits.lone(0).lone(0).lone(0).lone(0).put(0, 1);
        riff(&bits.bytes)
    };
    check_clean(MediaType::Webp, &two(3, 5)).unwrap();
    for wrong in [two(5, 3), two(4, 4)] {
        assert!(refused(MediaType::Webp, &wrong).contains("damaged"));
    }
}
