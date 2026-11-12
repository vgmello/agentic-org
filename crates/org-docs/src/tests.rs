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
