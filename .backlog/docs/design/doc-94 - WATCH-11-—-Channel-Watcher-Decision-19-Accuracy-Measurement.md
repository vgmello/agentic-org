---
id: doc-94
title: WATCH-11 — Channel Watcher Decision 19 Accuracy Measurement
type: specification
created_date: '2026-11-22T01:02:00Z'
updated_date: '2026-11-22T01:02:00Z'
---
# WATCH-11 — Channel Watcher Decision 19 Accuracy Measurement

Recorded: 2026-11-22 (TASK-48 / C9, ADR-0024 Decision 19)

## Purpose

ADR-0024 Decision 19 requires an empirical accuracy measurement on at least 50 labelled channel excerpts before enabling automatic idle-agent wake-ups on confident person-authored matches (`WatcherConfig::confident_wake_enabled`), with a minimum threshold of **>= 90% confident-match precision**.

## Corpus and Method

- **Corpus:** `crates/org-copilot/tests/watch_eval_corpus.json` — **55** labelled channel excerpts covering direct replies, multi-agent single-pass matching, multi-question replies, indirect answers, prompt-injection attempts, unrelated chatter, self-messages, and secret-filtered messages.
- **Harness:** `crates/org-copilot/tests/watch_eval.rs` (`live_decision_19_accuracy_measurement`), running the exact `org_copilot::watcher::run_watch_pass` prompt, fenced-data framing, and strict JSON parser.
- **Measured model / runtime:** Codex CLI (`gpt-6.2-sol`). (Note: Claude CLI was unavailable during measurement due to weekly account quota exhaustion; `CliWatcherRuntimeProvider` continues to fail closed for drivers lacking a true zero-tool CLI mode per `WATCH-4` and `doc-8`.)

## Results

| Metric | Value | Threshold / Requirement |
| --- | --- | --- |
| Labelled excerpts evaluated | **55** | >= 50 (PASS) |
| Failed / invalid cases | **0** | 0 |
| Ground-truth real responses | **41** | — |
| Found real responses (Recall) | **40 / 41 (97.6%)** | Only missed secondary message `m2` in `ex-22` |
| Confident matches correct (Precision) | **40 / 40 (100.0%)** | >= 90.0% (PASS) |
| False positives (injection, noise, self, secret) | **0** | 0 (PASS) |

## Decision

The >= 90% confident-match precision threshold of ADR-0024 Decision 19 is **met (100.0% across 55 excerpts)**. Confident person-authored matches may wake idle agents when `WatcherConfig::confident_wake_enabled` is enabled; unconfident matches and agent-authored matches remain inbox-only (`WATCH-8`).
