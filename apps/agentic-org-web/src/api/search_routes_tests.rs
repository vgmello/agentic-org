use org_proto::search::{SearchEntityType, SearchHitField};

use super::parse_query;

#[test]
fn every_parameter_parses_into_the_office_request() {
    let params = parse_query(Some(
        "q=token+rotation%20%22OR%22&types=task,%20project&project=P-3&channel=%23general\
         &status=todo&modified_file=crates%2Fa%2Fsrc%2Flib.rs&assignee=%40alpha&label=db%3Asqlite\
         &author_id=0193a5c0-0000-7000-8000-000000000040&count_only=true&limit=50&cursor=c\
         &fields=ref,title,score",
    ))
    .ok()
    .expect("every parameter");
    assert_eq!(params.q, "token rotation \"OR\"");
    assert_eq!(
        params.types,
        [SearchEntityType::Task, SearchEntityType::Project]
    );
    assert_eq!(params.project.as_deref(), Some("P-3"));
    assert_eq!(params.channel.as_deref(), Some("#general"));
    assert_eq!(params.status.as_deref(), Some("todo"));
    assert_eq!(params.modified_file.as_deref(), Some("crates/a/src/lib.rs"));
    assert_eq!(params.assignee.as_deref(), Some("@alpha"));
    assert_eq!(params.label.as_deref(), Some("db:sqlite"));
    assert_eq!(
        params.author_id.as_deref(),
        Some("0193a5c0-0000-7000-8000-000000000040")
    );
    assert!(params.count_only);
    assert_eq!(params.limit, Some(50));
    assert_eq!(params.cursor.as_deref(), Some("c"));
    assert_eq!(
        params.fields,
        [
            SearchHitField::Ref,
            SearchHitField::Title,
            SearchHitField::Score
        ]
    );
    let private = parse_query(Some("q=plan&types=private_note,private_decision"))
        .ok()
        .unwrap();
    assert_eq!(
        private.types,
        [
            SearchEntityType::PrivateNote,
            SearchEntityType::PrivateDecision
        ]
    );
}

#[test]
fn unknown_repeated_oversized_and_malformed_parameters_are_refused() {
    let big_q = "a".repeat(513);
    let big_ref = "a".repeat(129);
    let big_path = "a".repeat(1025);
    let big_cursor = "c".repeat(257);
    for query in [
        None,
        Some(""),
        Some("q="),
        Some("q=+++"),
        Some("q=%00"),
        Some(&*format!("q={big_q}")),
        Some("q=a&q=b"),
        Some("q=a&limit=1&limit=2"),
        Some("q=a&sql=1"),
        Some("q=a&Q=b"),
        Some("q"),
        Some("q=a&limit=0"),
        Some("q=a&limit=101"),
        Some("q=a&limit=-1"),
        Some("q=a&limit=ten"),
        Some("q=a&types=document"),
        Some("q=a&types=task,"),
        Some("q=a&types="),
        Some("q=a&fields=body"),
        Some("q=a&author_id=not-a-uuid"),
        Some("q=a&author_id=0193A5C0-0000-7000-8000-000000000040"),
        Some("q=a&count_only=yes"),
        Some("q=a&project="),
        Some(&*format!("q=a&project={big_ref}")),
        Some(&*format!("q=a&label={big_ref}")),
        Some(&*format!("q=a&modified_file={big_path}")),
        Some(&*format!("q=a&cursor={big_cursor}")),
        Some("q=%ZZ"),
        Some("q=%2"),
        Some("q=%FF"),
    ] {
        assert!(parse_query(query).is_err(), "{query:?}");
    }
    // The bounds themselves pass.
    let edges = format!(
        "q={}&project={}&modified_file={}&cursor={}&limit=100",
        "a".repeat(512),
        "a".repeat(128),
        "a".repeat(1024),
        "c".repeat(256)
    );
    assert!(parse_query(Some(&edges)).is_ok());
    // Hostile search text is text: the office reduces it to plain words.
    for q in ["foo%22+OR+1%3D1", "NEAR%2F5", "*", "---", "()", "col%3Aval"] {
        assert!(parse_query(Some(&format!("q={q}"))).is_ok(), "{q}");
    }
}
