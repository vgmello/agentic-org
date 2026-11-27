//! Process and skill dispatch: the RPCs, the documents and the pages.
use org_command::{Answer, Refusal};
use org_proto::processes::{
    DocumentKindWire, DocumentStatusWire, EffectiveProcessEntryWire, ProcessDocumentWire,
    ProcessEffectiveResult, ProcessLayerWire, ProcessShowParams, ProcessShowResult,
    ProcessWhyResult, ProcessWriteResult, SkillCategoryWire, SkillListResult, SkillReadResult,
    SkillSummaryWire,
};
use serde_json::Value;

use crate::runtime::transport::OfficeTransport;

struct StubTransport {
    seen: Vec<(String, Value)>,
}

impl OfficeTransport for StubTransport {
    fn call_method(&mut self, method: &str, params: Value) -> Result<Value, Refusal> {
        self.seen.push((method.to_owned(), params));
        match method {
            "process.effective" => Ok(serde_json::to_value(ProcessEffectiveResult {
                scope_chain: vec!["Organization".into()],
                entries: vec![EffectiveProcessEntryWire {
                    id: "0190a5d2-7c3e-7a01-8b2c-3d4e5f6071a1".into(),
                    key: Some("code-review".into()),
                    kind: DocumentKindWire::Process,
                    applies: true,
                    mandatory: false,
                    winner_kind: Some("defined".into()),
                    document: sample_doc("code-review"),
                    replaces: None,
                    problem: None,
                    chain: vec![],
                }],
                problems: vec![],
                computed_at: "2026-11-22T00:00:00Z".into(),
                stale_since: None,
            })
            .unwrap()),
            "process.why" => Ok(serde_json::to_value(ProcessWhyResult {
                key: "code-review".into(),
                chain: vec![ProcessLayerWire {
                    document: sample_doc("code-review"),
                    outcome: "wins".into(),
                    detail: None,
                }],
                winner: Some(sample_doc("code-review")),
            })
            .unwrap()),
            "process.show" => Ok(serde_json::to_value(ProcessShowResult {
                document: sample_doc("code-review"),
                blob: "sha256:abc".into(),
                content: "# Review".into(),
                total_bytes: 8,
                next_offset: None,
                parent: None,
            })
            .unwrap()),
            "process.write" => Ok(serde_json::to_value(ProcessWriteResult {
                document_id: "0190a5d2-7c3e-7a01-8b2c-3d4e5f6071a1".into(),
                document: sample_doc("new-proc"),
                revision: 1,
            })
            .unwrap()),
            "skill.list" => Ok(serde_json::to_value(SkillListResult {
                skills: vec![SkillSummaryWire {
                    key: "agent-os-basics".into(),
                    title: "Using agent-os".into(),
                    description: "Basics".into(),
                    source: "built_in".into(),
                    category: Some(SkillCategoryWire::Workflow),
                }],
            })
            .unwrap()),
            "skill.read" => Ok(serde_json::to_value(SkillReadResult {
                key: "agent-os-basics".into(),
                title: "Using agent-os".into(),
                description: "Basics".into(),
                source: "built_in".into(),
                category: Some(SkillCategoryWire::Workflow),
                body_markdown: "# Basics".into(),
            })
            .unwrap()),
            other => panic!("unexpected method {other}"),
        }
    }
}

fn sample_doc(key: &str) -> ProcessDocumentWire {
    ProcessDocumentWire {
        id: "0190a5d2-7c3e-7a01-8b2c-3d4e5f6071a1".into(),
        key: Some(key.into()),
        revision: 1,
        kind: DocumentKindWire::Process,
        title: "Code Review".into(),
        status: DocumentStatusWire::Current,
        owner_employee_id: Some("0190a5d2-7c3e-7a01-8b2c-3d4e5f6071a2".into()),
        owner_name: Some("Owner".into()),
        level: "organization".into(),
        team_id: None,
        project_id: None,
        scope_name: Some("Organization".into()),
        overridable: true,
        summary: "Review all PRs".into(),
        body_markdown: "# Review".into(),
        path: "state/processes/0190a5d2-7c3e-7a01-8b2c-3d4e5f6071a1.md".into(),
        created_at: None,
        created_by_kind: "employee".into(),
        created_by_id: None,
        updated_at: None,
        updated_by_kind: "employee".into(),
        updated_by_id: None,
    }
}

struct StubOffice {
    tr: StubTransport,
}

impl org_command::Office for StubOffice {
    fn call(&mut self, request: &org_command::Request) -> Result<Answer, Refusal> {
        super::dispatch(&mut self.tr, request, crate::registry::runtime_table()).unwrap_or_else(
            || {
                panic!(
                    "unhandled command {} {}",
                    request.command.resource, request.command.verb
                )
            },
        )
    }
}

#[test]
fn process_and_skill_cli_commands_dispatch_expected_rpcs() {
    let mut office = StubOffice {
        tr: StubTransport { seen: vec![] },
    };

    for args in [
        vec!["process", "list"],
        vec!["process", "inspect", "--id", "code-review"],
        vec!["process", "inspect", "--key", "code-review", "--why"],
        vec![
            "process",
            "create",
            "--project",
            "0190a5d2-7c3e-7a01-8b2c-3d4e5f6071a9",
            "--kind",
            "process",
            "--title",
            "Local Setup",
            "--body",
            "# Setup",
        ],
        vec!["skill", "list"],
        vec!["skill", "list", "--category", "product_guide"],
        vec!["skill", "read", "--name", "agent-os-basics"],
    ] {
        let owned: Vec<String> = args.into_iter().map(str::to_owned).collect();
        let out = crate::execute_with_office("agent-os", &owned, &mut office);
        assert_eq!(out.exit_code, 0, "command {owned:?} failed: {}", out.stdout);
    }

    let methods: Vec<&str> = office.tr.seen.iter().map(|(m, _)| m.as_str()).collect();
    assert_eq!(
        methods,
        vec![
            "process.effective",
            "process.show",
            "process.why",
            "process.write",
            "skill.list",
            "skill.list",
            "skill.read",
        ]
    );
    assert_eq!(office.tr.seen[5].1["category"], "product_guide");

    // Passing both --task and --project is refused cleanly.
    let conflict_args: Vec<String> = vec!["process", "list", "--task", "T-1", "--project", "P-1"]
        .into_iter()
        .map(str::to_owned)
        .collect();
    let bad = crate::execute_with_office("agent-os", &conflict_args, &mut office);
    assert_ne!(bad.exit_code, 0);
    assert!(bad.stdout.contains("mutually exclusive"));
}

#[test]
fn process_inspect_why_preserves_chain_and_winner_through_shape_conform() {
    struct WhyTransport;
    impl OfficeTransport for WhyTransport {
        fn call_method(&mut self, method: &str, _params: Value) -> Result<Value, Refusal> {
            assert_eq!(method, "process.why");
            let mut org_doc = sample_doc("code-review");
            org_doc.level = "organization".into();
            org_doc.title = "Org Code Review".into();
            org_doc.scope_name = Some("Organization".into());
            let mut team_doc = sample_doc("code-review");
            team_doc.id = "0190a5d2-7c3e-7a01-8b2c-3d4e5f6071aa".into();
            team_doc.level = "team".into();
            team_doc.team_id = Some("0190a5d2-7c3e-7a01-8b2c-3d4e5f607110".into());
            team_doc.scope_name = Some("Platform".into());
            team_doc.title = "Team Code Review".into();
            let mut proj_doc = sample_doc("code-review");
            proj_doc.id = "0190a5d2-7c3e-7a01-8b2c-3d4e5f6071bb".into();
            proj_doc.level = "project".into();
            proj_doc.project_id = Some("0190a5d2-7c3e-7a01-8b2c-3d4e5f607120".into());
            proj_doc.scope_name = Some("Billing".into());
            proj_doc.title = "Project Code Review".into();
            proj_doc.revision = 2;
            Ok(serde_json::to_value(ProcessWhyResult {
                key: "code-review".into(),
                chain: vec![
                    ProcessLayerWire {
                        document: org_doc,
                        outcome: "replaced".into(),
                        detail: None,
                    },
                    ProcessLayerWire {
                        document: team_doc,
                        outcome: "replaced".into(),
                        detail: Some("replaced by project layer".into()),
                    },
                    ProcessLayerWire {
                        document: proj_doc.clone(),
                        outcome: "applies".into(),
                        detail: None,
                    },
                ],
                winner: Some(proj_doc),
            })
            .unwrap())
        }
    }

    let mut office = crate::runtime::transport::TransportOffice(WhyTransport);
    let args: Vec<String> = vec!["process", "inspect", "--key", "code-review", "--why"]
        .into_iter()
        .map(str::to_owned)
        .collect();
    let out = crate::execute_with_office("agent-os", &args, &mut office);
    assert_eq!(out.exit_code, 0, "{}", out.stdout);
    let env: Value = serde_json::from_str(&out.stdout).unwrap();
    assert_eq!(env["ok"], Value::Bool(true));
    assert_eq!(env["data"]["key"], "code-review");
    let chain = env["data"]["chain"].as_array().expect("chain preserved");
    assert_eq!(chain.len(), 3);
    assert_eq!(chain[0]["document"]["level"], "organization");
    assert_eq!(chain[1]["outcome"], "replaced");
    assert_eq!(chain[1]["detail"], "replaced by project layer");
    assert_eq!(
        chain[1]["document"]["team_id"],
        "0190a5d2-7c3e-7a01-8b2c-3d4e5f607110"
    );
    assert_eq!(chain[1]["document"]["scope_name"], "Platform");
    assert_eq!(chain[2]["outcome"], "applies");
    assert_eq!(
        chain[2]["document"]["project_id"],
        "0190a5d2-7c3e-7a01-8b2c-3d4e5f607120"
    );
    assert_eq!(chain[2]["document"]["scope_name"], "Billing");
    assert_eq!(
        env["data"]["winner"]["id"],
        "0190a5d2-7c3e-7a01-8b2c-3d4e5f6071bb"
    );
    assert_eq!(env["data"]["winner"]["title"], "Project Code Review");

    // Combining --why with --offset or combining --key with --id fails closed before any RPC.
    for bad_args in [
        vec![
            "process",
            "inspect",
            "--key",
            "code-review",
            "--why",
            "--offset",
            "10",
        ],
        vec![
            "process",
            "inspect",
            "--key",
            "code-review",
            "--id",
            "0190a5d2-7c3e-7a01-8b2c-3d4e5f6071bb",
        ],
    ] {
        let owned: Vec<String> = bad_args.into_iter().map(str::to_owned).collect();
        let out_bad = crate::execute_with_office("agent-os", &owned, &mut office);
        assert_eq!(out_bad.exit_code, 1, "case {owned:?}: {}", out_bad.stdout);
    }
}

#[test]
fn process_inspect_offset_paginates_chunked_doc_and_preserves_parent() {
    const CHUNK_LIMIT: usize = 512 * 1024;
    let full_body = format!("{}END", "a".repeat(600_000));
    let total_len = full_body.len();

    struct ChunkedTransport {
        body: String,
        seen_offsets: Vec<Option<usize>>,
    }
    impl OfficeTransport for ChunkedTransport {
        fn call_method(&mut self, method: &str, params: Value) -> Result<Value, Refusal> {
            assert_eq!(method, "process.show");
            let parsed: ProcessShowParams = serde_json::from_value(params).unwrap();
            self.seen_offsets.push(parsed.offset);
            let start = parsed.offset.unwrap_or(0).min(self.body.len());
            let end = (start + CHUNK_LIMIT).min(self.body.len());
            let next_offset = (end < self.body.len()).then_some(end);
            let mut parent = sample_doc("code-review");
            parent.id = "0190a5d2-7c3e-7a01-8b2c-3d4e5f607100".into();
            parent.level = "organization".into();
            let mut doc = sample_doc("code-review");
            doc.level = "project".into();
            Ok(serde_json::to_value(ProcessShowResult {
                document: doc,
                blob: "sha256:chunked".into(),
                content: self.body[start..end].to_owned(),
                total_bytes: self.body.len(),
                next_offset,
                parent: Some(parent),
            })
            .unwrap())
        }
    }

    let mut office = crate::runtime::transport::TransportOffice(ChunkedTransport {
        body: full_body.clone(),
        seen_offsets: vec![],
    });

    let p1_args: Vec<String> = vec!["process", "inspect", "--key", "code-review"]
        .into_iter()
        .map(str::to_owned)
        .collect();
    let out1 = crate::execute_with_office("agent-os", &p1_args, &mut office);
    assert_eq!(out1.exit_code, 0, "{}", out1.stdout);
    let env1: Value = serde_json::from_str(&out1.stdout).unwrap();
    assert_eq!(env1["data"]["total_bytes"], total_len);
    assert_eq!(env1["data"]["next_offset"], CHUNK_LIMIT);
    assert_eq!(
        env1["data"]["parent"]["id"],
        "0190a5d2-7c3e-7a01-8b2c-3d4e5f607100"
    );
    let chunk1 = env1["data"]["content"].as_str().unwrap();
    assert_eq!(chunk1.len(), CHUNK_LIMIT);

    let next_str = CHUNK_LIMIT.to_string();
    let p2_args: Vec<String> = vec![
        "process",
        "inspect",
        "--key",
        "code-review",
        "--offset",
        &next_str,
    ]
    .into_iter()
    .map(str::to_owned)
    .collect();
    let out2 = crate::execute_with_office("agent-os", &p2_args, &mut office);
    assert_eq!(out2.exit_code, 0, "{}", out2.stdout);
    let env2: Value = serde_json::from_str(&out2.stdout).unwrap();
    assert_eq!(env2["data"]["total_bytes"], total_len);
    assert!(env2["data"].get("next_offset").is_none());
    let chunk2 = env2["data"]["content"].as_str().unwrap();
    assert_eq!(format!("{chunk1}{chunk2}"), full_body);
    assert_eq!(office.0.seen_offsets, vec![None, Some(CHUNK_LIMIT)]);

    // Negative --offset fails closed with zero RPC calls.
    let neg_args: Vec<String> = vec![
        "process",
        "inspect",
        "--key",
        "code-review",
        "--offset",
        "-1",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect();
    let out_neg = crate::execute_with_office("agent-os", &neg_args, &mut office);
    assert_eq!(out_neg.exit_code, 1);
    assert_eq!(office.0.seen_offsets.len(), 2);
}
