//! Fail-closed HTML, Markdown, CSS, and resource-URL sanitization for `org-doc-render`.
//!
//! Zero filesystem access and zero network fetches are ever allowed:
//! - Active HTML/SVG elements (`<script>`, `<iframe>`, `<object>`, `<embed>`, `<applet>`,
//!   `<foreignObject>`, `<base>`, `<svg>`, `<template>`, `<meta http-equiv="refresh">`, `<link>`)
//!   and `on*=` event handlers fail closed with [`RenderError::ForbiddenContent`].
//! - All external and local resource URLs (`http://`, `https://`, `file://`, `ftp://`, `//`,
//!   `/etc/passwd`, relative paths, CSS `@import`, CSS `url(...)`, `javascript:`, `vbscript:`,
//!   `data:image/svg+xml`, `data:text/html`) fail closed with [`RenderError::ForbiddenResourceUrl`].
//! - Only safe inline `data:image/(png|jpeg|webp|gif);base64,...` URIs and caller-resolved
//!   `attachment:<uuidv7>` image references are permitted.

use std::collections::{BTreeMap, BTreeSet};

use base64::engine::general_purpose::STANDARD;
use base64::Engine as _;
use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd};
use scraper::{Html, Node};

use crate::css::{parse_and_validate_css, parse_inline_style};
use crate::model::{
    check_deadline, RenderDeadline, RenderError, RenderRequest, ResolvedImage, SourceKind,
    MAX_EMBEDDED_IMAGES, MAX_IMAGE_DECODED_BYTES,
};

const FORBIDDEN_TAGS: &[&str] = &[
    "script",
    "iframe",
    "object",
    "embed",
    "applet",
    "foreignobject",
    "base",
    "svg",
    "template",
    "frame",
    "frameset",
    "link",
    "form",
    "input",
    "button",
    "textarea",
    "select",
    "audio",
    "video",
    "source",
    "track",
    "portal",
    "math",
];

const RESOURCE_ATTRS: &[&str] = &[
    "src",
    "srcset",
    "poster",
    "background",
    "data",
    "codebase",
    "action",
    "formaction",
    "xlink:href",
    "dynsrc",
    "lowsrc",
];

const SAFE_DATA_IMAGE_PREFIXES: &[(&str, &str)] = &[
    ("data:image/png;base64,", "image/png"),
    ("data:image/jpeg;base64,", "image/jpeg"),
    ("data:image/webp;base64,", "image/webp"),
    ("data:image/gif;base64,", "image/gif"),
];

/// Validated image reference allowed to reach the PDF renderer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum SafeImageRef {
    Inline {
        mime_type: &'static str,
        bytes: Vec<u8>,
    },
    Attachment {
        id: String,
    },
}

/// O(1) input-budget precheck before acquiring a worker slot or parsing markup.
pub(crate) fn validate_input_budget(
    request: &RenderRequest,
    max_bytes: usize,
) -> Result<(), RenderError> {
    let actual = request.total_input_bytes();
    if actual > max_bytes {
        return Err(RenderError::InputTooLarge {
            actual,
            max: max_bytes,
        });
    }
    if request.source.trim().is_empty() {
        return Err(RenderError::EmptySource);
    }
    Ok(())
}

/// Runs the full fail-closed security audit on `request` inside the bounded worker.
pub(crate) fn audit_request_with_deadline(
    request: &RenderRequest,
    max_input_bytes: usize,
    deadline: Option<&RenderDeadline>,
) -> Result<(), RenderError> {
    validate_input_budget(request, max_input_bytes)?;
    check_deadline(deadline)?;
    if let Some(css) = request.css.as_deref() {
        parse_and_validate_css(css, &|url| validate_css_url(url, &request.attachments))?;
    }
    check_deadline(deadline)?;
    let mut image_count = 0usize;
    match request.source_kind {
        SourceKind::Html => audit_html_with_count(
            &request.source,
            &request.attachments,
            &mut image_count,
            deadline,
        ),
        SourceKind::Markdown => audit_markdown(
            &request.source,
            &request.attachments,
            &mut image_count,
            deadline,
        ),
    }
}

/// Scans `source` and optional `css` for unique canonical `attachment:<uuidv7>` image references
/// (capped at [`MAX_EMBEDDED_IMAGES`]) so `org-office-server` can authorize and pre-load them via
/// `AuthorizationPolicy::can_read` before invoking the renderer.
pub fn extract_attachment_refs(
    source_kind: SourceKind,
    source: &str,
    css: Option<&str>,
) -> Vec<String> {
    let mut found = BTreeSet::new();
    if let Some(css_text) = css {
        collect_attachment_tokens(css_text, &mut found);
    }
    match source_kind {
        SourceKind::Html => extract_html_attachment_refs(source, &mut found),
        SourceKind::Markdown => extract_markdown_attachment_refs(source, &mut found),
    }
    found.into_iter().take(MAX_EMBEDDED_IMAGES).collect()
}

fn extract_html_attachment_refs(source: &str, found: &mut BTreeSet<String>) {
    let doc = Html::parse_document(source);
    for node in doc.tree.nodes() {
        let Node::Element(el) = node.value() else {
            continue;
        };
        for attr in ["src", "href", "xlink:href", "style"] {
            if let Some(val) = el.attr(attr) {
                collect_attachment_tokens(val, found);
            }
        }
    }
}

fn extract_markdown_attachment_refs(source: &str, found: &mut BTreeSet<String>) {
    let mut html_block: Option<String> = None;
    for event in Parser::new_ext(source, markdown_options()) {
        match event {
            Event::Start(Tag::Image { dest_url, .. }) => {
                collect_attachment_tokens(&dest_url, found);
            }
            Event::Start(Tag::HtmlBlock) => html_block = Some(String::new()),
            Event::End(TagEnd::HtmlBlock) => {
                if let Some(buf) = html_block.take() {
                    collect_attachment_tokens(&buf, found);
                }
            }
            Event::Html(raw) => {
                if let Some(buf) = html_block.as_mut() {
                    buf.push_str(&raw);
                } else {
                    collect_attachment_tokens(&raw, found);
                }
            }
            Event::InlineHtml(raw) => collect_attachment_tokens(&raw, found),
            _ => {}
        }
    }
    if let Some(buf) = html_block {
        collect_attachment_tokens(&buf, found);
    }
}

fn collect_attachment_tokens(text: &str, out: &mut BTreeSet<String>) {
    let lower = text.to_ascii_lowercase();
    let mut pos = 0usize;
    while let Some(rel) = lower[pos..].find("attachment:") {
        let start = pos + rel + "attachment:".len();
        let tail = &text[start..];
        let id: String = tail
            .chars()
            .take(36)
            .take_while(|c| c.is_ascii_hexdigit() || *c == '-')
            .collect();
        if is_canonical_uuid_v7(&id) {
            out.insert(id);
            if out.len() >= MAX_EMBEDDED_IMAGES {
                return;
            }
        }
        pos = start;
    }
}

pub(crate) fn markdown_options() -> Options {
    Options::ENABLE_TABLES | Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TASKLISTS
}

/// Validates an HTML document DOM fail-closed against active content and external/local URLs.
fn audit_html_with_count(
    html: &str,
    attachments: &BTreeMap<String, ResolvedImage>,
    image_count: &mut usize,
    deadline: Option<&RenderDeadline>,
) -> Result<(), RenderError> {
    let doc = Html::parse_document(html);
    for (idx, node) in doc.tree.nodes().enumerate() {
        if idx & 0xff == 0 {
            check_deadline(deadline)?;
        }
        let Some(el_ref) = scraper::ElementRef::wrap(node) else {
            continue;
        };
        audit_html_element(el_ref, attachments, image_count)?;
    }
    Ok(())
}

fn audit_html_element(
    el_ref: scraper::ElementRef<'_>,
    attachments: &BTreeMap<String, ResolvedImage>,
    image_count: &mut usize,
) -> Result<(), RenderError> {
    let el = el_ref.value();
    let tag = el.name().to_ascii_lowercase();
    if FORBIDDEN_TAGS.contains(&tag.as_str()) {
        return Err(RenderError::ForbiddenContent {
            reason: "forbidden active or embedded element",
        });
    }
    if tag == "meta" && el.attr("http-equiv").is_some() {
        return Err(RenderError::ForbiddenContent {
            reason: "<meta http-equiv> directives are forbidden",
        });
    }
    audit_element_attrs(&tag, el, attachments, image_count)?;
    if tag == "style" {
        let css_text: String = el_ref.text().collect();
        parse_and_validate_css(&css_text, &|url| validate_css_url(url, attachments))?;
    }
    Ok(())
}

fn audit_element_attrs(
    tag: &str,
    el: &scraper::node::Element,
    attachments: &BTreeMap<String, ResolvedImage>,
    image_count: &mut usize,
) -> Result<(), RenderError> {
    for (name, val) in el.attrs() {
        let attr_lower = name.to_ascii_lowercase();
        if is_event_handler(&attr_lower) {
            return Err(RenderError::ForbiddenContent {
                reason: "inline event handler attributes (on*=) are forbidden",
            });
        }
        if attr_lower == "style" {
            parse_inline_style(val, &|url| validate_css_url(url, attachments))?;
        } else if RESOURCE_ATTRS.contains(&attr_lower.as_str()) {
            count_and_validate_image(val, attachments, image_count)?;
        } else if attr_lower == "href" {
            validate_link_href(tag, val, attachments)?;
        }
    }
    Ok(())
}

fn count_and_validate_image(
    url: &str,
    attachments: &BTreeMap<String, ResolvedImage>,
    image_count: &mut usize,
) -> Result<(), RenderError> {
    *image_count += 1;
    if *image_count > MAX_EMBEDDED_IMAGES {
        return Err(RenderError::ForbiddenContent {
            reason: "too many embedded images in one document",
        });
    }
    let _ = validate_image_url(url, attachments)?;
    Ok(())
}

/// Validates Markdown events AND buffered raw HTML blocks/fragments fail-closed.
fn audit_markdown(
    md: &str,
    attachments: &BTreeMap<String, ResolvedImage>,
    image_count: &mut usize,
    deadline: Option<&RenderDeadline>,
) -> Result<(), RenderError> {
    let parser = Parser::new_ext(md, markdown_options());
    let mut html_block: Option<String> = None;
    for (idx, event) in parser.enumerate() {
        if idx & 0xff == 0 {
            check_deadline(deadline)?;
        }
        match event {
            Event::Start(Tag::Image { dest_url, .. }) => {
                count_and_validate_image(&dest_url, attachments, image_count)?;
            }
            Event::Start(Tag::Link { dest_url, .. }) => {
                validate_link_href("a", &dest_url, attachments)?;
            }
            Event::Start(Tag::HtmlBlock) => html_block = Some(String::new()),
            Event::End(TagEnd::HtmlBlock) => {
                if let Some(buf) = html_block.take() {
                    audit_raw_md_html(&buf, false, attachments, image_count, deadline)?;
                }
            }
            Event::Html(raw) => {
                if let Some(buf) = html_block.as_mut() {
                    buf.push_str(&raw);
                } else {
                    audit_raw_md_html(&raw, false, attachments, image_count, deadline)?;
                }
            }
            Event::InlineHtml(raw) => {
                audit_raw_md_html(&raw, true, attachments, image_count, deadline)?;
            }
            _ => {}
        }
    }
    if let Some(buf) = html_block {
        audit_raw_md_html(&buf, false, attachments, image_count, deadline)?;
    }
    Ok(())
}

fn audit_raw_md_html(
    raw: &str,
    is_inline: bool,
    attachments: &BTreeMap<String, ResolvedImage>,
    image_count: &mut usize,
    deadline: Option<&RenderDeadline>,
) -> Result<(), RenderError> {
    reject_partial_or_unclosed_html(raw, is_inline)?;
    audit_html_with_count(raw, attachments, image_count, deadline)
}

/// Rejects unclosed `<...` tags and stray forbidden opening tags (such as `<script`, `<style`
/// inline, `<iframe`, `<object`, `<embed`, `<template`) before HTML5 parsing can silently drop
/// or split them across Markdown events.
fn reject_partial_or_unclosed_html(raw: &str, is_inline: bool) -> Result<(), RenderError> {
    let mut i = 0usize;
    while let Some(rel) = raw[i..].find('<') {
        let lt = i + rel;
        let tail = &raw[lt + 1..];
        if let Some(comment_body) = tail.strip_prefix("!--") {
            let Some(end) = comment_body.find("-->") else {
                return Err(RenderError::ForbiddenContent {
                    reason: "unclosed raw HTML comment in Markdown is forbidden",
                });
            };
            i = lt + 1 + 3 + end + 3;
            continue;
        }
        let Some(first) = tail.chars().next() else {
            return Err(RenderError::ForbiddenContent {
                reason: "unclosed raw HTML tag in Markdown is forbidden",
            });
        };
        if !(first.is_ascii_alphabetic() || matches!(first, '/' | '!' | '?')) {
            i = lt + 1;
            continue;
        }
        check_raw_tag_name(tail, is_inline)?;
        let Some(close_rel) = find_tag_close(tail) else {
            return Err(RenderError::ForbiddenContent {
                reason: "unclosed raw HTML tag in Markdown is forbidden",
            });
        };
        i = lt + 1 + close_rel + 1;
    }
    Ok(())
}

fn check_raw_tag_name(after_lt: &str, is_inline: bool) -> Result<(), RenderError> {
    let trimmed = after_lt.trim_start_matches('/');
    let name: String = trimmed
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == ':')
        .map(|c| c.to_ascii_lowercase())
        .collect();
    if FORBIDDEN_TAGS.contains(&name.as_str()) || (is_inline && name == "style") {
        return Err(RenderError::ForbiddenContent {
            reason: "forbidden raw HTML tag in Markdown",
        });
    }
    Ok(())
}

fn find_tag_close(after_lt: &str) -> Option<usize> {
    let mut quote: Option<u8> = None;
    for (idx, &b) in after_lt.as_bytes().iter().enumerate() {
        if let Some(q) = quote {
            if b == q {
                quote = None;
            }
            continue;
        }
        match b {
            b'"' | b'\'' => quote = Some(b),
            b'>' => return Some(idx),
            _ => {}
        }
    }
    None
}

fn is_event_handler(attr_lower: &str) -> bool {
    let Some(rest) = attr_lower.strip_prefix("on") else {
        return false;
    };
    !rest.is_empty() && rest.bytes().all(|b| b.is_ascii_alphanumeric())
}

fn validate_css_url(
    raw: &str,
    attachments: &BTreeMap<String, ResolvedImage>,
) -> Result<(), RenderError> {
    let _ = validate_image_url(raw, attachments)?;
    Ok(())
}

/// Strips ASCII control characters & whitespace that HTML/CSS parsers ignore before scheme matching.
fn strip_scheme_noise(raw: &str) -> String {
    raw.chars()
        .filter(|c| !c.is_ascii_control() && !c.is_ascii_whitespace())
        .collect()
}

fn sanitize_url_for_error(raw: &str) -> String {
    let trimmed = raw.trim();
    if trimmed.len() <= 96 {
        trimmed.to_owned()
    } else {
        let cut = trimmed.floor_char_boundary(93);
        format!("{}...", &trimmed[..cut])
    }
}

/// Validates a hyperlink `href` (only `<a href>` may hold `http(s)://`, `mailto:`, or `#fragment`;
/// `file://`, `javascript:`, `vbscript:`, `data:`, `ftp://`, `//`, and local paths fail closed).
fn validate_link_href(
    tag: &str,
    raw: &str,
    attachments: &BTreeMap<String, ResolvedImage>,
) -> Result<(), RenderError> {
    if tag != "a" {
        let _ = validate_image_url(raw, attachments)?;
        return Ok(());
    }
    let trimmed = raw.trim();
    if trimmed.is_empty() || trimmed.starts_with('#') {
        return Ok(());
    }
    let compact = strip_scheme_noise(trimmed).to_ascii_lowercase();
    if compact.starts_with("http://")
        || compact.starts_with("https://")
        || compact.starts_with("mailto:")
    {
        return Ok(());
    }
    Err(RenderError::ForbiddenResourceUrl {
        url: sanitize_url_for_error(trimmed),
        reason: "only http(s), mailto, or #fragment links are allowed in anchors",
    })
}

/// Strictly validates a resource URL (images, CSS `url(...)`, etc.).
///
/// Permits ONLY:
/// - `data:image/(png|jpeg|webp|gif);base64,<valid bounded base64>`
/// - `attachment:<canonical lowercase uuidv7>` present in `attachments`
pub(crate) fn validate_image_url(
    raw: &str,
    attachments: &BTreeMap<String, ResolvedImage>,
) -> Result<SafeImageRef, RenderError> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(RenderError::ForbiddenResourceUrl {
            url: String::new(),
            reason: "empty resource URL",
        });
    }
    let compact = strip_scheme_noise(trimmed);
    let compact_lower = compact.to_ascii_lowercase();

    if compact_lower.starts_with("data:") {
        return decode_safe_data_image(&compact, &compact_lower);
    }
    if let Some(id_raw) = compact_lower.strip_prefix("attachment:") {
        return validate_attachment_ref(trimmed, id_raw, attachments);
    }
    Err(RenderError::ForbiddenResourceUrl {
        url: sanitize_url_for_error(trimmed),
        reason: classify_forbidden_url_reason(trimmed, &compact_lower),
    })
}

fn validate_attachment_ref(
    trimmed: &str,
    id_raw: &str,
    attachments: &BTreeMap<String, ResolvedImage>,
) -> Result<SafeImageRef, RenderError> {
    if !is_canonical_uuid_v7(id_raw) {
        return Err(RenderError::ForbiddenResourceUrl {
            url: sanitize_url_for_error(trimmed),
            reason: "attachment:<id> must name a canonical lowercase UUIDv7",
        });
    }
    if !attachments.contains_key(id_raw) {
        return Err(RenderError::ForbiddenResourceUrl {
            url: sanitize_url_for_error(trimmed),
            reason: "referenced attachment is not authorized or not found",
        });
    }
    Ok(SafeImageRef::Attachment {
        id: id_raw.to_owned(),
    })
}

fn classify_forbidden_url_reason(trimmed: &str, compact_lower: &str) -> &'static str {
    if compact_lower.starts_with("file:") || trimmed.starts_with('/') {
        "local filesystem URLs and paths are forbidden"
    } else if compact_lower.starts_with("http:")
        || compact_lower.starts_with("https:")
        || compact_lower.starts_with("ftp:")
        || compact_lower.starts_with("//")
    {
        "external network resource URLs are forbidden"
    } else if compact_lower.starts_with("javascript:") || compact_lower.starts_with("vbscript:") {
        "script URLs are forbidden"
    } else {
        "relative or unsupported resource URLs are forbidden"
    }
}

fn decode_safe_data_image(compact: &str, compact_lower: &str) -> Result<SafeImageRef, RenderError> {
    let Some((prefix, mime_type)) = SAFE_DATA_IMAGE_PREFIXES
        .iter()
        .find(|(prefix, _)| compact_lower.starts_with(prefix))
        .copied()
    else {
        return Err(RenderError::ForbiddenResourceUrl {
            url: sanitize_url_for_error(compact),
            reason: "only base64 data:image/(png|jpeg|webp|gif) URIs are allowed",
        });
    };
    let b64 = &compact[prefix.len()..];
    let max_b64 = MAX_IMAGE_DECODED_BYTES.div_ceil(3) * 4;
    if b64.is_empty() || b64.len() > max_b64 {
        return Err(RenderError::ForbiddenResourceUrl {
            url: format!("{prefix}<oversized>"),
            reason: "inline data URI image is empty or exceeds 4 MiB limit",
        });
    }
    let bytes = STANDARD
        .decode(b64.as_bytes())
        .map_err(|_| RenderError::ForbiddenResourceUrl {
            url: format!("{prefix}<invalid-base64>"),
            reason: "inline data URI has invalid base64 payload",
        })?;
    if bytes.is_empty() || bytes.len() > MAX_IMAGE_DECODED_BYTES {
        return Err(RenderError::ForbiddenResourceUrl {
            url: format!("{prefix}<oversized>"),
            reason: "decoded data URI image exceeds 4 MiB limit",
        });
    }
    Ok(SafeImageRef::Inline { mime_type, bytes })
}

/// Checks canonical lowercase UUIDv7 (`8-4-4-4-12` hex, version 7, RFC 4122 variant).
pub(crate) fn is_canonical_uuid_v7(s: &str) -> bool {
    let b = s.as_bytes();
    if b.len() != 36 {
        return false;
    }
    if b[8] != b'-' || b[13] != b'-' || b[18] != b'-' || b[23] != b'-' {
        return false;
    }
    if b[14] != b'7' || !matches!(b[19], b'8' | b'9' | b'a' | b'b') {
        return false;
    }
    b.iter()
        .enumerate()
        .all(|(idx, ch)| matches!(idx, 8 | 13 | 18 | 23) || matches!(ch, b'0'..=b'9' | b'a'..=b'f'))
}
