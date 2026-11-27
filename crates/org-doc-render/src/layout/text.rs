//! Text measurement, word wrapping, table row sizing, and bounded image decoding.

use std::io::Cursor;

use crate::css::Rgb;
use crate::doc::{InlineSpan, TableRow};
use crate::model::{RenderError, MAX_IMAGE_DECODED_BYTES, MAX_IMAGE_DIMENSION};

/// Standard PDF Base-14 font slot used during layout and emission.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum FontKind {
    Regular,
    Bold,
    Italic,
    BoldItalic,
    Mono,
    MonoBold,
}

impl FontKind {
    pub(crate) const fn pdf_name(self) -> &'static [u8] {
        match self {
            Self::Regular => b"F1",
            Self::Bold => b"F2",
            Self::Italic => b"F3",
            Self::BoldItalic => b"F4",
            Self::Mono => b"F5",
            Self::MonoBold => b"F6",
        }
    }
}

/// Decoded RGBA raster image ready to be emitted once as a PDF ImageXObject.
#[derive(Clone, Debug)]
pub(crate) struct EmbeddedImage {
    pub width: u32,
    pub height: u32,
    pub rgb: Vec<u8>,
    pub alpha: Option<Vec<u8>>,
}

impl EmbeddedImage {
    /// Conservative decoded RGBA pixel byte footprint for per-document memory budgeting.
    pub(super) fn decoded_rgba_bytes(&self) -> usize {
        (self.width as usize)
            .saturating_mul(self.height as usize)
            .saturating_mul(4)
    }
}

pub(super) struct MeasuredRow {
    pub cells: Vec<Vec<Vec<PlacedRun>>>,
    pub height: f32,
    pub is_header: bool,
}

#[derive(Clone, Debug)]
pub(super) struct PlacedRun {
    pub text: String,
    pub font: FontKind,
    pub color: Rgb,
    pub width: f32,
}

struct WrapState {
    lines: Vec<Vec<PlacedRun>>,
    line_w: f32,
    avail: f32,
}

impl WrapState {
    fn new(max_w: f32) -> Self {
        Self {
            lines: vec![Vec::new()],
            line_w: 0.0,
            avail: max_w.max(24.0),
        }
    }

    fn newline(&mut self) {
        self.lines.push(Vec::new());
        self.line_w = 0.0;
    }

    fn push_run(&mut self, text: String, font: FontKind, color: Rgb, width: f32) {
        if let Some(cur) = self.lines.last_mut() {
            cur.push(PlacedRun {
                text,
                font,
                color,
                width,
            });
        }
        self.line_w += width;
    }
}

pub(super) fn measure_row(
    row: &TableRow,
    cols: usize,
    col_w: f32,
    pad: f32,
    font_size: f32,
    lead: f32,
    is_header: bool,
) -> MeasuredRow {
    let mut cells = Vec::with_capacity(cols);
    let mut max_lines = 1usize;
    let inner_w = (col_w - pad * 2.0).max(16.0);
    for col_idx in 0..cols {
        let lines = match row.cells.get(col_idx) {
            Some(cell) => wrap_spans(
                &cell.spans,
                inner_w,
                font_size,
                is_header || cell.header,
                cell.style.color,
            ),
            None => Vec::new(),
        };
        max_lines = max_lines.max(lines.len().max(1));
        cells.push(lines);
    }
    MeasuredRow {
        cells,
        height: max_lines as f32 * lead + pad * 2.0,
        is_header,
    }
}

pub(super) fn wrap_spans(
    spans: &[InlineSpan],
    max_w: f32,
    size: f32,
    force_bold: bool,
    default_color: Option<Rgb>,
) -> Vec<Vec<PlacedRun>> {
    let mut state = WrapState::new(max_w);
    for span in spans {
        let font = select_font(span.bold || force_bold, span.italic, span.code);
        let color = span.color.or(default_color).unwrap_or(Rgb::BLACK);
        for (seg_idx, segment) in span.text.split('\n').enumerate() {
            if seg_idx > 0 {
                state.newline();
            }
            wrap_segment(&mut state, segment, font, color, size);
        }
    }
    state.lines
}

fn wrap_segment(state: &mut WrapState, segment: &str, font: FontKind, color: Rgb, size: f32) {
    for token in split_words_keeping_spaces(segment) {
        push_wrapped_token(state, token, font, color, size);
    }
}

fn push_wrapped_token(state: &mut WrapState, token: String, font: FontKind, color: Rgb, size: f32) {
    let w = measure_text(&token, font, size);
    if state.line_w > 0.0 && state.line_w + w > state.avail {
        state.newline();
        if token.trim().is_empty() {
            return;
        }
    }
    if w > state.avail && token.chars().count() > 1 {
        push_long_token_chunks(state, &token, font, color, size);
        return;
    }
    state.push_run(token, font, color, w);
}

fn push_long_token_chunks(
    state: &mut WrapState,
    token: &str,
    font: FontKind,
    color: Rgb,
    size: f32,
) {
    for chunk in break_long_token(token, font, size, state.avail) {
        let cw = measure_text(&chunk, font, size);
        if state.line_w > 0.0 && state.line_w + cw > state.avail {
            state.newline();
        }
        state.push_run(chunk, font, color, cw);
    }
}

fn split_words_keeping_spaces(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut in_space = false;
    for ch in text.chars() {
        let is_sp = ch.is_whitespace();
        if !cur.is_empty() && is_sp != in_space {
            out.push(std::mem::take(&mut cur));
        }
        cur.push(if is_sp { ' ' } else { ch });
        in_space = is_sp;
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

fn break_long_token(token: &str, font: FontKind, size: f32, avail: f32) -> Vec<String> {
    let mut chunks = Vec::new();
    let mut cur = String::new();
    let mut cur_w = 0.0_f32;
    for ch in token.chars() {
        let cw = char_width_em(ch, font) * size;
        if !cur.is_empty() && cur_w + cw > avail {
            chunks.push(std::mem::take(&mut cur));
            cur_w = 0.0;
        }
        cur.push(ch);
        cur_w += cw;
    }
    if !cur.is_empty() {
        chunks.push(cur);
    }
    chunks
}

pub(super) fn wrap_mono(line: &str, max_chars: usize) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    for ch in line.chars() {
        cur.push(ch);
        if cur.chars().count() >= max_chars {
            out.push(std::mem::take(&mut cur));
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

fn select_font(bold: bool, italic: bool, code: bool) -> FontKind {
    if code {
        if bold {
            FontKind::MonoBold
        } else {
            FontKind::Mono
        }
    } else {
        match (bold, italic) {
            (true, true) => FontKind::BoldItalic,
            (true, false) => FontKind::Bold,
            (false, true) => FontKind::Italic,
            (false, false) => FontKind::Regular,
        }
    }
}

pub(super) fn heading_size(level: u8) -> f32 {
    match level {
        1 => 20.0,
        2 => 16.0,
        3 => 13.5,
        4 => 11.8,
        5 => 10.8,
        _ => 10.0,
    }
}

pub(super) fn measure_text(text: &str, font: FontKind, size: f32) -> f32 {
    text.chars().map(|c| char_width_em(c, font)).sum::<f32>() * size
}

/// Proportional Helvetica / fixed-width Courier glyph width approximation (in ems).
fn char_width_em(ch: char, font: FontKind) -> f32 {
    if matches!(font, FontKind::Mono | FontKind::MonoBold) {
        return 0.60;
    }
    let base = match ch {
        ' ' => 0.28,
        'i' | 'l' | '!' | '|' | '.' | ',' | ':' | ';' | '\'' | '`' => 0.26,
        'f' | 'j' | 'r' | 't' | '(' | ')' | '[' | ']' | '/' | '\\' | '-' => 0.34,
        'm' | 'w' | 'M' | 'W' | '@' | '%' => 0.84,
        'A'..='Z' => 0.68,
        '0'..='9' => 0.56,
        _ => 0.53,
    };
    if matches!(font, FontKind::Bold | FontKind::BoldItalic) {
        base * 1.06
    } else {
        base
    }
}

fn check_image_dimensions(w: u32, h: u32) -> Result<(), RenderError> {
    if w == 0 || h == 0 || w > MAX_IMAGE_DIMENSION || h > MAX_IMAGE_DIMENSION {
        return Err(RenderError::ForbiddenContent {
            reason: "embedded image dimensions exceed 4096x4096 safety bound",
        });
    }
    let rgba_bytes = u64::from(w).saturating_mul(u64::from(h)).saturating_mul(4);
    if rgba_bytes > MAX_IMAGE_DECODED_BYTES as u64 {
        return Err(RenderError::ForbiddenContent {
            reason: "embedded image decoded size exceeds 4 MiB safety bound",
        });
    }
    Ok(())
}

pub(super) fn decode_bounded_image(bytes: &[u8]) -> Result<EmbeddedImage, RenderError> {
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(MAX_IMAGE_DIMENSION);
    limits.max_image_height = Some(MAX_IMAGE_DIMENSION);
    limits.max_alloc = Some(MAX_IMAGE_DECODED_BYTES as u64);

    let mut dim_reader = image::ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|e| RenderError::RenderFailed(format!("unsupported embedded image: {e}")))?;
    dim_reader.limits(limits.clone());
    let (w, h) = dim_reader.into_dimensions().map_err(map_image_err)?;
    check_image_dimensions(w, h)?;

    let mut reader = image::ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|e| RenderError::RenderFailed(format!("unsupported embedded image: {e}")))?;
    reader.limits(limits);
    let dyn_img = reader.decode().map_err(map_image_err)?;
    let (dw, dh) = (dyn_img.width(), dyn_img.height());
    check_image_dimensions(dw, dh)?;

    let has_alpha = dyn_img.color().has_alpha();
    let rgba = dyn_img.to_rgba8();
    let pixel_count = (dw as usize).saturating_mul(dh as usize);
    let mut rgb = Vec::with_capacity(pixel_count * 3);
    let mut alpha = has_alpha.then(|| Vec::with_capacity(pixel_count));
    for px in rgba.pixels() {
        rgb.extend_from_slice(&px.0[0..3]);
        if let Some(a) = alpha.as_mut() {
            a.push(px.0[3]);
        }
    }
    Ok(EmbeddedImage {
        width: dw,
        height: dh,
        rgb,
        alpha,
    })
}

fn map_image_err(err: image::ImageError) -> RenderError {
    match err {
        image::ImageError::Limits(_) => RenderError::ForbiddenContent {
            reason: "embedded image exceeds decoder memory or dimension limits",
        },
        other => RenderError::RenderFailed(format!("could not decode embedded image: {other}")),
    }
}
