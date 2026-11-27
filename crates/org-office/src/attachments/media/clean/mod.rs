//! Whether an image is already in the form an office keeps it (`TASK-98.6`, ADR-0037). HQ keeps
//! a satellite's shared image byte for byte as the satellite's own sanitizer left it (its
//! SHA-256 names it everywhere), so before HQ stores it, it checks that the bytes carry pixels
//! and nothing else: every metadata block (EXIF and its GPS, XMP, ICC profiles, comments, text
//! chunks, thumbnails), every block the office's encoders never write, and any byte after the
//! format's end marker is refused, and a still image asks for no orientation.
//!
//! Each format's compressed pixel data is walked too, code by code where the format has no
//! length of its own, so no payload can ride inside the container after the last pixel
//! (`TASK-239`, `TASK-241`):
//!
//! - PNG (`png`): every chunk's CRC-32, and one zlib stream that ends at the last `IDAT` byte
//!   and inflates to exactly the declared rows.
//! - JPEG (`jpeg`): one baseline frame and scan with tables each defined once and used, the
//!   scan's Huffman codes holding exactly the frame's MCUs, `EOI` straight after them.
//! - WebP (`webp`, `vp8l`): one lossless `VP8L` chunk, zero-padded, whose bitstream ends in
//!   its last byte.
//! - GIF (`gif`): only looping and graphic control blocks besides the frames, each frame's
//!   LZW codes decoding to exactly its pixels and ending in its last byte.

mod gif;
mod huffman;
mod jpeg;
mod png;
mod refusal;
mod vp8l;
mod webp;

use image::metadata::Orientation;
use org_proto::ApiError;

use self::refusal::not_clean;
use super::MediaType;

/// Refuses an image (`media`, sniffed as such) whose bytes carry anything but pixels; any
/// other type passes.
pub fn check_clean(media: MediaType, raw: &[u8]) -> Result<(), ApiError> {
    match media {
        MediaType::Png => png::check(raw)?,
        MediaType::Jpeg => jpeg::check(raw)?,
        MediaType::Webp => webp::check(raw)?,
        MediaType::Gif => gif::check(raw)?,
        _ => return Ok(()),
    }
    if media != MediaType::Gif
        && super::image::orientation(media, raw)? != Orientation::NoTransforms
    {
        return Err(not_clean("an orientation"));
    }
    Ok(())
}
