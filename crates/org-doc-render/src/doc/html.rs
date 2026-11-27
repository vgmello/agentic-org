//! HTML+CSS -> [`Document`] converter backed by `scraper` (`html5ever`) with strict recursion
//! depth (`MAX_NESTING_DEPTH = 64`) and cooperative deadline enforcement.

use std::collections::BTreeMap;

use scraper::{ElementRef, Html, Node};

use crate::css::{
    parse_and_validate_css, parse_inline_style, parse_length_pt, ParsedStylesheet, Rgb, StyleProps,
};
use crate::model::{check_deadline, RenderDeadline, RenderError, ResolvedImage, MAX_NESTING_DEPTH};
use crate::sanitize::validate_image_url;

use super::{resolve_image_source, Block, Document, InlineSpan, TableCell, TableRow};

struct HtmlConvertCtx<'a> {
    sheet: &'a ParsedStylesheet,
    attachments: &'a BTreeMap<String, ResolvedImage>,
    deadline: Option<&'a RenderDeadline>,
    steps: usize,
}

impl HtmlConvertCtx<'_> {
    fn tick(&mut self) -> Result<(), RenderError> {
        self.steps = self.steps.wrapping_add(1);
        if self.steps & 0xff == 0 {
            check_deadline(self.deadline)?;
        }
        Ok(())
    }

    fn element_style(&self, el: &scraper::node::Element) -> Result<StyleProps, RenderError> {
        let classes: Vec<String> = el.classes().map(str::to_owned).collect();
        let inline = match el.attr("style") {
            Some(raw) => Some(parse_inline_style(raw, self.deadline, &|u| {
                validate_image_url(u, self.attachments).map(|_| ())
            })?),
            None => None,
        };
        Ok(self
            .sheet
            .resolve(el.name(), &classes, el.id(), inline.as_ref()))
    }
}

fn check_depth(depth: usize) -> Result<(), RenderError> {
    if depth > MAX_NESTING_DEPTH {
        return Err(RenderError::ForbiddenContent {
            reason: "HTML nesting depth exceeds maximum allowed (64)",
        });
    }
    Ok(())
}

/// Converts validated HTML + optional external CSS into a [`Document`].
pub(crate) fn html_to_document(
    html: &str,
    extra_css: Option<&str>,
    title: Option<&str>,
    attachments: &BTreeMap<String, ResolvedImage>,
    deadline: Option<&RenderDeadline>,
) -> Result<Document, RenderError> {
    check_deadline(deadline)?;
    let parsed = Html::parse_document(html);
    let mut sheet = ParsedStylesheet::default();
    let mut html_title = None;
    let mut body_el = None;

    for (idx, node) in parsed.tree.nodes().enumerate() {
        if idx & 0xff == 0 {
            check_deadline(deadline)?;
        }
        let Some(el_ref) = ElementRef::wrap(node) else {
            continue;
        };
        collect_document_head_node(
            el_ref,
            attachments,
            deadline,
            &mut sheet,
            &mut html_title,
            &mut body_el,
        )?;
    }
    if let Some(css_text) = extra_css {
        sheet.merge(parse_and_validate_css(css_text, deadline, &|u| {
            validate_image_url(u, attachments).map(|_| ())
        })?);
    }

    let root = body_el.unwrap_or_else(|| parsed.root_element());
    let mut blocks = Vec::new();
    let mut ctx = HtmlConvertCtx {
        sheet: &sheet,
        attachments,
        deadline,
        steps: 0,
    };
    collect_html_blocks(root, &mut ctx, 0, &mut blocks)?;
    Ok(Document {
        title: title.map(str::to_owned).or(html_title),
        page: sheet.page,
        blocks,
    })
}

fn collect_document_head_node<'a>(
    el_ref: ElementRef<'a>,
    attachments: &BTreeMap<String, ResolvedImage>,
    deadline: Option<&RenderDeadline>,
    sheet: &mut ParsedStylesheet,
    html_title: &mut Option<String>,
    body_el: &mut Option<ElementRef<'a>>,
) -> Result<(), RenderError> {
    let tag = el_ref.value().name().to_ascii_lowercase();
    if tag == "style" {
        let css_text: String = el_ref.text().collect();
        sheet.merge(parse_and_validate_css(&css_text, deadline, &|u| {
            validate_image_url(u, attachments).map(|_| ())
        })?);
    } else if tag == "title" && html_title.is_none() {
        let text: String = el_ref.text().collect();
        let trimmed = text.trim();
        if !trimmed.is_empty() {
            *html_title = Some(trimmed.to_owned());
        }
    } else if tag == "body" && body_el.is_none() {
        *body_el = Some(el_ref);
    }
    Ok(())
}

fn collect_html_blocks(
    parent: ElementRef<'_>,
    ctx: &mut HtmlConvertCtx<'_>,
    depth: usize,
    out: &mut Vec<Block>,
) -> Result<(), RenderError> {
    check_depth(depth)?;
    ctx.tick()?;
    let mut loose_spans = Vec::new();
    for child in parent.children() {
        match child.value() {
            Node::Text(t) => {
                let collapsed = collapse_ws(&t.text);
                if !collapsed.is_empty() {
                    loose_spans.push(InlineSpan::plain(collapsed));
                }
            }
            Node::Element(el) => {
                let Some(child_el) = ElementRef::wrap(child) else {
                    continue;
                };
                collect_html_child_element(child_el, el, ctx, depth, &mut loose_spans, out)?;
            }
            _ => {}
        }
    }
    flush_loose_spans(&mut loose_spans, ctx.sheet, out);
    Ok(())
}

fn collect_html_child_element(
    child_el: ElementRef<'_>,
    el: &scraper::node::Element,
    ctx: &mut HtmlConvertCtx<'_>,
    depth: usize,
    loose_spans: &mut Vec<InlineSpan>,
    out: &mut Vec<Block>,
) -> Result<(), RenderError> {
    let tag = el.name().to_ascii_lowercase();
    if matches!(tag.as_str(), "head" | "style" | "title" | "meta") {
        return Ok(());
    }
    if is_inline_html_tag(&tag) {
        return collect_inline_el(child_el, InlineCtx::default(), ctx, depth + 1, loose_spans);
    }
    flush_loose_spans(loose_spans, ctx.sheet, out);
    let style = ctx.element_style(el)?;
    if style.page_break_before {
        out.push(Block::PageBreak);
    }
    convert_html_block(child_el, &tag, style.clone(), ctx, depth + 1, out)?;
    if style.page_break_after {
        out.push(Block::PageBreak);
    }
    Ok(())
}

fn flush_loose_spans(spans: &mut Vec<InlineSpan>, sheet: &ParsedStylesheet, out: &mut Vec<Block>) {
    if spans.iter().any(|s| !s.text.trim().is_empty()) {
        out.push(Block::Paragraph {
            spans: std::mem::take(spans),
            style: sheet.resolve("p", &[], None, None),
        });
    } else {
        spans.clear();
    }
}

fn is_inline_html_tag(tag: &str) -> bool {
    matches!(
        tag,
        "span"
            | "strong"
            | "b"
            | "em"
            | "i"
            | "code"
            | "a"
            | "small"
            | "mark"
            | "u"
            | "s"
            | "del"
            | "sub"
            | "sup"
            | "br"
    )
}

fn convert_html_block(
    el_ref: ElementRef<'_>,
    tag: &str,
    style: StyleProps,
    ctx: &mut HtmlConvertCtx<'_>,
    depth: usize,
    out: &mut Vec<Block>,
) -> Result<(), RenderError> {
    check_depth(depth)?;
    ctx.tick()?;
    match tag {
        "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
            let level = tag.as_bytes()[1] - b'0';
            let mut spans = Vec::new();
            collect_inline_children(
                el_ref,
                InlineCtx::from_style(&style),
                ctx,
                depth + 1,
                &mut spans,
            )?;
            if !spans.is_empty() {
                out.push(Block::Heading {
                    level,
                    spans,
                    style,
                });
            }
        }
        "p" => {
            let mut spans = Vec::new();
            collect_inline_children(
                el_ref,
                InlineCtx::from_style(&style),
                ctx,
                depth + 1,
                &mut spans,
            )?;
            if !spans.is_empty() {
                out.push(Block::Paragraph { spans, style });
            }
        }
        "pre" => out.push(Block::Code {
            text: el_ref.text().collect(),
            style,
        }),
        "blockquote" => {
            let mut inner = Vec::new();
            collect_html_blocks(el_ref, ctx, depth + 1, &mut inner)?;
            out.push(Block::Quote { blocks: inner });
        }
        "ul" | "ol" => convert_html_list(el_ref, tag == "ol", ctx, depth + 1, out)?,
        "table" => convert_html_table(el_ref, ctx, depth + 1, out)?,
        "img" => convert_html_image(el_ref, ctx.attachments, out)?,
        "hr" => out.push(Block::HorizontalRule),
        _ => collect_html_blocks(el_ref, ctx, depth + 1, out)?,
    }
    Ok(())
}

fn convert_html_table(
    el_ref: ElementRef<'_>,
    ctx: &mut HtmlConvertCtx<'_>,
    depth: usize,
    out: &mut Vec<Block>,
) -> Result<(), RenderError> {
    let mut headers = Vec::new();
    let mut rows = Vec::new();
    collect_table_rows(el_ref, false, ctx, depth, &mut headers, &mut rows)?;
    if !headers.is_empty() || !rows.is_empty() {
        out.push(Block::Table { headers, rows });
    }
    Ok(())
}

fn convert_html_image(
    el_ref: ElementRef<'_>,
    attachments: &BTreeMap<String, ResolvedImage>,
    out: &mut Vec<Block>,
) -> Result<(), RenderError> {
    let el = el_ref.value();
    if let Some(src) = el.attr("src") {
        let source = resolve_image_source(src, attachments)?;
        out.push(Block::Image {
            source,
            alt: el.attr("alt").unwrap_or("").to_owned(),
            width_pt: el.attr("width").and_then(parse_length_pt),
            height_pt: el.attr("height").and_then(parse_length_pt),
        });
    }
    Ok(())
}

fn convert_html_list(
    el_ref: ElementRef<'_>,
    ordered: bool,
    ctx: &mut HtmlConvertCtx<'_>,
    depth: usize,
    out: &mut Vec<Block>,
) -> Result<(), RenderError> {
    check_depth(depth)?;
    let start = el_ref
        .value()
        .attr("start")
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(1);
    let mut items = Vec::new();
    for child in el_ref.children().filter_map(ElementRef::wrap) {
        if child.value().name().eq_ignore_ascii_case("li") {
            let mut item_blocks = Vec::new();
            collect_html_blocks(child, ctx, depth + 1, &mut item_blocks)?;
            items.push(item_blocks);
        }
    }
    if !items.is_empty() {
        out.push(Block::List {
            ordered,
            start,
            items,
        });
    }
    Ok(())
}

fn collect_table_rows(
    el_ref: ElementRef<'_>,
    in_thead: bool,
    ctx: &mut HtmlConvertCtx<'_>,
    depth: usize,
    headers: &mut Vec<TableRow>,
    rows: &mut Vec<TableRow>,
) -> Result<(), RenderError> {
    check_depth(depth)?;
    ctx.tick()?;
    for child in el_ref.children().filter_map(ElementRef::wrap) {
        match child.value().name().to_ascii_lowercase().as_str() {
            "thead" => collect_table_rows(child, true, ctx, depth + 1, headers, rows)?,
            "tbody" | "tfoot" => collect_table_rows(child, false, ctx, depth + 1, headers, rows)?,
            "tr" => {
                let (cells, all_th) = collect_row_cells(child, in_thead, ctx, depth + 1)?;
                if !cells.is_empty() {
                    if in_thead || all_th {
                        headers.push(TableRow { cells });
                    } else {
                        rows.push(TableRow { cells });
                    }
                }
            }
            _ => {}
        }
    }
    Ok(())
}

fn collect_row_cells(
    row_el: ElementRef<'_>,
    in_thead: bool,
    ctx: &mut HtmlConvertCtx<'_>,
    depth: usize,
) -> Result<(Vec<TableCell>, bool), RenderError> {
    check_depth(depth)?;
    let mut cells = Vec::new();
    let mut all_th = true;
    for cell_el in row_el.children().filter_map(ElementRef::wrap) {
        let cname = cell_el.value().name().to_ascii_lowercase();
        if cname != "th" && cname != "td" {
            continue;
        }
        let is_th = cname == "th" || in_thead;
        if cname == "td" {
            all_th = false;
        }
        let cstyle = ctx.element_style(cell_el.value())?;
        let mut spans = Vec::new();
        collect_inline_children(
            cell_el,
            InlineCtx::from_style(&cstyle),
            ctx,
            depth + 1,
            &mut spans,
        )?;
        cells.push(TableCell {
            spans,
            header: is_th,
            style: cstyle,
        });
    }
    Ok((cells, all_th))
}

#[derive(Clone, Default)]
struct InlineCtx {
    bold: bool,
    italic: bool,
    code: bool,
    color: Option<Rgb>,
}

impl InlineCtx {
    fn from_style(style: &StyleProps) -> Self {
        Self {
            bold: style.bold.unwrap_or(false),
            italic: style.italic.unwrap_or(false),
            code: style.monospace.unwrap_or(false),
            color: style.color,
        }
    }
}

fn collect_inline_children(
    parent: ElementRef<'_>,
    inline_ctx: InlineCtx,
    ctx: &mut HtmlConvertCtx<'_>,
    depth: usize,
    out: &mut Vec<InlineSpan>,
) -> Result<(), RenderError> {
    check_depth(depth)?;
    ctx.tick()?;
    for child in parent.children() {
        match child.value() {
            Node::Text(t) => push_inline_text(&t.text, &inline_ctx, out),
            Node::Element(_) => {
                if let Some(el_ref) = ElementRef::wrap(child) {
                    collect_inline_el(el_ref, inline_ctx.clone(), ctx, depth + 1, out)?;
                }
            }
            _ => {}
        }
    }
    Ok(())
}

fn push_inline_text(raw: &str, inline_ctx: &InlineCtx, out: &mut Vec<InlineSpan>) {
    let text = if inline_ctx.code {
        raw.to_owned()
    } else {
        collapse_ws(raw)
    };
    if !text.is_empty() {
        out.push(InlineSpan {
            text,
            bold: inline_ctx.bold,
            italic: inline_ctx.italic,
            code: inline_ctx.code,
            color: inline_ctx.color,
        });
    }
}

fn collect_inline_el(
    el_ref: ElementRef<'_>,
    mut inline_ctx: InlineCtx,
    ctx: &mut HtmlConvertCtx<'_>,
    depth: usize,
    out: &mut Vec<InlineSpan>,
) -> Result<(), RenderError> {
    check_depth(depth)?;
    ctx.tick()?;
    let el = el_ref.value();
    let tag = el.name().to_ascii_lowercase();
    if tag == "br" {
        out.push(InlineSpan::plain("\n"));
        return Ok(());
    }
    let style = ctx.element_style(el)?;
    if matches!(tag.as_str(), "strong" | "b") || style.bold == Some(true) {
        inline_ctx.bold = true;
    }
    if matches!(tag.as_str(), "em" | "i") || style.italic == Some(true) {
        inline_ctx.italic = true;
    }
    if tag == "code" || style.monospace == Some(true) {
        inline_ctx.code = true;
    }
    if let Some(c) = style.color {
        inline_ctx.color = Some(c);
    }
    if tag == "a" && el.attr("href").is_some() {
        inline_ctx.color = inline_ctx.color.or(Some(Rgb::LINK));
    }
    collect_inline_children(el_ref, inline_ctx, ctx, depth + 1, out)
}

fn collapse_ws(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut last_space = false;
    for ch in text.chars() {
        if ch.is_whitespace() {
            if !last_space {
                out.push(' ');
                last_space = true;
            }
        } else {
            out.push(ch);
            last_space = false;
        }
    }
    out
}
