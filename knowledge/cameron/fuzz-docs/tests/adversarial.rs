mod common;
use common::*;
use org_docs::*;
use std::time::{Duration, Instant};

fn budget() -> Duration {
    Duration::from_secs(std::env::var("FUZZ_SECS").ok().and_then(|s| s.parse().ok()).unwrap_or(60))
}

fn wrap(y: &str) -> String { format!("---\n{y}---\nbody\n") }

fn run(name: &str, y: &str) -> Result<(), FrontMatterError> {
    let text = wrap(y);
    let t = Instant::now();
    let r = std::panic::catch_unwind(|| parse(&text).map(|_| ()));
    let el = t.elapsed();
    let r = r.unwrap_or_else(|_| panic!("PANIC {name}"));
    eprintln!("{name:40} len={:6} {:>8.1?} -> {}", y.len(), el, match &r { Ok(_) => "ok".to_string(), Err(e) => format!("{e}") });
    assert!(el < Duration::from_millis(500), "SLOW {name}: {el:?}");
    r
}

#[test]
fn shapes_at_limit() {
    let n = MAX_FRONT_MATTER_BYTES;
    let mut shapes: Vec<(String, String)> = vec![];
    let fill = |unit: &str, total: usize| -> String { unit.repeat(total / unit.len() + 1)[..total].to_string() };
    for unit in ["[", "{", "- ", "a: &x ", "?", "? ", "a: ", "a:\n", "- - ", "? ? ", "a: [", "a: {", "&x ", "!t ", "*x ", "[a, ", "{a: ", "'", "\"", "|", "> ", "# ", "a", "a ", "a b ", "a\n", "a\n ", "x: a\n  ", "- a\n  ", "-", "\"a\" ", ": ", "- ? ", "[[", "{{", "\t", " ", "\n", "a: 1\n", "a:\n  - ", "[a]: ", "---\n"] {
        for (suffix, tag) in [("", ""), ("\n", "+nl")] {
            let mut s = fill(unit, n - 1);
            s.push_str(suffix);
            shapes.push((format!("fill {unit:?}{tag}"), s));
        }
    }
    // many keys
    let mut s = String::new(); let mut i = 0; while s.len() < n - 16 { s.push_str(&format!("k{i}: v\n")); i += 1; }
    shapes.push(("many keys".into(), s));
    let mut s = String::new(); let mut i = 0; while s.len() < n - 16 { s.push_str(&format!("k{i}: [1,2]\n")); i += 1; }
    shapes.push(("many keys with flow".into(), s));
    let mut s = String::from("a:\n"); while s.len() < n - 16 { s.push_str("  - 1\n"); }
    shapes.push(("long seq".into(), s));
    let mut s = String::from("a: [");  while s.len() < n - 16 { s.push_str("1,"); } s.push_str("]\n");
    shapes.push(("long flow seq".into(), s));
    shapes.push(("long plain scalar".into(), format!("a: {}\n", "x ".repeat(n / 2 - 8))));
    shapes.push(("long multiline plain".into(), format!("a: x\n{}", "  y\n".repeat(n / 4 - 4))));
    shapes.push(("long quoted multiline".into(), format!("a: \"x\n{}\"\n", "  y\n".repeat(n / 4 - 4))));
    shapes.push(("long block scalar".into(), format!("a: |\n{}", "  y\n".repeat(n / 4 - 4))));
    shapes.push(("long comment".into(), format!("# {}\na: 1\n", "x".repeat(n - 20))));
    shapes.push(("many comments".into(), "#\n".repeat(n / 2 - 8) + "a: 1\n"));
    shapes.push(("many blank lines".into(), "\n".repeat(n - 8) + "a: 1\n"));
    shapes.push(("many docs".into(), "a: 1\n--- \n".repeat(n / 12)));
    shapes.push(("many ... ".into(), "a: 1\n...\n".repeat(n / 10)));
    shapes.push(("many anchors in one line".into(), format!("a: [{}]\n", "&x 1, ".repeat(n / 6 - 4))));
    shapes.push(("many aliases".into(), format!("a: [{}]\n", "*x, ".repeat(n / 4 - 4))));
    shapes.push(("many tags".into(), format!("a: [{}]\n", "!t 1, ".repeat(n / 6 - 4))));
    shapes.push(("many empty flow maps".into(), format!("a: [{}]\n", "{},".repeat(n / 3 - 4))));
    shapes.push(("many ? keys".into(), "? a\n: b\n".repeat(n / 9)));
    shapes.push(("many single key flow".into(), format!("a: {{{}}}\n", "a: 1,".repeat(n / 5 - 4))));
    shapes.push(("implicit key lookahead".into(), format!("a: [{}x]\n", "b, ".repeat(n / 3 - 4))));
    shapes.push(("long simple key candidates".into(), format!("{}\n", "[a".repeat(n / 2 - 4))));
    shapes.push(("flow in key".into(), format!("{}: 1\n", "[".repeat(8) + &"a,".repeat(n / 2 - 40) + &"]".repeat(8))));
    shapes.push(("nested flow alternating".into(), format!("a: {}\n", "[{a: ".repeat(8))));
    shapes.push(("tabs".into(), "\ta: 1\n".repeat(n / 7)));
    shapes.push(("crlf many".into(), "a: 1\r\n".repeat(n / 6 - 2)));
    shapes.push(("lone cr many".into(), "a: 1\r".repeat(n / 5 - 2)));
    shapes.push(("nul".into(), "\0".repeat(n - 8)));
    shapes.push(("a: nul".into(), "a: \0\n".to_string()));
    for (name, y) in shapes {
        let y = if y.len() > n { y[..n].to_string() } else { y };
        // cut at char boundary
        let mut y = y; while !y.is_char_boundary(y.len()) { y.pop(); }
        if !y.ends_with('\n') { y.push('\n'); }
        // avoid accidental fence lines: skip if any line is exactly '---'
        let _ = run(&name, &y);
    }
}

#[test]
fn depth_shapes() {
    for d in 12..=20 {
        // block mappings: root is depth 1
        let mut y = String::new();
        for i in 0..d - 1 { y.push_str(&format!("{}k:\n", "  ".repeat(i))); }
        y.push_str(&format!("{}k: 1\n", "  ".repeat(d - 1)));
        let r = run(&format!("block maps depth {d}"), &y);
        assert_eq!(r.is_ok(), d <= MAX_DEPTH, "block maps depth {d}: {r:?}");
        // block seqs under root key: depth = 1 + n
        let mut y = String::from("a:\n");
        for i in 0..d - 2 { y.push_str(&format!("{}-\n", "  ".repeat(i + 1))); }
        y.push_str(&format!("{}- 1\n", "  ".repeat(d - 1)));
        let r = run(&format!("block seqs depth {d}"), &y);
        assert_eq!(r.is_ok(), d <= MAX_DEPTH, "block seqs depth {d}: {r:?}");
        // compact "- - - 1"
        let y = format!("a: {}1\n", "- ".repeat(d - 1));
        let _ = y;
        let y = format!("a:\n  {}1\n", "- ".repeat(d - 1));
        let r = run(&format!("compact seqs depth {d}"), &y);
        assert_eq!(r.is_ok(), d <= MAX_DEPTH, "compact seqs depth {d}: {r:?}");
        // flow
        let y = format!("a: {}1{}\n", "[".repeat(d - 1), "]".repeat(d - 1));
        let r = run(&format!("flow seqs depth {d}"), &y);
        assert_eq!(r.is_ok(), d <= MAX_DEPTH, "flow seq depth {d}: {r:?}");
        let y = format!("a: {}1{}\n", "{a: ".repeat(d - 1), "}".repeat(d - 1));
        let r = run(&format!("flow maps depth {d}"), &y);
        assert_eq!(r.is_ok(), d <= MAX_DEPTH, "flow maps depth {d}: {r:?}");
        // key-as-nested map: "a: {b: [..]}" mixture
        let mut y = String::from("a: ");
        for i in 0..d - 1 { y.push_str(if i % 2 == 0 { "[" } else { "{k: " }); }
        y.push('1');
        for i in (0..d - 1).rev() { y.push_str(if i % 2 == 0 { "]" } else { "}" }); }
        y.push('\n');
        let r = run(&format!("flow mixed depth {d}"), &y);
        assert_eq!(r.is_ok(), d <= MAX_DEPTH, "flow mixed depth {d}: {r:?}");
        // root flow mapping
        let y = format!("{}1{}\n", "{a: ".repeat(d), "}".repeat(d));
        let r = run(&format!("root flow maps depth {d}"), &y);
        assert_eq!(r.is_ok(), d <= MAX_DEPTH, "root flow maps depth {d}: {r:?}");
        // explicit key '?' nesting
        let mut y = String::new();
        for i in 0..d - 1 { y.push_str(&format!("{}a:\n", "  ".repeat(i))); }
        let _ = y;
    }
    // very deep must not overflow
    for d in [100, 1000, 20000] {
        let _ = run(&format!("flow seq {d}"), &format!("a: {}1{}\n", "[".repeat(d), "]".repeat(d)));
        let _ = run(&format!("unclosed flow seq {d}"), &format!("a: {}\n", "[".repeat(d)));
        let _ = run(&format!("block maps {d}"), &(0..d.min(3000)).map(|i| format!("{}k:\n", " ".repeat(i))).collect::<String>());
    }
}

#[test]
fn event_boundary() {
    for n in [MAX_EVENTS - 12, MAX_EVENTS - 9, MAX_EVENTS - 8, MAX_EVENTS - 7, MAX_EVENTS] {
        let y = format!("a: [{}]\n", vec!["0"; n].join(","));
        let r = run(&format!("flow items {n}"), &y);
        eprintln!("   => {r:?}");
    }
    // block seq events
    for n in [MAX_EVENTS - 10, MAX_EVENTS - 8, MAX_EVENTS] {
        let y = format!("a:\n{}", "  - 0\n".repeat(n));
        let r = run(&format!("block items {n}"), &y);
        eprintln!("   => {r:?}");
    }
    // many keys
    for n in [4900, 4995, 4996, 5000] {
        let y: String = (0..n).map(|i| format!("k{i}: 0\n")).collect();
        let r = run(&format!("keys {n}"), &y);
        eprintln!("   => {:?}", r.map(|_| ()));
    }
}

#[test]
fn utf8_boundary() {
    let n = MAX_FRONT_MATTER_BYTES;
    for ch in ["é", "中", "😀"] {
        let w = ch.len();
        for slack in 0..=8usize {
            // total len of yaml about n - slack .. n + slack
            for delta in -4i32..=4 {
                let target = (n as i32 + delta) as usize;
                let head = "a: \"";
                let tail = "\"\n";
                let room = target.saturating_sub(head.len() + tail.len());
                let k = room / w;
                let mut y = format!("{head}{}", ch.repeat(k));
                y.push_str("x".repeat(room - k * w).as_str());
                y.push_str(tail);
                let r = run(&format!("utf8 {ch} slack{slack} delta{delta}"), &y);
                assert_eq!(r.is_ok(), y.len() <= n, "len {}: {r:?}", y.len());
                if slack > 0 { break; }
            }
        }
    }
}

#[test]
fn bom_variants() {
    let cases: Vec<(&str, String)> = vec![
        ("bom", "\u{feff}---\na: 1\n---\nb".into()),
        ("two boms", "\u{feff}\u{feff}---\na: 1\n---\nb".into()),
        ("bom in front", "---\n\u{feff}a: 1\n---\nb".into()),
        ("bom before closing", "---\na: 1\n\u{feff}---\nb".into()),
        ("bom body", "---\na: 1\n---\n\u{feff}b".into()),
        ("bom only", "\u{feff}".into()),
        ("empty", "".into()),
        ("nul", "---\n\0---\n".into()),
        ("nul in key", "---\na\0b: 1\n---\n".into()),
        ("lone cr", "---\ra: 1\r---\r".into()),
        ("cr fences", "---\r\na: 1\r\n---\r\nbody\r\n".into()),
        ("cr cr fence close", "---\na: 1\n---\r\r\nbody".into()),
        ("bare cr close at eof", "---\na: 1\n---\r".into()),
        ("crlf open lf close", "---\r\na: 1\n---\nbody".into()),
        ("open w/ nothing", "---\n".into()),
        ("open crlf nothing", "---\r\n".into()),
        ("---\\n---", "---\n---".into()),
        ("tabs fence", "---\t\na: 1\n---\n".into()),
        ("fence in comment", "---\n# ---\na: 1\n---\n".into()),
        ("4 dashes close", "---\na: 1\n----\n---\nx".into()),
    ];
    for (name, t) in cases {
        let r = std::panic::catch_unwind(|| parse(&t).map(|d| (d.front_matter.is_some(), d.body.to_string())));
        eprintln!("{name:24} {:?} -> {:?}", t, r.expect("panic"));
    }
}

fn alphabet_fuzz(seed: u64, secs: Duration, alpha: &[&str], maxlen: usize, wrapit: bool) -> (u64, Duration) {
    let mut r = Rng(seed);
    let end = Instant::now() + secs;
    let mut n = 0;
    let mut worst = Duration::ZERO;
    while Instant::now() < end {
        let len = r.below(maxlen);
        let mut y = String::new();
        for _ in 0..len { y.push_str(alpha.get(r.below(alpha.len())).unwrap()); }
        let text = if wrapit {
            if y.lines().any(|l| l.trim_end_matches(['\r']) == "---") { continue; }
            format!("---\n{y}\n---\nb")
        } else { y };
        let t = Instant::now();
        let res = std::panic::catch_unwind(|| { let _ = parse(&text); });
        let el = t.elapsed();
        if res.is_err() { panic!("PANIC on {text:?}"); }
        worst = worst.max(el);
        if el > Duration::from_millis(500) { panic!("SLOW {el:?} {text:?}"); }
        n += 1;
    }
    (n, worst)
}

#[test]
fn fuzz_structural_fragments() {
    let alpha = ["-", ":", "?", "[", "]", "{", "}", ",", "&a", "*a", "!t", "!!str", "|", ">", "|-", ">+", "'", "\"", "#", "%", "@", "`", " ", "  ", "\n", "\n  ", "\n- ", "\n? ", "\n: ", "\t", "a", "b", "01", ".", "~", "<<", "\\", "\r", "\r\n", "...", "--- ", "\u{feff}", "é", "\0", "\u{85}", "\u{2028}", "null", "1", ": ", "- ", "? ", "{a: ", "[a, ", "&x ", "key: ", "!<tag> ", "%YAML 1.2\n", "%TAG ! x:\n"];
    let secs = budget();
    let (n, w) = alphabet_fuzz(1, secs / 3, &alpha, 60, true);
    eprintln!("fragments(wrapped): {n} cases, worst {w:?}");
    let (n, w) = alphabet_fuzz(2, secs / 3, &alpha, 200, true);
    eprintln!("fragments(wrapped, long): {n} cases, worst {w:?}");
    let (n, w) = alphabet_fuzz(3, secs / 3, &alpha, 80, false);
    eprintln!("fragments(raw text): {n} cases, worst {w:?}");
}

#[test]
fn fuzz_arbitrary_unicode() {
    let mut r = Rng(77);
    let end = Instant::now() + budget() / 3;
    let mut n = 0u64;
    let mut worst = Duration::ZERO;
    while Instant::now() < end {
        let len = r.below(80);
        let mut s = String::new();
        if r.chance(2) { s.push_str(if r.chance(2) { "---\n" } else { "\u{feff}---\r\n" }); }
        for _ in 0..len {
            let c = match r.below(5) {
                0 => char::from_u32(r.below(0x110000) as u32),
                1 => char::from_u32(r.below(0x3000) as u32),
                2 => Some(*r.pick(&['\n', '\r', '-', ':', ' ', '"', '\'', '#', '\u{85}', '\u{2028}', '\u{feff}', '\0', '\t', '[', '{', '&', '*', '!', '|', '>', '?', '%'])),
                _ => Some((b' ' + r.below(95) as u8) as char),
            };
            if let Some(c) = c { s.push(c); }
        }
        if r.chance(2) { s.push_str("\n---\n"); }
        let t = Instant::now();
        let res = std::panic::catch_unwind(|| { let _ = parse(&s); });
        let el = t.elapsed();
        if res.is_err() { panic!("PANIC on {s:?}"); }
        worst = worst.max(el);
        assert!(el < Duration::from_millis(500), "SLOW {el:?} {s:?}");
        n += 1;
    }
    eprintln!("unicode: {n} cases worst {worst:?}");
}

#[test]
fn split_properties() {
    let mut r = Rng(5);
    let end = Instant::now() + budget() / 4;
    let mut n = 0;
    while Instant::now() < end {
        let body = {
            let mut b = String::new();
            for _ in 0..r.below(6) { b.push_str(*r.pick(&["---\n", "---", "a\n", "\r\n", "\n", "\u{feff}", "...\n", "# T\n", "é", "\r", "--- \n", "a: 1\n"])); }
            b
        };
        // no front matter: text not starting with exact fence
        let start = *r.pick(&["", "# x\n", "--- \n", "----\n", "---", "--\n", " ---\n", "\n---\n", "---\t\n", "\u{feff}\u{feff}---\n"]);
        let t = format!("{start}{body}");
        if t.strip_prefix('\u{feff}').unwrap_or(&t).starts_with("---\n") || t.strip_prefix('\u{feff}').unwrap_or(&t).starts_with("---\r\n") { continue; }
        let d = parse(&t).unwrap_or_else(|e| panic!("{t:?}: {e}"));
        let exp = t.strip_prefix('\u{feff}').unwrap_or(&t);
        if d.front_matter.is_some() || d.body != exp {
            // two boms case: after one strip text begins with BOM then '---': no fence
            panic!("no-front-matter text {t:?} -> {:?} body {:?}", d.front_matter, d.body);
        }
        // with front matter: body is exact, for each fence flavour
        for (open, close) in [("---\n", "---\n"), ("---\r\n", "---\r\n"), ("---\n", "---"), ("\u{feff}---\n", "---\r\n")] {
            let t = format!("{open}a: 1\n{close}{body}");
            let t = if close == "---" { format!("{open}a: 1\n---") } else { t };
            let d = parse(&t).unwrap();
            let expect_body = if close == "---" { "" } else { &body[..] };
            assert_eq!(d.body, expect_body, "{t:?}");
            assert!(d.front_matter.is_some());
        }
        n += 1;
    }
    eprintln!("split cases {n}");
}

#[test]
fn scaling() {
    // quadratic detector: time at 64K vs 16K for many unit shapes
    let units = ["[a", "{a: ", "a: [", "- a\n", "a: b\n", "? a\n: b\n", "a:\n  b:\n", "  ", "\n", "x ", "'a' ", "\"a\\n\" ", "a: &x\n", "a: |\n  x\n", "a: >\n  x\n", "- - a\n", "[a, b]: c\n", "a: 'x\n", "k: v # c\n", "!t a: ", "? [", "a: b\n  ", "a: \"\n", "a:\t", "1111111111", "a: 1.", "a: 0x", "{? a: b, ", "[? a, ", "[a: ", "a: -", "- ? a\n  : b\n", "--- \n", "...\n", "%YAML 1.2\n", "&a ", "*a ", "<<: ", "# c\n", "- #\n", "a: \"a\n  \n", "a: 'a\n  \n"];
    let mut worst = (0.0f64, String::new());
    for unit in units {
        let mk = |n: usize| { let mut s = unit.repeat(n / unit.len() + 1); s.truncate(n); while !s.is_char_boundary(s.len()) { s.pop(); } s };
        let mut times = vec![];
        for n in [8 * 1024, 16 * 1024, 32 * 1024, 64 * 1024 - 8] {
            let y = mk(n);
            let t = format!("---\n{y}\n---\n");
            let mut best = Duration::MAX;
            for _ in 0..3 { let st = Instant::now(); let _ = parse(&t); best = best.min(st.elapsed()); }
            times.push(best);
        }
        let ratio = times[3].as_secs_f64() / times[1].as_secs_f64().max(1e-7);
        eprintln!("{unit:?}: {:?} ratio64/16={ratio:.1}", times);
        if ratio > worst.0 && times[3] > Duration::from_micros(500) { worst = (ratio, unit.to_string()); }
    }
    eprintln!("worst ratio {worst:?} (4.0 = linear)");
    // huge numbers
    for y in [format!("a: {}.0\n", "1".repeat(60000)), format!("a: {}\n", "1".repeat(60000)), format!("a: 0x{}\n", "f".repeat(60000)), format!("a: 1e{}\n", "9".repeat(60000)), format!("a: 0.{}1\n", "0".repeat(60000)), format!("a: -{}\n", "0".repeat(60000)), format!("a: 0o{}\n", "7".repeat(60000))] {
        let _ = run("bignum", &y).map_err(|e| eprintln!("  {e}"));
    }
}

#[test]
fn deep_unbounded() {
    for d in [100usize, 1000, 5000, 20000, 60000] {
        let _ = run(&format!("flow seq {d}"), &format!("a: {}1{}\n", "[".repeat(d), "]".repeat(d)));
        let _ = run(&format!("unclosed flow seq {d}"), &format!("a: {}\n", "[".repeat(d)));
        let _ = run(&format!("flow map {d}"), &format!("a: {}1{}\n", "{a: ".repeat(d / 2), "}".repeat(d / 2)));
        let _ = run(&format!("root flow seq {d}"), &format!("{}1{}\n", "[".repeat(d), "]".repeat(d)));
        let _ = run(&format!("root flow map {d}"), &format!("{}1{}\n", "{a: ".repeat(d / 2), "}".repeat(d / 2)));
        let y: String = (0..d.min(2500)).map(|i| format!("{}k:\n", " ".repeat(i))).collect();
        let _ = run(&format!("block maps {d}"), &y);
        let y: String = (0..d.min(2500)).map(|i| format!("{}- \n", " ".repeat(i))).collect();
        let _ = run(&format!("block seq {d}"), &format!("a:\n{y}"));
        let _ = run(&format!("? ? {d}"), &format!("{}a\n", "? ".repeat(d.min(30000))));
        let _ = run(&format!("anchors nested {d}"), &format!("a: {}\n", "&x [".repeat(d / 4)));
    }
}

#[test]
fn refusal_matrix() {
    let cases: &[(&str, &str)] = &[
        ("\"<<\": 1\n", "quoted merge key"),
        ("'<<': 1\n", "single quoted merge key"),
        ("a: {\"<<\": 1}\n", "nested quoted merge"),
        ("a: <<\n", "plain << value"),
        ("a:\n  - <<: 1\n", "merge in seq map"),
        ("<<: *x\n", "merge alias"),
        ("<<: {a: 1}\n<<: {b: 1}\n", "two merges"),
        ("a: 1\na: &x 2\n", "dup before anchor"),
        ("a: &x 1\na: 2\n", "anchor before dup"),
        ("a: !t 1\na: 2\n", "tag before dup"),
        ("a: 99999999999999999999\na: 2\n", "badnumber before dup"),
        ("a: 99999999999999999999\nb: &x 2\n", "badnumber then anchor"),
        ("a: .inf\nb: !t 2\n", "bad then tag"),
        ("a: [1, 2\n", "unclosed"),
        ("a: 1\n--- b\n", "second doc"),
        ("a: 1\n...\nb: 2\n", "docs via ..."),
        ("a: 1\n...\n", "doc end only"),
        ("a: 1\n...\n# c\n", "doc end comment"),
        ("a: 1\n...\n\n", "doc end blank"),
        ("--- a: 1\n", "explicit start same line"),
        ("---\na: 1\n", "explicit start (non fence `--- `)"),
        ("--- \na: 1\n--- \n", "trailing explicit start"),
        ("--- \n--- \n", "two empty docs"),
        ("%YAML 1.2\n--- \na: 1\n", "yaml directive"),
        ("%YAML 1.1\n--- \na: yes\n", "yaml 1.1 directive"),
        ("%TAG ! tag:x,2000:\n--- \na: !t 1\n", "tag directive"),
        ("%TAG ! tag:x,2000:\n--- \na: 1\n", "tag directive unused"),
        ("{a: 1}\n{b: 2}\n", "two flow maps"),
        ("a: 1\n{b: 2}\n", "map then flow"),
        ("a: 1\n- b\n", "map then seq"),
        ("- a\na: 1\n", "seq then map"),
        ("null\n", "null root"),
        ("~\n", "tilde root"),
        ("\"a\"\n", "quoted root"),
        ("*a\n", "alias root"),
        ("&a a\n", "anchored scalar root"),
        ("!t a\n", "tagged scalar root"),
        ("!!str\n", "tag only root"),
        ("a: *x\n", "alias undefined"),
        ("a: &x 1\nb: [*x]\n", "alias seq"),
        ("a: [&x 1, *x]\n", "alias seq 2"),
        ("[&x a]: 1\n", "anchor in complex key"),
        ("? &x a\n: 1\n", "anchor on explicit key"),
        ("? a\n? a\n", "dup explicit keys"),
        ("? a\n: 1\n? a\n: 2\n", "dup explicit keys with values"),
        ("a: 1\n\"a\": 2\n", "dup quoted vs plain"),
        ("a: 1\n'a': 2\n", "dup single"),
        ("\"a\\u0062\": 1\nab: 2\n", "dup escaped"),
        ("a b: 1\n\"a b\": 2\n", "dup spaced"),
        ("a: 1\n? a\n: 2\n", "dup explicit"),
        ("{a: 1, a: 2}\n", "dup flow root"),
        ("{a, a}\n", "dup flow no value"),
        ("{a: 1, b}\n", "flow key no value"),
        ("a: [b: 1, b: 2]\n", "single pair maps in seq"),
        ("a: [? b : 1]\n", "explicit in flow"),
        ("a:\n  - ? x\n    : 1\n", "explicit in seq"),
        ("1: x\n", "int key"),
        ("1.5: x\n", "float key"),
        ("true: x\n", "bool key"),
        ("null: x\n", "null key"),
        ("~: x\n", "tilde key"),
        (": x\n", "empty key"),
        ("? \n: x\n", "explicit empty key"),
        ("{? : x}\n", "flow empty key"),
        ("m:\n  1: x\n", "nested int key"),
        ("a:\n  - m:\n      1: x\n", "deep nonstring key"),
        (".inf: 1\n", "inf key"),
        ("99999999999999999999: 1\n", "huge int key"),
        ("a: [{b: 1, 2: 3}]\n", "nonstring in seq map"),
        ("a: [[1], [2]]: 3\n", "seq as key"),
        ("{a: 1}: x\n", "map as key"),
        ("? a: 1\n: x\n", "map as explicit key"),
        ("a: &x\n", "anchor on empty"),
        ("a: !!null\n", "tag on empty"),
        ("a: !!str ''\n", "tag on quoted"),
        ("a: !<tag:yaml.org,2002:str> x\n", "verbatim tag"),
        ("a: ! x\n", "non-specific tag"),
        ("a: |\n  x\n  y\nb: >-\n  z\n", "block scalars"),
        ("a: !t |\n  x\n", "tagged block"),
        ("a: &x |\n  x\n", "anchored block"),
        ("a: \"\\q\"\n", "bad escape"),
        ("a: 'x\n", "unterminated"),
        ("a: \"x\n", "unterminated dq"),
        ("a\n", "scalar root plain"),
        ("a: 1\n b: 2\n", "bad indent"),
        ("\ta: 1\n", "tab indent"),
        ("a: 1 # c\n# d\nb: 2\n", "comments"),
        ("# only comment\n", "comment only"),
        ("   \n", "spaces only"),
        ("\n\n", "blank"),
        ("{}\n", "empty flow root"),
        ("{ }\n", "empty flow root sp"),
        ("[]\n", "empty seq root"),
        ("a: {}\nb: []\n", "empties"),
        ("a: [,]\n", "bad flow"),
        ("a: [1,,2]\n", "bad flow2"),
        ("a: [1,]\n", "trailing comma"),
        ("a: {b: 1,}\n", "trailing comma map"),
        ("a: 1\r\nb: 2\r\n", "crlf inner"),
        ("a: |\r\n  x\r\n  y\r\n", "crlf block"),
        ("a: \"x\r\n  y\"\r\n", "crlf dq fold"),
        ("a: x\r\n  y\r\n", "crlf plain fold"),
        ("a: x\ry: 1\n", "lone cr"),
        ("a: 'it''s'\n", "single quote esc"),
        ("a: \"\\x41\\u00e9\\U0001F600\\/\\ \\_\\N\\L\\P\\0\\a\\e\\v\\f\\b\"\n", "all escapes"),
        ("? |\n  block key\n: 1\n", "block scalar key"),
        ("a: -\n", "dash value"),
        ("a: ?\n", "q value"),
        ("a: :\n", "colon value"),
        ("a: -1\nb: - 1\n", "dash space"),
        ("a: @x\n", "at"),
        ("a: `x\n", "backtick"),
        ("a: %x\n", "percent value"),
        ("a: x: y\n", "colon in plain"),
        ("a: [x: y]\n", "colon in flow"),
        ("a: 'x' y\n", "trailing"),
    ];
    for (y, name) in cases {
        let text = format!("---\n{y}---\nbody\n");
        let r = parse(&text);
        match r {
            Ok(d) => eprintln!("{name:36} {y:?} => OK {}", serde_json::Value::Object(d.front_matter.unwrap())),
            Err(e) => eprintln!("{name:36} {y:?} => ERR field={:?} {:?}", e.field, e.reason),
        }
    }
}
