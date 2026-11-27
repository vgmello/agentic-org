//! CSS value parsers (lengths, colors, `@page` sizing/margins, style declarations) with strict
//! finite-float and printable-box clamping.

use crate::model::{check_deadline, RenderDeadline, RenderError};

use super::{PageStyle, Rgb, StyleProps, TextAlign};

const MAX_LENGTH_PT: f32 = 4096.0;
const MIN_PAGE_DIM_PT: f32 = 144.0;
const MIN_PRINTABLE_PT: f32 = 72.0;

/// Clamps page dimensions and margins so printable width and height always stay `>= 72pt` and
/// finite even under adversarial `@page` declarations.
pub(crate) fn clamp_page_margins(page: &mut PageStyle) {
    if !page.width_pt.is_finite() {
        page.width_pt = 595.28;
    }
    if !page.height_pt.is_finite() {
        page.height_pt = 841.89;
    }
    page.width_pt = page.width_pt.clamp(MIN_PAGE_DIM_PT, 2000.0);
    page.height_pt = page.height_pt.clamp(MIN_PAGE_DIM_PT, 2000.0);
    for m in [
        &mut page.margin_top,
        &mut page.margin_right,
        &mut page.margin_bottom,
        &mut page.margin_left,
    ] {
        if !m.is_finite() || *m < 0.0 {
            *m = 0.0;
        }
    }
    let max_horiz = (page.width_pt - MIN_PRINTABLE_PT).max(0.0);
    let horiz = page.margin_left + page.margin_right;
    if horiz > max_horiz && horiz > 0.0 {
        let scale = max_horiz / horiz;
        page.margin_left *= scale;
        page.margin_right *= scale;
    }
    let max_vert = (page.height_pt - MIN_PRINTABLE_PT).max(0.0);
    let vert = page.margin_top + page.margin_bottom;
    if vert > max_vert && vert > 0.0 {
        let scale = max_vert / vert;
        page.margin_top *= scale;
        page.margin_bottom *= scale;
    }
}

pub(super) fn apply_page_decl(page: &mut PageStyle, key: &str, val: &str) {
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
            (
                dims[0].clamp(MIN_PAGE_DIM_PT, 2000.0),
                dims[1].clamp(MIN_PAGE_DIM_PT, 2000.0),
            )
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

pub(super) fn parse_declarations(
    body: &str,
    props: &mut StyleProps,
    deadline: Option<&RenderDeadline>,
) -> Result<(), RenderError> {
    for (idx, decl) in body.split(';').enumerate() {
        if idx & 0xff == 0 {
            check_deadline(deadline)?;
        }
        let Some((raw_k, raw_v)) = decl.split_once(':') else {
            continue;
        };
        let key = raw_k.trim().to_ascii_lowercase();
        let val = raw_v.trim().trim_end_matches("!important").trim();
        apply_style_decl(props, &key, val);
    }
    Ok(())
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
    let pt = parsed * factor;
    (parsed >= 0.0 && pt.is_finite() && pt <= MAX_LENGTH_PT).then_some(pt)
}

pub(super) fn parse_color(raw: &str) -> Option<Rgb> {
    let s = raw.trim().to_ascii_lowercase();
    if let Some(hex) = s.strip_prefix('#') {
        return parse_hex_color(hex);
    }
    if let Some(inner) = s.strip_prefix("rgb(").and_then(|v| v.strip_suffix(')')) {
        let parts: Vec<f32> = inner
            .split(',')
            .filter_map(|p| p.trim().parse::<f32>().ok())
            .collect();
        if parts.len() == 3 && parts.iter().all(|v| v.is_finite()) {
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
