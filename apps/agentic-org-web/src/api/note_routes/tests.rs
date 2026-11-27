use org_proto::private_notes::{PrivateNoteField, PrivateNoteKind, PrivateNoteListParams};

use super::{decisions_params, parse_list_query, ListScope};

const MEMBER: &str = "0193a5c0-0000-7000-8000-000000000040";

#[test]
fn the_decisions_list_is_always_the_members_own_decisions() {
    let params = decisions_params(MEMBER, None)
        .ok()
        .expect("a plain request");
    assert_eq!(params.author_id.as_deref(), Some(MEMBER));
    assert_eq!(params.kind, Some(PrivateNoteKind::Decision));
    assert_eq!(params.task, None);

    let params = decisions_params(
        MEMBER,
        Some(
            "tag=db%3Asqlite&project_id=0193a5c0-0000-7000-8000-000000000050&task_id=T-A1\
             &limit=20&cursor=c&since=2026-11-26T00%3A00%3A00Z&fields=title,%20tags",
        ),
    )
    .ok()
    .expect("every filter");
    assert_eq!(params.tag.as_deref(), Some("db:sqlite"));
    assert_eq!(
        params.project_id.as_deref(),
        Some("0193a5c0-0000-7000-8000-000000000050")
    );
    assert_eq!(params.task.as_deref(), Some("T-A1"));
    assert_eq!(params.limit, Some(20));
    assert_eq!(params.cursor.as_deref(), Some("c"));
    assert_eq!(params.since.as_deref(), Some("2026-11-26T00:00:00Z"));
    assert_eq!(
        params.fields,
        [PrivateNoteField::Title, PrivateNoteField::Tags]
    );
    assert_eq!(params.author_id.as_deref(), Some(MEMBER));
    assert_eq!(params.kind, Some(PrivateNoteKind::Decision));
}

#[test]
fn the_decisions_list_refuses_other_authors_kinds_and_malformed_input() {
    for member in [
        "not-a-uuid",
        "0193A5C0-0000-7000-8000-000000000040",
        "550e8400-e29b-41d4-a716-446655440000",
        "",
    ] {
        assert!(decisions_params(member, None).is_err(), "{member:?}");
    }
    for query in [
        "kind=note",
        "author_id=0193a5c0-0000-7000-8000-000000000041",
        "bogus=1",
        "tag",
        "limit=ten",
        "fields=body",
        "fields=title,",
        "tag=%ZZ",
        "tag=%2",
        "task_id=",
    ] {
        assert!(decisions_params(MEMBER, Some(query)).is_err(), "{query}");
    }
}

#[test]
fn a_task_notes_list_takes_author_and_kind_but_not_a_task_id() {
    let base = || PrivateNoteListParams {
        task: Some("T-A1".into()),
        ..PrivateNoteListParams::default()
    };
    let params = parse_list_query(
        ListScope::Task,
        Some("author_id=0193a5c0-0000-7000-8000-000000000041&kind=plan&fields=body_markdown"),
        base(),
    )
    .ok()
    .expect("a task filter");
    assert_eq!(params.task.as_deref(), Some("T-A1"));
    assert_eq!(
        params.author_id.as_deref(),
        Some("0193a5c0-0000-7000-8000-000000000041")
    );
    assert_eq!(params.kind, Some(PrivateNoteKind::Plan));
    assert_eq!(params.fields, [PrivateNoteField::BodyMarkdown]);
    assert!(parse_list_query(ListScope::Task, Some("task_id=T-A2"), base()).is_err());
    assert!(parse_list_query(ListScope::Task, Some("kind=memo"), base()).is_err());
    let empty = parse_list_query(ListScope::Task, Some(""), base())
        .ok()
        .expect("no query");
    assert_eq!(empty, base());
}
