//! Image attachments: decoded under strict limits and encoded again, so the office keeps only
//! pixels. EXIF (GPS included), XMP, ICC profiles, comments, trailing data and every other
//! block an uploader's file carried are gone, and the EXIF orientation is applied to the
//! pixels, before the content hash is taken.
//!
//! - At most [`MAX_DIMENSION`] pixels on a side and [`MAX_PIXELS`] in all, checked from the
//!   header before any pixel is decoded.
//! - Animations (GIF, WebP) at most [`MAX_FRAMES`] frames and [`MAX_ANIMATION_PIXELS`]
//!   decoded pixels in all, streamed frame by frame into a GIF so only one frame is held.
//! - PNG stays PNG, JPEG stays JPEG, a still WebP stays (lossless) WebP and GIF stays GIF; an
//!   animated WebP becomes an animated GIF. A still image whose re-encoding would exceed the
//!   10 MiB cap is kept as JPEG when it has no transparency, and refused otherwise.
//!
//! [`model_derivative`] makes the bounded copy a model reads: at most
//! [`DERIVATIVE_MAX_EDGE`] pixels on its longest side and [`DERIVATIVE_MAX_BYTES`] bytes.

use std::io::Cursor;

use image::codecs::gif::{GifDecoder, GifEncoder, Repeat};
use image::codecs::jpeg::JpegEncoder;
use image::codecs::png::PngEncoder;
use image::codecs::webp::{WebPDecoder, WebPEncoder};
use image::error::{ImageError, LimitErrorKind};
use image::imageops::FilterType;
use image::metadata::Orientation;
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
    let cap = ATTACHMENT_IMAGE_MAX_BYTES as usize;
    let mut kept = (media, encode(&image, media, JPEG_QUALITY)?);
    if kept.1.len() > cap && !image.color().has_alpha() && media != MediaType::Jpeg {
        kept = (
            MediaType::Jpeg,
            encode(&image, MediaType::Jpeg, JPEG_QUALITY)?,
        );
    }
    if kept.1.len() > cap {
        return Err(too_large(
            "the image is over 10 MiB once its metadata is removed",
        ));
    }
    Ok(ValidatedFile {
        media: kept.0,
        bytes: kept.1,
        width: Some(image.width()),
        height: Some(image.height()),
        page_count: None,
    })
}

/// `image` encoded as `media`, with no metadata block of any kind.
fn encode(image: &DynamicImage, media: MediaType, quality: u8) -> Result<Vec<u8>, ApiError> {
    let mut out = Vec::new();
    match media {
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
    }
    .map_err(encode_error)?;
    Ok(out)
}

/// Streams every frame of an animation (a one-frame GIF included) into a new GIF.
fn animation<'a, D>(mut decoder: D) -> Result<ValidatedFile, ApiError>
where
    D: ImageDecoder + AnimationDecoder<'a>,
{
    let (width, height) = decoder.dimensions();
    check_dimensions(width, height)?;
    decoder.set_limits(limits()).map_err(decode_error)?;
    let bytes = reencode_frames(decoder.into_frames(), width, height)?;
    if bytes.len() as u64 > ATTACHMENT_IMAGE_MAX_BYTES {
        return Err(too_large(
            "the animation is over 10 MiB once its metadata is removed",
        ));
    }
    Ok(ValidatedFile {
        media: MediaType::Gif,
        bytes,
        width: Some(width),
        height: Some(height),
        page_count: None,
    })
}

fn reencode_frames(frames: Frames<'_>, width: u32, height: u32) -> Result<Vec<u8>, ApiError> {
    let canvas = u64::from(width) * u64::from(height);
    let mut out = Vec::new();
    let mut encoder = GifEncoder::new_with_speed(&mut out, GIF_SPEED);
    let mut frames = frames.peekable();
    let mut count: u64 = 0;
    while let Some(frame) = frames.next() {
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
        if count == 1 && frames.peek().is_some() {
            encoder.set_repeat(Repeat::Infinite).map_err(encode_error)?;
        }
        encoder.encode_frame(frame).map_err(encode_error)?;
    }
    if count == 0 {
        return Err(corrupt("the image has no frames"));
    }
    drop(encoder);
    Ok(out)
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
            let (out_media, bytes) = if alpha {
                (MediaType::Png, encode(&scaled, MediaType::Png, 0)?)
            } else {
                (MediaType::Jpeg, encode(&scaled, MediaType::Jpeg, quality)?)
            };
            if bytes.len() <= DERIVATIVE_MAX_BYTES {
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
