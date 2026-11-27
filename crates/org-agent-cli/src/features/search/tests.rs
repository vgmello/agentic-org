use org_command::{run_with_env_format, testkit::StubKeys, testkit::StubSources};
use org_proto::search::{
    SearchCoverage, SearchEntityType, SearchHit, SearchHitField, SearchOmission,
    SearchOmissionReason, SearchQuery, SearchQueryParams, SearchQueryResult, SearchSource,
};
use org_proto::Call;
use serde_json::{json, Value};

use crate::registry::runtime_table;
use crate::runtime::transport::{OfficeTransport, Refusal, TransportOffice};

struct StubSearchTransport {
    calls: Vec<(String, Value)>,
    reply: Value,
}

impl OfficeTransport for StubSearchTransport {
    fn call_method(&mut self, method: &str, params: Value) -> Result<Value, Refusal> {
        self.calls.push((method.to_owned(), params));
        Ok(self.reply.clone())
    }
}

fn sample_hit() -> SearchHit {
    SearchHit {
        entity_type: SearchEntityType::Task,
        entity_id: "0193a5c0-0000-7000-8000-000000000100".into(),
        reference: Some("T-A1".into()),
        title: Some("Fix auth timeout".into()),
        excerpt: Some("...auth timeout...".into()),
        score: Some(2.5),
        updated_at: Some("2026-11-22T00:00:00.000Z".into()),
        status: Some("in_progress".into()),
        project_id: None,
        conversation_id: None,
        task_id: None,
        author_id: None,
    }
}

fn args(words: &[&str]) -> Vec<String> {
    words.iter().map(|w| (*w).to_owned()).collect()
}

#[test]
fn search_query_forwards_filters_fields_and_returns_truthful_coverage() {
    let reply = json!(SearchQueryResult {
        hits: vec![sample_hit()],
        total: 3,
        next_cursor: Some("cur-2".into()),
        coverage: SearchCoverage {
            searched: vec![SearchSource::Shared, SearchSource::Messages],
            omitted: vec![],
        },
    });
    let mut transport = StubSearchTransport {
        calls: Vec::new(),
        reply,
    };
    let mut office = TransportOffice(&mut transport);
    let out = run_with_env_format(
        runtime_table(),
        "agent-os",
        &args(&[
            "search",
            "query",
            "--q",
            "auth timeout",
            "--types",
            "task",
            "--types",
            "process",
            "--project",
            "P-1",
            "--channel",
            "#eng",
            "--status",
            "in_progress",
            "--modified-file",
            "crates/auth/src/lib.rs",
            "--assignee",
            "@ada",
            "--label",
            "backend",
            "--limit",
            "10",
            "--cursor",
            "cur-1",
            "--fields",
            "ref,title,excerpt",
        ]),
        &mut StubSources::default(),
        &mut StubKeys::default(),
        &mut office,
        None,
    );
    assert_eq!(out.exit_code, 0, "{}", out.stdout);
    assert_eq!(transport.calls.len(), 1);
    assert_eq!(transport.calls[0].0, SearchQuery::METHOD);
    let sent: SearchQueryParams = serde_json::from_value(transport.calls[0].1.clone()).unwrap();
    assert_eq!(sent.q, "auth timeout");
    assert_eq!(
        sent.types,
        vec![SearchEntityType::Task, SearchEntityType::Process]
    );
    assert_eq!(sent.project.as_deref(), Some("P-1"));
    assert_eq!(sent.channel.as_deref(), Some("#eng"));
    assert_eq!(sent.status.as_deref(), Some("in_progress"));
    assert_eq!(
        sent.modified_file.as_deref(),
        Some("crates/auth/src/lib.rs")
    );
    assert_eq!(sent.assignee.as_deref(), Some("@ada"));
    assert_eq!(sent.label.as_deref(), Some("backend"));
    assert_eq!(sent.limit, Some(10));
    assert_eq!(sent.cursor.as_deref(), Some("cur-1"));
    assert_eq!(
        sent.fields,
        vec![
            SearchHitField::Ref,
            SearchHitField::Title,
            SearchHitField::Excerpt
        ]
    );

    let env: Value = serde_json::from_str(&out.stdout).unwrap();
    assert_eq!(env["ok"], json!(true));
    assert_eq!(env["meta"]["total"], json!(3));
    assert_eq!(env["meta"]["next_cursor"], json!("cur-2"));
    assert_eq!(
        env["meta"]["coverage"],
        json!({"searched": ["shared", "messages"]})
    );
    assert_eq!(
        env["data"],
        json!([{"ref": "T-A1", "title": "Fix auth timeout", "excerpt": "...auth timeout..."}])
    );
}

#[test]
fn search_query_compact_and_env_format_preserve_omitted_coverage() {
    let reply = json!(SearchQueryResult {
        hits: vec![],
        total: 0,
        next_cursor: None,
        coverage: SearchCoverage {
            searched: vec![],
            omitted: vec![SearchOmission {
                source: SearchSource::PrivateNotes,
                reason: SearchOmissionReason::HqOnly,
            }],
        },
    });
    let mut transport = StubSearchTransport {
        calls: Vec::new(),
        reply,
    };
    let mut office = TransportOffice(&mut transport);
    let out = run_with_env_format(
        runtime_table(),
        "agent-os",
        &args(&[
            "search",
            "query",
            "--q",
            "retry",
            "--types",
            "private_decision",
            "--count",
        ]),
        &mut StubSources::default(),
        &mut StubKeys::default(),
        &mut office,
        Some("compact"),
    );
    assert_eq!(out.exit_code, 0, "{}", out.stdout);
    let meta_json = out
        .stdout
        .trim()
        .strip_prefix("meta: ")
        .expect("compact search output includes meta line");
    let meta: Value = serde_json::from_str(meta_json).unwrap();
    assert_eq!(meta["total"], json!(0));
    assert_eq!(
        meta["coverage"],
        json!({"searched": [], "omitted": [{"source": "private_notes", "reason": "hq_only"}]})
    );
}

#[test]
fn search_query_refuses_bad_limit_count_with_cursor_and_mixed_private_types() {
    let mut transport = StubSearchTransport {
        calls: Vec::new(),
        reply: json!({}),
    };
    let mut office = TransportOffice(&mut transport);
    for line in [
        &["search", "query", "--q", "auth", "--limit", "101"][..],
        &[
            "search", "query", "--q", "auth", "--count", "--cursor", "c1",
        ],
        &[
            "search",
            "query",
            "--q",
            "auth",
            "--types",
            "task",
            "--types",
            "private_note",
        ],
    ] {
        let out = run_with_env_format(
            runtime_table(),
            "agent-os",
            &args(line),
            &mut StubSources::default(),
            &mut StubKeys::default(),
            &mut office,
            None,
        );
        assert_eq!(out.exit_code, 1, "{line:?}: {}", out.stdout);
        let env: Value = serde_json::from_str(&out.stdout).unwrap();
        assert_eq!(env["error"]["code"], json!("invalid_request"));
    }
    assert!(transport.calls.is_empty());
}
