//! Text measurement, word wrapping, table row sizing, and bounded image decoding.

use crate::css::Rgb;
use crate::doc::{InlineSpan, TableRow};
use crate::model::{RenderError, MAX_IMAGE_DIMENSION};

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
    let mut lines: Vec<Vec<PlacedRun>> = vec![Vec::new()];
    let mut line_w = 0.0_f32;
    let avail = max_w.max(24.0);

    for span in spans {
        let font = select_font(span.bold || force_bold, span.italic, span.code);
        let color = span.color.or(default_color).unwrap_or(Rgb::BLACK);
        for (seg_idx, segment) in span.text.split('\n').enumerate() {
            if seg_idx > 0 {
                lines.push(Vec::new());
                line_w = 0.0;
            }
            for token in split_words_keeping_spaces(segment) {
                let w = measure_text(&token, font, size);
                if line_w > 0.0 && line_w + w > avail {
                    lines.push(Vec::new());
                    line_w = 0.0;
                    if token.trim().is_empty() {
                        continue;
                    }
                }
                if w > avail && token.chars().count() > 1 {
                    for chunk in break_long_token(&token, font, size, avail) {
                        let cw = measure_text(&chunk, font, size);
                        if line_w > 0.0 && line_w + cw > avail {
                            lines.push(Vec::new());
                            line_w = 0.0;
                        }
                        if let Some(cur) = lines.last_mut() {
                            cur.push(PlacedRun {
                                text: chunk,
                                font,
                                color,
                                width: cw,
                            });
                        }
                        line_w += cw;
                    }
                    continue;
                }
                if let Some(cur) = lines.last_mut() {
                    cur.push(PlacedRun {
                        text: token,
                        font,
                        color,
                        width: w,
                    });
                }
                line_w += w;
            }
        }
    }
    lines
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

pub(super) fn decode_bounded_image(bytes: &[u8]) -> Result<EmbeddedImage, RenderError> {
    let reader = image::ImageReader::new(std::io::Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|e| RenderError::RenderFailed(format!("unsupported embedded image: {e}")))?;
    let (w, h) = reader
        .into_dimensions()
        .map_err(|e| RenderError::RenderFailed(format!("unreadable image dimensions: {e}")))?;
    if w == 0 || h == 0 || w > MAX_IMAGE_DIMENSION || h > MAX_IMAGE_DIMENSION {
        return Err(RenderError::ForbiddenContent {
            reason: "embedded image dimensions exceed 4096x4096 safety bound",
        });
    }
    let dyn_img = image::load_from_memory(bytes)
        .map_err(|e| RenderError::RenderFailed(format!("could not decode embedded image: {e}")))?;
    let has_alpha = dyn_img.color().has_alpha();
    let rgba = dyn_img.to_rgba8();
    let pixel_count = (w as usize).saturating_mul(h as usize);
    let mut rgb = Vec::with_capacity(pixel_count * 3);
    let mut alpha = has_alpha.then(|| Vec::with_capacity(pixel_count));
    for px in rgba.pixels() {
        rgb.extend_from_slice(&px.0[0..3]);
        if let Some(a) = alpha.as_mut() {
            a.push(px.0[3]);
        }
    }
    Ok(EmbeddedImage {
        width: w,
        height: h,
        rgb,
        alpha,
    })
}
