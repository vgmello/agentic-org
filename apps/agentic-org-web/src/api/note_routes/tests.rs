use hyper::StatusCode;
use org_proto::private_notes::{PrivateNoteField, PrivateNoteKind, PrivateNoteListParams};

use super::{decisions_params, parse_list_query, valid_ref, ListScope};

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
fn list_queries_refuse_bad_ids_limits_text_and_repeated_keys() {
    let oversized = "a".repeat(129);
    let mut refused = vec![
        "project_id=not-a-uuid".to_owned(),
        "project_id=0193A5C0-0000-7000-8000-000000000050".to_owned(),
        "project_id=550e8400-e29b-41d4-a716-446655440000".to_owned(),
        "project_id=".to_owned(),
        "limit=0".to_owned(),
        "limit=501".to_owned(),
        "limit=-1".to_owned(),
        "limit=".to_owned(),
        "tag=".to_owned(),
        "since=".to_owned(),
        "cursor=".to_owned(),
        format!("tag={oversized}"),
        format!("since={oversized}"),
        format!("cursor={oversized}"),
        format!("task_id={oversized}"),
        "tag=a&tag=b".to_owned(),
        "limit=5&limit=5".to_owned(),
        "fields=title&fields=tags".to_owned(),
        "task_id=T-A1&tag=x&task_id=T-A2".to_owned(),
    ];
    for query in &refused {
        assert!(decisions_params(MEMBER, Some(query)).is_err(), "{query}");
    }
    let base = || PrivateNoteListParams {
        task: Some("T-A1".into()),
        ..PrivateNoteListParams::default()
    };
    refused.retain(|query| !query.starts_with("task_id"));
    refused.extend([
        "author_id=not-a-uuid".to_owned(),
        "author_id=0193A5C0-0000-7000-8000-000000000041".to_owned(),
        "author_id=".to_owned(),
        "kind=plan&kind=note".to_owned(),
        "author_id=0193a5c0-0000-7000-8000-000000000041\
         &author_id=0193a5c0-0000-7000-8000-000000000042"
            .to_owned(),
    ]);
    for query in &refused {
        assert!(
            parse_list_query(ListScope::Task, Some(query), base()).is_err(),
            "{query}"
        );
    }
    let edges = decisions_params(
        MEMBER,
        Some(&format!("limit=1&tag={}&cursor=c", "a".repeat(128))),
    )
    .ok()
    .expect("the bounds themselves");
    assert_eq!(edges.limit, Some(1));
    let most = decisions_params(MEMBER, Some("limit=500"))
        .ok()
        .expect("the largest page");
    assert_eq!(most.limit, Some(500));
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

#[test]
fn cross_task_notes_query_accepts_all_filters_and_rejects_nul_blank_and_duplicate_task() {
    let params = parse_list_query(
        ListScope::All,
        Some(
            "kind=decision&tag=shortcut&task=T-A1&project_id=0193a5c0-0000-7000-8000-000000000050\
             &author_id=0193a5c0-0000-7000-8000-000000000040&limit=25&fields=title,why,tags",
        ),
        PrivateNoteListParams::default(),
    )
    .ok()
    .expect("cross-task query");
    assert_eq!(params.kind, Some(PrivateNoteKind::Decision));
    assert_eq!(params.tag.as_deref(), Some("shortcut"));
    assert_eq!(params.task.as_deref(), Some("T-A1"));
    assert_eq!(
        params.project_id.as_deref(),
        Some("0193a5c0-0000-7000-8000-000000000050")
    );
    assert_eq!(
        params.author_id.as_deref(),
        Some("0193a5c0-0000-7000-8000-000000000040")
    );
    assert_eq!(params.limit, Some(25));
    assert_eq!(
        params.fields,
        [
            PrivateNoteField::Title,
            PrivateNoteField::Why,
            PrivateNoteField::Tags
        ]
    );

    for bad in [
        "tag=%00",
        "tag=bad%00tag",
        "tag=%20%20",
        "since=%20",
        "cursor=%00",
        "task_id=T-A1&task=T-A2",
    ] {
        assert!(
            parse_list_query(ListScope::All, Some(bad), PrivateNoteListParams::default()).is_err(),
            "{bad}"
        );
        assert!(decisions_params(MEMBER, Some(bad)).is_err(), "{bad}");
    }
}

#[test]
fn private_note_params_debug_redacts_private_content() {
    use org_proto::private_notes::{PrivateNoteAddParams, PrivateNoteUpdateParams};
    let secret = "TOP_SECRET_PRIVATE_RATIONALE_DO_NOT_LOG";
    let add = PrivateNoteAddParams {
        command_id: MEMBER.into(),
        note_id: MEMBER.into(),
        task: Some("T-A1".into()),
        project_id: None,
        kind: PrivateNoteKind::Decision,
        title: Some(secret.into()),
        why: Some(secret.into()),
        alternatives: Some(secret.into()),
        tags: vec![secret.into()],
        body_markdown: secret.into(),
        checkpoint: false,
    };
    let upd = PrivateNoteUpdateParams {
        command_id: MEMBER.into(),
        note_id: MEMBER.into(),
        expected_revision: 1,
        kind: Some(PrivateNoteKind::Decision),
        title: Some(secret.into()),
        why: Some(secret.into()),
        alternatives: Some(secret.into()),
        tags: Some(vec![secret.into()]),
        body_markdown: Some(secret.into()),
    };
    assert!(!format!("{add:?}").contains(secret));
    assert!(!format!("{upd:?}").contains(secret));
}

#[test]
fn list_queries_and_refs_reject_control_characters_with_bad_request() {
    for query in [
        "tag=bad%0atag",
        "tag=bad%0dtag",
        "tag=bad%1btag",
        "tag=bad%7ftag",
        "tag=bad%00tag",
        "tag=bad%c2%9btag",
        "since=2026-11-26%0a",
        "cursor=abc%0ddef",
        "task=T-A1%1b",
        "task_id=T-A1%7f",
    ] {
        let err = parse_list_query(
            ListScope::All,
            Some(query),
            PrivateNoteListParams::default(),
        )
        .err()
        .unwrap_or_else(|| panic!("expected 400 for {query}"));
        assert_eq!(err.status(), StatusCode::BAD_REQUEST, "{query}");

        let err = decisions_params(MEMBER, Some(query))
            .err()
            .unwrap_or_else(|| panic!("expected 400 for decisions {query}"));
        assert_eq!(err.status(), StatusCode::BAD_REQUEST, "decisions: {query}");
    }

    for raw in [
        "T-A1\n",
        "T-A1\r",
        "T-A1\x1b[31m",
        "T-A1\x7f",
        "T-A1\0",
        "T-A1\u{009b}",
    ] {
        let err = valid_ref(raw).expect_err("control char ref must fail");
        assert_eq!(err.status(), StatusCode::BAD_REQUEST, "{raw:?}");
    }
}
