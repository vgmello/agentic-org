//! Tests for `org_copilot::watcher` (`ADR-0024`, `WATCH-3`, `WATCH-4`, `WATCH-10`, `WATCH-11`).

use std::process::Command;
use std::sync::Arc;
use std::time::Duration;

use org_copilot::watcher::{
    evaluate_watch_corpus, run_channel_summarize, run_watch_pass, ChannelSummarizeInput,
    SummarizeMessageInput, WatchError, WatchEvalCase, WatchMessageInput, WatchPassInput,
    WatchQuestionInput, DERIVED_SUMMARY_NOTICE,
};
use org_copilot::{
    CancelToken, CopilotRuntime, FakeRuntime, Limits, Reply, RunOutput, RunRequest, RuntimeError,
    Usage,
};
use serde_json::json;

fn load_corpus() -> Vec<WatchEvalCase> {
    let raw = include_str!("watch_eval_corpus.json");
    serde_json::from_str(raw).expect("watch_eval_corpus.json must parse")
}

#[test]
fn decision_19_corpus_has_at_least_50_labelled_cases_and_evaluator_computes_metrics() {
    let corpus = load_corpus();
    assert!(
        corpus.len() >= 50,
        "ADR-0024 decision 19 requires >= 50 labelled excerpts, found {}",
        corpus.len()
    );
    // Perfect oracle runtime verifies evaluator arithmetic and secret skipping across all 55 cases.
    let replies: Vec<Reply> = corpus
        .iter()
        .map(|case| {
            let matches: Vec<serde_json::Value> = case
                .expected_matches
                .iter()
                .map(|(m, q)| json!({ "message_id": m, "question_id": q, "confident": true }))
                .collect();
            Reply::Text(json!({ "draft": { "matches": matches }, "notes": [] }).to_string())
        })
        .collect();
    let runtime: Arc<dyn CopilotRuntime> = Arc::new(FakeRuntime::new(replies));
    let summary = evaluate_watch_corpus(runtime, Limits::default(), &corpus);
    assert_eq!(summary.total_cases, corpus.len());
    assert_eq!(summary.failed_cases, 0);
    assert!(summary.total_real_responses >= 30);
    assert_eq!(summary.found_real_responses, summary.total_real_responses);
    assert_eq!(summary.confident_precision, 1.0);
    assert_eq!(summary.recall, 1.0);
    assert!(summary.passed_90pct_gate);
}

#[test]
fn watch_pass_drops_self_matches_skips_secrets_and_rejects_fabricated_ids() {
    // 1. Secret message is skipped without failing the pass; asker self-match is dropped; real match kept.
    let input = WatchPassInput {
        open_questions: vec![WatchQuestionInput {
            id: "q1".into(),
            asker: "ada".into(),
            text: "Which DB version?".into(),
            additions: vec![],
        }],
        new_messages: vec![
            WatchMessageInput {
                id: "m-secret".into(),
                author: "dev".into(),
                text: "token ghp_1234567890abcdefghij1234567890abcd".into(),
            },
            WatchMessageInput {
                id: "m-self".into(),
                author: "ada".into(),
                text: "Never mind I found it".into(),
            },
            WatchMessageInput {
                id: "m-real".into(),
                author: "owner".into(),
                text: "Postgres 16.4".into(),
            },
        ],
        earlier_messages: vec![WatchMessageInput {
            id: "m-old".into(),
            author: "owner".into(),
            text: "Earlier message".into(),
        }],
    };
    struct FixedUsageRuntime {
        text: String,
        usage: Usage,
    }
    impl CopilotRuntime for FixedUsageRuntime {
        fn available(&self) -> bool {
            true
        }
        fn run(&self, _req: &RunRequest, _c: &CancelToken) -> Result<RunOutput, RuntimeError> {
            Ok(RunOutput {
                text: self.text.clone(),
                usage: Some(self.usage),
            })
        }
    }

    let rt_ok = FixedUsageRuntime {
        text: json!({
            "draft": {
                "matches": [
                    { "message_id": "m-self", "question_id": "q1", "confident": true },
                    { "message_id": "m-real", "question_id": "q1", "confident": true }
                ]
            },
            "notes": []
        })
        .to_string(),
        usage: Usage {
            input_tokens: Some(120),
            output_tokens: Some(25),
        },
    };
    let out = run_watch_pass(rt_ok, Limits::default(), "chan-1", &input);
    assert!(out.model_called);
    assert_eq!(out.usage.and_then(|u| u.input_tokens), Some(120));
    let res = out.result.expect("pass should succeed");
    assert_eq!(res.skipped_secret_message_ids, vec!["m-secret"]);
    assert_eq!(res.matches.len(), 1);
    assert_eq!(res.matches[0].message_id, "m-real");

    // 2. Fabricated message_id or earlier_messages ID fails the entire pass & preserves usage (WATCH-4, d15).
    let rt_bad = FixedUsageRuntime {
        text: json!({
            "draft": {
                "matches": [
                    { "message_id": "m-real", "question_id": "q1", "confident": true },
                    { "message_id": "m-old", "question_id": "q1", "confident": true }
                ]
            },
            "notes": []
        })
        .to_string(),
        usage: Usage {
            input_tokens: Some(100),
            output_tokens: Some(20),
        },
    };
    let out_bad = run_watch_pass(rt_bad, Limits::default(), "chan-1", &input);
    assert!(out_bad.model_called);
    assert_eq!(out_bad.result, Err(WatchError::InvalidReference));
    assert_eq!(out_bad.usage.and_then(|u| u.input_tokens), Some(100));
}

#[test]
fn channel_summarize_validates_cited_ids_skips_secrets_and_attaches_derived_notice() {
    let input = ChannelSummarizeInput {
        channel: "#backend".into(),
        question: "What DB version was chosen?".into(),
        messages: vec![
            SummarizeMessageInput {
                id: "m1".into(),
                author: "dev".into(),
                text: "Oops sk-ant-oat01-1234567890abcdefghijklmnop".into(),
                created_at: "2026-11-21T10:00:00Z".into(),
            },
            SummarizeMessageInput {
                id: "m2".into(),
                author: "owner".into(),
                text: "We picked Postgres 16.4.".into(),
                created_at: "2026-11-21T10:01:00Z".into(),
            },
        ],
    };
    let rt = FakeRuntime::new(vec![Reply::Text(
        json!({
            "draft": {
                "summary": "The team chose Postgres 16.4.",
                "message_ids": ["m2"]
            },
            "notes": []
        })
        .to_string(),
    )]);
    let out = run_channel_summarize(rt, Limits::default(), "ada", &input);
    let res = out.result.expect("summarize should succeed");
    assert_eq!(res.summary, "The team chose Postgres 16.4.");
    assert_eq!(res.message_ids, vec!["m2"]);
    assert_eq!(res.derived_notice, DERIVED_SUMMARY_NOTICE);
    assert_eq!(res.skipped_secret_message_ids, vec!["m1"]);

    // Fabricated citation fails closed.
    let rt_bad = FakeRuntime::new(vec![Reply::Text(
        json!({
            "draft": {
                "summary": "Claimed from invented message.",
                "message_ids": ["m-invented"]
            },
            "notes": []
        })
        .to_string(),
    )]);
    let out_bad = run_channel_summarize(rt_bad, Limits::default(), "ada", &input);
    assert_eq!(out_bad.result, Err(WatchError::InvalidReference));
}

/// Optional live Decision 19 measurement harness (`WATCH-11`).
/// Supports both `codex` (default, e.g. `gpt-5.4-mini`) and `claude` (e.g. `claude-haiku-4-5`).
/// Run explicitly with:
/// `cargo test -p org-copilot --test watch_eval live_decision_19_accuracy_measurement -- --ignored --nocapture`
#[derive(Clone)]
struct LiveCliEvalRuntime {
    driver: String,
    model: String,
}

impl CopilotRuntime for LiveCliEvalRuntime {
    fn available(&self) -> bool {
        true
    }

    fn run(&self, request: &RunRequest, cancel: &CancelToken) -> Result<RunOutput, RuntimeError> {
        if cancel.is_cancelled() {
            return Err(RuntimeError::TimedOut);
        }
        if self.driver == "claude" {
            self.run_claude(request)
        } else {
            self.run_codex(request)
        }
    }
}

impl LiveCliEvalRuntime {
    fn run_claude(&self, request: &RunRequest) -> Result<RunOutput, RuntimeError> {
        let dir = std::env::temp_dir();
        let output = Command::new("claude")
            .current_dir(&dir)
            .args([
                "-p",
                "--model",
                &self.model,
                "--tools",
                "",
                "--output-format",
                "json",
                "--no-session-persistence",
            ])
            .arg(&request.prompt)
            .output()
            .map_err(|_| RuntimeError::Failed)?;
        if !output.status.success() {
            return Err(RuntimeError::Failed);
        }
        let envelope: serde_json::Value =
            serde_json::from_slice(&output.stdout).map_err(|_| RuntimeError::Failed)?;
        if envelope
            .get("is_error")
            .and_then(|v| v.as_bool())
            .unwrap_or(false)
        {
            return Err(RuntimeError::Failed);
        }
        let raw_text = envelope
            .get("result")
            .and_then(|v| v.as_str())
            .ok_or(RuntimeError::Failed)?
            .trim()
            .to_owned();
        let usage = envelope.get("usage").map(|u| Usage {
            input_tokens: u.get("input_tokens").and_then(|v| v.as_u64()),
            output_tokens: u.get("output_tokens").and_then(|v| v.as_u64()),
        });
        Ok(RunOutput {
            text: strip_json_fence(&raw_text),
            usage,
        })
    }

    fn run_codex(&self, request: &RunRequest) -> Result<RunOutput, RuntimeError> {
        use std::sync::atomic::{AtomicU64, Ordering};
        static SEQ: AtomicU64 = AtomicU64::new(1);
        let id = SEQ.fetch_add(1, Ordering::Relaxed);
        let work_dir =
            std::env::temp_dir().join(format!("org-watch-eval-{}-{id}", std::process::id()));
        let _ = std::fs::create_dir_all(&work_dir);
        let out_file = work_dir.join("last.txt");
        let mut cmd = Command::new("codex");
        cmd.current_dir(&work_dir).args([
            "exec",
            "--skip-git-repo-check",
            "--sandbox",
            "read-only",
            "--ephemeral",
            "-C",
            work_dir.to_str().unwrap_or("/tmp"),
            "-o",
            out_file.to_str().unwrap_or("/tmp/codex-watch-eval.txt"),
        ]);
        if !self.model.is_empty() {
            cmd.args(["-m", &self.model]);
        }
        cmd.arg(&request.prompt);
        let output = cmd.output().map_err(|_| RuntimeError::Failed)?;
        let raw = std::fs::read_to_string(&out_file).unwrap_or_default();
        let _ = std::fs::remove_dir_all(&work_dir);
        if !output.status.success() || raw.trim().is_empty() {
            return Err(RuntimeError::Failed);
        }
        Ok(RunOutput {
            text: strip_json_fence(&raw),
            usage: None,
        })
    }
}

fn strip_json_fence(text: &str) -> String {
    let trimmed = text.trim();
    if let Some(rest) = trimmed.strip_prefix("```json") {
        if let Some(inner) = rest.strip_suffix("```") {
            return inner.trim().to_owned();
        }
    }
    if let Some(rest) = trimmed.strip_prefix("```") {
        if let Some(inner) = rest.strip_suffix("```") {
            return inner.trim().to_owned();
        }
    }
    trimmed.to_owned()
}

#[test]
#[ignore = "runs 55 live watching-model calls for ADR-0024 decision 19 measurement"]
fn live_decision_19_accuracy_measurement() {
    use std::collections::BTreeSet;
    let corpus = load_corpus();
    let driver = std::env::var("WATCH_EVAL_DRIVER").unwrap_or_else(|_| "codex".into());
    let model = std::env::var("WATCH_EVAL_MODEL").unwrap_or_else(|_| {
        if driver == "claude" {
            "claude-haiku-4-5".into()
        } else {
            String::new()
        }
    });
    let rt = LiveCliEvalRuntime {
        driver: driver.clone(),
        model: model.clone(),
    };
    let limits = Limits {
        timeout: Duration::from_secs(60),
        max_concurrent_runs: 16,
        ..Limits::default()
    };

    // Run in 6 parallel worker slices so all 55 live excerpts complete quickly.
    let mut handles = Vec::new();
    let chunk_size = corpus.len().div_ceil(6);
    for chunk in corpus.chunks(chunk_size) {
        let cases = chunk.to_vec();
        let rt_c = rt.clone();
        let lim_c = limits.clone();
        handles.push(std::thread::spawn(move || {
            let mut results = Vec::new();
            for c in cases {
                let out = run_watch_pass(rt_c.clone(), lim_c.clone(), &c.id, &c.input);
                results.push((c, out.result));
            }
            results
        }));
    }

    let mut failed_cases = 0usize;
    let mut total_real = 0usize;
    let mut found_real = 0usize;
    let mut total_confident = 0usize;
    let mut correct_confident = 0usize;
    for h in handles {
        for (case, res) in h.join().unwrap() {
            let expected: BTreeSet<(String, String)> =
                case.expected_matches.iter().cloned().collect();
            total_real += expected.len();
            match res {
                Err(e) => {
                    failed_cases += 1;
                    eprintln!("FAIL {}: {e}", case.id);
                }
                Ok(pass) => {
                    let returned: BTreeSet<(String, String)> = pass
                        .matches
                        .iter()
                        .map(|m| (m.message_id.clone(), m.question_id.clone()))
                        .collect();
                    found_real += expected.intersection(&returned).count();
                    for m in &pass.matches {
                        if m.confident {
                            total_confident += 1;
                            if expected.contains(&(m.message_id.clone(), m.question_id.clone())) {
                                correct_confident += 1;
                            } else {
                                eprintln!(
                                    "FALSE_CONFIDENT {} ({}): ({}, {})",
                                    case.id, case.scenario, m.message_id, m.question_id
                                );
                            }
                        }
                    }
                    for miss in expected.difference(&returned) {
                        eprintln!(
                            "MISSED {} ({}): ({}, {})",
                            case.id, case.scenario, miss.0, miss.1
                        );
                    }
                }
            }
        }
    }
    let confident_precision = if total_confident == 0 {
        1.0
    } else {
        correct_confident as f64 / total_confident as f64
    };
    let recall = if total_real == 0 {
        1.0
    } else {
        found_real as f64 / total_real as f64
    };
    let passed_90pct_gate = failed_cases == 0
        && total_confident > 0
        && confident_precision >= org_copilot::watcher::DECISION_19_PRECISION_THRESHOLD;
    let summary = org_copilot::watcher::WatchEvalSummary {
        total_cases: corpus.len(),
        failed_cases,
        total_real_responses: total_real,
        found_real_responses: found_real,
        total_confident_matches: total_confident,
        correct_confident_matches: correct_confident,
        confident_precision,
        recall,
        passed_90pct_gate,
    };
    println!("DRIVER: {driver} MODEL: {model}");
    println!(
        "SUMMARY: {}",
        serde_json::to_string_pretty(&summary).unwrap()
    );
    assert_eq!(summary.total_cases, corpus.len());
}
