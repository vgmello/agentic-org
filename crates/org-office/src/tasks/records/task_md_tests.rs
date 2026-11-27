use org_proto::tasks::{TaskPriority, TaskStatus, WorkType};
use org_store::Id;

use super::*;
use crate::tasks::records::{
    DoneAnywayRecord, FinalSummaryRecord, TaskDetailsRecord, TaskLinkRecord, TaskMeta, TaskRecord,
    TaskRepositoryRecord,
};

fn id(n: u64) -> Id {
    Id::parse(&format!("01900000-0000-7000-8000-{n:012x}")).unwrap()
}

pub(super) fn sample_task() -> TaskRecord {
    TaskRecord {
        id: id(1),
        office_code: "AB".into(),
        number: 42,
        team_id: id(2),
        assigned_team_id: Some(id(3)),
        project_id: None,
        task_type: "bug".into(),
        work_type: Some(WorkType::Build),
        title: "Fix: the \"quoted\" title — Zoë".into(),
        status: TaskStatus::Blocked,
        assignee_id: Some(id(4)),
        body_markdown: "---\nnot front matter\n\n- [ ] body\r\nwith CRLF\n".into(),
        feature_id: None,
        milestone_id: None,
        priority: TaskPriority::High,
        effort: Some(3),
        severity: Some("major".into()),
        resolution: None,
        blocked_reason: Some("waiting: on review".into()),
        blocked_since: Some("2026-11-26T00:30:00.000Z".into()),
        waiting_on_owner: true,
        rank_order: Some(7),
        labels: vec!["area:api".into(), "backend".into()],
        reference_files: vec!["docs/a.md".into()],
        modified_files: vec!["src/b.rs".into(), "src/c.rs".into()],
        repositories: vec![TaskRepositoryRecord {
            repository_id: id(9),
            branch: Some("feature/x".into()),
            integrated_commit: Some("abc123".into()),
        }],
        details: Some(TaskDetailsRecord {
            what_happened: Some("Done when green".into()),
            reopen_count: 2,
            severity_confirmed: true,
            duplicate_of_task_id: Some(id(5)),
            done_anyway: Some(DoneAnywayRecord {
                reason: "Waived: \"docs\" — later".into(),
                steps: vec![id(7), id(8)],
            }),
            ..Default::default()
        }),
        final_summary: Some(FinalSummaryRecord {
            text: "Fixed the \"quoted\" title — Zoë\nSecond line".into(),
            at: "2026-11-26T02:00:00.000Z".into(),
            by_id: id(4),
        }),
        meta: TaskMeta {
            revision: 6,
            created_at: Some("2026-11-26T00:00:00.000Z".into()),
            created_by_kind: "employee".into(),
            created_by_id: Some(id(4)),
            updated_at: Some("2026-11-26T01:00:00.000Z".into()),
            updated_by_kind: "system".into(),
            updated_by_id: None,
        },
    }
}

fn links() -> Vec<TaskLinkRecord> {
    vec![
        TaskLinkRecord {
            source_id: id(1),
            id: id(21),
            kind: "relates_to".into(),
            target_kind: "task".into(),
            target_id: id(6),
        },
        TaskLinkRecord {
            source_id: id(1),
            id: id(20),
            kind: "depends_on".into(),
            target_kind: "task".into(),
            target_id: id(7),
        },
    ]
}

#[test]
fn every_field_of_a_task_survives_its_file() {
    let task = sample_task();
    let bytes = encode_task_md(&task, &links(), &[]).unwrap();
    let text = String::from_utf8(bytes.clone()).unwrap();
    assert!(text.starts_with("---\nassigned_team_id: "), "{text}");
    for key in [
        "schema_version: 1",
        "number: 42",
        "type: \"bug\"",
        "revision: 6",
        "repository_ids: [",
        "branches: {",
        "integrated_commits: {",
        "attachments: []",
        "links: [",
    ] {
        assert!(text.contains(key), "{key} in {text}");
    }
    let doc = decode_task_md(&bytes).unwrap();
    assert_eq!(doc.task, task);
    // Links come back in one canonical order, whatever order they were given in.
    let mut sorted = links();
    sorted.reverse();
    assert_eq!(doc.links, sorted);
    // Re-encoding the decoded task gives the same bytes (no engine-dependent order).
    assert_eq!(
        encode_task_md(&doc.task, &doc.links, &doc.attachments).unwrap(),
        bytes
    );
}

#[test]
fn a_minimal_task_round_trips_with_nulls_and_an_empty_body() {
    let mut task = sample_task();
    task.assigned_team_id = None;
    task.project_id = Some(id(8));
    task.work_type = None;
    task.assignee_id = None;
    task.effort = None;
    task.severity = None;
    task.blocked_reason = None;
    task.rank_order = None;
    task.labels.clear();
    task.reference_files.clear();
    task.modified_files.clear();
    task.repositories.clear();
    task.details = None;
    task.body_markdown = String::new();
    let bytes = encode_task_md(&task, &[], &[]).unwrap();
    assert!(bytes.ends_with(b"---\n"));
    let doc = decode_task_md(&bytes).unwrap();
    assert_eq!(doc.task, task);
    assert!(doc.links.is_empty());
    assert!(doc.attachments.is_empty());
}

#[test]
fn front_matter_past_its_bound_is_refused_when_written() {
    let mut task = sample_task();
    task.reference_files = (0..100)
        .map(|n| format!("src/{n:04}/{}", "p".repeat(900)))
        .collect();
    let err = encode_task_md(&task, &[], &[]).unwrap_err();
    assert!(err.contains("front matter"), "{err}");
}

#[test]
fn a_file_that_is_not_a_task_is_refused() {
    let good = String::from_utf8(encode_task_md(&sample_task(), &links(), &[]).unwrap()).unwrap();
    let swap = |from: &str, to: &str| {
        assert!(good.contains(from), "{from}");
        good.replacen(from, to, 1)
    };
    for bad in [
        swap("schema_version: 1", "schema_version: 2"),
        swap("number: 42", "number: 0"),
        swap("revision: 6", "revision: 0"),
        swap("office_code: \"AB\"", "office_code: \"IO\""),
        swap("status: \"blocked\"", "status: \"open\""),
        swap("attachments: []", "attachments: [\"a\"]"),
        swap("schema_version: 1\n", "schema_version: 1\nextra: 1\n"),
        swap(
            "labels: [\"area:api\",\"backend\"]",
            "labels: [\"backend\",\"area:api\"]",
        ),
        swap("labels: [\"area:api\",", "labels: [\"Area:API\","),
        swap(
            "created_by_kind: \"employee\"",
            "created_by_kind: \"system\"",
        ),
        swap("\"kind\":\"depends_on\"", "\"kind\":\"blocks\""),
        swap(
            "branches: {",
            "branches: {\"01900000-0000-7000-8000-0000000000ff\":\"x\",",
        ),
        swap("team_id: \"", "team_id: \"x"),
        "no front matter".to_owned(),
    ] {
        assert!(decode_task_md(bad.as_bytes()).is_err(), "{bad}");
    }
    // A body over the source-document bound is refused on read.
    let huge = format!("{good}{}", "x".repeat(org_docs::MAX_SOURCE_BODY_BYTES + 1));
    assert!(decode_task_md(huge.as_bytes()).is_err());
}

/// The file `sample_task` would have been before `TASK-88`: no `final_summary*` keys, and its
/// details with the retired `definition_of_done` key (always null) and no `done_anyway`.
fn pre_task_88(text: &str) -> String {
    // Split on `\n` only: the body keeps its `\r\n`.
    let lines: Vec<String> = text
        .split('\n')
        .filter(|line| !line.starts_with("final_summary"))
        .map(|line| match line.strip_prefix("details: ") {
            Some(details) => format!(
                "details: {}",
                details.replace("\"done_anyway\":null,", "").replacen(
                    '{',
                    "{\"definition_of_done\":null,",
                    1
                )
            ),
            None => line.to_owned(),
        })
        .collect();
    lines.join("\n")
}

#[test]
fn a_task_written_before_task_88_still_reads_without_a_summary_or_a_done_anyway() {
    let mut task = sample_task();
    task.final_summary = None;
    if let Some(details) = task.details.as_mut() {
        details.done_anyway = None;
    }
    let text = String::from_utf8(encode_task_md(&task, &[], &[]).unwrap()).unwrap();
    assert!(text.contains("final_summary: null"), "{text}");
    let old = pre_task_88(&text);
    assert!(!old.contains("final_summary"), "{old}");
    assert!(old.contains("\"definition_of_done\":null"), "{old}");
    assert_eq!(decode_task_md(old.as_bytes()).unwrap().task, task);
    // A retired definition of done that holds text is not dropped silently.
    let filled = old.replace(
        "\"definition_of_done\":null",
        "\"definition_of_done\":\"Ship it\"",
    );
    let refused = decode_task_md(filled.as_bytes()).unwrap_err();
    assert!(
        refused.to_string().contains("definition_of_done"),
        "{refused}"
    );
}

#[test]
fn a_final_summary_is_all_four_fields_or_none() {
    let task = sample_task();
    let text = String::from_utf8(encode_task_md(&task, &[], &[]).unwrap()).unwrap();
    for (from, to) in [
        (
            "final_summary_by_kind: \"employee\"",
            "final_summary_by_kind: \"system\"",
        ),
        (
            "final_summary_by_kind: \"employee\"",
            "final_summary_by_kind: null",
        ),
        (
            "final_summary_at: \"2026-11-26T02:00:00.000Z\"",
            "final_summary_at: null",
        ),
    ] {
        assert!(text.contains(from), "{from} in {text}");
        let broken = text.replace(from, to);
        assert!(decode_task_md(broken.as_bytes()).is_err(), "{to}");
    }
    let blank = text.replace(
        "final_summary: \"Fixed the \\\"quoted\\\" title — Zoë\\nSecond line\"",
        "final_summary: \"   \"",
    );
    assert_ne!(blank, text);
    assert!(decode_task_md(blank.as_bytes()).is_err());
    // A "done anyway" record names its reason and step IDs.
    let empty_reason = text.replace("\"reason\":\"Waived:", "\"reason\":\"   \",\"x\":\"");
    assert!(decode_task_md(empty_reason.as_bytes()).is_err());

    // Control characters (including NUL, which truncates SQLite length()) and overlong values
    // are refused on decode so Memory and SQLite never diverge on rebuild.
    let nul_summary = text.replace(
        "final_summary: \"Fixed the \\\"quoted\\\" title — Zoë\\nSecond line\"",
        "final_summary: \"\\u0000hello\"",
    );
    assert!(decode_task_md(nul_summary.as_bytes()).is_err());
    let esc_summary = text.replace(
        "final_summary: \"Fixed the \\\"quoted\\\" title — Zoë\\nSecond line\"",
        "final_summary: \"bad\\u001b[31m\"",
    );
    assert!(decode_task_md(esc_summary.as_bytes()).is_err());
    let huge_summary = text.replace(
        "final_summary: \"Fixed the \\\"quoted\\\" title — Zoë\\nSecond line\"",
        &format!("final_summary: \"{}\"", "é".repeat(8001)),
    );
    assert!(decode_task_md(huge_summary.as_bytes()).is_err());
    let ctrl_reason = text.replace("\"reason\":\"Waived:", "\"reason\":\"Waived\\n");
    assert_ne!(ctrl_reason, text);
    assert!(decode_task_md(ctrl_reason.as_bytes()).is_err());
    let huge_reason = text.replace(
        "\"reason\":\"Waived: \\\"docs\\\" — later\"",
        &format!("\"reason\":\"{}\"", "r".repeat(1001)),
    );
    assert_ne!(huge_reason, text);
    assert!(decode_task_md(huge_reason.as_bytes()).is_err());
}
