//! Pure-Rust PDF 1.7 emission (`pdf-writer` + `flate2`) and independent PDF page counting.

use std::io::{Read as _, Write as _};

use flate2::read::ZlibDecoder;
use flate2::write::ZlibEncoder;
use flate2::Compression;
use pdf_writer::types::FontFlags;
use pdf_writer::{Content, Filter, Finish, Name, Pdf, Rect, Ref, Str, TextStr};

use crate::layout::{DrawOp, EmbeddedImage, FontKind, LayoutPlan};
use crate::model::{RenderError, RenderedPdf};

const INFLATE_BUDGET: u64 = 32 * 1024 * 1024;
const MAX_OBJECT_STREAMS: usize = 2_048;
const TAIL_BYTES: usize = 2_048;

/// Emits a valid PDF 1.7 document from a paginated [`LayoutPlan`].
pub(crate) fn emit_pdf(plan: &LayoutPlan) -> Result<RenderedPdf, RenderError> {
    let mut pdf = Pdf::new();
    let mut next_id = 1i32;
    let mut alloc = || {
        let id = Ref::new(next_id);
        next_id += 1;
        id
    };

    let catalog_id = alloc();
    let pages_id = alloc();
    let info_id = alloc();
    let font_ids = [alloc(), alloc(), alloc(), alloc(), alloc(), alloc()];
    let font_specs: [(FontKind, &[u8]); 6] = [
        (FontKind::Regular, b"Helvetica"),
        (FontKind::Bold, b"Helvetica-Bold"),
        (FontKind::Italic, b"Helvetica-Oblique"),
        (FontKind::BoldItalic, b"Helvetica-BoldOblique"),
        (FontKind::Mono, b"Courier"),
        (FontKind::MonoBold, b"Courier-Bold"),
    ];

    let mut image_refs = Vec::with_capacity(plan.images.len());
    for img in &plan.images {
        let img_id = alloc();
        let smask_id = img.alpha.as_ref().map(|_| alloc());
        image_refs.push((img_id, smask_id));
    }

    let mut page_ids = Vec::with_capacity(plan.pages.len());
    let mut content_ids = Vec::with_capacity(plan.pages.len());
    for _ in &plan.pages {
        page_ids.push(alloc());
        content_ids.push(alloc());
    }

    pdf.catalog(catalog_id).pages(pages_id);
    pdf.pages(pages_id)
        .kids(page_ids.iter().copied())
        .count(i32::try_from(page_ids.len()).unwrap_or(i32::MAX));

    let mut info = pdf.document_info(info_id);
    if let Some(title) = plan.title.as_deref() {
        info.title(TextStr(title));
    }
    info.producer(TextStr("Agentic Org org-doc-render"));
    info.finish();

    for (idx, (_, base_font)) in font_specs.iter().enumerate() {
        let mut font = pdf.type1_font(font_ids[idx]);
        font.base_font(Name(base_font));
        font.encoding_predefined(Name(b"WinAnsiEncoding"));
        let _ = FontFlags::NON_SYMBOLIC;
        font.finish();
    }

    for (img, (img_id, smask_id)) in plan.images.iter().zip(&image_refs) {
        write_image_xobject(&mut pdf, *img_id, *smask_id, img)?;
    }

    let media_box = Rect::new(0.0, 0.0, plan.page.width_pt, plan.page.height_pt);
    for (idx, page_plan) in plan.pages.iter().enumerate() {
        let mut page = pdf.page(page_ids[idx]);
        page.parent(pages_id);
        page.media_box(media_box);
        page.contents(content_ids[idx]);
        {
            let mut resources = page.resources();
            {
                let mut fonts = resources.fonts();
                for (f_idx, (kind, _)) in font_specs.iter().enumerate() {
                    fonts.pair(Name(kind.pdf_name()), font_ids[f_idx]);
                }
            }
            if !image_refs.is_empty() {
                let mut xobjs = resources.x_objects();
                for (img_idx, (img_id, _)) in image_refs.iter().enumerate() {
                    let name = format!("Im{}", img_idx + 1);
                    xobjs.pair(Name(name.as_bytes()), *img_id);
                }
            }
        }
        page.finish();

        let stream_bytes = build_page_content(&page_plan.ops);
        let compressed = compress_zlib(&stream_bytes)?;
        let mut stream = pdf.stream(content_ids[idx], &compressed);
        stream.filter(Filter::FlateDecode);
        stream.finish();
    }

    let bytes = pdf.finish();
    let page_count = u32::try_from(plan.pages.len()).unwrap_or(u32::MAX);
    let byte_count = bytes.len() as u64;
    Ok(RenderedPdf {
        bytes,
        page_count,
        byte_count,
    })
}

fn write_image_xobject(
    pdf: &mut Pdf,
    img_id: Ref,
    smask_id: Option<Ref>,
    img: &EmbeddedImage,
) -> Result<(), RenderError> {
    let encoded_rgb = compress_zlib(&img.rgb)?;
    let mut xobj = pdf.image_xobject(img_id, &encoded_rgb);
    xobj.filter(Filter::FlateDecode);
    xobj.width(i32::try_from(img.width).unwrap_or(1));
    xobj.height(i32::try_from(img.height).unwrap_or(1));
    xobj.color_space().device_rgb();
    xobj.bits_per_component(8);
    if let Some(mask_ref) = smask_id {
        xobj.s_mask(mask_ref);
    }
    xobj.finish();

    if let (Some(mask_ref), Some(alpha)) = (smask_id, img.alpha.as_ref()) {
        let encoded_alpha = compress_zlib(alpha)?;
        let mut mask = pdf.image_xobject(mask_ref, &encoded_alpha);
        mask.filter(Filter::FlateDecode);
        mask.width(i32::try_from(img.width).unwrap_or(1));
        mask.height(i32::try_from(img.height).unwrap_or(1));
        mask.color_space().device_gray();
        mask.bits_per_component(8);
        mask.finish();
    }
    Ok(())
}

fn build_page_content(ops: &[DrawOp]) -> Vec<u8> {
    let mut content = Content::new();
    for op in ops {
        match op {
            DrawOp::Rect {
                x,
                y,
                w,
                h,
                fill,
                stroke,
            } => {
                content.save_state();
                if let Some(c) = fill {
                    content.set_fill_rgb(c.r, c.g, c.b);
                }
                if let Some((c, width)) = stroke {
                    content.set_stroke_rgb(c.r, c.g, c.b);
                    content.set_line_width(*width);
                }
                content.rect(*x, *y, *w, *h);
                match (fill.is_some(), stroke.is_some()) {
                    (true, true) => content.fill_nonzero_and_stroke(),
                    (true, false) => content.fill_nonzero(),
                    (false, true) => content.stroke(),
                    (false, false) => content.end_path(),
                };
                content.restore_state();
            }
            DrawOp::Line {
                x1,
                y1,
                x2,
                y2,
                color,
                width,
            } => {
                content.save_state();
                content.set_stroke_rgb(color.r, color.g, color.b);
                content.set_line_width(*width);
                content.move_to(*x1, *y1);
                content.line_to(*x2, *y2);
                content.stroke();
                content.restore_state();
            }
            DrawOp::Text {
                x,
                y,
                size,
                font,
                color,
                text,
            } => {
                let encoded = encode_winansi(text);
                content.save_state();
                content.set_fill_rgb(color.r, color.g, color.b);
                content.begin_text();
                content.set_font(Name(font.pdf_name()), *size);
                content.next_line(*x, *y);
                content.show(Str(&encoded));
                content.end_text();
                content.restore_state();
            }
            DrawOp::Image {
                image_index,
                x,
                y,
                w,
                h,
            } => {
                let name = format!("Im{}", image_index + 1);
                content.save_state();
                content.transform([*w, 0.0, 0.0, *h, *x, *y]);
                content.x_object(Name(name.as_bytes()));
                content.restore_state();
            }
        }
    }
    content.finish().to_vec()
}

fn compress_zlib(raw: &[u8]) -> Result<Vec<u8>, RenderError> {
    let mut enc = ZlibEncoder::new(Vec::new(), Compression::default());
    enc.write_all(raw)
        .map_err(|e| RenderError::RenderFailed(format!("zlib encode failed: {e}")))?;
    enc.finish()
        .map_err(|e| RenderError::RenderFailed(format!("zlib finish failed: {e}")))
}

/// Maps UTF-8 text safely into PDF WinAnsiEncoding (Windows-1252) with clean ASCII fallbacks.
fn encode_winansi(text: &str) -> Vec<u8> {
    let mut out = Vec::with_capacity(text.len());
    for ch in text.chars() {
        match ch {
            '\u{0020}'..='\u{007E}' | '\u{00A0}'..='\u{00FF}' => out.push(ch as u8),
            '\u{20AC}' => out.push(0x80),
            '\u{2018}' => out.push(0x91),
            '\u{2019}' => out.push(0x92),
            '\u{201C}' => out.push(0x93),
            '\u{201D}' => out.push(0x94),
            '\u{2022}' => out.push(0x95),
            '\u{2013}' => out.push(0x96),
            '\u{2014}' => out.push(0x97),
            '\u{2026}' => out.push(0x85),
            '\u{2192}' => out.extend_from_slice(b"->"),
            '\u{2190}' => out.extend_from_slice(b"<-"),
            '\u{2264}' => out.extend_from_slice(b"<="),
            '\u{2265}' => out.extend_from_slice(b">="),
            '\t' => out.extend_from_slice(b"    "),
            c if c.is_control() => {}
            _ => out.push(b'?'),
        }
    }
    out
}

/// Validates `%PDF-` header, `startxref`/`%%EOF` trailer, and returns the PDF page count.
pub fn count_pdf_pages(bytes: &[u8]) -> Result<u32, RenderError> {
    if !bytes.starts_with(b"%PDF-") {
        return Err(RenderError::InvalidPdf("missing %PDF- magic header"));
    }
    let version = bytes.get(5..8).unwrap_or_default();
    if !(version.len() == 3
        && version[0].is_ascii_digit()
        && version[1] == b'.'
        && version[2].is_ascii_digit())
    {
        return Err(RenderError::InvalidPdf("missing PDF version after %PDF-"));
    }
    check_pdf_trailer(bytes)?;

    let mut counts = PageCounts::default();
    counts.scan(bytes);
    if counts.pages_nodes == 0 && counts.leaves == 0 {
        counts.scan_object_streams(bytes)?;
    }
    let pages = counts.total();
    if pages == 0 {
        return Err(RenderError::InvalidPdf(
            "PDF contains no readable page tree",
        ));
    }
    Ok(pages)
}

fn check_pdf_trailer(bytes: &[u8]) -> Result<(), RenderError> {
    let start = bytes.len().saturating_sub(TAIL_BYTES);
    let tail = &bytes[start..];
    let trimmed_len = tail
        .iter()
        .rposition(|b| !b.is_ascii_whitespace() && *b != 0)
        .map_or(0, |at| at + 1);
    let trimmed = &tail[..trimmed_len];
    if !trimmed.ends_with(b"%%EOF") {
        return Err(RenderError::InvalidPdf("PDF does not end with %%EOF"));
    }
    let Some(at) = find_last(trimmed, b"startxref") else {
        return Err(RenderError::InvalidPdf("PDF is missing startxref"));
    };
    let digits: Vec<u8> = trimmed[at + b"startxref".len()..]
        .iter()
        .copied()
        .skip_while(u8::is_ascii_whitespace)
        .take_while(u8::is_ascii_digit)
        .take(20)
        .collect();
    let offset = std::str::from_utf8(&digits)
        .ok()
        .and_then(|t| t.parse::<u64>().ok())
        .ok_or(RenderError::InvalidPdf(
            "startxref has no valid byte offset",
        ))?;
    if offset == 0 || offset >= bytes.len() as u64 {
        return Err(RenderError::InvalidPdf(
            "startxref offset points outside PDF",
        ));
    }
    Ok(())
}

#[derive(Default)]
struct PageCounts {
    max_pages_count: u64,
    pages_nodes: usize,
    leaves: u64,
}

impl PageCounts {
    fn total(&self) -> u32 {
        let raw = if self.pages_nodes > 0 {
            self.max_pages_count
        } else {
            self.leaves
        };
        u32::try_from(raw).unwrap_or(u32::MAX)
    }

    fn scan(&mut self, data: &[u8]) {
        let mut from = 0;
        while let Some(found) = find(&data[from..], b"/Type") {
            let at = from + found + b"/Type".len();
            from = at;
            let rest = skip_ws(&data[at..]);
            if !rest.starts_with(b"/Page") {
                continue;
            }
            let after = &rest[b"/Page".len()..];
            if after.first() == Some(&b's') && !after.get(1).is_some_and(|b| is_name_char(*b)) {
                self.pages_nodes += 1;
                if let Some(count) = count_near(data, at) {
                    self.max_pages_count = self.max_pages_count.max(count);
                }
            } else if !after.first().is_some_and(|b| is_name_char(*b)) {
                self.leaves += 1;
            }
        }
    }

    fn scan_object_streams(&mut self, data: &[u8]) -> Result<(), RenderError> {
        let ends = positions(data, b"endstream");
        let mut budget = INFLATE_BUDGET;
        let mut streams = 0usize;
        let mut from = 0usize;
        while let Some(found) = find(&data[from..], b"/ObjStm") {
            let at = from + found;
            from = at + b"/ObjStm".len();
            streams += 1;
            if streams > MAX_OBJECT_STREAMS {
                return Err(RenderError::InvalidPdf("too many PDF object streams"));
            }
            let Some((start, end)) = stream_body(data, from, &ends) else {
                continue;
            };
            from = from.max(end);
            let mut inflated = Vec::new();
            let mut dec = ZlibDecoder::new(&data[start..end]).take(budget + 1);
            let _ = dec.read_to_end(&mut inflated);
            if inflated.len() as u64 > budget {
                return Err(RenderError::InvalidPdf(
                    "PDF object streams exceed inflate budget",
                ));
            }
            budget -= inflated.len() as u64;
            self.scan(&inflated);
        }
        Ok(())
    }
}

fn stream_body(data: &[u8], from: usize, ends: &[usize]) -> Option<(usize, usize)> {
    let win_end = data.len().min(from.saturating_add(4_096));
    let start = from
        .checked_add(find(data.get(from..win_end)?, b"stream")?)?
        .checked_add(b"stream".len())?;
    let start = match data.get(start..start.saturating_add(2)) {
        Some(b"\r\n") => start + 2,
        _ if matches!(data.get(start), Some(b'\n' | b'\r')) => start + 1,
        _ => start,
    };
    let end = *ends.get(ends.partition_point(|&e| e < start))?;
    Some((start, end))
}

fn positions(data: &[u8], needle: &[u8]) -> Vec<usize> {
    let mut out = Vec::new();
    let mut from = 0;
    while let Some(at) = find(&data[from..], needle) {
        out.push(from + at);
        from += at + needle.len();
    }
    out
}

fn count_near(data: &[u8], at: usize) -> Option<u64> {
    let open = find_last(&data[at.saturating_sub(4_096)..at], b"<<")
        .map(|f| at.saturating_sub(4_096) + f)?;
    let close = at.checked_add(find(
        &data[at..data.len().min(at.saturating_add(4_096))],
        b">>",
    )?)?;
    let dict = &data[open..close];
    let count_at = find(dict, b"/Count")? + b"/Count".len();
    let digits: Vec<u8> = skip_ws(&dict[count_at..])
        .iter()
        .copied()
        .take_while(u8::is_ascii_digit)
        .take(12)
        .collect();
    std::str::from_utf8(&digits).ok()?.parse().ok()
}

fn is_name_char(b: u8) -> bool {
    b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'.')
}

fn skip_ws(data: &[u8]) -> &[u8] {
    let start = data
        .iter()
        .position(|b| !b.is_ascii_whitespace())
        .unwrap_or(data.len());
    &data[start..]
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}

fn find_last(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).rposition(|w| w == needle)
}
