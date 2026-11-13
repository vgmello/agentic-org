use serde_json::json;

use super::*;

fn front(yaml: &str) -> Result<Value, FrontMatterError> {
    parse(&format!("---\n{yaml}---\nbody\n")).map(|doc| Value::Object(doc.front_matter.unwrap()))
}

fn refused(yaml: &str) -> (String, Reason) {
    let error = front(yaml).expect_err(yaml);
    (error.field, error.reason)
}

#[test]
fn the_spec_example_parses_with_its_types() {
    let yaml = r#"id: doc-01J
process: review-and-merge
level: organization
scope: null
version: 3
overridable: true
status: active
moments:
  waking_up: "If your branch is waiting, check on it first."
checklist:
  - { text: "Own fresh review", required: true }
  - { text: "The lead's go", required: false }
"#;
    let expected = json!({
        "id": "doc-01J", "process": "review-and-merge", "level": "organization", "scope": null,
        "version": 3, "overridable": true, "status": "active",
        "moments": { "waking_up": "If your branch is waiting, check on it first." },
        "checklist": [
            { "text": "Own fresh review", "required": true },
            { "text": "The lead's go", "required": false }
        ]
    });
    assert_eq!(front(yaml).unwrap(), expected);
}

#[test]
fn fences_split_the_doc() {
    let none = parse("# Title\n---\nnot front matter\n").unwrap();
    assert_eq!(
        (none.front_matter, none.body),
        (None, "# Title\n---\nnot front matter\n")
    );
    let crlf = parse("\u{feff}---\r\na: 1\r\n---\r\n# Body\r\n").unwrap();
    assert_eq!(crlf.front_matter.unwrap()["a"], json!(1));
    assert_eq!(crlf.body, "# Body\r\n");
    let at_end = parse("---\na: b\n---").unwrap();
    assert_eq!(
        (at_end.front_matter.unwrap()["a"].clone(), at_end.body),
        (json!("b"), "")
    );
    assert_eq!(parse("---\n---\n").unwrap().front_matter, Some(Map::new()));
    assert_eq!(parse("---\na: 1\n").unwrap_err().reason, Reason::Unclosed);
    assert_eq!(
        parse("---\na: 1\n----\n").unwrap_err().reason,
        Reason::Unclosed
    );
}

#[test]
fn core_schema_typing_keeps_yaml_1_1_traps_out() {
    let cases = [
        ("no", json!("no")),
        ("yes", json!("yes")),
        ("on", json!("on")),
        ("off", json!("off")),
        ("No", json!("No")),
        ("y", json!("y")),
        ("0777", json!(777)),
        ("1_000", json!("1_000")),
        ("~", json!(null)),
        ("", json!(null)),
        ("Null", json!(null)),
        ("TRUE", json!(true)),
        ("False", json!(false)),
        ("tRUE", json!("tRUE")),
        ("-12", json!(-12)),
        ("+7", json!(7)),
        ("0o17", json!(15)),
        ("0x1F", json!(31)),
        ("0x", json!("0x")),
        ("1.5", json!(1.5)),
        ("-.5", json!(-0.5)),
        ("1e3", json!(1000.0)),
        ("1.", json!(1.0)),
        ("12:30", json!("12:30")),
        ("2026-11-12", json!("2026-11-12")),
        ("'no'", json!("no")),
        ("\"3\"", json!("3")),
    ];
    for (scalar, expected) in cases {
        assert_eq!(
            front(&format!("v: {scalar}\n")).unwrap()["v"],
            expected,
            "{scalar:?}"
        );
    }
    assert_eq!(front("v: |\n  true\n").unwrap()["v"], json!("true\n"));
    for bad in [
        ".inf",
        "-.Inf",
        ".NaN",
        "99999999999999999999",
        "0x8000000000000000",
    ] {
        assert_eq!(
            refused(&format!("v: {bad}\n")),
            ("v".into(), Reason::BadNumber),
            "{bad}"
        );
    }
}

#[test]
fn every_refusal_names_its_field() {
    let cases = [
        ("a: &x 1\nb: *x\n", "a", Reason::Anchor),
        ("a: !!str 1\n", "a", Reason::Tag),
        ("a: !custom x\n", "a", Reason::Tag),
        (
            "base: {x: 1}\nmore:\n  <<: {y: 2}\n",
            "more.<<",
            Reason::MergeKey,
        ),
        ("a: 1\nb: 2\na: 3\n", "a", Reason::DuplicateKey),
        (
            "list:\n  - {k: 1, k: 2}\n",
            "list[0].k",
            Reason::DuplicateKey,
        ),
        ("1: one\n", "", Reason::NonStringKey),
        ("m:\n  true: x\n", "m", Reason::NonStringKey),
        ("? [a]\n: x\n", "", Reason::NonStringKey),
        ("a: [1\n", "a", Reason::Syntax(String::new())),
    ];
    for (yaml, field, reason) in cases {
        let (got_field, got_reason) = refused(yaml);
        assert_eq!(got_field, field, "{yaml:?}");
        match (&reason, &got_reason) {
            (Reason::Syntax(_), Reason::Syntax(_)) => {}
            _ => assert_eq!(got_reason, reason, "{yaml:?}"),
        }
    }
    // Quoted keys that look like other types are strings.
    assert_eq!(
        front("'1': one\n\"true\": x\n").unwrap(),
        json!({"1": "one", "true": "x"})
    );
}

#[test]
fn the_root_must_be_one_mapping() {
    for yaml in ["- a\n- b\n", "just text\n", "[1, 2]\n"] {
        assert_eq!(
            refused(yaml),
            (String::new(), Reason::NotAMapping),
            "{yaml:?}"
        );
    }
    assert_eq!(refused("a: 1\n--- {b: 2}\n").1, Reason::MultipleDocuments);
}

#[test]
fn size_depth_and_event_caps_hold_at_their_bounds() {
    let fill = |n: usize| format!("a: \"{}\"\n", "x".repeat(n));
    let at_cap = fill(MAX_FRONT_MATTER_BYTES - 6);
    assert_eq!(at_cap.len(), MAX_FRONT_MATTER_BYTES);
    assert!(front(&at_cap).is_ok());
    assert_eq!(
        refused(&fill(MAX_FRONT_MATTER_BYTES - 5)),
        (String::new(), Reason::TooLarge)
    );

    // The root mapping is depth 1: 15 more nested sequences are 16 levels, one more is 17.
    let nest = |levels: usize| format!("a: {}1{}\n", "[".repeat(levels), "]".repeat(levels));
    assert!(front(&nest(MAX_DEPTH - 1)).is_ok());
    let (field, reason) = refused(&nest(MAX_DEPTH));
    assert_eq!(reason, Reason::TooDeep);
    assert!(field.starts_with("a[0]"), "{field}");

    // A one-key mapping holding a flow list of n items is n + 9 events.
    let items = |n: usize| format!("a: [{}]\n", vec!["0"; n].join(","));
    assert!(front(&items(MAX_EVENTS - 9)).is_ok());
    assert_eq!(refused(&items(MAX_EVENTS - 8)).1, Reason::TooManyEvents);
}

#[test]
fn malformed_input_never_panics() {
    // A fixed-seed generator over YAML's structural characters.
    let alphabet: Vec<char> = "-:?[]{},&*!|>'\"#%@` \n\tab01.~<".chars().collect();
    let mut state: u64 = 0x2545_f491_4f6c_dd1d;
    for _ in 0..3000 {
        let mut yaml = String::new();
        for _ in 0..(state % 40) {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            yaml.push(alphabet[(state % alphabet.len() as u64) as usize]);
        }
        let _ = parse(&format!("---\n{yaml}\n---\n"));
    }
}

#[test]
fn aliases_never_resolve() {
    // An alias needs an anchor, and every anchor is refused before its alias is read; an alias
    // with no anchor is the parser's own error. Either way nothing is ever expanded.
    assert_eq!(refused("a: &x 1\nb: *x\n"), ("a".into(), Reason::Anchor));
    assert!(matches!(refused("a: [1, *x]\n").1, Reason::Syntax(_)));
}

#[test]
fn tags_and_anchors_are_refused_on_every_node_kind() {
    // Scalars are covered in `every_refusal_names_its_field`; these are the other places a tag or
    // an anchor can sit: on a sequence, on a mapping, on a key, on the root. A refusal on a key
    // names the mapping that holds the key (as `NonStringKey` does), not the key itself.
    let cases = [
        ("a: !!seq [1]\n", "a", Reason::Tag),
        ("a: !!map {b: 1}\n", "a", Reason::Tag),
        ("a: &s [1]\n", "a", Reason::Anchor),
        ("a: &m {b: 1}\n", "a", Reason::Anchor),
        ("a:\n  - &s [1]\n", "a[0]", Reason::Anchor),
        ("a:\n  - !!map {b: 1}\n", "a[0]", Reason::Tag),
        ("&k a: x\n", "", Reason::Anchor),
        ("!!str a: x\n", "", Reason::Tag),
        ("m:\n  &k a: x\n", "m", Reason::Anchor),
        ("{&k a: 1}\n", "", Reason::Anchor),
        ("&r\na: 1\n", "", Reason::Anchor),
        ("!!map\na: 1\n", "", Reason::Tag),
    ];
    for (yaml, field, reason) in cases {
        assert_eq!(refused(yaml), (field.into(), reason), "{yaml:?}");
    }
}

#[test]
fn a_refusal_names_the_index_it_is_at() {
    // The path index counts the items before it and is popped after each one: a refusal in the
    // third item is `[2]`, not `[0]` and not a path left over from the items before.
    assert_eq!(
        refused("checklist:\n  - {text: a}\n  - {text: b}\n  - {text: &x c}\n"),
        ("checklist[2].text".into(), Reason::Anchor)
    );
    assert_eq!(refused("a: [1, &x 2]\n"), ("a[1]".into(), Reason::Anchor));
    assert_eq!(
        refused("a: [[1, 2], [3, &x 4]]\n"),
        ("a[1][1]".into(), Reason::Anchor)
    );
}

#[test]
fn numbers_out_of_range_are_refused_not_nulled() {
    // `f64` parses these to infinity, which JSON cannot hold; `Value::from` would turn that into
    // null, so the refusal has to come from `Number::from_f64`.
    for bad in [
        "1e999", "-1e999", "1.5e999", ".5e999", "+1e999", ".nan", ".NAN", ".INF", "+.inf",
    ] {
        assert_eq!(
            refused(&format!("v: {bad}\n")),
            ("v".into(), Reason::BadNumber),
            "{bad}"
        );
    }
}

#[test]
fn core_float_edges_follow_the_yaml_1_2_pattern() {
    // `( \. [0-9]+ | [0-9]+ ( \. [0-9]* )? ) ( [eE] [-+]? [0-9]+ )?`, and a float needs a dot or
    // an exponent; everything else that is not a number is a string.
    let cases = [
        (".", json!(".")),
        ("1e", json!("1e")),
        ("1e+", json!("1e+")),
        ("1.e5", json!(100_000.0)),
        ("1.E5", json!(100_000.0)),
        (".e5", json!(".e5")),
        ("e5", json!("e5")),
        (".5e1", json!(5.0)),
        ("1e+5", json!(100_000.0)),
        ("1E-2", json!(0.01)),
        ("-.5e1", json!(-5.0)),
        ("1.5.2", json!("1.5.2")),
        ("1e5e5", json!("1e5e5")),
    ];
    for (scalar, expected) in cases {
        assert_eq!(
            front(&format!("v: {scalar}\n")).unwrap()["v"],
            expected,
            "{scalar:?}"
        );
    }
}

#[test]
fn the_fence_lines_are_read_exactly() {
    // Kept behaviour, pinned: the opening fence is exactly `---` and a line end (`\n` or `\r\n`,
    // after an optional byte-order mark). `--- ` with trailing space, with a comment, with a
    // fourth dash or with no line end is not a fence, so the doc silently has no front matter and
    // the whole text is the body. The closing fence is read the same way, so `--- ` there leaves
    // the front matter unclosed.
    for text in [
        "--- \na: 1\n---\nbody\n",
        "---  # c\na: 1\n---\nbody\n",
        "----\na: 1\n---\nbody\n",
        "---",
    ] {
        let doc = parse(text).unwrap();
        assert_eq!((doc.front_matter, doc.body), (None, text), "{text:?}");
    }
    assert_eq!(
        parse("---\na: 1\n--- \nbody\n").unwrap_err().reason,
        Reason::Unclosed
    );
}

#[test]
fn a_bare_digit_run_is_not_a_core_float() {
    // `typed` reads integers first, so this guard is defensive: a float needs a dot or an
    // exponent, and `is_core_float` says so on its own.
    assert!(!is_core_float("12"));
    assert!(is_core_float("12."));
    assert!(is_core_float("12e1"));
}

#[test]
fn a_refusal_reads_as_its_field_and_reason() {
    // `Display` is what a caller shows: the field path (or "front matter" for the whole), a
    // colon and the reason.
    let shown = |yaml: &str| front(yaml).unwrap_err().to_string();
    assert_eq!(shown("a: &x 1\n"), "a: Anchor");
    assert_eq!(
        shown("checklist:\n  - {text: a}\n  - {text: &x b}\n"),
        "checklist[1].text: Anchor"
    );
    assert_eq!(shown("- a\n"), "front matter: NotAMapping");
    assert_eq!(
        parse("---\na: 1\n").unwrap_err().to_string(),
        "front matter: Unclosed"
    );
}

#[test]
fn line_breaks_and_hidden_text_are_flagged_but_joiners_and_marks_are_not() {
    let flagged = [
        '\u{2028}',
        '\u{2029}',
        '\u{200b}',
        '\u{202a}',
        '\u{202e}',
        '\u{2060}',
        '\u{2064}',
        '\u{2066}',
        '\u{206f}',
        '\u{feff}',
        '\u{180e}',
        '\u{fff9}',
        '\u{fffb}',
        '\u{115f}',
        '\u{1160}',
        '\u{3164}',
        '\u{ffa0}',
        '\u{e0000}',
        '\u{e007f}',
        '\u{e0100}',
        '\u{e01ef}',
    ];
    for c in flagged {
        assert!(is_invisible_or_break(c), "{:04x}", c as u32);
    }
    // The (non-)joiners, direction marks, variation selector 16, ordinary text, the newline
    // (a control character, for `char::is_control`) and the neighbours of each range stay.
    let kept = [
        '\n',
        '\r',
        '\u{205f}',
        '\u{180d}',
        '\u{fefe}',
        '\u{ff00}',
        '\u{fff8}',
        '\u{115e}',
        '\u{1161}',
        '\u{3163}',
        '\u{3165}',
        '\u{ff9f}',
        '\u{ffa1}',
        '\u{e00ff}',
        '\u{200c}',
        '\u{200d}',
        '\u{200e}',
        '\u{200f}',
        '\u{fe0f}',
        'a',
        'é',
        '中',
        '\u{2027}',
        '\u{202f}',
        '\u{2065}',
        '\u{2070}',
        '\u{fffc}',
        '\u{e0080}',
        '\u{e01f0}',
        '\u{180f}',
    ];
    for c in kept {
        assert!(!is_invisible_or_break(c), "{:04x}", c as u32);
    }
}

#[test]
fn the_closing_fence_is_exactly_three_dashes_and_a_line_end() {
    // BUG-194: like the opening fence; a stray `\r` before the line end does not close it.
    for text in [
        "---\na: 1\n---\r\r\nbody",
        "---\na: 1\n---\r",
        "---\na: 1\n---\r\r",
    ] {
        assert_eq!(
            parse(text).unwrap_err().reason,
            Reason::Unclosed,
            "{text:?}"
        );
    }
    assert!(parse("---\na: 1\n---\r\nbody").is_ok() && parse("---\na: 1\n---").is_ok());
}

#[test]
fn field_paths_tell_apart_keys_that_would_look_like_paths() {
    // BUG-195: a key that is empty or holds `.`, `[`, `]`, `"` or `\` is written as a JSON string
    // in brackets, so two different places never share one field text.
    let field = |yaml: &str| refused(yaml).0;
    assert_eq!(field("\"\": {a: &x 1}\n"), "[\"\"].a");
    assert_eq!(field("a: {\"\": &x 1}\n"), "a[\"\"]");
    assert_eq!(field("\"a.b\": {c: &x 1}\n"), "[\"a.b\"].c");
    assert_eq!(field("a: {b: {c: &x 1}}\n"), "a.b.c");
    assert_eq!(field("\"x[0]\": {c: &x 1}\n"), "[\"x[0]\"].c");
    assert_eq!(field("x: [{c: &x 1}]\n"), "x[0].c");
    assert_eq!(field("\"q\\\"\": &x 1\n"), "[\"q\\\"\"]");
    assert_eq!(field("a: {\"b\\\\c\": {d: &x 1}}\n"), "a[\"b\\\\c\"].d");
}

#[test]
fn very_deep_flow_nesting_is_too_deep_not_a_parser_limit() {
    // BUG-196: past a few hundred levels the YAML parser gives up first; that is still TooDeep.
    let yaml = format!("a: {}1{}\n", "[".repeat(300), "]".repeat(300));
    assert_eq!(refused(&yaml).1, Reason::TooDeep);
}
