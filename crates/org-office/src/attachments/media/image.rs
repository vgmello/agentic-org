//! Image attachments: decoded under strict limits and encoded again, so the office keeps only
//! pixels. EXIF (GPS included), XMP, ICC profiles, comments, trailing data and every other
//! block an uploader's file carried are gone, and the EXIF orientation is applied to the
//! pixels, before the content hash is taken.
//!
//! - At most [`MAX_DIMENSION`] pixels on a side and [`MAX_PIXELS`] in all, checked from the
//!   header before any pixel is decoded.
//! - Animations (GIF, WebP) at most [`MAX_FRAMES`] frames and [`MAX_ANIMATION_PIXELS`]
//!   decoded pixels in all, streamed frame by frame into a GIF so only one decoded frame is
//!   held. No frame may be larger than the canvas the header declares, and one decode may
//!   allocate only what a few canvas-sized buffers need, so a tiny canvas never hides a huge
//!   sub-frame.
//! - Every encoder writes into a [`BoundedVecWriter`] that refuses its first byte past the
//!   cap, so an encoding that would be too large fails before it is held in memory.
//! - PNG stays PNG, JPEG stays JPEG, a still WebP stays (lossless) WebP and GIF stays GIF; an
//!   animated WebP becomes an animated GIF. A still image whose re-encoding would exceed the
//!   10 MiB cap is kept as JPEG when it has no transparency, and refused otherwise.
//!
//! [`model_derivative`] makes the bounded copy a model reads: at most
//! [`DERIVATIVE_MAX_EDGE`] pixels on its longest side and [`DERIVATIVE_MAX_BYTES`] bytes.

use std::io::{self, Cursor, Write};

use image::codecs::gif::{GifDecoder, GifEncoder, Repeat};
use image::codecs::jpeg::JpegEncoder;
use image::codecs::png::PngEncoder;
use image::codecs::webp::{WebPDecoder, WebPEncoder};
use image::error::{ImageError, LimitErrorKind};
use image::imageops::FilterType;
use image::metadata::{LoopCount, Orientation};
use image::{
    AnimationDecoder, DynamicImage, Frames, ImageDecoder, ImageFormat, ImageReader, Limits,
};
use org_proto::attachments::ATTACHMENT_IMAGE_MAX_BYTES;
use org_proto::{ApiError, ErrorCode};

use super::{corrupt, unsupported, MediaType, ValidatedFile};

/// The longest side of an accepted image, in pixels.
pub const MAX_DIMENSION: u32 = 16_384;
/// The most pixels an accepted image (or one animation frame) may have.
pub const MAX_PIXELS: u64 = 64_000_000;
/// The most frames an animation may have.
pub const MAX_FRAMES: u64 = 200;
/// The most pixels all of an animation's frames may decode to together.
pub const MAX_ANIMATION_PIXELS: u64 = 128_000_000;
/// The longest side of a model derivative.
pub const DERIVATIVE_MAX_EDGE: u32 = 1_568;
/// The largest model derivative, in bytes.
pub const DERIVATIVE_MAX_BYTES: usize = 1024 * 1024;
/// What one decode may allocate: a 64 MP image at 16 bits per RGBA channel, and some.
const MAX_ALLOC: u64 = 600 * 1024 * 1024;
/// The least one animation decode is allowed, whatever its canvas.
const MIN_ANIMATION_ALLOC: u64 = 1024 * 1024;
/// GIF palette quantization effort (1 best and slowest, 30 fastest).
const GIF_SPEED: i32 = 10;
const JPEG_QUALITY: u8 = 90;

/// A model's copy of an image.
#[derive(Clone, PartialEq, Eq)]
pub struct ModelDerivative {
    pub media: MediaType,
    pub bytes: Vec<u8>,
    pub width: u32,
    pub height: u32,
}

impl std::fmt::Debug for ModelDerivative {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ModelDerivative")
            .field("media", &self.media)
            .field("bytes", &self.bytes.len())
            .field("width", &self.width)
            .field("height", &self.height)
            .finish()
    }
}

fn limits() -> Limits {
    let mut limits = Limits::default();
    limits.max_image_width = Some(MAX_DIMENSION);
    limits.max_image_height = Some(MAX_DIMENSION);
    limits.max_alloc = Some(MAX_ALLOC);
    limits
}

/// An animation's limits, bounded by the canvas its header declares (already checked): no
/// frame wider or taller than the canvas, and room for the three RGBA buffers one frame needs
/// at most (the composited canvas kept between frames, the frame itself, and the new
/// composite when the frame covers only part of the canvas).
fn animation_limits(width: u32, height: u32) -> Limits {
    let canvas_bytes = u64::from(width)
        .saturating_mul(u64::from(height))
        .saturating_mul(4);
    let mut limits = Limits::default();
    limits.max_image_width = Some(width);
    limits.max_image_height = Some(height);
    limits.max_alloc = Some(
        canvas_bytes
            .saturating_mul(3)
            .clamp(MIN_ANIMATION_ALLOC, MAX_ALLOC),
    );
    limits
}

/// A `Vec<u8>` sink that refuses to grow past `cap` bytes: the write that would cross it fails
/// (and nothing past the cap is ever allocated), and [`BoundedVecWriter::overflowed`] says so.
pub(crate) struct BoundedVecWriter {
    bytes: Vec<u8>,
    cap: usize,
    overflowed: bool,
}

impl BoundedVecWriter {
    pub(crate) fn new(cap: usize) -> Self {
        Self {
            bytes: Vec::new(),
            cap,
            overflowed: false,
        }
    }

    /// Whether a write was refused for crossing the cap.
    pub(crate) fn overflowed(&self) -> bool {
        self.overflowed
    }

    pub(crate) fn into_inner(self) -> Vec<u8> {
        self.bytes
    }
}

impl Write for BoundedVecWriter {
    fn write(&mut self, data: &[u8]) -> io::Result<usize> {
        let needed = self.bytes.len().saturating_add(data.len());
        if self.overflowed || needed > self.cap {
            self.overflowed = true;
            return Err(io::Error::other("the encoded image is over its size cap"));
        }
        if needed > self.bytes.capacity() {
            // Grow geometrically, but never reserve past the cap.
            let target = needed
                .max(self.bytes.capacity().saturating_mul(2))
                .min(self.cap);
            self.bytes.reserve_exact(target - self.bytes.len());
        }
        self.bytes.extend_from_slice(data);
        Ok(data.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

fn too_large(message: &str) -> ApiError {
    ApiError::new(ErrorCode::TooLarge, message)
}

fn decode_error(error: ImageError) -> ApiError {
    match error {
        ImageError::Limits(limit) if matches!(limit.kind(), LimitErrorKind::DimensionError) => {
            too_large("the image's dimensions exceed the limit")
        }
        ImageError::Limits(_) => too_large("the image needs more memory to decode than allowed"),
        _ => corrupt("the image is damaged or not what its type says"),
    }
}

fn encode_error(_: ImageError) -> ApiError {
    corrupt("the image could not be encoded again")
}

/// The encoding in `out`, `None` when it crossed its cap, or the encoder's own error.
fn finish(
    out: BoundedVecWriter,
    result: Result<(), ImageError>,
) -> Result<Option<Vec<u8>>, ApiError> {
    if out.overflowed() {
        return Ok(None);
    }
    result.map_err(encode_error)?;
    Ok(Some(out.into_inner()))
}

/// Refuses an empty image, a side over [`MAX_DIMENSION`] or more than [`MAX_PIXELS`] pixels.
fn check_dimensions(width: u32, height: u32) -> Result<(), ApiError> {
    if width == 0 || height == 0 {
        return Err(corrupt("the image has no pixels"));
    }
    if width > MAX_DIMENSION || height > MAX_DIMENSION {
        return Err(too_large("an image is at most 16384 pixels on a side"));
    }
    if u64::from(width) * u64::from(height) > MAX_PIXELS {
        return Err(too_large("an image has at most 64 megapixels"));
    }
    Ok(())
}

fn format_of(media: MediaType) -> Result<ImageFormat, ApiError> {
    match media {
        MediaType::Png => Ok(ImageFormat::Png),
        MediaType::Jpeg => Ok(ImageFormat::Jpeg),
        MediaType::Webp => Ok(ImageFormat::WebP),
        MediaType::Gif => Ok(ImageFormat::Gif),
        _ => Err(unsupported("not an image type")),
    }
}

/// Decodes and re-encodes `raw`, declared (and sniffed) as `media`.
pub fn sanitize(media: MediaType, raw: &[u8]) -> Result<ValidatedFile, ApiError> {
    match media {
        MediaType::Gif => {
            let decoder = GifDecoder::new(Cursor::new(raw)).map_err(decode_error)?;
            animation(decoder)
        }
        MediaType::Webp => {
            let decoder = WebPDecoder::new(Cursor::new(raw)).map_err(decode_error)?;
            if decoder.has_animation() {
                animation(decoder)
            } else {
                keep_still(decode_still(raw, media)?, media)
            }
        }
        MediaType::Png | MediaType::Jpeg => keep_still(decode_still(raw, media)?, media),
        _ => Err(unsupported("not an image type")),
    }
}

/// The first (or only) frame, its orientation applied.
fn decode_still(raw: &[u8], media: MediaType) -> Result<DynamicImage, ApiError> {
    let mut reader = ImageReader::with_format(Cursor::new(raw), format_of(media)?);
    reader.limits(limits());
    let mut decoder = reader.into_decoder().map_err(decode_error)?;
    let (width, height) = decoder.dimensions();
    check_dimensions(width, height)?;
    let orientation = decoder.orientation().unwrap_or(Orientation::NoTransforms);
    let mut image = DynamicImage::from_decoder(decoder).map_err(decode_error)?;
    image.apply_orientation(orientation);
    Ok(image)
}

/// A still image in its own format, or as JPEG when that alone fits the cap.
fn keep_still(image: DynamicImage, media: MediaType) -> Result<ValidatedFile, ApiError> {
    let cap = image_cap();
    let mut kept = encode(&image, media, JPEG_QUALITY, cap)?.map(|bytes| (media, bytes));
    if kept.is_none() && !image.color().has_alpha() && media != MediaType::Jpeg {
        kept = encode(&image, MediaType::Jpeg, JPEG_QUALITY, cap)?
            .map(|bytes| (MediaType::Jpeg, bytes));
    }
    let Some((media, bytes)) = kept else {
        return Err(too_large(
            "the image is over 10 MiB once its metadata is removed",
        ));
    };
    Ok(ValidatedFile {
        media,
        bytes,
        width: Some(image.width()),
        height: Some(image.height()),
        page_count: None,
    })
}

/// The 10 MiB cap of a kept image.
fn image_cap() -> usize {
    usize::try_from(ATTACHMENT_IMAGE_MAX_BYTES).unwrap_or(usize::MAX)
}

/// `image` encoded as `media`, with no metadata block of any kind; `None` when the encoding
/// would be over `cap` bytes (it stops at the first byte past it).
fn encode(
    image: &DynamicImage,
    media: MediaType,
    quality: u8,
    cap: usize,
) -> Result<Option<Vec<u8>>, ApiError> {
    let mut out = BoundedVecWriter::new(cap);
    let result = match media {
        MediaType::Png => image.write_with_encoder(PngEncoder::new(&mut out)),
        MediaType::Jpeg => {
            let pixels = if image.color().has_color() {
                DynamicImage::ImageRgb8(image.to_rgb8())
            } else {
                DynamicImage::ImageLuma8(image.to_luma8())
            };
            pixels.write_with_encoder(JpegEncoder::new_with_quality(&mut out, quality))
        }
        MediaType::Webp => {
            let pixels = if image.color().has_alpha() {
                DynamicImage::ImageRgba8(image.to_rgba8())
            } else {
                DynamicImage::ImageRgb8(image.to_rgb8())
            };
            pixels.write_with_encoder(WebPEncoder::new_lossless(&mut out))
        }
        _ => return Err(unsupported("not a still image type")),
    };
    finish(out, result)
}

/// Streams every frame of an animation (a one-frame GIF included) into a new GIF.
fn animation<'a, D>(mut decoder: D) -> Result<ValidatedFile, ApiError>
where
    D: ImageDecoder + AnimationDecoder<'a>,
{
    let (width, height) = decoder.dimensions();
    check_dimensions(width, height)?;
    decoder
        .set_limits(animation_limits(width, height))
        .map_err(decode_error)?;
    // How often it plays is the source's own, decided before any frame is decoded.
    let repeat = match decoder.loop_count() {
        LoopCount::Infinite => Repeat::Infinite,
        LoopCount::Finite(times) => {
            u16::try_from(times.get()).map_or(Repeat::Infinite, Repeat::Finite)
        }
    };
    let Some(bytes) = reencode_frames(decoder.into_frames(), width, height, repeat)? else {
        return Err(too_large(
            "the animation is over 10 MiB once its metadata is removed",
        ));
    };
    Ok(ValidatedFile {
        media: MediaType::Gif,
        bytes,
        width: Some(width),
        height: Some(height),
        page_count: None,
    })
}

/// Every frame into a new GIF, one decoded frame at a time (each is encoded and dropped before
/// the next is decoded); `None` when the GIF would be over the 10 MiB cap.
fn reencode_frames(
    frames: Frames<'_>,
    width: u32,
    height: u32,
    repeat: Repeat,
) -> Result<Option<Vec<u8>>, ApiError> {
    let canvas = u64::from(width) * u64::from(height);
    let mut out = BoundedVecWriter::new(image_cap());
    let mut encoder = GifEncoder::new_with_speed(&mut out, GIF_SPEED);
    encoder.set_repeat(repeat).map_err(encode_error)?;
    let mut count: u64 = 0;
    let mut written = Ok(());
    for frame in frames {
        count += 1;
        if count > MAX_FRAMES {
            return Err(too_large("an animated image has at most 200 frames"));
        }
        if canvas.saturating_mul(count) > MAX_ANIMATION_PIXELS {
            return Err(too_large(
                "an animated image decodes to at most 128 megapixels in all",
            ));
        }
        let frame = frame.map_err(decode_error)?;
        written = encoder.encode_frame(frame);
        if written.is_err() {
            break;
        }
    }
    // Dropping the encoder writes the GIF trailer, which may itself cross the cap.
    drop(encoder);
    let encoded = finish(out, written)?;
    if count == 0 {
        return Err(corrupt("the image has no frames"));
    }
    Ok(encoded)
}

/// The copy of a kept image a model reads: its first frame, scaled down to at most
/// [`DERIVATIVE_MAX_EDGE`] pixels on its longest side and [`DERIVATIVE_MAX_BYTES`] bytes,
/// as JPEG (or PNG when it has transparency).
pub fn model_derivative(media: MediaType, kept: &[u8]) -> Result<ModelDerivative, ApiError> {
    if !media.is_image() {
        return Err(unsupported("only images have a model derivative"));
    }
    let image = decode_still(kept, media)?;
    let alpha = image.color().has_alpha();
    let mut edge = DERIVATIVE_MAX_EDGE;
    for _ in 0..8 {
        let scaled = if image.width().max(image.height()) > edge {
            image.resize(edge, edge, FilterType::Triangle)
        } else {
            image.clone()
        };
        let attempts: &[u8] = if alpha { &[0] } else { &[85, 70, 55] };
        for &quality in attempts {
            let out_media = if alpha {
                MediaType::Png
            } else {
                MediaType::Jpeg
            };
            if let Some(bytes) = encode(&scaled, out_media, quality, DERIVATIVE_MAX_BYTES)? {
                return Ok(ModelDerivative {
                    media: out_media,
                    bytes,
                    width: scaled.width(),
                    height: scaled.height(),
                });
            }
        }
        edge = (edge * 3 / 4).max(1);
    }
    Err(too_large("no model derivative of this image fits 1 MiB"))
}
