//! Fail-closed CSS security validation and print-rule parsing (`@page`, margin boxes,
//! typography, colors, and pagination hints) for `org-doc-render`.
//!
//! All scanners run strictly in O(N) time and check cooperative [`RenderDeadline`] ticks so
//! adversarial stylesheets can never wedge worker semaphore permits.

mod values;

use std::collections::BTreeMap;

use crate::model::{check_deadline, RenderDeadline, RenderError};

use values::{apply_page_decl, parse_color, parse_declarations};

pub(crate) use values::{clamp_page_margins, parse_length_pt};

/// RGB color in `[0.0, 1.0]`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Rgb {
    pub r: f32,
    pub g: f32,
    pub b: f32,
}

impl Rgb {
    pub(crate) const BLACK: Self = Self {
        r: 0.1,
        g: 0.1,
        b: 0.12,
    };
    pub(crate) const MUTED: Self = Self {
        r: 0.42,
        g: 0.45,
        b: 0.50,
    };
    pub(crate) const LINK: Self = Self {
        r: 0.10,
        g: 0.34,
        b: 0.68,
    };
    pub(crate) const CODE_BG: Self = Self {
        r: 0.95,
        g: 0.96,
        b: 0.97,
    };
    pub(crate) const HEADER_BG: Self = Self {
        r: 0.92,
        g: 0.94,
        b: 0.96,
    };
    pub(crate) const BORDER: Self = Self {
        r: 0.78,
        g: 0.81,
        b: 0.85,
    };
}

/// Horizontal text alignment.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum TextAlign {
    #[default]
    Left,
    Center,
    Right,
}

/// One `@page` margin-box header/footer specification (e.g. `@top-left`, `@bottom-right`).
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct MarginBoxSpec {
    pub content: String,
    pub font_size: Option<f32>,
    pub color: Option<Rgb>,
}

/// Parsed `@page` dimensions, margins, and running header/footer boxes.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct PageStyle {
    pub width_pt: f32,
    pub height_pt: f32,
    pub margin_top: f32,
    pub margin_right: f32,
    pub margin_bottom: f32,
    pub margin_left: f32,
    pub top_left: Option<MarginBoxSpec>,
    pub top_center: Option<MarginBoxSpec>,
    pub top_right: Option<MarginBoxSpec>,
    pub bottom_left: Option<MarginBoxSpec>,
    pub bottom_center: Option<MarginBoxSpec>,
    pub bottom_right: Option<MarginBoxSpec>,
}

impl Default for PageStyle {
    fn default() -> Self {
        Self {
            width_pt: 595.28,
            height_pt: 841.89,
            margin_top: 56.0,
            margin_right: 54.0,
            margin_bottom: 56.0,
            margin_left: 54.0,
            top_left: None,
            top_center: None,
            top_right: None,
            bottom_left: None,
            bottom_center: None,
            bottom_right: None,
        }
    }
}

/// Visual style properties resolved from CSS rules or inline `style="..."` attributes.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct StyleProps {
    pub font_size: Option<f32>,
    pub bold: Option<bool>,
    pub italic: Option<bool>,
    pub monospace: Option<bool>,
    pub color: Option<Rgb>,
    pub background: Option<Rgb>,
    pub align: Option<TextAlign>,
    pub page_break_before: bool,
    pub page_break_after: bool,
    pub margin_top: Option<f32>,
    pub margin_bottom: Option<f32>,
}

impl StyleProps {
    pub(crate) fn merge_from(&mut self, other: &Self) {
        if other.font_size.is_some() {
            self.font_size = other.font_size;
        }
        if other.bold.is_some() {
            self.bold = other.bold;
        }
        if other.italic.is_some() {
            self.italic = other.italic;
        }
        if other.monospace.is_some() {
            self.monospace = other.monospace;
        }
        if other.color.is_some() {
            self.color = other.color;
        }
        if other.background.is_some() {
            self.background = other.background;
        }
        if other.align.is_some() {
            self.align = other.align;
        }
        self.page_break_before |= other.page_break_before;
        self.page_break_after |= other.page_break_after;
        if other.margin_top.is_some() {
            self.margin_top = other.margin_top;
        }
        if other.margin_bottom.is_some() {
            self.margin_bottom = other.margin_bottom;
        }
    }
}

/// Validated stylesheet combining `@page` rules and simple selector rules.
#[derive(Clone, Debug, Default)]
pub(crate) struct ParsedStylesheet {
    pub page: PageStyle,
    pub rules: BTreeMap<String, StyleProps>,
}

impl ParsedStylesheet {
    /// Merges another stylesheet into `self` in cascade order.
    pub(crate) fn merge(&mut self, other: Self) {
        merge_page_style(&mut self.page, &other.page);
        for (sel, props) in other.rules {
            self.rules.entry(sel).or_default().merge_from(&props);
        }
    }

    /// Resolves effective style properties for an HTML element by tag, classes, id, and inline style.
    pub(crate) fn resolve(
        &self,
        tag: &str,
        classes: &[String],
        id: Option<&str>,
        inline: Option<&StyleProps>,
    ) -> StyleProps {
        let mut out = StyleProps::default();
        if let Some(p) = self.rules.get("body") {
            out.merge_from(p);
        }
        if let Some(p) = self.rules.get(&tag.to_ascii_lowercase()) {
            out.merge_from(p);
        }
        for cls in classes {
            let key = format!(".{}", cls.to_ascii_lowercase());
            if let Some(p) = self.rules.get(&key) {
                out.merge_from(p);
            }
        }
        if let Some(elem_id) = id {
            let key = format!("#{}", elem_id.to_ascii_lowercase());
            if let Some(p) = self.rules.get(&key) {
                out.merge_from(p);
            }
        }
        if let Some(inl) = inline {
            out.merge_from(inl);
        }
        out
    }
}

fn merge_page_style(dst: &mut PageStyle, src: &PageStyle) {
    let def = PageStyle::default();
    if (src.width_pt - def.width_pt).abs() > 0.1 || (src.height_pt - def.height_pt).abs() > 0.1 {
        dst.width_pt = src.width_pt;
        dst.height_pt = src.height_pt;
    }
    if (src.margin_top - def.margin_top).abs() > 0.1 {
        dst.margin_top = src.margin_top;
    }
    if (src.margin_right - def.margin_right).abs() > 0.1 {
        dst.margin_right = src.margin_right;
    }
    if (src.margin_bottom - def.margin_bottom).abs() > 0.1 {
        dst.margin_bottom = src.margin_bottom;
    }
    if (src.margin_left - def.margin_left).abs() > 0.1 {
        dst.margin_left = src.margin_left;
    }
    for (slot, val) in [
        (&mut dst.top_left, &src.top_left),
        (&mut dst.top_center, &src.top_center),
        (&mut dst.top_right, &src.top_right),
        (&mut dst.bottom_left, &src.bottom_left),
        (&mut dst.bottom_center, &src.bottom_center),
        (&mut dst.bottom_right, &src.bottom_right),
    ] {
        if val.is_some() {
            *slot = val.clone();
        }
    }
    clamp_page_margins(dst);
}

/// Validates CSS text against all air-gap / SSRF / active-script rules and parses safe print rules.
pub(crate) fn parse_and_validate_css(
    raw: &str,
    deadline: Option<&RenderDeadline>,
    validate_url: &impl Fn(&str) -> Result<(), RenderError>,
) -> Result<ParsedStylesheet, RenderError> {
    check_deadline(deadline)?;
    let cleaned = strip_css_comments(raw, deadline)?;
    let normalized = decode_css_escapes(&cleaned, deadline)?;
    reject_dangerous_css(&normalized, deadline, validate_url)?;
    let mut sheet = ParsedStylesheet::default();
    parse_top_level_rules(&cleaned, &mut sheet, deadline)?;
    clamp_page_margins(&mut sheet.page);
    Ok(sheet)
}

/// Validates and parses an inline `style="..."` attribute.
pub(crate) fn parse_inline_style(
    raw: &str,
    deadline: Option<&RenderDeadline>,
    validate_url: &impl Fn(&str) -> Result<(), RenderError>,
) -> Result<StyleProps, RenderError> {
    check_deadline(deadline)?;
    let cleaned = strip_css_comments(raw, deadline)?;
    let normalized = decode_css_escapes(&cleaned, deadline)?;
    reject_dangerous_css(&normalized, deadline, validate_url)?;
    let mut props = StyleProps::default();
    parse_declarations(&cleaned, &mut props, deadline)?;
    Ok(props)
}

fn strip_css_comments(
    input: &str,
    deadline: Option<&RenderDeadline>,
) -> Result<String, RenderError> {
    let mut out = String::with_capacity(input.len());
    let mut rest = input;
    let mut iters = 0usize;
    while let Some(start) = rest.find("/*") {
        iters += 1;
        if iters & 0xff == 0 {
            check_deadline(deadline)?;
        }
        out.push_str(&rest[..start]);
        let after = &rest[start + 2..];
        match after.find("*/") {
            Some(end) => {
                out.push(' ');
                rest = &after[end + 2..];
            }
            None => return Ok(out),
        }
    }
    out.push_str(rest);
    Ok(out)
}

/// Decodes CSS backslash escapes (`\75 rl(...)`, `@\69 mport`) so obfuscation cannot bypass checks.
fn decode_css_escapes(
    input: &str,
    deadline: Option<&RenderDeadline>,
) -> Result<String, RenderError> {
    let mut out = String::with_capacity(input.len());
    let mut chars = input.chars().peekable();
    let mut steps = 0usize;
    while let Some(ch) = chars.next() {
        steps += 1;
        if steps & 0xfff == 0 {
            check_deadline(deadline)?;
        }
        if ch != '\\' {
            out.push(ch);
            continue;
        }
        let mut hex = String::new();
        while hex.len() < 6 && chars.peek().is_some_and(char::is_ascii_hexdigit) {
            if let Some(d) = chars.next() {
                hex.push(d);
            }
        }
        if !hex.is_empty() {
            if chars.peek().is_some_and(|c| c.is_ascii_whitespace()) {
                chars.next();
            }
            let decoded = u32::from_str_radix(&hex, 16)
                .ok()
                .and_then(char::from_u32)
                .unwrap_or('\u{FFFD}');
            out.push(decoded);
        } else if let Some(escaped) = chars.next() {
            out.push(escaped);
        }
    }
    Ok(out)
}

fn reject_dangerous_css(
    css: &str,
    deadline: Option<&RenderDeadline>,
    validate_url: &impl Fn(&str) -> Result<(), RenderError>,
) -> Result<(), RenderError> {
    check_deadline(deadline)?;
    let lower = css.to_ascii_lowercase();
    reject_forbidden_css_at_rules(&lower)?;
    reject_forbidden_css_functions(&lower)?;
    if lower.contains("javascript:") || lower.contains("vbscript:") {
        return Err(RenderError::ForbiddenResourceUrl {
            url: "script-uri".to_owned(),
            reason: "script URIs in CSS are forbidden",
        });
    }
    scan_css_urls(css, &lower, deadline, validate_url)
}

fn reject_forbidden_css_at_rules(lower: &str) -> Result<(), RenderError> {
    for (rule, reason) in [
        ("@import", "CSS @import rules are forbidden"),
        ("@font-face", "CSS @font-face rules are forbidden"),
        ("@namespace", "CSS @namespace rules are forbidden"),
    ] {
        if lower.contains(rule) {
            return Err(RenderError::ForbiddenResourceUrl {
                url: rule.to_owned(),
                reason,
            });
        }
    }
    Ok(())
}

fn reject_forbidden_css_functions(lower: &str) -> Result<(), RenderError> {
    if contains_css_function_call(lower, "image-set") {
        return Err(RenderError::ForbiddenResourceUrl {
            url: "image-set(...)".to_owned(),
            reason: "CSS image-set() resource references are forbidden",
        });
    }
    if contains_css_function_call(lower, "expression")
        || lower.contains("-moz-binding")
        || lower.contains("behavior:")
    {
        return Err(RenderError::ForbiddenContent {
            reason: "legacy active CSS expressions or bindings are forbidden",
        });
    }
    Ok(())
}

fn contains_css_function_call(lower: &str, name: &str) -> bool {
    let mut offset = 0usize;
    while let Some(rel) = lower[offset..].find(name) {
        let after = offset + rel + name.len();
        if lower[after..].trim_start().starts_with('(') {
            return true;
        }
        offset = after;
    }
    false
}

fn scan_css_urls(
    css: &str,
    lower: &str,
    deadline: Option<&RenderDeadline>,
    validate_url: &impl Fn(&str) -> Result<(), RenderError>,
) -> Result<(), RenderError> {
    let mut offset = 0usize;
    let mut iters = 0usize;
    while let Some(rel) = lower[offset..].find("url(") {
        iters += 1;
        if iters & 0x3f == 0 {
            check_deadline(deadline)?;
        }
        let start = offset + rel;
        let after = &css[start + 4..];
        let Some(end) = after.find(')') else {
            return Err(RenderError::ForbiddenResourceUrl {
                url: "url(...)".to_owned(),
                reason: "unterminated CSS url() expression",
            });
        };
        let raw_url = after[..end]
            .trim()
            .trim_matches('"')
            .trim_matches('\'')
            .trim();
        validate_url(raw_url)?;
        offset = start + 4 + end + 1;
    }
    Ok(())
}

fn parse_top_level_rules(
    css: &str,
    sheet: &mut ParsedStylesheet,
    deadline: Option<&RenderDeadline>,
) -> Result<(), RenderError> {
    let bytes = css.as_bytes();
    let mut i = 0usize;
    let mut rules = 0usize;
    while i < bytes.len() {
        rules += 1;
        if rules & 0x3f == 0 {
            check_deadline(deadline)?;
        }
        while i < bytes.len() && bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        if i >= bytes.len() {
            break;
        }
        let Some(brace_pos) = find_unquoted_byte(css, i, b'{') else {
            break;
        };
        let header = css[i..brace_pos].trim();
        let body_start = brace_pos + 1;
        let Some(body_end) = find_matching_brace(css, body_start) else {
            break;
        };
        let body = &css[body_start..body_end];
        let lower_header = header.to_ascii_lowercase();
        if lower_header.starts_with("@page") {
            parse_page_rule(body, &mut sheet.page, deadline)?;
        } else if !lower_header.starts_with('@') {
            let mut props = StyleProps::default();
            parse_declarations(body, &mut props, deadline)?;
            for sel in header.split(',').map(str::trim).filter(|s| !s.is_empty()) {
                sheet
                    .rules
                    .entry(sel.to_ascii_lowercase())
                    .or_default()
                    .merge_from(&props);
            }
        }
        i = body_end + 1;
    }
    Ok(())
}

/// Quote-aware forward search for an unquoted ASCII byte (`{`), guaranteeing O(N) scanning.
fn find_unquoted_byte(s: &str, start: usize, needle: u8) -> Option<usize> {
    let mut quote: Option<u8> = None;
    for (rel, &b) in s.as_bytes()[start..].iter().enumerate() {
        if let Some(q) = quote {
            if b == q {
                quote = None;
            }
            continue;
        }
        if b == b'"' || b == b'\'' {
            quote = Some(b);
        } else if b == needle {
            return Some(start + rel);
        }
    }
    None
}

fn find_matching_brace(s: &str, start: usize) -> Option<usize> {
    let mut depth = 1usize;
    let mut quote: Option<u8> = None;
    for (rel, &b) in s.as_bytes()[start..].iter().enumerate() {
        if let Some(q) = quote {
            if b == q {
                quote = None;
            }
            continue;
        }
        match b {
            b'"' | b'\'' => quote = Some(b),
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(start + rel);
                }
            }
            _ => {}
        }
    }
    None
}

fn parse_page_rule(
    body: &str,
    page: &mut PageStyle,
    deadline: Option<&RenderDeadline>,
) -> Result<(), RenderError> {
    let mut i = 0usize;
    let mut steps = 0usize;
    let mut can_have_margin_box = true;
    let mut decl_buf = String::new();
    while i < body.len() {
        steps += 1;
        if steps & 0xfff == 0 {
            check_deadline(deadline)?;
        }
        if can_have_margin_box && body.as_bytes()[i] == b'@' {
            match find_unquoted_byte(body, i, b'{')
                .and_then(|brace_pos| Some((brace_pos, find_matching_brace(body, brace_pos + 1)?)))
            {
                Some((brace_pos, inner_end)) => {
                    let name = body[i..brace_pos].trim().to_ascii_lowercase();
                    let spec = parse_margin_box(&body[brace_pos + 1..inner_end], deadline)?;
                    assign_margin_box(page, &name, spec);
                    i = inner_end + 1;
                    continue;
                }
                None => {
                    // No balanced unquoted `{...}` remains anywhere in `body`; never rescan.
                    can_have_margin_box = false;
                }
            }
        }
        let ch = body[i..].chars().next().unwrap_or(' ');
        decl_buf.push(ch);
        i += ch.len_utf8();
    }
    for (idx, decl) in decl_buf.split(';').enumerate() {
        if idx & 0xff == 0 {
            check_deadline(deadline)?;
        }
        let Some((k, v)) = decl.split_once(':') else {
            continue;
        };
        apply_page_decl(page, k.trim().to_ascii_lowercase().as_str(), v.trim());
    }
    clamp_page_margins(page);
    Ok(())
}

fn assign_margin_box(page: &mut PageStyle, name: &str, spec: MarginBoxSpec) {
    if spec.content.is_empty() {
        return;
    }
    match name {
        "@top-left" => page.top_left = Some(spec),
        "@top-center" => page.top_center = Some(spec),
        "@top-right" => page.top_right = Some(spec),
        "@bottom-left" => page.bottom_left = Some(spec),
        "@bottom-center" => page.bottom_center = Some(spec),
        "@bottom-right" => page.bottom_right = Some(spec),
        _ => {}
    }
}

fn parse_margin_box(
    body: &str,
    deadline: Option<&RenderDeadline>,
) -> Result<MarginBoxSpec, RenderError> {
    let mut spec = MarginBoxSpec::default();
    for (idx, decl) in body.split(';').enumerate() {
        if idx & 0xff == 0 {
            check_deadline(deadline)?;
        }
        let Some((k, v)) = decl.split_once(':') else {
            continue;
        };
        match k.trim().to_ascii_lowercase().as_str() {
            "content" => spec.content = parse_css_content(v.trim(), deadline)?,
            "font-size" => spec.font_size = parse_length_pt(v.trim()).map(|pt| pt.clamp(6.0, 48.0)),
            "color" => spec.color = parse_color(v.trim()),
            _ => {}
        }
    }
    Ok(spec)
}

enum CounterScan {
    NotCounter,
    Consumed(usize),
    NoCloseParen,
}

/// Parses a CSS `@page` `content:` expression supporting strings and `counter(page)` / `counter(pages)`.
fn parse_css_content(raw: &str, deadline: Option<&RenderDeadline>) -> Result<String, RenderError> {
    let mut out = String::new();
    let mut i = 0usize;
    let mut steps = 0usize;
    let mut can_have_counter = true;
    while let Some(ch) = raw[i..].chars().next() {
        steps += 1;
        if steps & 0xfff == 0 {
            check_deadline(deadline)?;
        }
        if ch == '"' || ch == '\'' {
            i = consume_quoted_css_string(raw, i, ch, &mut out);
        } else if can_have_counter {
            match consume_css_counter(raw, i, &mut out) {
                CounterScan::Consumed(next_i) => i = next_i,
                CounterScan::NoCloseParen => {
                    // No `)` exists in the remainder of `raw`; never rescan for `counter(`.
                    can_have_counter = false;
                    i += ch.len_utf8();
                }
                CounterScan::NotCounter => i += ch.len_utf8(),
            }
        } else {
            i += ch.len_utf8();
        }
    }
    Ok(out)
}

fn consume_quoted_css_string(
    raw: &str,
    start_quote: usize,
    quote: char,
    out: &mut String,
) -> usize {
    let mut i = start_quote + quote.len_utf8();
    let content_start = i;
    while let Some(ch) = raw[i..].chars().next() {
        if ch == quote {
            out.push_str(&raw[content_start..i]);
            return i + ch.len_utf8();
        }
        i += ch.len_utf8();
    }
    out.push_str(&raw[content_start..i]);
    i
}

fn consume_css_counter(raw: &str, start: usize, out: &mut String) -> CounterScan {
    let slice = &raw[start..];
    if !slice
        .get(..8)
        .is_some_and(|p| p.eq_ignore_ascii_case("counter("))
    {
        return CounterScan::NotCounter;
    }
    let Some(end) = slice.find(')') else {
        return CounterScan::NoCloseParen;
    };
    let name = slice[8..end].trim().to_ascii_lowercase();
    if name == "page" {
        out.push_str("{page}");
    } else if name == "pages" {
        out.push_str("{pages}");
    }
    CounterScan::Consumed(start + end + 1)
}
