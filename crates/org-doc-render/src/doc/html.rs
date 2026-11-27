//! HTML+CSS -> [`Document`] converter backed by `scraper` (`html5ever`).

use std::collections::BTreeMap;

use scraper::{ElementRef, Html, Node};

use crate::css::{
    parse_and_validate_css, parse_inline_style, parse_length_pt, ParsedStylesheet, Rgb, StyleProps,
};
use crate::model::{RenderError, ResolvedImage};
use crate::sanitize::validate_image_url;

use super::{resolve_image_bytes, Block, Document, InlineSpan, TableCell, TableRow};

/// Converts validated HTML + optional external CSS into a [`Document`].
pub(crate) fn html_to_document(
    html: &str,
    extra_css: Option<&str>,
    title: Option<&str>,
    attachments: &BTreeMap<String, ResolvedImage>,
) -> Result<Document, RenderError> {
    let parsed = Html::parse_document(html);
    let mut sheet = ParsedStylesheet::default();
    let mut html_title = None;
    let mut body_el = None;

    for node in parsed.tree.nodes() {
        let Some(el_ref) = ElementRef::wrap(node) else {
            continue;
        };
        let tag = el_ref.value().name().to_ascii_lowercase();
        if tag == "style" {
            let css_text: String = el_ref.text().collect();
            sheet.merge(parse_and_validate_css(&css_text, &|u| {
                validate_image_url(u, attachments).map(|_| ())
            })?);
        } else if tag == "title" && html_title.is_none() {
            let text: String = el_ref.text().collect();
            let trimmed = text.trim();
            if !trimmed.is_empty() {
                html_title = Some(trimmed.to_owned());
            }
        } else if tag == "body" && body_el.is_none() {
            body_el = Some(el_ref);
        }
    }
    if let Some(css_text) = extra_css {
        sheet.merge(parse_and_validate_css(css_text, &|u| {
            validate_image_url(u, attachments).map(|_| ())
        })?);
    }

    let root = body_el.unwrap_or_else(|| parsed.root_element());
    let mut blocks = Vec::new();
    collect_html_blocks(root, &sheet, attachments, &mut blocks)?;
    Ok(Document {
        title: title.map(str::to_owned).or(html_title),
        page: sheet.page,
        blocks,
    })
}

fn element_style(
    el: &scraper::node::Element,
    sheet: &ParsedStylesheet,
    attachments: &BTreeMap<String, ResolvedImage>,
) -> Result<StyleProps, RenderError> {
    let classes: Vec<String> = el.classes().map(str::to_owned).collect();
    let inline = match el.attr("style") {
        Some(raw) => Some(parse_inline_style(raw, &|u| {
            validate_image_url(u, attachments).map(|_| ())
        })?),
        None => None,
    };
    Ok(sheet.resolve(el.name(), &classes, el.id(), inline.as_ref()))
}

fn collect_html_blocks(
    parent: ElementRef<'_>,
    sheet: &ParsedStylesheet,
    attachments: &BTreeMap<String, ResolvedImage>,
    out: &mut Vec<Block>,
) -> Result<(), RenderError> {
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
                let tag = el.name().to_ascii_lowercase();
                if matches!(tag.as_str(), "head" | "style" | "title" | "meta") {
                    continue;
                }
                if is_inline_html_tag(&tag) {
                    collect_inline_el(
                        child_el,
                        InlineCtx::default(),
                        sheet,
                        attachments,
                        &mut loose_spans,
                    )?;
                    continue;
                }
                flush_loose_spans(&mut loose_spans, sheet, out);
                let style = element_style(el, sheet, attachments)?;
                if style.page_break_before {
                    out.push(Block::PageBreak);
                }
                convert_html_block(child_el, &tag, style.clone(), sheet, attachments, out)?;
                if style.page_break_after {
                    out.push(Block::PageBreak);
                }
            }
            _ => {}
        }
    }
    flush_loose_spans(&mut loose_spans, sheet, out);
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
    sheet: &ParsedStylesheet,
    attachments: &BTreeMap<String, ResolvedImage>,
    out: &mut Vec<Block>,
) -> Result<(), RenderError> {
    match tag {
        "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
            let level = tag.as_bytes()[1] - b'0';
            let mut spans = Vec::new();
            collect_inline_children(
                el_ref,
                InlineCtx::from_style(&style),
                sheet,
                attachments,
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
                sheet,
                attachments,
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
            collect_html_blocks(el_ref, sheet, attachments, &mut inner)?;
            out.push(Block::Quote { blocks: inner });
        }
        "ul" | "ol" => convert_html_list(el_ref, tag == "ol", sheet, attachments, out)?,
        "table" => {
            let mut headers = Vec::new();
            let mut rows = Vec::new();
            collect_table_rows(el_ref, false, sheet, attachments, &mut headers, &mut rows)?;
            if !headers.is_empty() || !rows.is_empty() {
                out.push(Block::Table { headers, rows });
            }
        }
        "img" => {
            let el = el_ref.value();
            if let Some(src) = el.attr("src") {
                let bytes = resolve_image_bytes(src, attachments)?;
                out.push(Block::Image {
                    bytes,
                    alt: el.attr("alt").unwrap_or("").to_owned(),
                    width_pt: el.attr("width").and_then(parse_length_pt),
                    height_pt: el.attr("height").and_then(parse_length_pt),
                });
            }
        }
        "hr" => out.push(Block::HorizontalRule),
        _ => collect_html_blocks(el_ref, sheet, attachments, out)?,
    }
    Ok(())
}

fn convert_html_list(
    el_ref: ElementRef<'_>,
    ordered: bool,
    sheet: &ParsedStylesheet,
    attachments: &BTreeMap<String, ResolvedImage>,
    out: &mut Vec<Block>,
) -> Result<(), RenderError> {
    let start = el_ref
        .value()
        .attr("start")
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(1);
    let mut items = Vec::new();
    for child in el_ref.children().filter_map(ElementRef::wrap) {
        if child.value().name().eq_ignore_ascii_case("li") {
            let mut item_blocks = Vec::new();
            collect_html_blocks(child, sheet, attachments, &mut item_blocks)?;
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
    sheet: &ParsedStylesheet,
    attachments: &BTreeMap<String, ResolvedImage>,
    headers: &mut Vec<TableRow>,
    rows: &mut Vec<TableRow>,
) -> Result<(), RenderError> {
    for child in el_ref.children().filter_map(ElementRef::wrap) {
        match child.value().name().to_ascii_lowercase().as_str() {
            "thead" => collect_table_rows(child, true, sheet, attachments, headers, rows)?,
            "tbody" | "tfoot" => {
                collect_table_rows(child, false, sheet, attachments, headers, rows)?;
            }
            "tr" => {
                let (cells, all_th) = collect_row_cells(child, in_thead, sheet, attachments)?;
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
    sheet: &ParsedStylesheet,
    attachments: &BTreeMap<String, ResolvedImage>,
) -> Result<(Vec<TableCell>, bool), RenderError> {
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
        let cstyle = element_style(cell_el.value(), sheet, attachments)?;
        let mut spans = Vec::new();
        collect_inline_children(
            cell_el,
            InlineCtx::from_style(&cstyle),
            sheet,
            attachments,
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
    ctx: InlineCtx,
    sheet: &ParsedStylesheet,
    attachments: &BTreeMap<String, ResolvedImage>,
    out: &mut Vec<InlineSpan>,
) -> Result<(), RenderError> {
    for child in parent.children() {
        match child.value() {
            Node::Text(t) => {
                let text = if ctx.code {
                    t.text.to_string()
                } else {
                    collapse_ws(&t.text)
                };
                if !text.is_empty() {
                    out.push(InlineSpan {
                        text,
                        bold: ctx.bold,
                        italic: ctx.italic,
                        code: ctx.code,
                        color: ctx.color,
                    });
                }
            }
            Node::Element(_) => {
                if let Some(el_ref) = ElementRef::wrap(child) {
                    collect_inline_el(el_ref, ctx.clone(), sheet, attachments, out)?;
                }
            }
            _ => {}
        }
    }
    Ok(())
}

fn collect_inline_el(
    el_ref: ElementRef<'_>,
    mut ctx: InlineCtx,
    sheet: &ParsedStylesheet,
    attachments: &BTreeMap<String, ResolvedImage>,
    out: &mut Vec<InlineSpan>,
) -> Result<(), RenderError> {
    let el = el_ref.value();
    let tag = el.name().to_ascii_lowercase();
    if tag == "br" {
        out.push(InlineSpan::plain("\n"));
        return Ok(());
    }
    let style = element_style(el, sheet, attachments)?;
    if matches!(tag.as_str(), "strong" | "b") || style.bold == Some(true) {
        ctx.bold = true;
    }
    if matches!(tag.as_str(), "em" | "i") || style.italic == Some(true) {
        ctx.italic = true;
    }
    if tag == "code" || style.monospace == Some(true) {
        ctx.code = true;
    }
    if let Some(c) = style.color {
        ctx.color = Some(c);
    }
    if tag == "a" && el.attr("href").is_some() {
        ctx.color = ctx.color.or(Some(Rgb::LINK));
    }
    collect_inline_children(el_ref, ctx, sheet, attachments, out)
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
