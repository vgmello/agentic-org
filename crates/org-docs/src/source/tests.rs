use super::*;

const SOURCE: &str = "---\nlabel: café 🦀\nrevision: 1 # revision comment\nassigned_team_id: 'old-team' # team comment\nassignee_id: \"old-employee\" # employee comment\nupdated_at: 'old-time'\nupdated_by_kind: owner\nupdated_by_id: null\nextension:\n  revision: 41\n  exact: 1.234567890123456789\n---\nbody revision: 999\nassigned_team_id: unchanged\n";

fn edits() -> [RootScalarEdit<'static>; 6] {
    [
        RootScalarEdit {
            key: "revision",
            value: ScalarValue::Integer(2),
        },
        RootScalarEdit {
            key: "assigned_team_id",
            value: ScalarValue::Null,
        },
        RootScalarEdit {
            key: "assignee_id",
            value: ScalarValue::String("new-employee"),
        },
        RootScalarEdit {
            key: "updated_at",
            value: ScalarValue::String("2026-11-15T12:00:00.000Z"),
        },
        RootScalarEdit {
            key: "updated_by_kind",
            value: ScalarValue::String("owner"),
        },
        RootScalarEdit {
            key: "updated_by_id",
            value: ScalarValue::Null,
        },
    ]
}

#[test]
fn six_edits_preserve_all_other_bytes_including_markers_and_numeric_spellings() {
    let cases = [
        SOURCE.to_owned(),
        format!("\u{feff}{}", SOURCE.replace('\n', "\r\n")),
        SOURCE.replace("revision: 1", "\"revi\\u0073ion\": 1"),
        SOURCE.replace(
            "assigned_team_id: 'old-team'",
            "'assigned_team_id': 'old''team'",
        ),
        SOURCE.replace("\"old-employee\"", r#""old\"employee""#),
        SOURCE.replace(
            "exact: 1.234567890123456789",
            "exact: -0.000000000000123456789e+012",
        ),
        // Comments containing braces are not flow-root mappings.
        SOURCE.replace("---\nlabel", "---\n# { preserve this }\nlabel"),
    ];
    for original in cases {
        let parsed = SourceDoc::parse(&original).unwrap();
        assert_eq!(parsed.source(), original);
        let edited = parsed.replace(&edits()).unwrap();
        let mut expected = original.clone();
        for (old, new) in [
            (": 1 # revision", ": 2 # revision"),
            ("'old-team'", "null"),
            ("'old''team'", "null"),
            ("\"old-employee\"", "\"new-employee\""),
            (r#""old\"employee""#, "\"new-employee\""),
            ("'old-time'", "\"2026-11-15T12:00:00.000Z\""),
            ("updated_by_kind: owner", "updated_by_kind: \"owner\""),
        ] {
            expected = expected.replace(old, new);
        }
        assert_eq!(edited.as_bytes(), expected.as_bytes());
        assert_eq!(
            SourceDoc::parse(&edited).unwrap().body().as_bytes(),
            parsed.body().as_bytes()
        );
    }
}

#[test]
fn unsupported_mutable_representations_remain_intact() {
    let cases = [
        SOURCE.replace("revision: 1 # revision comment", "revision: # implicit null"),
        SOURCE.replace("updated_at: 'old-time'", "updated_at: |\n  old-time"),
        SOURCE.replace("revision: 1", "? revision\n: 1"),
        SOURCE.replace("assigned_team_id: 'old-team'", "assigned_team_id: [old-team]"),
        SOURCE.replace("assigned_team_id: 'old-team'", "assigned_team_id: 'old\n  team'"),
        SOURCE.replace("updated_at: 'old-time'", "updated_at: old\n  time"),
        SOURCE.replace("assigned_team_id: 'old-team' # team comment\n", ""),
        "---\n{revision: 1, assigned_team_id: null, assignee_id: null, updated_at: old, updated_by_kind: owner, updated_by_id: null}\n---\nbody".to_owned(),
    ];
    for original in cases {
        let retained = original.clone();
        match SourceDoc::parse(&original) {
            Ok(doc) => assert!(doc.replace(&edits()).is_err()),
            Err(e) => assert_eq!(e, SourceError::UnsupportedRepresentation),
        }
        assert_eq!(original, retained);
    }
}

#[test]
fn strict_guards_are_not_bypassed_by_source_spans() {
    for source in [
        SOURCE.replace("revision: 1", "revision: 1\nrevision: 1"),
        SOURCE.replace("revision: 1", "revision: &revision 1"),
        SOURCE.replace("revision: 1", "revision: !!int 1"),
        SOURCE.replace(
            "exact: 1.234567890123456789",
            "exact: 184467440737095516160000000000000",
        ),
        SOURCE.replace("exact: 1.234567890123456789", "exact: .inf"),
    ] {
        assert!(matches!(
            SourceDoc::parse(&source),
            Err(SourceError::Parse(_))
        ));
    }
    let source = SOURCE.replace("  exact: 1.234567890123456789", "  <<: {} ");
    assert!(matches!(
        SourceDoc::parse(&source),
        Err(SourceError::Parse(_))
    ));
}

#[test]
fn replacement_strings_are_yaml_safe_and_revisions_remain_exact_i64() {
    let parsed = SourceDoc::parse(SOURCE).unwrap();
    let value = "🦀\\\"\n\r\t\0\u{85}\u{2028}\u{2029}";
    let out = parsed
        .replace(&[
            RootScalarEdit {
                key: "assignee_id",
                value: ScalarValue::String(value),
            },
            RootScalarEdit {
                key: "revision",
                value: ScalarValue::Integer(i64::MAX),
            },
        ])
        .unwrap();
    let checked = SourceDoc::parse(&out).unwrap();
    assert_eq!(checked.front_matter()["revision"].as_i64(), Some(i64::MAX));
    assert_eq!(checked.front_matter()["assignee_id"].as_str(), Some(value));
    assert!(out.contains("revision: 9223372036854775807 # revision comment"));
    assert!(out.contains("\\u2028\\u2029"));
    assert_eq!(checked.body(), parsed.body());
}

#[test]
fn duplicate_edits_and_resource_failures_return_no_partial_document() {
    let parsed = SourceDoc::parse(SOURCE).unwrap();
    let edit = edits()[0];
    assert_eq!(
        parsed.replace(&[edit, edit]),
        Err(SourceError::DuplicateEdit)
    );
    assert_eq!(
        parsed.replace(&[RootScalarEdit {
            key: "missing",
            value: ScalarValue::Null
        }]),
        Err(SourceError::MissingKey)
    );
    let oversized = "a".repeat(MAX_FRONT_MATTER_BYTES);
    assert_eq!(
        parsed.replace(&[RootScalarEdit {
            key: "assignee_id",
            value: ScalarValue::String(&oversized)
        }]),
        Err(SourceError::FrontMatterTooLarge)
    );
    let too_large = format!("{SOURCE}{}", "a".repeat(MAX_SOURCE_BODY_BYTES));
    assert!(matches!(
        SourceDoc::parse(&too_large),
        Err(SourceError::BodyTooLarge)
    ));
    assert_eq!(parsed.source(), SOURCE);
}
