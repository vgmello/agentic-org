//! Markdown -> [`Document`] converter (`pulldown-cmark` + fail-closed raw HTML handling).

use std::collections::BTreeMap;

use pulldown_cmark::{Event, HeadingLevel, Parser, Tag, TagEnd};

use crate::css::{parse_and_validate_css, ParsedStylesheet, Rgb};
use crate::model::{RenderError, ResolvedImage};
use crate::sanitize::{markdown_options, validate_image_url};

use super::html::html_to_document;
use super::{resolve_image_bytes, Block, Document, InlineSpan, TableCell, TableRow};

/// Converts validated Markdown + optional CSS into a [`Document`].
pub(crate) fn markdown_to_document(
    md: &str,
    css: Option<&str>,
    title: Option<&str>,
    attachments: &BTreeMap<String, ResolvedImage>,
) -> Result<Document, RenderError> {
    let mut sheet = ParsedStylesheet::default();
    if let Some(css_text) = css {
        sheet.merge(parse_and_validate_css(css_text, &|u| {
            validate_image_url(u, attachments).map(|_| ())
        })?);
    }
    let (blocks, first_heading) = {
        let mut builder = MdBuilder::new(&sheet, attachments);
        let parser = Parser::new_ext(md, markdown_options());
        for event in parser {
            builder.push(event)?;
        }
        builder.finish()
    };
    Ok(Document {
        title: title.map(str::to_owned).or(first_heading),
        page: sheet.page,
        blocks,
    })
}

struct MdBuilder<'a> {
    sheet: &'a ParsedStylesheet,
    attachments: &'a BTreeMap<String, ResolvedImage>,
    stack: Vec<MdFrame>,
    root: Vec<Block>,
    bold: usize,
    italic: usize,
    link: Option<String>,
    first_heading: Option<String>,
}

enum MdFrame {
    Paragraph(Vec<InlineSpan>),
    Heading {
        level: u8,
        spans: Vec<InlineSpan>,
    },
    Code(String),
    Quote(Vec<Block>),
    List {
        ordered: bool,
        start: u64,
        items: Vec<Vec<Block>>,
    },
    Item(Vec<Block>),
    Table {
        headers: Vec<TableRow>,
        rows: Vec<TableRow>,
        in_head: bool,
    },
    TableRow {
        cells: Vec<TableCell>,
        is_head: bool,
    },
    TableCell {
        spans: Vec<InlineSpan>,
        is_head: bool,
    },
    Image {
        bytes: Vec<u8>,
        alt: String,
    },
}

impl<'a> MdBuilder<'a> {
    fn new(sheet: &'a ParsedStylesheet, attachments: &'a BTreeMap<String, ResolvedImage>) -> Self {
        Self {
            sheet,
            attachments,
            stack: Vec::new(),
            root: Vec::new(),
            bold: 0,
            italic: 0,
            link: None,
            first_heading: None,
        }
    }

    fn push_block(&mut self, block: Block) {
        for frame in self.stack.iter_mut().rev() {
            match frame {
                MdFrame::Quote(blocks) | MdFrame::Item(blocks) => {
                    blocks.push(block);
                    return;
                }
                _ => {}
            }
        }
        self.root.push(block);
    }

    fn push_span(&mut self, span: InlineSpan) {
        for frame in self.stack.iter_mut().rev() {
            match frame {
                MdFrame::Paragraph(spans)
                | MdFrame::Heading { spans, .. }
                | MdFrame::TableCell { spans, .. } => {
                    spans.push(span);
                    return;
                }
                MdFrame::Image { alt, .. } => {
                    alt.push_str(&span.text);
                    return;
                }
                MdFrame::Code(text) => {
                    text.push_str(&span.text);
                    return;
                }
                MdFrame::Item(blocks) => {
                    if let Some(Block::Paragraph { spans, .. }) = blocks.last_mut() {
                        spans.push(span);
                    } else {
                        blocks.push(Block::Paragraph {
                            spans: vec![span],
                            style: self.sheet.resolve("p", &[], None, None),
                        });
                    }
                    return;
                }
                _ => {}
            }
        }
        self.root.push(Block::Paragraph {
            spans: vec![span],
            style: self.sheet.resolve("p", &[], None, None),
        });
    }

    fn push(&mut self, event: Event<'_>) -> Result<(), RenderError> {
        match event {
            Event::Start(tag) => self.start_tag(tag)?,
            Event::End(end) => self.end_tag(end),
            Event::Text(t) => self.push_span(InlineSpan {
                text: t.into_string(),
                bold: self.bold > 0,
                italic: self.italic > 0,
                code: false,
                color: self.link.as_ref().map(|_| Rgb::LINK),
            }),
            Event::Code(t) => self.push_span(InlineSpan {
                text: t.into_string(),
                bold: self.bold > 0,
                italic: self.italic > 0,
                code: true,
                color: None,
            }),
            Event::SoftBreak => self.push_span(InlineSpan::plain(" ")),
            Event::HardBreak => self.push_span(InlineSpan::plain("\n")),
            Event::Rule => self.push_block(Block::HorizontalRule),
            Event::TaskListMarker(checked) => {
                self.push_span(InlineSpan::plain(if checked { "[x] " } else { "[ ] " }));
            }
            Event::Html(raw) | Event::InlineHtml(raw) => {
                let frag = html_to_document(&raw, None, None, self.attachments)?;
                for b in frag.blocks {
                    self.push_block(b);
                }
            }
            _ => {}
        }
        Ok(())
    }

    fn start_tag(&mut self, tag: Tag<'_>) -> Result<(), RenderError> {
        match tag {
            Tag::Paragraph => self.stack.push(MdFrame::Paragraph(Vec::new())),
            Tag::Heading { level, .. } => self.stack.push(MdFrame::Heading {
                level: heading_num(level),
                spans: Vec::new(),
            }),
            Tag::CodeBlock(_) => self.stack.push(MdFrame::Code(String::new())),
            Tag::BlockQuote(_) => self.stack.push(MdFrame::Quote(Vec::new())),
            Tag::List(start) => self.stack.push(MdFrame::List {
                ordered: start.is_some(),
                start: start.unwrap_or(1),
                items: Vec::new(),
            }),
            Tag::Item => self.stack.push(MdFrame::Item(Vec::new())),
            Tag::Table(_) => self.stack.push(MdFrame::Table {
                headers: Vec::new(),
                rows: Vec::new(),
                in_head: false,
            }),
            Tag::TableHead => {
                if let Some(MdFrame::Table { in_head, .. }) = self.stack.last_mut() {
                    *in_head = true;
                }
                self.stack.push(MdFrame::TableRow {
                    cells: Vec::new(),
                    is_head: true,
                });
            }
            Tag::TableRow => {
                let is_head = matches!(
                    self.stack.last(),
                    Some(MdFrame::Table { in_head: true, .. })
                );
                self.stack.push(MdFrame::TableRow {
                    cells: Vec::new(),
                    is_head,
                });
            }
            Tag::TableCell => {
                let is_head = matches!(
                    self.stack.last(),
                    Some(MdFrame::TableRow { is_head: true, .. })
                );
                self.stack.push(MdFrame::TableCell {
                    spans: Vec::new(),
                    is_head,
                });
            }
            Tag::Strong => self.bold += 1,
            Tag::Emphasis => self.italic += 1,
            Tag::Link { dest_url, .. } => self.link = Some(dest_url.into_string()),
            Tag::Image { dest_url, .. } => {
                let bytes = resolve_image_bytes(&dest_url, self.attachments)?;
                self.stack.push(MdFrame::Image {
                    bytes,
                    alt: String::new(),
                });
            }
            _ => {}
        }
        Ok(())
    }

    fn end_tag(&mut self, end: TagEnd) {
        match end {
            TagEnd::Strong => self.bold = self.bold.saturating_sub(1),
            TagEnd::Emphasis => self.italic = self.italic.saturating_sub(1),
            TagEnd::Link => self.link = None,
            TagEnd::Paragraph => self.end_paragraph(),
            TagEnd::Heading(_) => self.end_heading(),
            TagEnd::CodeBlock => {
                if let Some(MdFrame::Code(text)) = self.stack.pop() {
                    let style = self.sheet.resolve("pre", &[], None, None);
                    self.push_block(Block::Code { text, style });
                }
            }
            TagEnd::BlockQuote(_) => {
                if let Some(MdFrame::Quote(blocks)) = self.stack.pop() {
                    self.push_block(Block::Quote { blocks });
                }
            }
            TagEnd::Item => {
                if let Some(MdFrame::Item(blocks)) = self.stack.pop() {
                    if let Some(MdFrame::List { items, .. }) = self.stack.last_mut() {
                        items.push(blocks);
                    }
                }
            }
            TagEnd::List(_) => {
                if let Some(MdFrame::List {
                    ordered,
                    start,
                    items,
                }) = self.stack.pop()
                {
                    self.push_block(Block::List {
                        ordered,
                        start,
                        items,
                    });
                }
            }
            TagEnd::TableCell | TagEnd::TableHead | TagEnd::TableRow | TagEnd::Table => {
                self.end_table_part(end);
            }
            TagEnd::Image => {
                if let Some(MdFrame::Image { bytes, alt }) = self.stack.pop() {
                    self.push_block(Block::Image {
                        bytes,
                        alt,
                        width_pt: None,
                        height_pt: None,
                    });
                }
            }
            _ => {}
        }
    }

    fn end_paragraph(&mut self) {
        if let Some(MdFrame::Paragraph(spans)) = self.stack.pop() {
            if !spans.is_empty() {
                let style = self.sheet.resolve("p", &[], None, None);
                self.push_block(Block::Paragraph { spans, style });
            }
        }
    }

    fn end_heading(&mut self) {
        if let Some(MdFrame::Heading { level, spans }) = self.stack.pop() {
            if self.first_heading.is_none() {
                let text: String = spans.iter().map(|s| s.text.as_str()).collect();
                let trimmed = text.trim();
                if !trimmed.is_empty() {
                    self.first_heading = Some(trimmed.to_owned());
                }
            }
            let style = self.sheet.resolve(&format!("h{level}"), &[], None, None);
            self.push_block(Block::Heading {
                level,
                spans,
                style,
            });
        }
    }

    fn end_table_part(&mut self, end: TagEnd) {
        match end {
            TagEnd::TableCell => {
                if let Some(MdFrame::TableCell { spans, is_head }) = self.stack.pop() {
                    let style =
                        self.sheet
                            .resolve(if is_head { "th" } else { "td" }, &[], None, None);
                    if let Some(MdFrame::TableRow { cells, .. }) = self.stack.last_mut() {
                        cells.push(TableCell {
                            spans,
                            header: is_head,
                            style,
                        });
                    }
                }
            }
            TagEnd::TableHead => {
                if let Some(MdFrame::TableRow { cells, .. }) = self.stack.pop() {
                    if let Some(MdFrame::Table {
                        headers, in_head, ..
                    }) = self.stack.last_mut()
                    {
                        *in_head = false;
                        if !cells.is_empty() {
                            headers.push(TableRow { cells });
                        }
                    }
                }
            }
            TagEnd::TableRow => {
                if let Some(MdFrame::TableRow { cells, is_head }) = self.stack.pop() {
                    if let Some(MdFrame::Table { headers, rows, .. }) = self.stack.last_mut() {
                        if !cells.is_empty() {
                            if is_head {
                                headers.push(TableRow { cells });
                            } else {
                                rows.push(TableRow { cells });
                            }
                        }
                    }
                }
            }
            TagEnd::Table => {
                if let Some(MdFrame::Table { headers, rows, .. }) = self.stack.pop() {
                    self.push_block(Block::Table { headers, rows });
                }
            }
            _ => {}
        }
    }

    fn finish(self) -> (Vec<Block>, Option<String>) {
        (self.root, self.first_heading)
    }
}

fn heading_num(level: HeadingLevel) -> u8 {
    match level {
        HeadingLevel::H1 => 1,
        HeadingLevel::H2 => 2,
        HeadingLevel::H3 => 3,
        HeadingLevel::H4 => 4,
        HeadingLevel::H5 => 5,
        HeadingLevel::H6 => 6,
    }
}
