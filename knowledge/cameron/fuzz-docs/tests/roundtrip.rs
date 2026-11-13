mod common;
use common::*;
use serde_json::Value;
use std::time::{Duration, Instant};

fn budget() -> Duration {
    Duration::from_secs(std::env::var("FUZZ_SECS").ok().and_then(|s| s.parse().ok()).unwrap_or(60))
}

fn one(r: &mut Rng, style: Style, raw: bool, crlf: bool, fails: &mut Vec<String>) {
    let m = gen_map(r, 13);
    let nl = if crlf { "\r\n" } else { "\n" };
    let y = emit_doc(&m, style, r, raw, nl);
    let body = random_string(r);
    let text = format!("---{nl}{y}---{nl}{body}");
    let t = Instant::now();
    let res = org_docs::parse(&text);
    assert!(t.elapsed() < Duration::from_millis(500), "slow {:?}", text);
    let want = Value::Object(m);
    match res {
        Ok(d) => {
            if d.front_matter.map(Value::Object) != Some(want.clone()) || d.body != body {
                fails.push(format!("MISMATCH text={text:?}\n want={want}\n"));
            }
        }
        Err(e) => fails.push(format!("REFUSED {e} text={text:?}")),
    }
}

#[test]
fn roundtrip_all_styles() {
    let mut r = Rng(0x9E3779B97F4A7C15);
    let end = Instant::now() + budget();
    let mut n = 0u64;
    let mut fails = vec![];
    while Instant::now() < end {
        for style in [Style::Block, Style::Flow, Style::Mixed] {
            for crlf in [false, true] {
                one(&mut r, style, false, crlf, &mut fails);
                n += 1;
            }
        }
        if fails.len() > 20 { break; }
    }
    eprintln!("roundtrip cases: {n}, failures: {}", fails.len());
    for f in fails.iter().take(20) { eprintln!("{f}"); }
    assert!(fails.is_empty());
}

#[test]
fn roundtrip_raw_unicode() {
    let mut r = Rng(0xDEADBEEFCAFEF00D);
    let end = Instant::now() + budget() / 2;
    let mut n = 0u64;
    let mut fails = vec![];
    while Instant::now() < end {
        one(&mut r, Style::Mixed, true, false, &mut fails);
        n += 1;
        if fails.len() > 20 { break; }
    }
    eprintln!("raw cases: {n}, failures: {}", fails.len());
    for f in fails.iter().take(20) { eprintln!("{f}"); }
    assert!(fails.is_empty());
}

#[test]
fn crlf_equals_lf() {
    let mut r = Rng(0x1234567);
    let end = Instant::now() + budget() / 4;
    let mut n = 0;
    while Instant::now() < end {
        let m = gen_map(&mut r, 10);
        let y = emit_doc(&m, Style::Mixed, &mut r, false, "\n");
        let lf = format!("---\n{y}---\nb\n");
        let crlf = lf.replace('\n', "\r\n");
        let a = org_docs::parse(&lf).unwrap().front_matter;
        let b = org_docs::parse(&crlf).unwrap().front_matter;
        assert_eq!(a, b, "{lf:?}");
        n += 1;
    }
    eprintln!("crlf cases {n}");
}

#[test]
fn sample() {
    let mut r = Rng(42);
    for _ in 0..3 {
        let m = gen_map(&mut r, 6);
        eprintln!("{}\n=====", emit_doc(&m, Style::Mixed, &mut r, false, "\n"));
    }
    let mut maxd = 0;
    for _ in 0..20000 { let m = gen_map(&mut r, 13); maxd = maxd.max(depth(&serde_json::Value::Object(m))); }
    eprintln!("max depth generated {maxd}");
}
