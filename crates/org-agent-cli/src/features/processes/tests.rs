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

struct SkillBindingTransport {
    bound: bool,
    calls: Vec<(String, Value)>,
}

impl SkillBindingTransport {
    fn new(bound: bool) -> Self {
        Self {
            bound,
            calls: Vec::new(),
        }
    }
}

impl OfficeTransport for SkillBindingTransport {
    fn call_method(&mut self, method: &str, params: Value) -> Result<Value, Refusal> {
        self.calls.push((method.to_owned(), params));
        match method {
            "skill.list" => Ok(serde_json::to_value(SkillListResult {
                skills: vec![
                    SkillSummaryWire {
                        key: "agent-os-basics".into(),
                        title: "Using agent-os".into(),
                        description: "Basics".into(),
                        source: "built_in".into(),
                        category: Some(SkillCategoryWire::Workflow),
                    },
                    SkillSummaryWire {
                        key: "org-release-checklist".into(),
                        title: "Org Release Checklist".into(),
                        description: "Org-pushed release runbook.".into(),
                        source: "organization".into(),
                        category: Some(SkillCategoryWire::Workflow),
                    },
                ],
            })
            .unwrap()),
            "skill.read" => Ok(serde_json::to_value(SkillReadResult {
                key: "org-release-checklist".into(),
                title: "Org Release Checklist".into(),
                description: "Org-pushed release runbook.".into(),
                source: "organization".into(),
                category: Some(SkillCategoryWire::Workflow),
                body_markdown: "# Org Release Checklist\nAlways tag.".into(),
            })
            .unwrap()),
            other => panic!("unexpected method {other}"),
        }
    }

    fn is_bound(&self) -> bool {
        self.bound
    }
}

fn run_skill(bound: bool, line: &[&str]) -> (crate::Output, Vec<(String, Value)>) {
    let args: Vec<String> = line.iter().map(|word| (*word).to_owned()).collect();
    let mut office = crate::runtime::transport::TransportOffice(SkillBindingTransport::new(bound));
    let out = crate::execute_with_office("agent-os", &args, &mut office);
    (out, office.0.calls)
}

#[test]
fn offline_skill_list_succeeds_unbound_and_bound_without_office_rpc() {
    let builtins = org_process::list_builtin_skills();
    for bound in [false, true] {
        let (out, calls) = run_skill(bound, &["skill", "list", "--offline"]);
        assert_eq!(out.exit_code, 0, "bound={bound}: {}", out.stdout);
        assert!(calls.is_empty(), "bound={bound} must not hit office RPC");
        let env: Value = serde_json::from_str(&out.stdout).unwrap();
        assert_eq!(env["ok"], true);
        assert_eq!(env["error"], Value::Null);
        assert_eq!(env["meta"]["status"], "completed");
        assert_eq!(env["meta"]["total"], builtins.len());
        assert_eq!(env["data"]["mode"], "offline_builtin_only");
        assert_eq!(env["data"]["org_skills_included"], false);
        let caveat = env["data"]["caveat"].as_str().expect("caveat string");
        assert!(
            !caveat.is_empty() && caveat.to_ascii_lowercase().contains("organization"),
            "{caveat}"
        );
        let skills = env["data"]["skills"].as_array().expect("skills array");
        assert_eq!(skills.len(), builtins.len().min(20));
        assert_eq!(skills[0]["key"], builtins[0].key);
        assert_eq!(skills[0]["source"], "built_in");
        assert_eq!(skills[0]["category"], builtins[0].category.as_str());
    }
}

#[test]
fn offline_skill_list_supports_category_filter_pagination_formats_and_fields() {
    for (wire_cat, proc_cat) in [
        ("workflow", org_process::SkillCategory::Workflow),
        ("product_guide", org_process::SkillCategory::ProductGuide),
        ("chief_of_staff", org_process::SkillCategory::ChiefOfStaff),
    ] {
        let expected = org_process::list_builtin_skills_by_category(Some(proc_cat));
        let (out, calls) = run_skill(
            false,
            &["skill", "list", "--offline", "--category", wire_cat],
        );
        assert_eq!(out.exit_code, 0, "{wire_cat}: {}", out.stdout);
        assert!(calls.is_empty());
        let env: Value = serde_json::from_str(&out.stdout).unwrap();
        assert_eq!(env["data"]["mode"], "offline_builtin_only");
        assert_eq!(env["data"]["org_skills_included"], false);
        assert_eq!(env["meta"]["total"], expected.len());
        let keys: Vec<&str> = env["data"]["skills"]
            .as_array()
            .unwrap()
            .iter()
            .map(|row| row["key"].as_str().unwrap())
            .collect();
        let expected_keys: Vec<&str> = expected.iter().map(|s| s.key).collect();
        assert_eq!(keys, expected_keys, "{wire_cat}");
    }

    // Invalid category fails closed with invalid_request and zero RPCs.
    let (bad_cat, calls) = run_skill(
        false,
        &["skill", "list", "--offline", "--category", "nonexistent"],
    );
    assert_eq!(bad_cat.exit_code, 2, "{}", bad_cat.stdout);
    assert!(calls.is_empty());
    let bad_env: Value = serde_json::from_str(&bad_cat.stdout).unwrap();
    assert_eq!(bad_env["ok"], false);
    assert_eq!(bad_env["error"]["code"], "invalid_request");

    // Pagination (--limit / --cursor) works and preserves partial-scope metadata even on empty page.
    let (page1, _) = run_skill(false, &["skill", "list", "--offline", "--limit", "2"]);
    assert_eq!(page1.exit_code, 0, "{}", page1.stdout);
    let env1: Value = serde_json::from_str(&page1.stdout).unwrap();
    assert_eq!(env1["data"]["skills"].as_array().unwrap().len(), 2);
    assert_eq!(env1["meta"]["next_cursor"], "2");

    let (page2, _) = run_skill(
        false,
        &[
            "skill",
            "list",
            "--offline",
            "--limit",
            "2",
            "--cursor",
            "2",
        ],
    );
    assert_eq!(page2.exit_code, 0, "{}", page2.stdout);
    let env2: Value = serde_json::from_str(&page2.stdout).unwrap();
    assert_eq!(env2["data"]["skills"].as_array().unwrap().len(), 2);
    assert_eq!(
        env2["data"]["skills"][0]["key"],
        org_process::list_builtin_skills()[2].key
    );

    let (empty_page, _) = run_skill(false, &["skill", "list", "--offline", "--cursor", "999"]);
    assert_eq!(empty_page.exit_code, 0, "{}", empty_page.stdout);
    let empty_env: Value = serde_json::from_str(&empty_page.stdout).unwrap();
    assert_eq!(empty_env["data"]["mode"], "offline_builtin_only");
    assert_eq!(empty_env["data"]["org_skills_included"], false);
    assert!(empty_env["data"]["caveat"]
        .as_str()
        .is_some_and(|c| !c.is_empty()));
    assert_eq!(empty_env["data"]["skills"], serde_json::json!([]));
    assert!(empty_env["meta"].get("next_cursor").is_none());

    // --json input, --fields projection, --format yaml, and --format compact all report offline scope.
    let (via_json, calls) = run_skill(
        false,
        &[
            "skill",
            "list",
            "--json",
            r#"{"offline":true,"category":"chief_of_staff"}"#,
            "--fields",
            "key,title",
        ],
    );
    assert_eq!(via_json.exit_code, 0, "{}", via_json.stdout);
    assert!(calls.is_empty());
    let json_env: Value = serde_json::from_str(&via_json.stdout).unwrap();
    assert_eq!(json_env["data"]["mode"], "offline_builtin_only");
    assert_eq!(json_env["data"]["org_skills_included"], false);
    let row = json_env["data"]["skills"][0].as_object().unwrap();
    assert_eq!(row.len(), 2);
    assert_eq!(row["key"], "tess-workflows");
    assert!(row.contains_key("title"));

    let (yaml_out, _) = run_skill(
        false,
        &[
            "skill",
            "list",
            "--offline",
            "--category",
            "chief_of_staff",
            "--format",
            "yaml",
        ],
    );
    assert_eq!(yaml_out.exit_code, 0, "{}", yaml_out.stdout);
    assert!(yaml_out.stdout.contains("mode: offline_builtin_only"));
    assert!(yaml_out.stdout.contains("org_skills_included: false"));
    assert!(yaml_out.stdout.contains("caveat:"));
    assert!(yaml_out.stdout.contains("key: tess-workflows"));

    let (compact_out, _) = run_skill(
        false,
        &[
            "skill",
            "list",
            "--offline",
            "--limit",
            "1",
            "--format",
            "compact",
        ],
    );
    assert_eq!(compact_out.exit_code, 0, "{}", compact_out.stdout);
    assert!(compact_out.stdout.contains("mode: offline_builtin_only\n"));
    assert!(compact_out.stdout.contains("org_skills_included: false\n"));
    assert!(compact_out.stdout.contains("caveat: "));
    assert!(compact_out.stdout.contains("meta: "));
}

#[test]
fn offline_skill_read_returns_builtin_with_metadata_and_not_found_for_unknown() {
    let expected = org_process::find_builtin_skill("agent-os-basics").unwrap();
    for (bound, queried_name) in [(false, "agent-os-basics"), (true, "AGENT-OS-BASICS")] {
        let (out, calls) = run_skill(
            bound,
            &["skill", "read", "--offline", "--name", queried_name],
        );
        assert_eq!(out.exit_code, 0, "bound={bound}: {}", out.stdout);
        assert!(calls.is_empty(), "bound={bound} must not hit office RPC");
        let env: Value = serde_json::from_str(&out.stdout).unwrap();
        assert_eq!(env["ok"], true);
        assert_eq!(env["data"]["key"], expected.key);
        assert_eq!(env["data"]["title"], expected.title);
        assert_eq!(env["data"]["description"], expected.description);
        assert_eq!(env["data"]["source"], "built_in");
        assert_eq!(env["data"]["category"], expected.category.as_str());
        assert_eq!(env["data"]["body_markdown"], expected.body_markdown);
        assert_eq!(env["data"]["mode"], "offline_builtin_only");
        assert_eq!(env["data"]["org_skills_included"], false);
        assert!(env["data"]["caveat"]
            .as_str()
            .is_some_and(|c| !c.is_empty() && c.to_ascii_lowercase().contains("organization")));
    }

    // YAML format includes explicit partial-scope fields clearly.
    let (yaml_out, _) = run_skill(
        false,
        &[
            "skill",
            "read",
            "--offline",
            "--name",
            "agent-os-basics",
            "--format",
            "yaml",
        ],
    );
    assert_eq!(yaml_out.exit_code, 0, "{}", yaml_out.stdout);
    assert!(yaml_out.stdout.contains("mode: offline_builtin_only"));
    assert!(yaml_out.stdout.contains("org_skills_included: false"));
    assert!(yaml_out.stdout.contains("caveat:"));

    // Unknown skill name returns clean NotFound with helpful hint and zero RPCs.
    let (missing, calls) = run_skill(
        false,
        &["skill", "read", "--offline", "--name", "nonexistent-skill"],
    );
    assert_eq!(missing.exit_code, 1, "{}", missing.stdout);
    assert!(calls.is_empty());
    let env: Value = serde_json::from_str(&missing.stdout).unwrap();
    assert_eq!(env["ok"], false);
    assert_eq!(env["error"]["code"], "not_found");
    let hint = env["error"]["hint"].as_str().unwrap();
    assert!(
        hint.contains("skill list --offline") && hint.contains("agent-os login"),
        "{hint}"
    );
}

#[test]
fn unbound_skill_commands_without_offline_fail_closed_with_helpful_hint() {
    for line in [
        &["skill", "list"][..],
        &["skill", "read", "--name", "agent-os-basics"],
    ] {
        let (out, calls) = run_skill(false, line);
        assert_eq!(out.exit_code, 1, "{line:?}: {}", out.stdout);
        assert!(calls.is_empty(), "{line:?} must not call office");
        let env: Value = serde_json::from_str(&out.stdout).unwrap();
        assert_eq!(env["ok"], false);
        assert_eq!(env["error"]["code"], "not_authorized");
        let hint = env["error"]["hint"].as_str().unwrap();
        assert!(
            hint.contains("--offline") && hint.contains("agent-os login"),
            "{line:?}: {hint}"
        );
    }
}

#[test]
fn online_bound_skill_commands_keep_exact_unchanged_response_shapes() {
    let (list_out, list_calls) = run_skill(true, &["skill", "list"]);
    assert_eq!(list_out.exit_code, 0, "{}", list_out.stdout);
    assert_eq!(list_calls.len(), 1);
    assert_eq!(list_calls[0].0, "skill.list");
    let list_env: Value = serde_json::from_str(&list_out.stdout).unwrap();
    let arr = list_env["data"]
        .as_array()
        .expect("online skill list returns array unchanged");
    assert_eq!(arr.len(), 2);
    assert_eq!(arr[0]["key"], "agent-os-basics");
    assert_eq!(arr[1]["key"], "org-release-checklist");
    assert_eq!(arr[1]["source"], "organization");
    assert_eq!(list_env["meta"]["total"], 2);

    let (read_out, read_calls) =
        run_skill(true, &["skill", "read", "--name", "org-release-checklist"]);
    assert_eq!(read_out.exit_code, 0, "{}", read_out.stdout);
    assert_eq!(read_calls.len(), 1);
    assert_eq!(read_calls[0].0, "skill.read");
    let read_env: Value = serde_json::from_str(&read_out.stdout).unwrap();
    assert_eq!(read_env["data"]["key"], "org-release-checklist");
    assert_eq!(read_env["data"]["source"], "organization");
    assert!(read_env["data"].get("mode").is_none());
    assert!(read_env["data"].get("org_skills_included").is_none());
    assert!(read_env["data"].get("caveat").is_none());
}

#[test]
fn offline_skill_read_fields_and_compact_always_retain_partial_scope_metadata() {
    for (fields, expected_keys) in [
        (
            "key,body_markdown",
            &[
                "key",
                "body_markdown",
                "mode",
                "org_skills_included",
                "caveat",
            ][..],
        ),
        (
            "key,mode",
            &["key", "mode", "org_skills_included", "caveat"][..],
        ),
    ] {
        let (json_out, calls) = run_skill(
            false,
            &[
                "skill",
                "read",
                "--offline",
                "--name",
                "agent-os-basics",
                "--fields",
                fields,
            ],
        );
        assert_eq!(json_out.exit_code, 0, "{fields}: {}", json_out.stdout);
        assert!(calls.is_empty());
        let env: Value = serde_json::from_str(&json_out.stdout).unwrap();
        let data = env["data"].as_object().unwrap();
        assert_eq!(data.len(), expected_keys.len(), "exact keys for {fields}");
        let mut prev_pos = 0;
        for key in expected_keys {
            assert!(data.contains_key(*key), "missing {key} for {fields}");
            let needle = format!("\"{key}\":");
            let pos = json_out
                .stdout
                .find(&needle)
                .expect("field present in serialized JSON");
            assert!(pos >= prev_pos, "schema order broken at {key} for {fields}");
            prev_pos = pos;
        }
        assert_eq!(data["mode"], "offline_builtin_only");
        assert_eq!(data["org_skills_included"], false);
        assert!(data["caveat"].as_str().is_some_and(|c| !c.is_empty()));

        let (yaml_out, _) = run_skill(
            false,
            &[
                "skill",
                "read",
                "--offline",
                "--name",
                "agent-os-basics",
                "--fields",
                fields,
                "--format",
                "yaml",
            ],
        );
        assert_eq!(yaml_out.exit_code, 0, "{fields}: {}", yaml_out.stdout);
        assert!(yaml_out.stdout.contains("mode: offline_builtin_only"));
        assert!(yaml_out.stdout.contains("org_skills_included: false"));
        assert!(yaml_out.stdout.contains("caveat:"));

        let (compact_out, _) = run_skill(
            false,
            &[
                "skill",
                "read",
                "--offline",
                "--name",
                "agent-os-basics",
                "--fields",
                fields,
                "--format",
                "compact",
            ],
        );
        assert_eq!(compact_out.exit_code, 0, "{fields}: {}", compact_out.stdout);
        assert!(compact_out.stdout.contains("mode: offline_builtin_only\n"));
        assert!(compact_out.stdout.contains("org_skills_included: false\n"));
        assert!(compact_out.stdout.contains("caveat: "));
    }

    // Compact output without --fields also prints all partial-scope fields cleanly.
    let (compact_full, _) = run_skill(
        false,
        &[
            "skill",
            "read",
            "--offline",
            "--name",
            "agent-os-basics",
            "--format",
            "compact",
        ],
    );
    assert_eq!(compact_full.exit_code, 0, "{}", compact_full.stdout);
    assert!(compact_full.stdout.contains("key: agent-os-basics\n"));
    assert!(compact_full.stdout.contains("mode: offline_builtin_only\n"));
    assert!(compact_full.stdout.contains("org_skills_included: false\n"));
    assert!(compact_full.stdout.contains("caveat: "));
}

#[test]
fn offline_skill_attach_compact_edges_bad_cursor_and_hint_enrichment() {
    use std::io::Cursor;

    // 1. `session attach` must route offline skill list|read through `registry::run_command`.
    let lines = concat!(
        r#"{"command":"skill list","input":{"offline":true,"category":"chief_of_staff"}}"#,
        "\n",
        r#"{"command":"skill list","options":{"offline":true,"category":"chief_of_staff"}}"#,
        "\n",
        r#"{"command":"skill read","input":{"name":"agent-os-basics","offline":true},"options":{"fields":"key,body_markdown"}}"#,
        "\n"
    );
    let mut office = crate::runtime::transport::TransportOffice(SkillBindingTransport::new(true));
    let mut out = Vec::new();
    let code = crate::registry::run_attach_loop(
        "agent-os",
        Cursor::new(lines.as_bytes()),
        &mut out,
        &mut office,
    );
    assert_eq!(code, 0);
    assert!(office.0.calls.is_empty());
    let rendered = String::from_utf8(out).unwrap();
    let reply_lines: Vec<&str> = rendered.lines().collect();
    assert_eq!(reply_lines.len(), 3, "{rendered}");
    for (idx, line) in reply_lines.iter().enumerate() {
        let env: Value = serde_json::from_str(line).unwrap();
        assert_eq!(env["ok"], true, "line {idx}: {line}");
        assert_eq!(env["data"]["mode"], "offline_builtin_only", "line {idx}");
        assert_eq!(env["data"]["org_skills_included"], false, "line {idx}");
        assert!(
            env["data"]["caveat"]
                .as_str()
                .is_some_and(|c| !c.is_empty()),
            "line {idx}"
        );
        if idx < 2 {
            assert!(env["data"]["skills"].is_array(), "line {idx}: {line}");
        } else {
            assert_eq!(env["data"]["key"], "agent-os-basics", "line {idx}");
        }
    }

    // 2. Compact empty page emits `skills: []` + `meta:`; single full page omits `meta:`.
    let (empty_compact, _) = run_skill(
        false,
        &[
            "skill",
            "list",
            "--offline",
            "--cursor",
            "999",
            "--format",
            "compact",
        ],
    );
    assert_eq!(empty_compact.exit_code, 0, "{}", empty_compact.stdout);
    assert!(empty_compact.stdout.contains("skills: []\n"));
    assert!(empty_compact.stdout.contains("meta: "));

    let (full_compact, _) = run_skill(
        false,
        &[
            "skill",
            "list",
            "--offline",
            "--category",
            "chief_of_staff",
            "--format",
            "compact",
        ],
    );
    assert_eq!(full_compact.exit_code, 0, "{}", full_compact.stdout);
    assert!(full_compact.stdout.contains("mode: offline_builtin_only\n"));
    assert!(!full_compact.stdout.contains("\nmeta: "));

    // 3. Invalid cursor fails closed with invalid_request.
    let (bad_cursor, _) = run_skill(false, &["skill", "list", "--offline", "--cursor", "bogus"]);
    assert_eq!(bad_cursor.exit_code, 1, "{}", bad_cursor.stdout);
    let bad_env: Value = serde_json::from_str(&bad_cursor.stdout).unwrap();
    assert_eq!(bad_env["error"]["code"], "invalid_request");

    // 4. `enrich_auth_refusal` still appends skill-specific `--offline` hint even when refusal cites `context --offline`.
    let base = Refusal::new(
        org_proto::ErrorCode::NotAuthorized,
        "cannot read binding file",
        "Run `agent-os context --offline` for offline rules.",
    );
    let enriched = super::skill::enrich_auth_refusal_for_test(base, "list");
    assert!(
        enriched.hint.contains("agent-os skill list --offline")
            && enriched.hint.contains("agent-os login"),
        "{}",
        enriched.hint
    );

    // 5. Offline JSON envelopes match `org_command::envelope_schema()` top-level contract.
    let schema = org_command::envelope_schema();
    let req_fields: Vec<&str> = schema["required"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap())
        .collect();
    for args in [
        &["skill", "list", "--offline"][..],
        &["skill", "read", "--offline", "--name", "agent-os-basics"][..],
    ] {
        let (resp, _) = run_skill(false, args);
        assert_eq!(resp.exit_code, 0);
        let env: Value = serde_json::from_str(&resp.stdout).unwrap();
        let obj = env.as_object().unwrap();
        for key in &req_fields {
            assert!(obj.contains_key(*key), "{args:?} missing {key}");
        }
        assert_eq!(obj.len(), req_fields.len());
    }
}
