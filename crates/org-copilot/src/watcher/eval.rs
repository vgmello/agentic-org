//! The Decision 19 accuracy harness: a labelled corpus run through `run_watch_pass`.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use super::watch::run_watch_pass;
use super::{WatchPassInput, DECISION_19_PRECISION_THRESHOLD};
use crate::limits::Limits;
use crate::runtime::CopilotRuntime;

/// One labelled channel excerpt for `ADR-0024` decision 19 (`WATCH-11`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WatchEvalCase {
    pub id: String,
    pub scenario: String,
    pub input: WatchPassInput,
    /// Ground-truth `(message_id, question_id)` pairs that genuinely respond to an open question.
    pub expected_matches: Vec<(String, String)>,
}

/// Aggregate accuracy report for `ADR-0024` decision 19 (`WATCH-11`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WatchEvalSummary {
    pub total_cases: usize,
    pub failed_cases: usize,
    pub total_real_responses: usize,
    pub found_real_responses: usize,
    pub total_confident_matches: usize,
    pub correct_confident_matches: usize,
    pub confident_precision: f64,
    pub recall: f64,
    pub passed_90pct_gate: bool,
}

/// Runs a corpus of labelled channel excerpts through `run_watch_pass` and computes Decision 19
/// confident-match precision and recall (`WATCH-11`).
pub fn evaluate_watch_corpus<R: CopilotRuntime + Clone>(
    runtime: R,
    limits: Limits,
    cases: &[WatchEvalCase],
) -> WatchEvalSummary {
    let mut failed_cases = 0usize;
    let mut total_real = 0usize;
    let mut found_real = 0usize;
    let mut total_confident = 0usize;
    let mut correct_confident = 0usize;

    for case in cases {
        let expected: BTreeSet<(String, String)> = case.expected_matches.iter().cloned().collect();
        total_real += expected.len();
        let outcome = run_watch_pass(runtime.clone(), limits.clone(), &case.id, &case.input);
        let Ok(res) = outcome.result else {
            failed_cases += 1;
            continue;
        };
        let returned_pairs: BTreeSet<(String, String)> = res
            .matches
            .iter()
            .map(|m| (m.message_id.clone(), m.question_id.clone()))
            .collect();
        found_real += expected.intersection(&returned_pairs).count();
        for m in &res.matches {
            if m.confident {
                total_confident += 1;
                if expected.contains(&(m.message_id.clone(), m.question_id.clone())) {
                    correct_confident += 1;
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
        && confident_precision >= DECISION_19_PRECISION_THRESHOLD;

    WatchEvalSummary {
        total_cases: cases.len(),
        failed_cases,
        total_real_responses: total_real,
        found_real_responses: found_real,
        total_confident_matches: total_confident,
        correct_confident_matches: correct_confident,
        confident_precision,
        recall,
        passed_90pct_gate,
    }
}
