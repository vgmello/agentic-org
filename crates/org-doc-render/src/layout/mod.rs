//! Paginated document layout engine: multi-page table header repetition, shaded code blocks,
//! blockquotes, lists, raster images, `@page` headers/footers, and fail-fast `max_pages` +
//! cooperative deadline enforcement.

mod text;

use std::time::Instant;

use crate::css::{MarginBoxSpec, PageStyle, Rgb, TextAlign};
use crate::doc::{Block, Document, InlineSpan, TableRow};
use crate::model::RenderError;

use text::{
    decode_bounded_image, heading_size, measure_row, measure_text, wrap_mono, wrap_spans,
    MeasuredRow, PlacedRun,
};

pub(crate) use text::{EmbeddedImage, FontKind};

/// One placed drawing primitive on a PDF page (coordinates in PDF points from bottom-left).
#[derive(Clone, Debug)]
pub(crate) enum DrawOp {
    Text {
        x: f32,
        y: f32,
        size: f32,
        font: FontKind,
        color: Rgb,
        text: String,
    },
    Rect {
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        fill: Option<Rgb>,
        stroke: Option<(Rgb, f32)>,
    },
    Line {
        x1: f32,
        y1: f32,
        x2: f32,
        y2: f32,
        color: Rgb,
        width: f32,
    },
    Image {
        image_index: usize,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
    },
}

/// One laid-out page ready for PDF stream emission.
#[derive(Clone, Debug)]
pub(crate) struct PagePlan {
    pub ops: Vec<DrawOp>,
}

/// Complete paginated plan ready for `pdf-writer`.
#[derive(Clone, Debug)]
pub(crate) struct LayoutPlan {
    pub title: Option<String>,
    pub page: PageStyle,
    pub pages: Vec<PagePlan>,
    pub images: Vec<EmbeddedImage>,
}

/// Paginates `doc` into pages while enforcing `max_pages` and optional `deadline`.
pub(crate) fn paginate(
    doc: Document,
    max_pages: u32,
    deadline: Option<Instant>,
) -> Result<LayoutPlan, RenderError> {
    let mut ctx = Paginator::new(doc.page.clone(), max_pages, deadline);
    for block in &doc.blocks {
        ctx.place_block(block, 0.0)?;
    }
    if ctx.pages.is_empty() {
        ctx.pages.push(PagePlan { ops: Vec::new() });
    }
    ctx.apply_running_headers_and_footers();
    Ok(LayoutPlan {
        title: doc.title,
        page: ctx.page,
        pages: ctx.pages,
        images: ctx.images,
    })
}

struct Paginator {
    page: PageStyle,
    max_pages: u32,
    deadline: Option<Instant>,
    pages: Vec<PagePlan>,
    images: Vec<EmbeddedImage>,
    cursor_y: f32,
}

impl Paginator {
    fn new(page: PageStyle, max_pages: u32, deadline: Option<Instant>) -> Self {
        let start_y = page.height_pt - page.margin_top;
        Self {
            page,
            max_pages: max_pages.max(1),
            deadline,
            pages: vec![PagePlan { ops: Vec::new() }],
            images: Vec::new(),
            cursor_y: start_y,
        }
    }

    fn content_left(&self) -> f32 {
        self.page.margin_left
    }

    fn content_width(&self) -> f32 {
        (self.page.width_pt - self.page.margin_left - self.page.margin_right).max(72.0)
    }

    fn content_bottom(&self) -> f32 {
        self.page.margin_bottom
    }

    fn check_deadline(&self) -> Result<(), RenderError> {
        if let Some(limit) = self.deadline {
            if Instant::now() >= limit {
                return Err(RenderError::Timeout { timeout_ms: 0 });
            }
        }
        Ok(())
    }

    fn new_page(&mut self) -> Result<(), RenderError> {
        self.check_deadline()?;
        let next = self.pages.len() as u32 + 1;
        if next > self.max_pages {
            return Err(RenderError::PageLimitExceeded {
                actual: next,
                max: self.max_pages,
            });
        }
        self.pages.push(PagePlan { ops: Vec::new() });
        self.cursor_y = self.page.height_pt - self.page.margin_top;
        Ok(())
    }

    fn ensure_space(&mut self, height: f32) -> Result<(), RenderError> {
        if self.cursor_y - height < self.content_bottom()
            && self.cursor_y < self.page.height_pt - self.page.margin_top - 1.0
        {
            self.new_page()?;
        }
        Ok(())
    }

    fn push_op(&mut self, op: DrawOp) {
        if let Some(page) = self.pages.last_mut() {
            page.ops.push(op);
        }
    }

    fn place_block(&mut self, block: &Block, indent: f32) -> Result<(), RenderError> {
        self.check_deadline()?;
        match block {
            Block::Heading {
                level,
                spans,
                style,
            } => {
                let size = style.font_size.unwrap_or_else(|| heading_size(*level));
                let lead = size * 1.35;
                self.cursor_y -= style
                    .margin_top
                    .unwrap_or(if *level <= 2 { 14.0 } else { 10.0 });
                self.ensure_space(lead * 1.5)?;
                let avail = self.content_width() - indent;
                let lines = wrap_spans(spans, avail, size, true, style.color);
                self.emit_lines(
                    &lines,
                    self.content_left() + indent,
                    avail,
                    size,
                    lead,
                    style.align.unwrap_or(TextAlign::Left),
                )?;
                self.cursor_y -= style.margin_bottom.unwrap_or(6.0);
            }
            Block::Paragraph { spans, style } => {
                let size = style.font_size.unwrap_or(10.5);
                let lead = size * 1.42;
                self.cursor_y -= style.margin_top.unwrap_or(0.0);
                let avail = self.content_width() - indent;
                let lines =
                    wrap_spans(spans, avail, size, style.bold.unwrap_or(false), style.color);
                self.emit_lines(
                    &lines,
                    self.content_left() + indent,
                    avail,
                    size,
                    lead,
                    style.align.unwrap_or(TextAlign::Left),
                )?;
                self.cursor_y -= style.margin_bottom.unwrap_or(7.5);
            }
            Block::Code { text, style } => {
                self.place_code_block(
                    text,
                    indent,
                    style.font_size.unwrap_or(8.8),
                    style.background.unwrap_or(Rgb::CODE_BG),
                )?;
            }
            Block::Quote { blocks } => self.place_blockquote(blocks, indent)?,
            Block::List {
                ordered,
                start,
                items,
            } => self.place_list(*ordered, *start, items, indent)?,
            Block::Table { headers, rows } => self.place_table(headers, rows, indent)?,
            Block::Image {
                bytes,
                alt,
                width_pt,
                height_pt,
            } => self.place_image(bytes, alt, *width_pt, *height_pt, indent)?,
            Block::HorizontalRule => {
                self.ensure_space(12.0)?;
                let y = self.cursor_y - 6.0;
                self.push_op(DrawOp::Line {
                    x1: self.content_left() + indent,
                    y1: y,
                    x2: self.content_left() + self.content_width(),
                    y2: y,
                    color: Rgb::BORDER,
                    width: 0.75,
                });
                self.cursor_y -= 12.0;
            }
            Block::PageBreak => self.new_page()?,
        }
        Ok(())
    }

    fn place_blockquote(&mut self, blocks: &[Block], indent: f32) -> Result<(), RenderError> {
        let bar_x = self.content_left() + indent + 4.0;
        for inner in blocks {
            let start_page = self.pages.len() - 1;
            let start_y = self.cursor_y;
            self.place_block(inner, indent + 14.0)?;
            let end_page = self.pages.len() - 1;
            let end_y = self.cursor_y.max(self.content_bottom());
            if start_page == end_page && start_y > end_y + 2.0 {
                self.push_op(DrawOp::Line {
                    x1: bar_x,
                    y1: start_y,
                    x2: bar_x,
                    y2: end_y + 4.0,
                    color: Rgb::BORDER,
                    width: 2.0,
                });
            }
        }
        Ok(())
    }

    fn emit_lines(
        &mut self,
        lines: &[Vec<PlacedRun>],
        left: f32,
        avail_w: f32,
        size: f32,
        lead: f32,
        align: TextAlign,
    ) -> Result<(), RenderError> {
        for line in lines {
            self.ensure_space(lead)?;
            let line_w: f32 = line.iter().map(|r| r.width).sum();
            let mut x = match align {
                TextAlign::Left => left,
                TextAlign::Center => left + ((avail_w - line_w) * 0.5).max(0.0),
                TextAlign::Right => left + (avail_w - line_w).max(0.0),
            };
            let baseline = self.cursor_y - size;
            for run in line {
                if !run.text.is_empty() {
                    self.push_op(DrawOp::Text {
                        x,
                        y: baseline,
                        size,
                        font: run.font,
                        color: run.color,
                        text: run.text.clone(),
                    });
                }
                x += run.width;
            }
            self.cursor_y -= lead;
        }
        Ok(())
    }

    fn place_code_block(
        &mut self,
        text: &str,
        indent: f32,
        size: f32,
        bg: Rgb,
    ) -> Result<(), RenderError> {
        let lead = size * 1.35;
        let pad = 7.0;
        let left = self.content_left() + indent;
        let width = (self.content_width() - indent).max(60.0);
        let max_chars = ((width - pad * 2.0) / (size * 0.60)).floor().max(12.0) as usize;
        let mut wrapped = Vec::new();
        for raw_line in text.trim_end_matches('\n').lines() {
            if raw_line.is_empty() {
                wrapped.push(String::new());
            } else {
                wrapped.extend(wrap_mono(raw_line, max_chars));
            }
        }
        if wrapped.is_empty() {
            return Ok(());
        }
        let mut idx = 0usize;
        while idx < wrapped.len() {
            let min_h = lead + pad * 2.0;
            self.ensure_space(min_h)?;
            let avail_h = (self.cursor_y - self.content_bottom()).max(min_h);
            let fits = ((avail_h - pad * 2.0) / lead).floor().max(1.0) as usize;
            let end = (idx + fits).min(wrapped.len());
            let box_h = (end - idx) as f32 * lead + pad * 2.0;
            let box_y = self.cursor_y - box_h;
            self.push_op(DrawOp::Rect {
                x: left,
                y: box_y,
                w: width,
                h: box_h,
                fill: Some(bg),
                stroke: Some((Rgb::BORDER, 0.5)),
            });
            let mut y = self.cursor_y - pad - size;
            for line in &wrapped[idx..end] {
                if !line.is_empty() {
                    self.push_op(DrawOp::Text {
                        x: left + pad,
                        y,
                        size,
                        font: FontKind::Mono,
                        color: Rgb::BLACK,
                        text: line.clone(),
                    });
                }
                y -= lead;
            }
            self.cursor_y = box_y - 6.0;
            idx = end;
            if idx < wrapped.len() {
                self.new_page()?;
            }
        }
        Ok(())
    }

    fn place_list(
        &mut self,
        ordered: bool,
        start: u64,
        items: &[Vec<Block>],
        indent: f32,
    ) -> Result<(), RenderError> {
        let marker_w = if ordered { 22.0 } else { 14.0 };
        for (idx, item) in items.iter().enumerate() {
            self.ensure_space(15.0)?;
            let marker = if ordered {
                format!("{}.", start + idx as u64)
            } else {
                "\u{2022}".to_owned()
            };
            self.push_op(DrawOp::Text {
                x: self.content_left() + indent + 2.0,
                y: self.cursor_y - 10.5,
                size: 10.5,
                font: FontKind::Bold,
                color: Rgb::BLACK,
                text: marker,
            });
            for block in item {
                self.place_block(block, indent + marker_w)?;
            }
        }
        self.cursor_y -= 4.0;
        Ok(())
    }

    fn place_table(
        &mut self,
        headers: &[TableRow],
        rows: &[TableRow],
        indent: f32,
    ) -> Result<(), RenderError> {
        let cols = headers
            .iter()
            .chain(rows.iter())
            .map(|r| r.cells.len())
            .max()
            .unwrap_or(0);
        if cols == 0 {
            return Ok(());
        }
        let left = self.content_left() + indent;
        let table_w = (self.content_width() - indent).max(72.0);
        let col_w = table_w / cols as f32;
        let font_size = 9.2;
        let lead = font_size * 1.35;
        let pad = 4.5;

        let measured_headers: Vec<MeasuredRow> = headers
            .iter()
            .map(|r| measure_row(r, cols, col_w, pad, font_size, lead, true))
            .collect();
        let headers_h: f32 = measured_headers.iter().map(|r| r.height).sum();

        if !measured_headers.is_empty() {
            self.ensure_space(headers_h + lead + pad * 2.0)?;
            for hrow in &measured_headers {
                self.draw_table_row(hrow, left, col_w, pad, font_size, lead);
            }
        }

        for row in rows {
            let measured = measure_row(row, cols, col_w, pad, font_size, lead, false);
            if self.cursor_y - measured.height < self.content_bottom()
                && self.cursor_y < self.page.height_pt - self.page.margin_top - 1.0
            {
                self.new_page()?;
                for hrow in &measured_headers {
                    self.draw_table_row(hrow, left, col_w, pad, font_size, lead);
                }
            }
            self.draw_table_row(&measured, left, col_w, pad, font_size, lead);
        }
        self.cursor_y -= 8.0;
        Ok(())
    }

    fn draw_table_row(
        &mut self,
        row: &MeasuredRow,
        left: f32,
        col_w: f32,
        pad: f32,
        font_size: f32,
        lead: f32,
    ) {
        let row_y = self.cursor_y - row.height;
        for (col_idx, cell_lines) in row.cells.iter().enumerate() {
            let cx = left + col_idx as f32 * col_w;
            let fill = row.is_header.then_some(Rgb::HEADER_BG);
            self.push_op(DrawOp::Rect {
                x: cx,
                y: row_y,
                w: col_w,
                h: row.height,
                fill,
                stroke: Some((Rgb::BORDER, 0.5)),
            });
            let mut ty = self.cursor_y - pad - font_size;
            for line in cell_lines {
                let mut rx = cx + pad;
                for run in line {
                    if !run.text.is_empty() {
                        self.push_op(DrawOp::Text {
                            x: rx,
                            y: ty,
                            size: font_size,
                            font: run.font,
                            color: run.color,
                            text: run.text.clone(),
                        });
                    }
                    rx += run.width;
                }
                ty -= lead;
            }
        }
        self.cursor_y = row_y;
    }

    fn place_image(
        &mut self,
        bytes: &[u8],
        alt: &str,
        width_pt: Option<f32>,
        height_pt: Option<f32>,
        indent: f32,
    ) -> Result<(), RenderError> {
        let decoded = decode_bounded_image(bytes)?;
        let nat_w = decoded.width as f32;
        let nat_h = decoded.height as f32;
        let max_w = (self.content_width() - indent).max(48.0);
        let max_h = (self.page.height_pt - self.page.margin_top - self.page.margin_bottom) * 0.65;

        let (mut draw_w, mut draw_h) = match (width_pt, height_pt) {
            (Some(w), Some(h)) => (w, h),
            (Some(w), None) => (w, w * (nat_h / nat_w.max(1.0))),
            (None, Some(h)) => (h * (nat_w / nat_h.max(1.0)), h),
            (None, None) => (nat_w * 0.75, nat_h * 0.75),
        };
        let scale = (max_w / draw_w.max(1.0))
            .min(max_h / draw_h.max(1.0))
            .min(1.0);
        draw_w *= scale;
        draw_h *= scale;

        self.ensure_space(draw_h + 10.0)?;
        let img_idx = self.images.len();
        self.images.push(decoded);
        let y = self.cursor_y - draw_h;
        self.push_op(DrawOp::Image {
            image_index: img_idx,
            x: self.content_left() + indent,
            y,
            w: draw_w,
            h: draw_h,
        });
        self.cursor_y = y - 8.0;
        if !alt.trim().is_empty() {
            let cap_spans = vec![InlineSpan {
                text: alt.trim().to_owned(),
                bold: false,
                italic: true,
                code: false,
                color: Some(Rgb::MUTED),
            }];
            let lines = wrap_spans(&cap_spans, max_w, 8.5, false, Some(Rgb::MUTED));
            self.emit_lines(
                &lines,
                self.content_left() + indent,
                max_w,
                8.5,
                11.5,
                TextAlign::Left,
            )?;
            self.cursor_y -= 4.0;
        }
        Ok(())
    }

    fn apply_running_headers_and_footers(&mut self) {
        let total = self.pages.len();
        let top_y =
            (self.page.height_pt - (self.page.margin_top * 0.55)).min(self.page.height_pt - 18.0);
        let bot_y = (self.page.margin_bottom * 0.45).max(16.0);
        let left = self.page.margin_left;
        let width = self.content_width();

        for (idx, page) in self.pages.iter_mut().enumerate() {
            let num = idx + 1;
            let boxes: [(&Option<MarginBoxSpec>, f32, TextAlign); 6] = [
                (&self.page.top_left, top_y, TextAlign::Left),
                (&self.page.top_center, top_y, TextAlign::Center),
                (&self.page.top_right, top_y, TextAlign::Right),
                (&self.page.bottom_left, bot_y, TextAlign::Left),
                (&self.page.bottom_center, bot_y, TextAlign::Center),
                (&self.page.bottom_right, bot_y, TextAlign::Right),
            ];
            for (spec_opt, y, align) in boxes {
                let Some(spec) = spec_opt else {
                    continue;
                };
                let text = spec
                    .content
                    .replace("{page}", &num.to_string())
                    .replace("{pages}", &total.to_string());
                let size = spec.font_size.unwrap_or(8.5);
                let w = measure_text(&text, FontKind::Regular, size);
                let x = match align {
                    TextAlign::Left => left,
                    TextAlign::Center => left + ((width - w) * 0.5).max(0.0),
                    TextAlign::Right => left + (width - w).max(0.0),
                };
                page.ops.push(DrawOp::Text {
                    x,
                    y,
                    size,
                    font: FontKind::Regular,
                    color: spec.color.unwrap_or(Rgb::MUTED),
                    text,
                });
            }
        }
    }
}
