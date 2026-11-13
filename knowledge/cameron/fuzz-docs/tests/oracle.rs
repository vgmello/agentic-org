mod common;
use common::*;
use org_docs::*;
use serde_json::Value;
use std::time::{Duration, Instant};

#[derive(Debug, PartialEq)]
enum O { Null, Bool(bool), Int(i64), Float(f64), Bad, Str }

fn oracle(s: &str) -> O {
    match s {
        "" | "~" | "null" | "Null" | "NULL" => return O::Null,
        "true" | "True" | "TRUE" => return O::Bool(true),
        "false" | "False" | "FALSE" => return O::Bool(false),
        _ => {}
    }
    let b: Vec<char> = s.chars().collect();
    // int dec
    let mut i = 0;
    if i < b.len() && (b[i] == '-' || b[i] == '+') { i += 1; }
    let sign_len = i;
    let all = |r: &[char], f: &dyn Fn(char) -> bool| !r.is_empty() && r.iter().all(|&c| f(c));
    if all(&b[sign_len..], &|c| c.is_ascii_digit()) {
        return s.parse::<i64>().map(O::Int).unwrap_or(O::Bad);
    }
    if b.len() > 2 && b[0] == '0' && b[1] == 'o' && all(&b[2..], &|c| ('0'..='7').contains(&c)) {
        return i64::from_str_radix(&s[2..], 8).map(O::Int).unwrap_or(O::Bad);
    }
    if b.len() > 2 && b[0] == '0' && b[1] == 'x' && all(&b[2..], &|c| c.is_ascii_hexdigit()) {
        return i64::from_str_radix(&s[2..], 16).map(O::Int).unwrap_or(O::Bad);
    }
    let rest: String = b[sign_len..].iter().collect();
    if matches!(rest.as_str(), ".inf" | ".Inf" | ".INF") { return O::Bad; }
    if matches!(s, ".nan" | ".NaN" | ".NAN") { return O::Bad; }
    // float regex by state machine on rest
    let r: Vec<char> = rest.chars().collect();
    let mut j = 0;
    let digits = |j: &mut usize| { let st = *j; while *j < r.len() && r[*j].is_ascii_digit() { *j += 1; } *j - st };
    let mut ok = true;
    let mut has_dot = false;
    if j < r.len() && r[j] == '.' {
        j += 1; has_dot = true;
        if digits(&mut j) == 0 { ok = false; }
    } else {
        if digits(&mut j) == 0 { ok = false; }
        else if j < r.len() && r[j] == '.' { j += 1; has_dot = true; digits(&mut j); }
    }
    let mut has_exp = false;
    if ok && j < r.len() && (r[j] == 'e' || r[j] == 'E') {
        j += 1; has_exp = true;
        if j < r.len() && (r[j] == '-' || r[j] == '+') { j += 1; }
        if digits(&mut j) == 0 { ok = false; }
    }
    if ok && j == r.len() && (has_dot || has_exp) {
        let f: f64 = s.parse().unwrap();
        return if f.is_finite() { O::Float(f) } else { O::Bad };
    }
    O::Str
}

#[test]
fn typing_oracle() {
    let secs: u64 = std::env::var("FUZZ_SECS").ok().and_then(|s| s.parse().ok()).unwrap_or(20);
    let alpha: Vec<char> = "0123456789012345+-..eEee_xXoO0abfABFnulNULtrueTRUEfalsy~ ".chars().collect();
    let mut r = Rng(31337);
    let end = Instant::now() + Duration::from_secs(secs);
    let (mut n, mut skipped) = (0u64, 0u64);
    while Instant::now() < end {
        let len = 1 + r.below(8);
        let mut s: String = (0..len).map(|_| alpha[r.below(alpha.len())]).collect();
        if r.chance(10) { s = ["9223372036854775807", "9223372036854775808", "-9223372036854775808", "-9223372036854775809", "0x7FFFFFFFFFFFFFFF", "0x8000000000000000", "1e308", "1e309", "-1e309", "0o777777777777777777777", "0o1000000000000000000000", "+0", "-0", "-0.0", "+.5e-3", "00", "0e0", "0.e0", ".0e0"][r.below(19)].to_string(); }
        let s = s.trim().to_string();
        if s.is_empty() { continue; }
        let text = format!("---\nv: {s}\n---\n");
        let got = parse(&text);
        let want = oracle(&s);
        match got {
            Err(e) if matches!(e.reason, Reason::Syntax(_)) => { skipped += 1; continue; }
            Err(e) => {
                assert!(want == O::Bad && e.reason == Reason::BadNumber && e.field == "v", "{s:?}: got {e}, oracle {want:?}");
            }
            Ok(d) => {
                let v = d.front_matter.unwrap()["v"].clone();
                let ok = match (&want, &v) {
                    (O::Null, Value::Null) => true,
                    (O::Bool(a), Value::Bool(b)) => a == b,
                    (O::Int(a), Value::Number(b)) => b.as_i64() == Some(*a),
                    (O::Float(a), Value::Number(b)) => b.as_i64().is_none() && b.as_f64() == Some(*a),
                    (O::Str, Value::String(b)) => b == &s,
                    _ => false,
                };
                assert!(ok, "{s:?}: got {v:?}, oracle {want:?}");
            }
        }
        // as a key: strings only
        let text = format!("---\n{s}: 1\n---\n");
        if let Ok(d) = parse(&text) {
            assert!(want == O::Str, "key {s:?} accepted but oracle {want:?}");
            assert!(d.front_matter.unwrap().contains_key(&s));
        }
        n += 1;
    }
    eprintln!("typing oracle: {n} cases ({skipped} syntax-skipped)");
}

#[test]
fn flow_threshold() {
    for d in [17, 18, 20, 30, 50, 64, 100, 128, 200, 256, 300, 500, 1000] {
        let t = format!("---\na: {}1{}\n---\n", "[".repeat(d), "]".repeat(d));
        eprintln!("flow seq {d}: {:?}", parse(&t).err().map(|e| (e.field.len(), e.reason)));
        let t = format!("---\n{}1{}\n---\n", "{a: ".repeat(d), "}".repeat(d));
        eprintln!("root flow map {d}: {:?}", parse(&t).err().map(|e| (e.field.len(), e.reason)));
    }
    for n in [300, 330, 340] {
        let y: String = (0..n).map(|i| format!("{}k:\n", " ".repeat(i))).collect();
        let t = format!("---\n{y}---\n");
        eprintln!("block maps {n} ({} bytes): {:?}", y.len(), parse(&t).err().map(|e| (e.field.len(), e.reason)));
    }
}
