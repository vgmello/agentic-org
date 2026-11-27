use std::fs;
use std::os::unix::fs::symlink;

use org_agent_runtime::transcript::{LoadedSubagentTranscript, TranscriptEntryKind as RtEntryKind};
use org_agent_runtime::AgentRuntime as _;
use org_proto::sessions::{FileEventKind, TranscriptEntryKind, TranscriptFormat};

use crate::runtime_registry::RuntimeRegistry;

use super::{list_agent_sessions, load_agent_transcript, TranscriptQuery};

fn temp_home(label: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "agentic-org-transcript-{label}-{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("create temp home");
    dir
}

#[test]
fn claude_parser_nests_subagent_applies_toggles_and_redacts_secrets() {
    let home = temp_home("claude");
    let runtimes = RuntimeRegistry::production();
    let proj_dir = home.join(".claude/projects/-tmp-work");
    let sub_dir = proj_dir.join("sess-c1/subagents");
    fs::create_dir_all(&sub_dir).unwrap();

    let active_tok = "tok_live_session_secret_9876543210";
    let main_log = [
        r#"{"type":"user","timestamp":"2026-11-22T01:00:00Z","message":{"role":"user","content":"Fix login with sk-ant-oat01-ABCDEF1234567890"}}"#,
        r#"{"type":"assistant","timestamp":"2026-11-22T01:00:02Z","message":{"model":"claude-sonnet-4-6","usage":{"input_tokens":100,"output_tokens":50,"cache_read_input_tokens":20},"content":[{"type":"thinking","thinking":"Checking auth module"},{"type":"text","text":"Launching helper and editing src/auth.rs"},{"type":"tool_use","id":"toolu_edit1","name":"Edit","input":{"file_path":"src/auth.rs","old_string":"let k = \"sk-proj-SECRET12345678\";","new_string":"let k = env(\"KEY\");","api_key":"super-secret-value"}},{"type":"tool_use","id":"toolu_sub1","name":"Agent","input":{"prompt":"Inspect logs"}} ]}}"#,
        &format!(
            r#"{{"type":"user","timestamp":"2026-11-22T01:00:03Z","message":{{"role":"user","content":[{{"type":"tool_result","tool_use_id":"toolu_edit1","content":"ok AGENT_OS_SESSION_TOKEN={active_tok} Bearer secret_jwt_token_12345"}}]}}}}"#
        ),
        r#"{"type":"custom_future_vendor_event","foo":"bar"}"#,
        "not valid json line",
    ]
    .join("\n");
    fs::write(proj_dir.join("sess-c1.jsonl"), main_log).unwrap();

    let sub_log = [
        r#"{"type":"user","parentToolUseId":"toolu_sub1","timestamp":"2026-11-22T01:00:02.500Z","message":{"role":"user","content":"Inspect auth logs"}}"#,
        r#"{"type":"assistant","timestamp":"2026-11-22T01:00:02.800Z","message":{"model":"claude-haiku-4-5","usage":{"input_tokens":40,"output_tokens":15},"content":[{"type":"tool_use","id":"toolu_r1","name":"Read","input":{"file_path":"logs/app.log"}},{"type":"text","text":"Found AIzaSyD1234567890abcdef in logs"}]}}"#,
    ]
    .join("\n");
    fs::write(sub_dir.join("agent-a5d3.jsonl"), sub_log).unwrap();
    fs::write(
        sub_dir.join("agent-a5d3.meta.json"),
        r#"{"description":"Log inspector","parent_tool_use_id":"toolu_sub1"}"#,
    )
    .unwrap();

    let listed = list_agent_sessions(
        "emp-1",
        &home,
        &runtimes,
        Some("sess-c1"),
        None,
        &[active_tok],
    );
    assert_eq!(listed.sessions.len(), 1);
    assert!(listed.sessions[0].is_live);
    assert_eq!(listed.sessions[0].format, TranscriptFormat::Claude);
    assert_eq!(listed.sessions[0].subagent_count, 1);
    let preview = listed.sessions[0].preview.as_deref().unwrap_or("");
    assert!(!preview.contains("sk-ant-oat01"));
    assert!(preview.contains("[REDACTED]"));

    let full = load_agent_transcript(
        "emp-1",
        &home,
        &runtimes,
        Some("sess-c1"),
        Some("sess-c1"),
        TranscriptQuery::default(),
        &[active_tok],
    )
    .unwrap()
    .expect("transcript loaded");
    assert_eq!(full.format, TranscriptFormat::Claude);
    assert!(full.is_live);
    assert_eq!(full.usage.input_tokens, 140);
    assert_eq!(full.usage.output_tokens, 65);
    assert_eq!(full.files.len(), 1);
    assert_eq!(full.files[0].path, "src/auth.rs");
    assert_eq!(full.files[0].operations, vec![FileEventKind::Edit]);

    // Verify SUB-6 nesting on the launching tool call.
    let sub_holder = full
        .entries
        .iter()
        .find(|e| e.tool_use_id.as_deref() == Some("toolu_sub1"))
        .expect("subagent tool call");
    let sub = sub_holder.subagent.as_ref().expect("nested subagent");
    assert_eq!(sub.agent_id, "a5d3");
    assert_eq!(sub.label.as_deref(), Some("Log inspector"));
    assert_eq!(sub.files.len(), 1);
    assert_eq!(sub.files[0].path, "logs/app.log");

    // Verify unknown + malformed entries preserved as Raw.
    let raw_count = full
        .entries
        .iter()
        .filter(|e| e.kind == TranscriptEntryKind::Raw)
        .count();
    assert_eq!(raw_count, 2);

    // Verify zero secret leakage anywhere in serialized JSON.
    let serialized = serde_json::to_string(&full).unwrap();
    for secret in [
        active_tok,
        "sk-ant-oat01-ABCDEF1234567890",
        "sk-proj-SECRET12345678",
        "super-secret-value",
        "secret_jwt_token_12345",
        "AIzaSyD1234567890abcdef",
    ] {
        assert!(
            !serialized.contains(secret),
            "secret leaked into transcript payload: {secret}"
        );
    }

    // Verify toggles hide tool calls and file visualizations while preserving nested sub-agents.
    let toggled = load_agent_transcript(
        "emp-1",
        &home,
        &runtimes,
        Some("sess-c1"),
        Some("sess-c1"),
        TranscriptQuery {
            include_tool_calls: false,
            include_file_changes: false,
            include_subagents: true,
            cursor: 0,
            limit: 100,
        },
        &[active_tok],
    )
    .unwrap()
    .unwrap();
    assert!(toggled.files.is_empty());
    assert!(toggled.entries.iter().all(|e| !matches!(
        e.kind,
        TranscriptEntryKind::ToolCall
            | TranscriptEntryKind::ToolResult
            | TranscriptEntryKind::FileEvent
    )));
    assert!(toggled
        .entries
        .iter()
        .any(|e| e.kind == TranscriptEntryKind::SubagentRun && e.subagent.is_some()));

    let _ = fs::remove_dir_all(&home);
}

#[test]
fn codex_and_agy_parsers_extract_messages_files_usage_and_block_symlinks() {
    let home = temp_home("codex-agy");
    let runtimes = RuntimeRegistry::production();
    let codex_dir = home.join(".codex/sessions/2026/11/22");
    fs::create_dir_all(&codex_dir).unwrap();
    let codex_log = [
        r#"{"timestamp":"2026-11-22T02:00:00Z","type":"session_meta","payload":{"id":"codex-sess-1","cli_version":"0.114.0","model_provider":"openai"}}"#,
        r#"{"timestamp":"2026-11-22T02:00:01Z","type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"Patch Cargo.toml"}]}}"#,
        r#"{"timestamp":"2026-11-22T02:00:02Z","type":"response_item","payload":{"type":"custom_tool_call","call_id":"call_1","name":"apply_patch","input":"*** Begin Patch\n*** Update File: Cargo.toml\n@@\n+dep = \"1\"\n*** End Patch"}}"#,
        r#"{"timestamp":"2026-11-22T02:00:03Z","type":"response_item","payload":{"type":"custom_tool_call_output","call_id":"call_1","output":"Updated Cargo.toml"}}"#,
        r#"{"timestamp":"2026-11-22T02:00:04Z","type":"event_msg","payload":{"type":"token_count","info":{"last_token_usage":{"input_tokens":80,"output_tokens":30,"cached_input_tokens":10,"reasoning_output_tokens":12}}}}"#,
        r#"{"timestamp":"2026-11-22T02:00:05Z","type":"response_item","payload":{"type":"message","role":"assistant","content":[{"type":"output_text","text":"Done patching Cargo.toml."}]}}"#,
    ]
    .join("\n");
    fs::write(codex_dir.join("rollout-2026-codex-sess-1.jsonl"), codex_log).unwrap();

    // Symlink escape attempt outside private_home must be ignored completely.
    let outside = std::env::temp_dir().join(format!("outside-secret-{}.jsonl", std::process::id()));
    fs::write(
        &outside,
        r#"{"type":"user","message":{"content":"TOP_SECRET"}}"#,
    )
    .unwrap();
    let _ = symlink(&outside, codex_dir.join("evil-link.jsonl"));

    // Agy main + sub-agent transcripts.
    let agy_main = home.join(".gemini/antigravity-cli/brain/agy-conv-1/.system_generated/logs");
    let agy_sub = home.join(".gemini/antigravity-cli/brain/agy-sub-2/.system_generated/logs");
    fs::create_dir_all(&agy_main).unwrap();
    fs::create_dir_all(&agy_sub).unwrap();
    let agy_log = [
        r#"{"step_index":1,"type":"USER_INPUT","created_at":"2026-11-22T03:00:00Z","content":"Review main.rs"}"#,
        r#"{"step_index":2,"type":"PLANNER_RESPONSE","created_at":"2026-11-22T03:00:01Z","thinking":"Inspecting file","content":"Reading main.rs and invoking reviewer","tool_calls":[{"id":"c1","name":"read_file","arguments":{"Path":"src/main.rs"}},{"id":"c2","name":"invoke_subagent","arguments":{"Role":"Reviewer"}}],"usage":{"input_tokens":50,"output_tokens":20}}"#,
        r#"{"step_index":3,"type":"TOOL_RESPONSE","tool_call_id":"c2","result":{"conversationId":"agy-sub-2"},"spec":{"role":"Reviewer"}}"#,
    ]
    .join("\n");
    fs::write(agy_main.join("transcript.jsonl"), agy_log).unwrap();
    fs::write(
        agy_sub.join("transcript.jsonl"),
        r#"{"step_index":1,"type":"PLANNER_RESPONSE","content":"Looks clean!","usage":{"input_tokens":10,"output_tokens":5}}"#,
    )
    .unwrap();

    let listed = list_agent_sessions("emp-2", &home, &runtimes, None, None, &[]);
    assert!(listed
        .sessions
        .iter()
        .any(|s| s.session_id == "codex-sess-1"));
    assert!(listed.sessions.iter().any(|s| s.session_id == "agy-conv-1"));
    assert!(!listed.sessions.iter().any(|s| s.session_id == "evil-link"));

    let codex_tx = load_agent_transcript(
        "emp-2",
        &home,
        &runtimes,
        Some("codex-sess-1"),
        None,
        TranscriptQuery::default(),
        &[],
    )
    .unwrap()
    .unwrap();
    assert_eq!(codex_tx.format, TranscriptFormat::Codex);
    assert_eq!(codex_tx.usage.input_tokens, 80);
    assert_eq!(codex_tx.usage.reasoning_tokens, 12);
    assert_eq!(codex_tx.files.len(), 1);
    assert_eq!(codex_tx.files[0].path, "Cargo.toml");
    assert_eq!(codex_tx.files[0].operations, vec![FileEventKind::Edit]);

    let agy_tx = load_agent_transcript(
        "emp-2",
        &home,
        &runtimes,
        Some("agy-conv-1"),
        None,
        TranscriptQuery::default(),
        &[],
    )
    .unwrap()
    .unwrap();
    assert_eq!(agy_tx.format, TranscriptFormat::Agy);
    assert_eq!(agy_tx.usage.input_tokens, 60);
    assert_eq!(agy_tx.files.len(), 1);
    assert_eq!(agy_tx.files[0].path, "src/main.rs");
    assert!(agy_tx.entries.iter().any(|e| e
        .subagent
        .as_ref()
        .is_some_and(|s| s.agent_id == "agy-sub-2")));

    let _ = fs::remove_file(&outside);
    let _ = fs::remove_dir_all(&home);
}

#[test]
fn pass1_redaction_and_parser_edge_cases() {
    // High #3 & #4: quoted env vars, case-insensitive bearer/basic, sk-ant-ort, PGP key, JSON secrets in text, camelCase keys.
    let raw = concat!(
        "export OPENAI_API_KEY=\"quoted-openai-secret-999\"\n",
        "AGENT_OS_SESSION_TOKEN='single-quoted-tok-888'\n",
        "ANTHROPIC_AUTH_TOKEN=plain_auth_tok_777\n",
        "authorization: bearer lowercase_bearer_secret_666\n",
        "Authorization: BASIC YWRtaW46c2VjcmV0MTIz\n",
        "refresh=sk-ant-ort01-REFRESH1234567890\n",
        "{\"accessToken\":\"camel_secret_555\",\"refresh_token\":\"snake_refresh_444\",\"x-api-key\":\"hdr_key_333\"}\n",
        "-----BEGIN PGP PRIVATE KEY BLOCK-----\nabc123\n-----END PGP PRIVATE KEY BLOCK-----\n",
        "opaque-provider-cred-xyz987\n"
    );
    let redacted = super::redact::redact_text(raw, &["opaque-provider-cred-xyz987"]);
    for secret in [
        "quoted-openai-secret-999",
        "single-quoted-tok-888",
        "plain_auth_tok_777",
        "lowercase_bearer_secret_666",
        "YWRtaW46c2VjcmV0MTIz",
        "REFRESH1234567890",
        "camel_secret_555",
        "snake_refresh_444",
        "hdr_key_333",
        "abc123",
        "opaque-provider-cred-xyz987",
    ] {
        assert!(
            !redacted.contains(secret),
            "Pass 1 redaction leaked `{secret}` in:\n{redacted}"
        );
    }

    let codex_rt = org_agent_runtime_codex::CodexRuntime::new();
    let codex_parser = codex_rt.transcript_parser().unwrap();
    let claude_rt = org_agent_runtime_claude::ClaudeRuntime::new();
    let claude_parser = claude_rt.transcript_parser().unwrap();
    let agy_rt = org_agent_runtime_agy::AgyRuntime::new();
    let agy_parser = agy_rt.transcript_parser().unwrap();

    // Medium #2: Codex item.started ignored, cumulative total_token_usage maxed (not summed), unique raw-* IDs.
    let codex_edge = [
        r#"{"type":"item.started","item":{"type":"agent_message","text":"dup"}}"#,
        r#"{"type":"item.completed","item":{"type":"agent_message","text":"dup"}}"#,
        r#"{"type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":{"input_tokens":100,"output_tokens":40}}}}"#,
        r#"{"type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":{"input_tokens":150,"output_tokens":60}}}}"#,
        r#"{"type":"response_item","payload":{"type":"unknown_a"}}"#,
        r#"{"type":"response_item","payload":{"type":"unknown_b"}}"#,
    ]
    .join("\n");
    let parsed_codex = codex_parser.parse(&codex_edge);
    assert_eq!(
        parsed_codex
            .entries
            .iter()
            .filter(|e| e.kind == RtEntryKind::AssistantMessage)
            .count(),
        1
    );
    assert_eq!(parsed_codex.usage.input_tokens, 150);
    assert_eq!(parsed_codex.usage.output_tokens, 60);
    let raw_ids: Vec<&str> = parsed_codex
        .entries
        .iter()
        .filter(|e| e.kind == RtEntryKind::Raw)
        .map(|e| e.id.as_str())
        .collect();
    assert_eq!(raw_ids.len(), 2);
    assert_ne!(raw_ids[0], raw_ids[1]);

    // Medium #3: Claude duplicate message.id usage deduplicated & multi-subagent on same parent_tool_use_id preserved.
    let claude_edge = [
        r#"{"type":"assistant","message":{"id":"msg_1","model":"claude-sonnet-4-6","usage":{"input_tokens":100,"output_tokens":20},"content":[{"type":"text","text":"part 1"}]}}"#,
        r#"{"type":"assistant","message":{"id":"msg_1","model":"claude-sonnet-4-6","usage":{"input_tokens":100,"output_tokens":50},"content":[{"type":"tool_use","id":"tu_1","name":"Agent","input":{}}]}}"#,
    ]
    .join("\n");
    let mut parsed_claude = claude_parser.parse(&claude_edge);
    assert_eq!(parsed_claude.usage.input_tokens, 100);
    assert_eq!(parsed_claude.usage.output_tokens, 50);
    let sub1 = claude_parser
        .parse(r#"{"type":"assistant","message":{"content":[{"type":"text","text":"s1"}]}}"#);
    let sub2 = claude_parser
        .parse(r#"{"type":"assistant","message":{"content":[{"type":"text","text":"s2"}]}}"#);
    claude_parser.attach_subagents(
        &mut parsed_claude,
        vec![
            LoadedSubagentTranscript {
                agent_id: "a1".into(),
                label: Some("first".into()),
                parent_tool_use_id: Some("tu_1".into()),
                parsed: sub1,
            },
            LoadedSubagentTranscript {
                agent_id: "a2".into(),
                label: Some("second".into()),
                parent_tool_use_id: Some("tu_1".into()),
                parsed: sub2,
            },
        ],
    );
    let attached_ids: Vec<&str> = parsed_claude
        .entries
        .iter()
        .filter_map(|e| e.subagent.as_ref().map(|s| s.agent_id.as_str()))
        .collect();
    assert_eq!(attached_ids, vec!["a1", "a2"]);

    // Medium #1: Agy sub-agent matches by tool_call_id, skips self-ref and duplicates, never attaches to ToolResult.
    let agy_edge = [
        r#"{"type":"PLANNER_RESPONSE","tool_calls":[{"id":"call_first","name":"invoke_subagent","arguments":{}},{"id":"call_second","name":"invoke_subagent","arguments":{}}]}"#,
        r#"{"type":"TOOL_RESPONSE","tool_name":"invoke_subagent","tool_call_id":"call_second","result":{"conversationId":"sub-z"},"spec":{"role":"Second"}}"#,
        r#"{"type":"TOOL_RESPONSE","tool_name":"invoke_subagent","tool_call_id":"call_first","result":{"conversationId":"sub-a"},"spec":{"role":"First"}}"#,
        r#"{"type":"TOOL_RESPONSE","tool_name":"invoke_subagent","tool_call_id":"call_first","result":{"conversationId":"sub-a"},"spec":{"role":"Dup"}}"#,
        r#"{"type":"TOOL_RESPONSE","tool_name":"invoke_subagent","tool_call_id":"call_self","result":{"conversationId":"parent-conv"}}"#,
    ]
    .join("\n");
    let refs = agy_parser.extract_subagent_refs(&agy_edge, "parent-conv");
    assert_eq!(refs.len(), 2);
    let mut parsed_agy = agy_parser.parse(&agy_edge);
    let sub_list = refs
        .into_iter()
        .map(|r| {
            let parsed = agy_parser.parse(
                r#"{"type":"PLANNER_RESPONSE","content":"ok","usage":{"input_tokens":5,"output_tokens":2}}"#,
            );
            LoadedSubagentTranscript {
                agent_id: r.agent_id,
                label: r.label,
                parent_tool_use_id: r.parent_tool_use_id,
                parsed,
            }
        })
        .collect();
    agy_parser.attach_subagents(&mut parsed_agy, sub_list);
    assert_eq!(parsed_agy.usage.input_tokens, 10);
    let call_first = parsed_agy
        .entries
        .iter()
        .find(|e| e.kind == RtEntryKind::ToolCall && e.tool_use_id.as_deref() == Some("call_first"))
        .unwrap();
    assert_eq!(call_first.subagent.as_ref().unwrap().agent_id, "sub-a");
    let call_second = parsed_agy
        .entries
        .iter()
        .find(|e| {
            e.kind == RtEntryKind::ToolCall && e.tool_use_id.as_deref() == Some("call_second")
        })
        .unwrap();
    assert_eq!(call_second.subagent.as_ref().unwrap().agent_id, "sub-z");
    assert!(parsed_agy
        .entries
        .iter()
        .filter(|e| e.kind == RtEntryKind::ToolResult)
        .all(|e| e.subagent.is_none()));

    // Pass 2 High #1: multi-byte UTF-8 straddling byte 80 after "-----BEGIN " must not panic.
    let utf8_pem = format!("-----BEGIN {}PRIVATE", "é".repeat(40));
    let _ = super::redact::redact_text(&utf8_pem, &[]);

    // Pass 2 High #2: stray quote or escaped quote must NOT flip quote parity and leak JSON secrets.
    let stray = "Error: missing \" in config\n{\"accessToken\":\"secret_val_999\"}";
    let stray_redacted = super::redact::redact_text(stray, &[]);
    assert!(
        !stray_redacted.contains("secret_val_999"),
        "stray quote leaked secret: {stray_redacted}"
    );
    let escaped = "{\"msg\":\"say \\\"hi\",\"accessToken\":\"secret_val_888\"}";
    let escaped_redacted = super::redact::redact_text(escaped, &[]);
    assert!(
        !escaped_redacted.contains("secret_val_888"),
        "escaped quote leaked secret: {escaped_redacted}"
    );
}

#[test]
fn symlinked_intermediate_parent_directory_is_rejected_fail_closed() {
    let home = temp_home("parent-symlink-toctou");
    let canon_home = home.canonicalize().unwrap();
    let runtimes = RuntimeRegistry::production();

    // Outside directory holding a sensitive transcript-shaped file.
    let outside = temp_home("parent-symlink-outside");
    let outside_file = outside.join("sess-escape.jsonl");
    fs::write(
        &outside_file,
        r#"{"type":"user","message":{"role":"user","content":"OUTSIDE_SECRET_PAYLOAD"}}"#,
    )
    .unwrap();

    // Symlinked intermediate parent directory under `.claude/projects/<dir>` -> outside.
    let projects = canon_home.join(".claude/projects");
    fs::create_dir_all(&projects).unwrap();
    let linked_proj = projects.join("proj-link");
    symlink(&outside, &linked_proj).unwrap();

    // Direct open through the symlinked parent dir MUST fail closed via `open_regular_beneath`.
    let through_outside_link = linked_proj.join("sess-escape.jsonl");
    assert!(
        super::discover::open_safe_regular_file(&through_outside_link, &canon_home).is_err(),
        "open_safe_regular_file must reject symlinked parent directory escaping home"
    );

    // Even a symlinked intermediate parent pointing to ANOTHER directory INSIDE `canon_home`
    // MUST fail closed because `open_regular_beneath` walks every component with `O_NOFOLLOW`.
    let real_inside = canon_home.join("real-dir");
    fs::create_dir_all(&real_inside).unwrap();
    fs::write(
        real_inside.join("sess-inside.jsonl"),
        r#"{"type":"user","message":{"role":"user","content":"inside"}}"#,
    )
    .unwrap();
    let inside_link = projects.join("proj-inside-link");
    symlink(&real_inside, &inside_link).unwrap();
    assert!(
        super::discover::open_safe_regular_file(
            &inside_link.join("sess-inside.jsonl"),
            &canon_home
        )
        .is_err(),
        "open_safe_regular_file must reject symlinked intermediate directory even inside home"
    );

    // Agy symlinked intermediate `.system_generated` directory -> outside dir.
    let agy_conv = canon_home.join(".gemini/antigravity-cli/brain/agy-link-conv");
    fs::create_dir_all(&agy_conv).unwrap();
    let outside_logs = outside.join("logs");
    fs::create_dir_all(&outside_logs).unwrap();
    fs::write(
        outside_logs.join("transcript.jsonl"),
        r#"{"type":"USER_INPUT","content":"OUTSIDE_AGY_SECRET"}"#,
    )
    .unwrap();
    symlink(&outside, agy_conv.join(".system_generated")).unwrap();
    assert!(
        super::discover::open_safe_regular_file(
            &agy_conv.join(".system_generated/logs/transcript.jsonl"),
            &canon_home,
        )
        .is_err(),
        "open_safe_regular_file must reject symlinked Agy .system_generated directory"
    );

    // End-to-end session discovery and loading MUST ignore all symlinked parent directories.
    let listed = list_agent_sessions("emp-sym", &home, &runtimes, None, None, &[]);
    assert!(
        listed.sessions.is_empty(),
        "symlinked parent dirs must yield zero discovered sessions: {:?}",
        listed.sessions
    );
    let loaded = load_agent_transcript(
        "emp-sym",
        &home,
        &runtimes,
        Some("sess-escape"),
        None,
        TranscriptQuery::default(),
        &[],
    )
    .unwrap();
    assert!(loaded.is_none());

    let _ = fs::remove_dir_all(&outside);
    let _ = fs::remove_dir_all(&home);
}

#[test]
fn subagent_count_unsafe_ids_and_per_subagent_entries_are_capped() {
    let home = temp_home("subagent-caps");
    let runtimes = RuntimeRegistry::production();
    let proj_dir = home.join(".claude/projects/-tmp-work");
    let sub_dir = proj_dir.join("sess-cap/subagents");
    fs::create_dir_all(&sub_dir).unwrap();

    fs::write(
        proj_dir.join("sess-cap.jsonl"),
        r#"{"type":"user","message":{"role":"user","content":"Run many subagents"}}"#,
    )
    .unwrap();

    // Write 40 valid subagents (> MAX_SUBAGENTS_PER_TRANSCRIPT = 32), with the first subagent
    // containing 400 assistant entries (> MAX_SUBAGENT_ENTRIES = 256).
    let oversized_sub: String = (0..400)
        .map(|i| {
            format!(
                r#"{{"type":"assistant","message":{{"content":[{{"type":"text","text":"entry-{i}"}}]}}}}"#
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    fs::write(sub_dir.join("agent-sub-000.jsonl"), oversized_sub).unwrap();
    for idx in 1..40 {
        fs::write(
            sub_dir.join(format!("agent-sub-{idx:03}.jsonl")),
            format!(
                r#"{{"type":"assistant","message":{{"content":[{{"type":"text","text":"ok-{idx}"}}]}}}}"#
            ),
        )
        .unwrap();
    }

    // Plant unsafe `agent_id` filenames that MUST be rejected by `is_safe_session_id`.
    for bad in [
        "agent-..escape.jsonl",
        "agent-..jsonl",
        "agent-.jsonl",
        "agent-bad name!.jsonl",
    ] {
        fs::write(
            sub_dir.join(bad),
            r#"{"type":"assistant","message":{"content":[{"type":"text","text":"should never load"}]}}"#,
        )
        .unwrap();
    }

    let listed = list_agent_sessions("emp-cap", &home, &runtimes, None, None, &[]);
    assert_eq!(listed.sessions.len(), 1);
    assert_eq!(
        listed.sessions[0].subagent_count as usize,
        super::MAX_SUBAGENTS_PER_TRANSCRIPT
    );

    let loaded = load_agent_transcript(
        "emp-cap",
        &home,
        &runtimes,
        Some("sess-cap"),
        None,
        TranscriptQuery::default(),
        &[],
    )
    .unwrap()
    .expect("transcript loaded");
    assert!(
        loaded.truncated,
        "capped subagents/entries must mark truncated"
    );

    let subagents: Vec<_> = loaded
        .entries
        .iter()
        .filter_map(|e| e.subagent.as_ref())
        .collect();
    assert_eq!(subagents.len(), super::MAX_SUBAGENTS_PER_TRANSCRIPT);
    for sub in &subagents {
        assert!(
            super::is_safe_session_id(&sub.agent_id),
            "unsafe agent_id leaked: {}",
            sub.agent_id
        );
        assert!(
            sub.entries.len() <= super::MAX_SUBAGENT_ENTRIES,
            "subagent {} exceeded MAX_SUBAGENT_ENTRIES: {}",
            sub.agent_id,
            sub.entries.len()
        );
        assert_eq!(sub.entry_count as usize, sub.entries.len());
    }
    let first_sub = subagents
        .iter()
        .find(|s| s.agent_id == "sub-000")
        .expect("sub-000 loaded");
    assert_eq!(first_sub.entries.len(), super::MAX_SUBAGENT_ENTRIES);

    let _ = fs::remove_dir_all(&home);
}

#[test]
fn agy_sibling_subagents_are_capped_and_mark_truncated() {
    let home = temp_home("agy-subagent-caps");
    let runtimes = RuntimeRegistry::production();
    let agy_brain = home.join(".gemini/antigravity-cli/brain");
    let agy_main = agy_brain.join("agy-cap-main/.system_generated/logs");
    fs::create_dir_all(&agy_main).unwrap();
    let mut agy_lines = vec![
        r#"{"type":"USER_INPUT","content":"Fan out"}"#.to_owned(),
        r#"{"type":"TOOL_RESPONSE","tool_call_id":"bad_0","result":{"conversationId":"../escape"},"spec":{"role":"Bad"}}"#.to_owned(),
    ];
    for idx in 0..40 {
        agy_lines.push(format!(
            r#"{{"type":"TOOL_RESPONSE","tool_call_id":"call_{idx}","result":{{"conversationId":"agy-child-{idx:03}"}},"spec":{{"role":"Child {idx}"}}}}"#
        ));
        let child_dir = agy_brain.join(format!("agy-child-{idx:03}/.system_generated/logs"));
        fs::create_dir_all(&child_dir).unwrap();
        fs::write(
            child_dir.join("transcript.jsonl"),
            r#"{"type":"PLANNER_RESPONSE","content":"done"}"#,
        )
        .unwrap();
    }
    fs::write(agy_main.join("transcript.jsonl"), agy_lines.join("\n")).unwrap();
    let agy_loaded = load_agent_transcript(
        "emp-cap",
        &home,
        &runtimes,
        Some("agy-cap-main"),
        None,
        TranscriptQuery::default(),
        &[],
    )
    .unwrap()
    .expect("agy transcript loaded");
    let agy_subs = agy_loaded
        .entries
        .iter()
        .filter(|e| e.subagent.is_some())
        .count();
    assert_eq!(agy_subs, super::MAX_SUBAGENTS_PER_TRANSCRIPT);
    assert!(
        agy_loaded.truncated,
        "Agy sessions exceeding MAX_SUBAGENTS_PER_TRANSCRIPT must mark truncated"
    );

    let _ = fs::remove_dir_all(&home);
}

#[test]
fn cumulative_transcript_byte_budget_stops_loading_subagents_at_16_mib() {
    let home = temp_home("subagent-byte-budget");
    let runtimes = RuntimeRegistry::production();
    let proj_dir = home.join(".claude/projects/-tmp-work");
    let sub_dir = proj_dir.join("sess-big/subagents");
    fs::create_dir_all(&sub_dir).unwrap();

    fs::write(
        proj_dir.join("sess-big.jsonl"),
        r#"{"type":"user","message":{"role":"user","content":"Big subagents"}}"#,
    )
    .unwrap();

    // Build a ~4.5 MiB subagent file using ~4 KiB JSONL lines so parsing stays fast (~ms).
    let filler = "a".repeat(4000);
    let one_line = format!(
        r#"{{"type":"assistant","message":{{"content":[{{"type":"text","text":"{filler}"}}]}}}}"#
    );
    let mut big_payload = String::with_capacity(4_600_000);
    while big_payload.len() < 4_500_000 {
        big_payload.push_str(&one_line);
        big_payload.push('\n');
    }
    // 5 x 4.5 MiB = 22.5 MiB > MAX_TOTAL_TRANSCRIPT_BYTES (16 MiB).
    // Subagents 0..=2 consume ~13.5 MiB, subagent 3 consumes the remaining ~2.5 MiB (truncated),
    // and subagent 4 MUST NOT be loaded at all.
    for idx in 0..5 {
        fs::write(sub_dir.join(format!("agent-big-{idx}.jsonl")), &big_payload).unwrap();
    }

    let loaded = load_agent_transcript(
        "emp-big",
        &home,
        &runtimes,
        Some("sess-big"),
        None,
        TranscriptQuery::default(),
        &[],
    )
    .unwrap()
    .expect("transcript loaded");
    assert!(
        loaded.truncated,
        "exhausted cumulative byte budget must mark truncated"
    );
    let loaded_ids: Vec<&str> = loaded
        .entries
        .iter()
        .filter_map(|e| e.subagent.as_ref().map(|s| s.agent_id.as_str()))
        .collect();
    assert!(
        loaded_ids.len() < 5,
        "cumulative 16 MiB budget must stop loading further subagents: {loaded_ids:?}"
    );
    assert!(
        !loaded_ids.contains(&"big-4"),
        "subagent past 16 MiB budget must not be loaded: {loaded_ids:?}"
    );

    let _ = fs::remove_dir_all(&home);
}

#[test]
fn secrets_in_subagent_agent_id_and_entry_id_are_redacted() {
    let home = temp_home("redact-ids");
    let runtimes = RuntimeRegistry::production();
    let proj_dir = home.join(".claude/projects/-tmp-work");
    let sub_dir = proj_dir.join("sess-id-secret/subagents");
    fs::create_dir_all(&sub_dir).unwrap();

    let active_secret = "tok_active_id_secret_123456789";
    let vendor_secret = "sk-ant-oat01-SECRETID1234567890";
    fs::write(
        proj_dir.join("sess-id-secret.jsonl"),
        r#"{"type":"user","message":{"role":"user","content":"Check ids"}}"#,
    )
    .unwrap();
    // Both filenames pass `is_safe_session_id` and flow into `sub.agent_id` AND `entry.id`.
    fs::write(
        sub_dir.join(format!("agent-{vendor_secret}.jsonl")),
        r#"{"type":"assistant","message":{"content":[{"type":"text","text":"sub-1"}]}}"#,
    )
    .unwrap();
    fs::write(
        sub_dir.join(format!("agent-{active_secret}.jsonl")),
        r#"{"type":"assistant","message":{"content":[{"type":"text","text":"sub-2"}]}}"#,
    )
    .unwrap();

    let loaded = load_agent_transcript(
        "emp-redact",
        &home,
        &runtimes,
        Some("sess-id-secret"),
        None,
        TranscriptQuery::default(),
        &[active_secret],
    )
    .unwrap()
    .expect("transcript loaded");

    let serialized = serde_json::to_string(&loaded).unwrap();
    assert!(
        !serialized.contains(vendor_secret),
        "vendor secret leaked via sub.agent_id / entry.id: {serialized}"
    );
    assert!(
        !serialized.contains(active_secret),
        "active secret leaked via sub.agent_id / entry.id: {serialized}"
    );
    for entry in &loaded.entries {
        assert!(
            !entry.id.contains(vendor_secret),
            "entry.id leaked vendor secret"
        );
        assert!(
            !entry.id.contains(active_secret),
            "entry.id leaked active secret"
        );
        if let Some(sub) = &entry.subagent {
            assert!(
                sub.agent_id.contains("[REDACTED]"),
                "sub.agent_id should be redacted: {}",
                sub.agent_id
            );
            assert!(
                entry.id.contains("[REDACTED]"),
                "subagent entry.id should be redacted: {}",
                entry.id
            );
        }
    }

    let _ = fs::remove_dir_all(&home);
}

#[test]
fn walk_regular_files_enforces_shared_visited_entries_budget_across_empty_dirs_and_symlinks() {
    let home = temp_home("visited-budget");
    let canon_home = fs::canonicalize(&home).unwrap();
    let root = canon_home.join(".claude/projects");
    fs::create_dir_all(&root).unwrap();

    // Populate more than `MAX_VISITED_ENTRIES` empty directories and symlinks (0 regular files,
    // so the `out.len() >= 512` cap never triggers).
    let target = canon_home.join("dangling-target");
    let total_entries = super::discover::MAX_VISITED_ENTRIES + 256;
    for idx in 0..total_entries {
        if idx % 2 == 0 {
            fs::create_dir(root.join(format!("empty-{idx:05}"))).unwrap();
        } else {
            org_platform::fs::create_symlink(&target, &root.join(format!("link-{idx:05}")))
                .unwrap();
        }
    }

    let mut visited = 0usize;
    let mut files = Vec::new();
    super::discover::walk_regular_files(&root, &canon_home, 0, &mut visited, &mut files);
    assert_eq!(
        visited,
        super::discover::MAX_VISITED_ENTRIES,
        "walk_regular_files must stop once MAX_VISITED_ENTRIES is reached"
    );
    assert!(files.is_empty());

    let _ = fs::remove_dir_all(&home);
}
