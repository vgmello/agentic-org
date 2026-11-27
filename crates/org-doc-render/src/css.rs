//! Fail-closed CSS security validation and print-rule parsing (`@page`, margin boxes,
//! typography, colors, and pagination hints) for `org-doc-render`.

use std::collections::BTreeMap;

use crate::model::RenderError;

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
        for (selector, props) in other.rules {
            self.rules.entry(selector).or_default().merge_from(&props);
        }
    }

    /// Resolves cascading styles for an element tag, classes, id, and inline declarations.
    pub(crate) fn resolve(
        &self,
        tag: &str,
        classes: &[String],
        id: Option<&str>,
        inline: Option<&StyleProps>,
    ) -> StyleProps {
        let mut out = StyleProps::default();
        if let Some(body) = self.rules.get("body") {
            out.font_size = body.font_size;
            out.color = body.color;
        }
        if let Some(r) = self.rules.get(&tag.to_ascii_lowercase()) {
            out.merge_from(r);
        }
        for class in classes {
            let key = format!(".{}", class.to_ascii_lowercase());
            if let Some(r) = self.rules.get(&key) {
                out.merge_from(r);
            }
        }
        if let Some(id_val) = id {
            let key = format!("#{}", id_val.to_ascii_lowercase());
            if let Some(r) = self.rules.get(&key) {
                out.merge_from(r);
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
}

/// Validates CSS text against all air-gap / SSRF / active-script rules and parses safe print rules.
pub(crate) fn parse_and_validate_css(
    raw: &str,
    validate_url: &impl Fn(&str) -> Result<(), RenderError>,
) -> Result<ParsedStylesheet, RenderError> {
    let cleaned = strip_css_comments(raw);
    let normalized = decode_css_escapes(&cleaned);
    reject_dangerous_css(&normalized, validate_url)?;
    let mut sheet = ParsedStylesheet::default();
    parse_top_level_rules(&cleaned, &mut sheet);
    Ok(sheet)
}

/// Validates and parses an inline `style="..."` attribute.
pub(crate) fn parse_inline_style(
    raw: &str,
    validate_url: &impl Fn(&str) -> Result<(), RenderError>,
) -> Result<StyleProps, RenderError> {
    let cleaned = strip_css_comments(raw);
    let normalized = decode_css_escapes(&cleaned);
    reject_dangerous_css(&normalized, validate_url)?;
    let mut props = StyleProps::default();
    parse_declarations(&cleaned, &mut props);
    Ok(props)
}

fn strip_css_comments(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut rest = input;
    while let Some(start) = rest.find("/*") {
        out.push_str(&rest[..start]);
        let after = &rest[start + 2..];
        match after.find("*/") {
            Some(end) => {
                out.push(' ');
                rest = &after[end + 2..];
            }
            None => return out,
        }
    }
    out.push_str(rest);
    out
}

/// Decodes CSS backslash escapes (`\75 rl(...)`, `@\69 mport`) so obfuscation cannot bypass checks.
fn decode_css_escapes(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut chars = input.chars().peekable();
    while let Some(ch) = chars.next() {
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
    out
}

fn reject_dangerous_css(
    css: &str,
    validate_url: &impl Fn(&str) -> Result<(), RenderError>,
) -> Result<(), RenderError> {
    let lower = css.to_ascii_lowercase();
    reject_forbidden_css_at_rules(&lower)?;
    reject_forbidden_css_functions(&lower)?;
    if lower.contains("javascript:") || lower.contains("vbscript:") {
        return Err(RenderError::ForbiddenResourceUrl {
            url: "script-uri".to_owned(),
            reason: "script URIs in CSS are forbidden",
        });
    }
    scan_css_urls(css, &lower, validate_url)
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
    validate_url: &impl Fn(&str) -> Result<(), RenderError>,
) -> Result<(), RenderError> {
    let mut offset = 0usize;
    while let Some(rel) = lower[offset..].find("url(") {
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

fn parse_top_level_rules(css: &str, sheet: &mut ParsedStylesheet) {
    let bytes = css.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        while i < bytes.len() && bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        if i >= bytes.len() {
            break;
        }
        let Some(brace_rel) = css[i..].find('{') else {
            break;
        };
        let header = css[i..i + brace_rel].trim();
        let body_start = i + brace_rel + 1;
        let Some(body_end) = find_matching_brace(css, body_start) else {
            break;
        };
        let body = &css[body_start..body_end];
        let lower_header = header.to_ascii_lowercase();
        if lower_header.starts_with("@page") {
            parse_page_rule(body, &mut sheet.page);
        } else if !lower_header.starts_with('@') {
            let mut props = StyleProps::default();
            parse_declarations(body, &mut props);
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

fn parse_page_rule(body: &str, page: &mut PageStyle) {
    let mut i = 0usize;
    let mut decl_buf = String::new();
    while i < body.len() {
        if body[i..].starts_with('@') {
            if let Some(brace_rel) = body[i..].find('{') {
                let name = body[i..i + brace_rel].trim().to_ascii_lowercase();
                let inner_start = i + brace_rel + 1;
                if let Some(inner_end) = find_matching_brace(body, inner_start) {
                    let spec = parse_margin_box(&body[inner_start..inner_end]);
                    assign_margin_box(page, &name, spec);
                    i = inner_end + 1;
                    continue;
                }
            }
        }
        let ch = body[i..].chars().next().unwrap_or(' ');
        decl_buf.push(ch);
        i += ch.len_utf8();
    }
    for decl in decl_buf.split(';') {
        let Some((k, v)) = decl.split_once(':') else {
            continue;
        };
        apply_page_decl(page, k.trim().to_ascii_lowercase().as_str(), v.trim());
    }
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

fn parse_margin_box(body: &str) -> MarginBoxSpec {
    let mut spec = MarginBoxSpec::default();
    for decl in body.split(';') {
        let Some((k, v)) = decl.split_once(':') else {
            continue;
        };
        match k.trim().to_ascii_lowercase().as_str() {
            "content" => spec.content = parse_css_content(v.trim()),
            "font-size" => spec.font_size = parse_length_pt(v.trim()),
            "color" => spec.color = parse_color(v.trim()),
            _ => {}
        }
    }
    spec
}

/// Parses a CSS `@page` `content:` expression supporting strings and `counter(page)` / `counter(pages)`.
fn parse_css_content(raw: &str) -> String {
    let mut out = String::new();
    let mut i = 0usize;
    while let Some(ch) = raw[i..].chars().next() {
        if ch == '"' || ch == '\'' {
            i = consume_quoted_css_string(raw, i, ch, &mut out);
        } else if let Some(next_i) = consume_css_counter(raw, i, &mut out) {
            i = next_i;
        } else {
            i += ch.len_utf8();
        }
    }
    out
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

fn consume_css_counter(raw: &str, start: usize, out: &mut String) -> Option<usize> {
    let slice = &raw[start..];
    if !slice
        .get(..8)
        .is_some_and(|p| p.eq_ignore_ascii_case("counter("))
    {
        return None;
    }
    let end = slice.find(')')?;
    let name = slice[8..end].trim().to_ascii_lowercase();
    if name == "page" {
        out.push_str("{page}");
    } else if name == "pages" {
        out.push_str("{pages}");
    }
    Some(start + end + 1)
}

fn apply_page_decl(page: &mut PageStyle, key: &str, val: &str) {
    match key {
        "size" => apply_page_size(page, val),
        "margin" => {
            let parts: Vec<f32> = val.split_whitespace().filter_map(parse_length_pt).collect();
            match parts.as_slice() {
                [all] => {
                    page.margin_top = *all;
                    page.margin_right = *all;
                    page.margin_bottom = *all;
                    page.margin_left = *all;
                }
                [tb, lr] => {
                    page.margin_top = *tb;
                    page.margin_bottom = *tb;
                    page.margin_left = *lr;
                    page.margin_right = *lr;
                }
                [t, r, b, l] => {
                    page.margin_top = *t;
                    page.margin_right = *r;
                    page.margin_bottom = *b;
                    page.margin_left = *l;
                }
                _ => {}
            }
        }
        "margin-top" => {
            if let Some(v) = parse_length_pt(val) {
                page.margin_top = v;
            }
        }
        "margin-right" => {
            if let Some(v) = parse_length_pt(val) {
                page.margin_right = v;
            }
        }
        "margin-bottom" => {
            if let Some(v) = parse_length_pt(val) {
                page.margin_bottom = v;
            }
        }
        "margin-left" => {
            if let Some(v) = parse_length_pt(val) {
                page.margin_left = v;
            }
        }
        _ => {}
    }
}

fn apply_page_size(page: &mut PageStyle, val: &str) {
    let lower = val.to_ascii_lowercase();
    let tokens: Vec<&str> = lower.split_whitespace().collect();
    let landscape = tokens.contains(&"landscape");
    let (mut w, mut h) = if tokens.contains(&"letter") {
        (612.0_f32, 792.0_f32)
    } else if tokens.contains(&"legal") {
        (612.0_f32, 1008.0_f32)
    } else if tokens.contains(&"a4") {
        (595.28_f32, 841.89_f32)
    } else {
        let dims: Vec<f32> = tokens.iter().filter_map(|t| parse_length_pt(t)).collect();
        if dims.len() == 2 {
            (dims[0].clamp(144.0, 2000.0), dims[1].clamp(144.0, 2000.0))
        } else {
            (page.width_pt, page.height_pt)
        }
    };
    if landscape && h > w {
        std::mem::swap(&mut w, &mut h);
    }
    page.width_pt = w;
    page.height_pt = h;
}

fn parse_declarations(body: &str, props: &mut StyleProps) {
    for decl in body.split(';') {
        let Some((raw_k, raw_v)) = decl.split_once(':') else {
            continue;
        };
        let key = raw_k.trim().to_ascii_lowercase();
        let val = raw_v.trim().trim_end_matches("!important").trim();
        apply_style_decl(props, &key, val);
    }
}

fn apply_style_decl(props: &mut StyleProps, key: &str, val: &str) {
    let lower = val.to_ascii_lowercase();
    match key {
        "font-size" => {
            if let Some(pt) = parse_length_pt(val) {
                props.font_size = Some(pt.clamp(6.0, 48.0));
            }
        }
        "font-weight" => {
            props.bold = Some(lower == "bold" || lower.parse::<u16>().is_ok_and(|w| w >= 600));
        }
        "font-style" => props.italic = Some(lower == "italic" || lower == "oblique"),
        "font-family" => {
            if lower.contains("mono") || lower.contains("courier") || lower.contains("code") {
                props.monospace = Some(true);
            }
        }
        "color" => props.color = parse_color(val).or(props.color),
        "background" | "background-color" => {
            props.background = parse_color(val).or(props.background)
        }
        "text-align" => match lower.as_str() {
            "left" => props.align = Some(TextAlign::Left),
            "center" => props.align = Some(TextAlign::Center),
            "right" => props.align = Some(TextAlign::Right),
            _ => {}
        },
        "page-break-before" | "break-before" => {
            if matches!(lower.as_str(), "always" | "page") {
                props.page_break_before = true;
            }
        }
        "page-break-after" | "break-after" => {
            if matches!(lower.as_str(), "always" | "page") {
                props.page_break_after = true;
            }
        }
        "margin-top" => props.margin_top = parse_length_pt(val).or(props.margin_top),
        "margin-bottom" => props.margin_bottom = parse_length_pt(val).or(props.margin_bottom),
        _ => {}
    }
}

pub(crate) fn parse_length_pt(raw: &str) -> Option<f32> {
    let s = raw.trim().to_ascii_lowercase();
    let (num, factor) = if let Some(v) = s.strip_suffix("pt") {
        (v, 1.0_f32)
    } else if let Some(v) = s.strip_suffix("px") {
        (v, 0.75_f32)
    } else if let Some(v) = s.strip_suffix("mm") {
        (v, 72.0_f32 / 25.4_f32)
    } else if let Some(v) = s.strip_suffix("cm") {
        (v, 72.0_f32 / 2.54_f32)
    } else if let Some(v) = s.strip_suffix("in") {
        (v, 72.0_f32)
    } else if let Some(v) = s.strip_suffix("em").or_else(|| s.strip_suffix("rem")) {
        (v, 10.5_f32)
    } else {
        (s.as_str(), 1.0_f32)
    };
    let parsed = num.trim().parse::<f32>().ok()?;
    (parsed.is_finite() && parsed >= 0.0).then_some(parsed * factor)
}

fn parse_color(raw: &str) -> Option<Rgb> {
    let s = raw.trim().to_ascii_lowercase();
    if let Some(hex) = s.strip_prefix('#') {
        return parse_hex_color(hex);
    }
    if let Some(inner) = s.strip_prefix("rgb(").and_then(|v| v.strip_suffix(')')) {
        let parts: Vec<f32> = inner
            .split(',')
            .filter_map(|p| p.trim().parse::<f32>().ok())
            .collect();
        if parts.len() == 3 {
            return Some(Rgb {
                r: (parts[0] / 255.0).clamp(0.0, 1.0),
                g: (parts[1] / 255.0).clamp(0.0, 1.0),
                b: (parts[2] / 255.0).clamp(0.0, 1.0),
            });
        }
    }
    match s.as_str() {
        "black" => Some(Rgb {
            r: 0.0,
            g: 0.0,
            b: 0.0,
        }),
        "white" => Some(Rgb {
            r: 1.0,
            g: 1.0,
            b: 1.0,
        }),
        "gray" | "grey" => Some(Rgb::MUTED),
        "red" | "crimson" => Some(Rgb {
            r: 0.72,
            g: 0.12,
            b: 0.12,
        }),
        "green" => Some(Rgb {
            r: 0.10,
            g: 0.48,
            b: 0.22,
        }),
        "blue" | "navy" => Some(Rgb::LINK),
        _ => None,
    }
}

fn parse_hex_color(hex: &str) -> Option<Rgb> {
    let bytes = hex.as_bytes();
    if !bytes.iter().all(u8::is_ascii_hexdigit) {
        return None;
    }
    let (r, g, b) = match bytes.len() {
        3 => (
            u8::from_str_radix(&hex[0..1], 16).ok()? * 17,
            u8::from_str_radix(&hex[1..2], 16).ok()? * 17,
            u8::from_str_radix(&hex[2..3], 16).ok()? * 17,
        ),
        6 => (
            u8::from_str_radix(&hex[0..2], 16).ok()?,
            u8::from_str_radix(&hex[2..4], 16).ok()?,
            u8::from_str_radix(&hex[4..6], 16).ok()?,
        ),
        _ => return None,
    };
    Some(Rgb {
        r: f32::from(r) / 255.0,
        g: f32::from(g) / 255.0,
        b: f32::from(b) / 255.0,
    })
}
