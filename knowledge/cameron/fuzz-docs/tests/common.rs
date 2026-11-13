#![allow(dead_code)]
use serde_json::{Map, Number, Value};
use std::time::{Duration, Instant};

pub struct Rng(pub u64);
impl Rng {
    pub fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x.wrapping_mul(0x2545F4914F6CDD1D)
    }
    pub fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
    pub fn chance(&mut self, n: usize) -> bool {
        self.below(n) == 0
    }
    pub fn pick<'a, T>(&mut self, xs: &'a [T]) -> &'a T {
        &xs[self.below(xs.len())]
    }
}

/// parse with a time budget; returns result summary. Panics are caught.
pub fn timed(text: &str, what: &str) -> Result<(), String> {
    let t = Instant::now();
    let r = std::panic::catch_unwind(|| org_docs::parse(text).map(|d| d.front_matter.is_some()));
    let el = t.elapsed();
    if r.is_err() {
        let shown: String = text.chars().take(200).collect();
        panic!("PANIC on {what}: {shown:?}");
    }
    if el > Duration::from_millis(500) {
        let shown: String = text.chars().take(120).collect();
        panic!("SLOW {el:?} ({} bytes) on {what}: {shown:?}", text.len());
    }
    Ok(())
}

pub const TRICKY_STRINGS: &[&str] = &[
    "", " ", "no", "yes", "on", "off", "No", "y", "n", "null", "Null", "NULL", "~", "true", "True", "TRUE",
    "false", "FALSE", "tRUE", "0", "007", "0777", "1_000", "0x1F", "0o17", "0x", "0o", ".5", "1e3", "-.inf",
    ".inf", ".nan", "+1", "-1", "1.", ".", "1e", "1e+", ".e5", "e5", "1.5.2", "1e5e5", "12:30", "2026-11-12",
    "<<", "<<:", "a<<b", "- a", "? a", ": a", "a: b", "a #b", "#a", "&x", "*x", "!x", "!!str", "|", ">", "'",
    "\"", "\\", "\n", "\r\n", "a\nb", "a\n\nb", "  lead", "trail  ", "\t", "a\tb", "[", "]", "{", "}", ",", "%",
    "@", "`", "---", "...", "--- ", "é", "中文", "😀", "a\u{2028}b", "a\u{2029}b", "a\u{0085}b", "\u{feff}",
    "\u{200b}", "\u{7f}", "\u{0}", "\u{1}", "\u{fffe}", "\u{ffff}", "\u{10ffff}", "9223372036854775807",
    "9223372036854775808", "-9223372036854775808", "-9223372036854775809", "1.0", "-0", "-0.0", "0.1",
    "Infinity", "NaN", "inf", "0b101", "1,000", "0x_1", "=", "=:", "key", "k.e.y", "a[0]", "a.b",
];

pub fn random_string(r: &mut Rng) -> String {
    if r.chance(2) {
        return (*r.pick(TRICKY_STRINGS)).to_string();
    }
    let n = r.below(12);
    let mut s = String::new();
    for _ in 0..n {
        let c = match r.below(6) {
            0 => char::from_u32(r.below(0x80) as u32).unwrap(),
            1 => char::from_u32(0x80 + r.below(0x700) as u32).unwrap(),
            2 => char::from_u32(0x2000 + r.below(0x100) as u32).unwrap(),
            3 => char::from_u32(0x1F300 + r.below(0x200) as u32).unwrap(),
            _ => (b'a' + r.below(26) as u8) as char,
        };
        s.push(c);
    }
    s
}

pub fn random_float(r: &mut Rng) -> f64 {
    loop {
        let f = match r.below(5) {
            0 => f64::from_bits(r.next()),
            1 => (r.below(2000) as f64 - 1000.0) / 8.0,
            2 => [0.0, -0.0, 1.0, -1.0, f64::MAX, f64::MIN, f64::MIN_POSITIVE, 5e-324, 1e21, 1e-7, 123456789.125, 0.1, 1e15, 1e16, 1e300][r.below(15)],
            3 => (r.next() % 1000) as f64 * 10f64.powi(r.below(40) as i32 - 20),
            _ => r.below(100) as f64,
        };
        if f.is_finite() {
            return f;
        }
    }
}

pub fn random_int(r: &mut Rng) -> i64 {
    match r.below(4) {
        0 => [i64::MIN, i64::MAX, 0, -1, 1, i64::MAX - 1, i64::MIN + 1, 777, 1_000][r.below(9)],
        1 => r.next() as i64,
        2 => (r.next() % 2000) as i64 - 1000,
        _ => (r.next() >> r.below(63)) as i64,
    }
}

pub fn gen_value(r: &mut Rng, depth_left: usize) -> Value {
    let k = if depth_left == 0 { r.below(5) } else { r.below(8) };
    match k {
        0 => Value::Null,
        1 => Value::Bool(r.chance(2)),
        2 => Value::from(random_int(r)),
        3 => Value::Number(Number::from_f64(random_float(r)).unwrap()),
        4 => Value::String(random_string(r)),
        5 | 6 => {
            let n = r.below(5);
            Value::Array((0..n).map(|_| gen_value(r, depth_left - 1)).collect())
        }
        _ => Value::Object(gen_map(r, depth_left - 1)),
    }
}

pub fn gen_key(r: &mut Rng) -> String {
    loop {
        let k = random_string(r);
        if k != "<<" {
            return k;
        }
    }
}

pub fn gen_map(r: &mut Rng, depth_left: usize) -> Map<String, Value> {
    let n = r.below(5);
    let mut m = Map::new();
    for _ in 0..n {
        let k = gen_key(r);
        m.insert(k, gen_value(r, depth_left));
    }
    m
}

fn is_plain_safe(s: &str) -> bool {
    // letters, digits, _ . - ; starts with a letter; not a null/bool keyword
    let mut cs = s.chars();
    match cs.next() {
        Some(c) if c.is_ascii_alphabetic() => {}
        _ => return false,
    }
    if !s.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '.' || c == '-') {
        return false;
    }
    !matches!(s, "null" | "Null" | "NULL" | "true" | "True" | "TRUE" | "false" | "False" | "FALSE")
}

/// Double-quoted scalar with explicit \u escapes for everything but printable ASCII/plain unicode.
pub fn quote(s: &str, raw_unicode: bool) -> String {
    let mut out = String::from("\"");
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 || c as u32 == 0x7f => out.push_str(&format!("\\u{:04x}", c as u32)),
            c if !raw_unicode && !c.is_ascii() => {
                let n = c as u32;
                if n > 0xFFFF {
                    out.push_str(&format!("\\U{:08x}", n));
                } else {
                    out.push_str(&format!("\\u{:04x}", n));
                }
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

#[derive(Clone, Copy, PartialEq)]
pub enum Style {
    Block,
    Flow,
    Mixed,
}

pub fn scalar(v: &Value, r: &mut Rng, raw: bool, in_flow: bool) -> String {
    match v {
        Value::Null => ["null", "~", "Null", "NULL"][r.below(4)].to_string(),
        Value::Bool(b) => {
            let t = ["true", "True", "TRUE"][r.below(3)];
            let f = ["false", "False", "FALSE"][r.below(3)];
            (if *b { t } else { f }).to_string()
        }
        Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                match r.below(5) {
                    0 if i >= 0 => format!("+{i}"),
                    1 if i >= 0 => format!("0x{:X}", i),
                    2 if i >= 0 => format!("0o{:o}", i),
                    3 if i >= 0 && i < 1_000_000 => format!("000{i}"),
                    _ => format!("{i}"),
                }
            } else {
                let f = n.as_f64().unwrap();
                match r.below(3) {
                    0 => format!("{:?}", f),
                    1 => format!("{:e}", f),
                    _ => format!("{:E}", f),
                }
            }
        }
        Value::String(s) => {
            if !in_flow && is_plain_safe(s) && r.chance(2) {
                s.clone()
            } else if r.chance(4) && !s.contains('\'') && s.chars().all(|c| !c.is_control() && c != '\u{85}' && c != '\u{2028}' && c != '\u{2029}' && c != '\u{feff}') {
                format!("'{}'", s)
            } else {
                quote(s, raw)
            }
        }
        _ => unreachable!(),
    }
}

pub fn key(k: &str, r: &mut Rng, raw: bool) -> String {
    if is_plain_safe(k) && r.chance(2) {
        k.to_string()
    } else {
        quote(k, raw)
    }
}

pub fn flow(v: &Value, r: &mut Rng, raw: bool) -> String {
    match v {
        Value::Array(a) => {
            let sp = if r.chance(2) { " " } else { "" };
            let items: Vec<String> = a.iter().map(|x| flow(x, r, raw)).collect();
            format!("[{sp}{}{sp}]", items.join(&format!(",{sp}")))
        }
        Value::Object(m) => {
            let items: Vec<String> = m.iter().map(|(k, x)| format!("{}: {}", quote(k, raw), flow(x, r, raw))).collect();
            format!("{{{}}}", items.join(", "))
        }
        _ => scalar(v, r, raw, true),
    }
}

fn is_coll(v: &Value) -> bool {
    matches!(v, Value::Array(a) if !a.is_empty()) || matches!(v, Value::Object(m) if !m.is_empty())
}

/// Emit block `v` (a non-empty collection) at `indent`; each line terminated by nl.
pub fn block(v: &Value, indent: usize, style: Style, r: &mut Rng, raw: bool, nl: &str, out: &mut String) {
    let pad = " ".repeat(indent);
    match v {
        Value::Object(m) => {
            for (k, x) in m {
                out.push_str(&pad);
                out.push_str(&key(k, r, raw));
                out.push(':');
                emit_child(x, indent + 2, style, r, raw, nl, out, false);
            }
        }
        Value::Array(a) => {
            for x in a {
                out.push_str(&pad);
                out.push('-');
                emit_child(x, indent + 2, style, r, raw, nl, out, true);
            }
        }
        _ => unreachable!(),
    }
}

fn emit_child(x: &Value, indent: usize, style: Style, r: &mut Rng, raw: bool, nl: &str, out: &mut String, in_seq: bool) {
    let want_flow = match style {
        Style::Flow => true,
        Style::Block => false,
        Style::Mixed => r.chance(3),
    };
    if is_coll(x) && !want_flow {
        // sequence item that is a mapping/seq can be compact: "- k: v"
        if in_seq && r.chance(2) {
            let mut tmp = String::new();
            block(x, indent, style, r, raw, nl, &mut tmp);
            // replace the first line's pad with a single space
            let stripped = &tmp[indent..];
            out.push(' ');
            out.push_str(stripped);
        } else {
            out.push_str(nl);
            block(x, indent, style, r, raw, nl, out);
        }
    } else if is_coll(x) || matches!(x, Value::Array(_) | Value::Object(_)) {
        out.push(' ');
        out.push_str(&flow(x, r, raw));
        out.push_str(nl);
    } else {
        out.push(' ');
        out.push_str(&scalar(x, r, raw, false));
        out.push_str(nl);
    }
}

pub fn emit_doc(m: &Map<String, Value>, style: Style, r: &mut Rng, raw: bool, nl: &str) -> String {
    if m.is_empty() {
        return format!("{{}}{nl}");
    }
    if style == Style::Flow && r.chance(2) {
        return format!("{}{nl}", flow(&Value::Object(m.clone()), r, raw));
    }
    let mut out = String::new();
    block(&Value::Object(m.clone()), 0, style, r, raw, nl, &mut out);
    out
}

pub fn depth(v: &Value) -> usize {
    match v {
        Value::Array(a) => 1 + a.iter().map(depth).max().unwrap_or(0),
        Value::Object(m) => 1 + m.values().map(depth).max().unwrap_or(0),
        _ => 0,
    }
}
