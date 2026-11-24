"""Unit tests for kit_efficiency_watchdog.py (deterministic Sol/Codex efficiency watchdog)."""

from datetime import datetime, timedelta, timezone
import errno
import json
import os
from pathlib import Path
import stat
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "runtime"))
import kit_efficiency_watchdog as watchdog


def write_jsonl(path, records):
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text("\n".join(json.dumps(r) for r in records) + "\n", encoding="utf-8")


class EfficiencyWatchdogTests(unittest.TestCase):
    def iso_offset(self, hours=0.0, seconds=0.0):
        return (datetime.now(timezone.utc) + timedelta(hours=hours, seconds=seconds)).strftime("%Y-%m-%dT%H:%M:%SZ")

    def test_detects_real_hindsight_effort_drift_premature_compaction_wait_storm_and_unwrapped(self):
        with tempfile.TemporaryDirectory() as tmp:
            repo = Path(tmp) / "repo"
            (repo / ".codex").mkdir(parents=True)
            (repo / ".codex" / "config.toml").write_text(
                'model = "gpt-6.2-sol"\nmodel_auto_compact_token_limit = 250000\n',
                encoding="utf-8",
            )
            codex_home = Path(tmp) / "codex_home"
            codex_home.mkdir()
            (codex_home / "config.toml").write_text(
                'model = "gpt-6-luna"\nmodel_auto_compact_token_limit = 250000\n',
                encoding="utf-8",
            )
            sid = "01a20d5d-4d72-73b0-8b7d-67717bb89f66"
            ts1 = self.iso_offset(0)
            rollout = codex_home / "sessions" / "2026" / "11" / "24" / f"rollout-test-{sid}.jsonl"
            records = [
                {"timestamp": ts1, "type": "session_meta", "payload": {"id": sid, "cwd": str(repo)}},
                {"timestamp": ts1, "type": "turn_context", "payload": {"turn_id": "t1", "model": "gpt-6.2-sol", "effort": "xhigh", "sandbox_policy": {"type": "read-only"}}},
                {"timestamp": ts1, "type": "response_item", "payload": {"type": "message", "role": "developer", "content": [{"type": "input_text", "text": "<hindsight_pages>\nInjected"}]}},
                {"timestamp": ts1, "type": "event_msg", "payload": {"type": "item_completed", "turn_id": "t1", "item": {"type": "McpToolCall", "server": "hindsight", "tool": "hindsight_list_pages"}}},
                {"timestamp": ts1, "type": "event_msg", "payload": {"type": "token_count", "info": {"model_context_window": 475000, "last_token_usage": {"input_tokens": 250000, "cached_input_tokens": 200000}}}},
                {"timestamp": ts1, "type": "compacted", "payload": {}},
            ]
            # Short 1-second wait polls on the SAME cell_id="12" trigger WAIT_POLLING_STORM
            for i in range(5):
                records.append({"timestamp": self.iso_offset(seconds=i), "type": "response_item", "payload": {"type": "function_call", "name": "wait", "arguments": '{"cell_id":"12","yield_time_ms":1000}'}})
                records.append({"timestamp": self.iso_offset(seconds=i), "type": "event_msg", "payload": {"type": "token_count", "info": {"model_context_window": 475000, "last_token_usage": {"input_tokens": 120000, "cached_input_tokens": 115000}}}})
            # Turn 2 drifts effort xhigh -> high and reuses only 7500 of prior 120000 prefix (6.25%)
            records.extend([
                {"timestamp": self.iso_offset(seconds=10), "type": "turn_context", "payload": {"turn_id": "t2", "model": "gpt-6.2-sol", "effort": "high", "sandbox_policy": {"type": "read-only"}}},
                {"timestamp": self.iso_offset(seconds=10), "type": "event_msg", "payload": {"type": "token_count", "info": {"model_context_window": 475000, "last_token_usage": {"input_tokens": 150000, "cached_input_tokens": 7500}}}},
            ])
            write_jsonl(rollout, records)

            report = watchdog.run_watchdog(repo, window_hours=24.0, codex_home=codex_home, include_commits=False)
            codes = {a["code"] for a in report["alerts"]}
            self.assertEqual(report["status"], "ACTION_REQUIRED")
            self.assertIn("PREMATURE_COMPACT_CONFIG", codes)
            self.assertIn("HINDSIGHT_IN_READONLY_REVIEW", codes)
            self.assertIn("EFFORT_DRIFT_CACHE_MISS", codes)
            self.assertIn("PREMATURE_AUTO_COMPACTION", codes)
            self.assertIn("WAIT_POLLING_STORM", codes)
            self.assertIn("UNWRAPPED_CODEX_REVIEW", codes)

    def test_no_false_positives_for_js_text_writable_turn_duplicate_turn_id_prompt_growth_long_waits_and_old_failures(self):
        with tempfile.TemporaryDirectory() as tmp:
            repo = Path(tmp) / "repo"
            (repo / ".codex").mkdir(parents=True)
            (repo / ".codex" / "config.toml").write_text(
                'model = "gpt-6.2-sol"\nmodel_auto_compact_token_limit = 390000\n',
                encoding="utf-8",
            )
            codex_home = Path(tmp) / "codex_home"
            codex_home.mkdir()
            (codex_home / "config.toml").write_text('model = "gpt-6-luna"\n', encoding="utf-8")

            sid = "01a20e6b-954c-7c60-9b3f-79a55f3e5ab9"
            old_ts = self.iso_offset(-72)
            now_ts = self.iso_offset(0)
            manifest_dir = repo / ".agent-kit" / "logs" / "sol-reviews"
            manifest_dir.mkdir(parents=True)
            (manifest_dir / f"{sid}.json").write_text(
                json.dumps(
                    {
                        "session_id": sid,
                        "effort": "max",
                        "last_finished_at": now_ts,
                        "passes": [
                            {"pass": 1, "finished_at": old_ts, "wall_seconds": 1200.0, "timed_out": True},
                            {"pass": 2, "finished_at": now_ts, "wall_seconds": 120.0, "timed_out": False},
                        ],
                    }
                ),
                encoding="utf-8",
            )
            rollout = codex_home / "sessions" / "2026" / "11" / "24" / f"rollout-clean-{sid}.jsonl"
            records = [
                {"timestamp": now_ts, "type": "session_meta", "payload": {"id": sid, "cwd": str(repo)}},
                {"timestamp": now_ts, "type": "turn_context", "payload": {"turn_id": "t1", "model": "gpt-6.2-sol", "effort": "max", "sandbox_policy": {"type": "read-only"}}},
                # Benign user quote, source search, and unreachable JS branch quoting tools.hindsight must NOT alert
                {"timestamp": now_ts, "type": "response_item", "payload": {"type": "message", "role": "user", "content": [{"type": "input_text", "text": "Inspect <hindsight_pages> matching code"}]}},
                {"timestamp": now_ts, "type": "response_item", "payload": {"type": "custom_tool_call", "name": "exec", "input": "if (false) { tools.mcp__hindsight__hindsight_list_pages(); } // rg 'tools.hindsight.recall()'"}},
                # 4 waits on the SAME cell at 60-second intervals (long waits, NOT short polling) must NOT alert
                *[
                    {"timestamp": self.iso_offset(seconds=i * 60), "type": "response_item", "payload": {"type": "function_call", "name": "wait", "arguments": '{"cell_id":"9","yield_time_ms":60000}'}}
                    for i in range(4)
                ],
                {"timestamp": self.iso_offset(seconds=240), "type": "event_msg", "payload": {"type": "token_count", "info": {"model_context_window": 475000, "last_token_usage": {"input_tokens": 5000, "cached_input_tokens": 4000}}}},
                # Duplicate turn_context with same turn_id="t1" must NOT inflate pass count or fake eviction
                {"timestamp": self.iso_offset(seconds=245), "type": "turn_context", "payload": {"turn_id": "t1", "model": "gpt-6.2-sol", "effort": "max", "sandbox_policy": {"type": "read-only"}}},
                {"timestamp": self.iso_offset(seconds=245), "type": "event_msg", "payload": {"type": "token_count", "info": {"model_context_window": 475000, "last_token_usage": {"input_tokens": 5000, "cached_input_tokens": 4800}}}},
                # Turn 2 adds 35k new prompt tokens while reusing all 5k prior prefix -> 100% prefix reuse -> NO eviction
                # AND Turn 2 is workspace-write, so Hindsight on this writable turn must NOT fire HINDSIGHT_IN_READONLY_REVIEW
                {"timestamp": self.iso_offset(seconds=260), "type": "turn_context", "payload": {"turn_id": "t2", "model": "gpt-6.2-sol", "effort": "max", "sandbox_policy": {"type": "workspace-write"}}},
                {"timestamp": self.iso_offset(seconds=260), "type": "response_item", "payload": {"type": "message", "role": "developer", "content": [{"type": "input_text", "text": "<hindsight_pages>\nAllowed on writable turn"}]}},
                {"timestamp": self.iso_offset(seconds=260), "type": "event_msg", "payload": {"type": "item_completed", "turn_id": "t2", "item": {"type": "McpToolCall", "server": "hindsight", "tool": "hindsight_list_pages"}}},
                {"timestamp": self.iso_offset(seconds=260), "type": "event_msg", "payload": {"type": "token_count", "info": {"model_context_window": 475000, "last_token_usage": {"input_tokens": 40000, "cached_input_tokens": 5000}}}},
            ]
            write_jsonl(rollout, records)

            report = watchdog.run_watchdog(repo, window_hours=24.0, codex_home=codex_home, include_commits=False)
            self.assertEqual(report["status"], "OK", report["alerts"])
            self.assertEqual(report["alerts"], [])

            json_path, md_path = watchdog.persist_report(repo, report)
            self.assertEqual(stat.S_IMODE(os.stat(json_path.parent).st_mode), 0o700)
            self.assertEqual(stat.S_IMODE(os.stat(json_path).st_mode), 0o600)
            self.assertEqual(stat.S_IMODE(os.stat(md_path).st_mode), 0o600)

    def test_manifest_pass_cap_without_rollout_orphan_inline_isolation_and_atomic_enospc_rollback(self):
        with tempfile.TemporaryDirectory() as tmp:
            repo = Path(tmp) / "repo"
            (repo / ".codex").mkdir(parents=True)
            (repo / ".codex" / "config.toml").write_text('model = "gpt-6.2-sol"\nmodel_auto_compact_token_limit = 390000\n', encoding="utf-8")
            empty_home = Path(tmp) / "empty_home"

            # 1. Manifest-only 4-pass xhigh review triggers EXCESSIVE_XHIGH_PASSES even with zero rollouts
            sid = "01a20d5d-4d72-73b0-8b7d-67717bb89f77"
            sdir = repo / ".agent-kit" / "logs" / "sol-reviews"
            sdir.mkdir(parents=True)
            now_ts = self.iso_offset(0)
            (sdir / f"{sid}.json").write_text(
                json.dumps(
                    {
                        "session_id": sid,
                        "effort": "xhigh",
                        "last_finished_at": now_ts,
                        "passes": [{"pass": i, "finished_at": now_ts, "wall_seconds": 30.0, "timed_out": False} for i in range(1, 5)],
                    }
                ),
                encoding="utf-8",
            )
            report = watchdog.run_watchdog(repo, window_hours=24.0, codex_home=empty_home, include_commits=False)
            self.assertIn("EXCESSIVE_XHIGH_PASSES", {a["code"] for a in report["alerts"]})

            # 2. Inline orphan timeout reads outcome_path directly without invoking scan_codex_rollouts
            inv = sdir / "20261124T170000Z-p1-deadbeef"
            inv.mkdir(parents=True)
            outcome_path = inv / "outcome.json"
            outcome_path.write_text(
                json.dumps({"session_id": None, "pass": 1, "finished_at": now_ts, "wall_seconds": 600.0, "timed_out": True, "exit_code": 124}),
                encoding="utf-8",
            )
            with patch.object(watchdog, "scan_codex_rollouts", side_effect=AssertionError("must not scan rollouts")):
                inline = watchdog.check_session_inline(repo, None, outcome_path=outcome_path, codex_home=empty_home)
            self.assertIn("REVIEW_PASS_TIMEOUT", {a["code"] for a in inline["alerts"]})

            # 3. Failed publication (simulated ENOSPC on latest.md) keeps previous latest.json + latest.md paired
            good_a = {**report, "generated_at": "2026-11-24T10:00:00Z", "status": "OK", "alerts": [], "high_alerts": 0, "medium_alerts": 0}
            json_path, md_path = watchdog.persist_report(repo, good_a)
            orig_write = watchdog.write_text_atomic_at

            def fail_on_md(dir_fd, filename, payload):
                if filename == "latest.md":
                    raise OSError(errno.ENOSPC, "No space left on device")
                return orig_write(dir_fd, filename, payload)

            bad_b = {**report, "generated_at": "2026-11-24T11:00:00Z", "status": "ACTION_REQUIRED"}
            with patch.object(watchdog, "write_text_atomic_at", side_effect=fail_on_md):
                with self.assertRaises(OSError):
                    watchdog.persist_report(repo, bad_b)
            self.assertEqual(json.loads(json_path.read_text(encoding="utf-8"))["generated_at"], "2026-11-24T10:00:00Z")
            self.assertIn("2026-11-24T10:00:00Z", md_path.read_text(encoding="utf-8"))

            # 4. Unrelated gen-local-archive directory must NEVER be deleted by generation pruning
            archive_dir = watchdog.watchdog_dir_for(repo) / "gen-local-archive"
            archive_dir.mkdir()
            (archive_dir / "keep.txt").write_text("keep", encoding="utf-8")
            watchdog.persist_report(repo, good_a)
            self.assertTrue((archive_dir / "keep.txt").is_file())

            # 5. Legacy regular files survive failed alias installation without splitting JSON vs MD
            repo_legacy = Path(tmp) / "repo_legacy"
            wdir_legacy = watchdog.watchdog_dir_for(repo_legacy)
            wdir_legacy.mkdir(parents=True)
            (wdir_legacy / "latest.json").write_text(json.dumps({"generated_at": "legacy-ts"}) + "\n", encoding="utf-8")
            (wdir_legacy / "latest.md").write_text("# Legacy legacy-ts\n", encoding="utf-8")
            orig_sym = watchdog._atomic_symlink_at

            def fail_on_md_alias(dir_fd, link_name, target_rel):
                if link_name == "latest.md":
                    raise OSError(errno.ENOSPC, "No space left on device")
                return orig_sym(dir_fd, link_name, target_rel)

            with patch.object(watchdog, "_atomic_symlink_at", side_effect=fail_on_md_alias):
                with self.assertRaises(OSError):
                    watchdog.persist_report(repo_legacy, bad_b)
            self.assertEqual(json.loads((wdir_legacy / "latest.json").read_text(encoding="utf-8"))["generated_at"], "legacy-ts")
            self.assertIn("legacy-ts", (wdir_legacy / "latest.md").read_text(encoding="utf-8"))

            # First-ever run failing on current switch must clean up AND allow subsequent retry to succeed
            repo_fresh = Path(tmp) / "repo_fresh"
            repo_fresh.mkdir()

            def fail_on_current(dir_fd, link_name, target_rel):
                if link_name == "current":
                    raise OSError(errno.ENOSPC, "No space left on device")
                return orig_sym(dir_fd, link_name, target_rel)

            with patch.object(watchdog, "_atomic_symlink_at", side_effect=fail_on_current):
                with self.assertRaises(OSError):
                    watchdog.persist_report(repo_fresh, bad_b)
            wdir_fresh = watchdog.watchdog_dir_for(repo_fresh)
            self.assertFalse((wdir_fresh / "latest.json").exists() or (wdir_fresh / "latest.json").is_symlink())
            self.assertFalse((wdir_fresh / "latest.md").exists() or (wdir_fresh / "latest.md").is_symlink())
            # Retry succeeds cleanly (even if pre-existing dangling managed aliases were left behind)
            os.symlink("current/latest.json", wdir_fresh / "latest.json")
            os.symlink("current/latest.md", wdir_fresh / "latest.md")
            jp, mp = watchdog.persist_report(repo_fresh, good_a)
            self.assertEqual(json.loads(jp.read_text(encoding="utf-8"))["generated_at"], "2026-11-24T10:00:00Z")
            self.assertIn("2026-11-24T10:00:00Z", mp.read_text(encoding="utf-8"))

            # Lone legacy latest.json must be preserved if latest.md alias fails
            repo_lone = Path(tmp) / "repo_lone"
            wdir_lone = watchdog.watchdog_dir_for(repo_lone)
            wdir_lone.mkdir(parents=True)
            (wdir_lone / "latest.json").write_text(json.dumps({"generated_at": "lone-ts"}) + "\n", encoding="utf-8")
            with patch.object(watchdog, "_atomic_symlink_at", side_effect=fail_on_md_alias):
                with self.assertRaises(OSError):
                    watchdog.persist_report(repo_lone, bad_b)
            self.assertEqual(json.loads((wdir_lone / "latest.json").read_text(encoding="utf-8"))["generated_at"], "lone-ts")

            # Trailing-newline generation name must NOT be deleted by pruning
            newline_gen = watchdog.watchdog_dir_for(repo) / "gen-20261124T170000Z-0123456789\n"
            newline_gen.mkdir()
            watchdog.persist_report(repo, good_a)
            self.assertTrue(newline_gen.is_dir())

            # Non-UUID session_id is rejected; mismatched embedded session_id is ignored
            with self.assertRaisesRegex(ValueError, "Invalid session_id"):
                watchdog.run_watchdog(repo, only_session_id="../../escape", codex_home=empty_home)
            mismatch_sid = "01a20d5d-4d72-73b0-8b7d-67717bb89f99"
            (sdir / f"{mismatch_sid}.json").write_text(
                json.dumps({"session_id": sid, "effort": "max", "passes": [{"pass": 1, "finished_at": now_ts, "timed_out": True}]}),
                encoding="utf-8",
            )
            self.assertEqual(watchdog.load_wrapper_manifests(repo, only_session_id=mismatch_sid), {})

            # Symlinked outcome_path and '..' traversal inside repo must both be refused
            symlink_outcome = inv / "linked-outcome.json"
            os.symlink(outcome_path, symlink_outcome)
            self.assertIsNone(watchdog._read_single_outcome_nonblocking(repo, symlink_outcome))
            with self.assertRaisesRegex(ValueError, "parent-traversal"):
                watchdog._relative_outcome_path(repo, repo / ".agent-kit" / ".." / "outside.json")
            self.assertIsNone(watchdog._read_single_outcome_nonblocking(repo, repo / ".agent-kit" / ".." / "outside.json"))

            # Interrupt right after `current` flip must NOT delete the newly active generation
            def interrupt_after_current(dir_fd, link_name, target_rel):
                orig_sym(dir_fd, link_name, target_rel)
                if link_name == "current":
                    raise KeyboardInterrupt("post-flip interrupt")

            repo_int = Path(tmp) / "repo_int"
            repo_int.mkdir()
            with patch.object(watchdog, "_atomic_symlink_at", side_effect=interrupt_after_current):
                with self.assertRaises(KeyboardInterrupt):
                    watchdog.persist_report(repo_int, good_a)
            wdir_int = watchdog.watchdog_dir_for(repo_int)
            self.assertEqual(json.loads((wdir_int / "latest.json").read_text(encoding="utf-8"))["generated_at"], "2026-11-24T10:00:00Z")
            self.assertIn("2026-11-24T10:00:00Z", (wdir_int / "latest.md").read_text(encoding="utf-8"))

            # 6. Rapid 1s-apart waits trigger WAIT_POLLING_STORM even with yield_time_ms=15000 (OR semantics)
            rapid_rollout = empty_home / "sessions" / "2026" / "11" / "24" / f"rollout-rapid-{sid}.jsonl"
            write_jsonl(
                rapid_rollout,
                [
                    {"timestamp": now_ts, "type": "session_meta", "payload": {"id": sid, "cwd": str(repo)}},
                    {"timestamp": now_ts, "type": "turn_context", "payload": {"turn_id": "t1", "model": "gpt-6.2-sol", "effort": "max", "sandbox_policy": {"type": "read-only"}}},
                    *[
                        {"timestamp": self.iso_offset(seconds=i), "type": "response_item", "payload": {"type": "function_call", "name": "wait", "arguments": '{"cell_id":"7","yield_time_ms":15000}'}}
                        for i in range(4)
                    ],
                ],
            )
            rapid_rep = watchdog.run_watchdog(repo, window_hours=24.0, codex_home=empty_home, include_commits=False, only_session_id=sid)
            self.assertIn("WAIT_POLLING_STORM", {a["code"] for a in rapid_rep["alerts"]})

            # 7. Unrelated FIFO in sol-reviews must NOT block inline manifest lookup or full scan
            os.mkfifo(sdir / "unrelated-fifo.json", 0o600)
            loaded = watchdog.load_wrapper_manifests(repo, only_session_id=sid)
            self.assertIn(sid, loaded)
            loaded_all = watchdog.load_wrapper_manifests(repo)
            self.assertIn(sid, loaded_all)

            # 8. Rust tests.rs and test directories are excluded from production LOC sizing
            self.assertTrue(watchdog.is_test_path("crates/org-office-sessions/src/tests.rs"))
            self.assertTrue(watchdog.is_test_path("crates/org-office-sessions/src/wake_tests.rs"))
            self.assertTrue(watchdog.is_test_path("crates/org-agent-runtime-codex/tests/conformance.rs"))
            self.assertFalse(watchdog.is_test_path("crates/org-office-sessions/src/preset.rs"))

    def test_strict_mode_exits_2_on_high_alert(self):
        with tempfile.TemporaryDirectory() as tmp:
            repo = Path(tmp) / "repo"
            (repo / ".codex").mkdir(parents=True)
            (repo / ".codex" / "config.toml").write_text(
                'model = "gpt-6.2-sol"\nmodel_auto_compact_token_limit = 200000\n',
                encoding="utf-8",
            )
            codex_home = Path(tmp) / "empty_home"
            rc = watchdog.main(
                [
                    "--repo",
                    str(repo),
                    "--codex-home",
                    str(codex_home),
                    "--no-write",
                    "--strict",
                    "--json",
                ]
            )
            self.assertEqual(rc, 2)

    def test_detects_shallow_approval_ungrounded_output_and_manifest_quality_outcomes(self):
        with tempfile.TemporaryDirectory() as tmp:
            repo = Path(tmp) / "repo"
            (repo / ".codex").mkdir(parents=True)
            (repo / ".codex" / "config.toml").write_text(
                'model = "gpt-6.2-sol"\nmodel_auto_compact_token_limit = 390000\n',
                encoding="utf-8",
            )
            (repo / "src").mkdir()
            (repo / "src" / "real.rs").write_text("pub fn ok() {}\n", encoding="utf-8")
            codex_home = Path(tmp) / "codex_home"
            codex_home.mkdir()
            (codex_home / "config.toml").write_text('model = "gpt-6-luna"\n', encoding="utf-8")
            sdir = repo / ".agent-kit" / "logs" / "sol-reviews"
            sdir.mkdir(parents=True)
            now_ts = self.iso_offset(0)

            # 1. Shallow Pass-1 APPROVE with 0 inspection actions -> HIGH SHALLOW_REVIEW_APPROVAL
            sid_shallow = "01a20e10-0000-7000-8000-000000000001"
            (sdir / f"{sid_shallow}.json").write_text(
                json.dumps(
                    {
                        "session_id": sid_shallow,
                        "effort": "max",
                        "diff_non_test_loc": 400,
                        "last_finished_at": now_ts,
                        "passes": [{"pass": 1, "finished_at": now_ts, "wall_seconds": 12.0, "timed_out": False, "exit_code": 0, "completed_turns": 1, "failed": False}],
                    }
                ),
                encoding="utf-8",
            )
            write_jsonl(
                codex_home / "sessions" / "2026" / "11" / "24" / f"rollout-shallow-{sid_shallow}.jsonl",
                [
                    {"timestamp": now_ts, "type": "session_meta", "payload": {"id": sid_shallow, "cwd": str(repo)}},
                    {"timestamp": now_ts, "type": "turn_context", "payload": {"turn_id": "t1", "model": "gpt-6.2-sol", "effort": "max", "sandbox_policy": {"type": "read-only"}}},
                    {"timestamp": now_ts, "type": "event_msg", "payload": {"type": "token_count", "info": {"model_context_window": 475000, "last_token_usage": {"input_tokens": 12000, "cached_input_tokens": 0, "reasoning_output_tokens": 120}}}},
                    {"timestamp": now_ts, "type": "event_msg", "payload": {"type": "task_complete", "turn_id": "t1", "last_agent_message": "Looks good!\n\nVERDICT: APPROVE"}},
                ],
            )

            # 2. Ungrounded REQUEST_CHANGES (no verified repo file:line) + missing verdict on Pass 2
            sid_ungrounded = "01a20e10-0000-7000-8000-000000000002"
            (sdir / f"{sid_ungrounded}.json").write_text(
                json.dumps(
                    {
                        "session_id": sid_ungrounded,
                        "effort": "xhigh",
                        "last_finished_at": now_ts,
                        "passes": [
                            {"pass": 1, "started_at": now_ts, "finished_at": now_ts, "wall_seconds": 40.0, "timed_out": False, "exit_code": 0, "completed_turns": 1, "failed": False},
                            {"pass": 2, "started_at": self.iso_offset(seconds=20), "finished_at": self.iso_offset(seconds=20), "wall_seconds": 25.0, "timed_out": False, "exit_code": 0, "completed_turns": 1, "failed": False},
                        ],
                        "outcome": {
                            "recorded_at": now_ts,
                            "confirmed_findings": 2,
                            "false_positives": 2,
                            "repair_regressions": 1,
                            "prior_session_id": "01a20e10-0000-7000-8000-000000000099",
                            "escaped_from_prior": 1,
                        },
                    }
                ),
                encoding="utf-8",
            )
            write_jsonl(
                codex_home / "sessions" / "2026" / "11" / "24" / f"rollout-ungrounded-{sid_ungrounded}.jsonl",
                [
                    {"timestamp": now_ts, "type": "session_meta", "payload": {"id": sid_ungrounded, "cwd": str(repo)}},
                    {"timestamp": now_ts, "type": "turn_context", "payload": {"turn_id": "t1", "model": "gpt-6.2-sol", "effort": "xhigh", "sandbox_policy": {"type": "read-only"}}},
                    {"timestamp": now_ts, "type": "response_item", "payload": {"type": "custom_tool_call", "name": "exec", "input": "git diff"}},
                    {"timestamp": now_ts, "type": "event_msg", "payload": {"type": "task_complete", "turn_id": "t1", "last_agent_message": "Fix bogus src/nonexistent.rs:42.\n\nVERDICT: REQUEST_CHANGES"}},
                    {"timestamp": self.iso_offset(seconds=20), "type": "turn_context", "payload": {"turn_id": "t2", "model": "gpt-6.2-sol", "effort": "xhigh", "sandbox_policy": {"type": "read-only"}}},
                    {"timestamp": self.iso_offset(seconds=20), "type": "event_msg", "payload": {"type": "task_complete", "turn_id": "t2", "last_agent_message": "Done checking everything."}},
                ],
            )

            # 3. Clean grounded batched JS exec review (1 exec running 3 CommandExecutions + 1200 reas) must NOT alert
            sid_good = "01a20e10-0000-7000-8000-000000000003"
            (sdir / f"{sid_good}.json").write_text(
                json.dumps(
                    {
                        "session_id": sid_good,
                        "effort": "max",
                        "diff_non_test_loc": 500,
                        "last_finished_at": now_ts,
                        "passes": [
                            {"pass": 1, "started_at": now_ts, "finished_at": now_ts, "wall_seconds": 60.0, "timed_out": False, "exit_code": 0, "completed_turns": 1, "failed": False},
                            {"pass": 2, "started_at": self.iso_offset(seconds=30), "finished_at": self.iso_offset(seconds=30), "wall_seconds": 40.0, "timed_out": False, "exit_code": 0, "completed_turns": 1, "failed": False},
                        ],
                        "outcome": {"recorded_at": now_ts, "confirmed_findings": 4, "false_positives": 1, "repair_regressions": 0},
                    }
                ),
                encoding="utf-8",
            )
            write_jsonl(
                codex_home / "sessions" / "2026" / "11" / "24" / f"rollout-good-{sid_good}.jsonl",
                [
                    {"timestamp": now_ts, "type": "session_meta", "payload": {"id": sid_good, "cwd": str(repo)}},
                    {"timestamp": now_ts, "type": "turn_context", "payload": {"turn_id": "t1", "model": "gpt-6.2-sol", "effort": "max", "sandbox_policy": {"type": "read-only"}}},
                    {"timestamp": now_ts, "type": "response_item", "payload": {"type": "custom_tool_call", "name": "exec", "input": "batch"}},
                    {"timestamp": now_ts, "type": "event_msg", "payload": {"type": "item_completed", "turn_id": "t1", "item": {"type": "CommandExecution"}}},
                    {"timestamp": now_ts, "type": "event_msg", "payload": {"type": "item_completed", "turn_id": "t1", "item": {"type": "CommandExecution"}}},
                    {"timestamp": now_ts, "type": "event_msg", "payload": {"type": "item_completed", "turn_id": "t1", "item": {"type": "CommandExecution"}}},
                    {"timestamp": now_ts, "type": "event_msg", "payload": {"type": "token_count", "info": {"model_context_window": 475000, "last_token_usage": {"input_tokens": 50000, "cached_input_tokens": 45000, "reasoning_output_tokens": 1200}}}},
                    {"timestamp": now_ts, "type": "event_msg", "payload": {"type": "task_complete", "turn_id": "t1", "last_agent_message": "Bug at src/real.rs:1.\n\nVERDICT: REQUEST_CHANGES"}},
                    {"timestamp": self.iso_offset(seconds=30), "type": "turn_context", "payload": {"turn_id": "t2", "model": "gpt-6.2-sol", "effort": "max", "sandbox_policy": {"type": "read-only"}}},
                    {"timestamp": self.iso_offset(seconds=30), "type": "response_item", "payload": {"type": "custom_tool_call", "name": "exec", "input": "git diff"}},
                    {"timestamp": self.iso_offset(seconds=30), "type": "event_msg", "payload": {"type": "token_count", "info": {"model_context_window": 475000, "last_token_usage": {"input_tokens": 55000, "cached_input_tokens": 50000, "reasoning_output_tokens": 900}}}},
                    {"timestamp": self.iso_offset(seconds=30), "type": "event_msg", "payload": {"type": "task_complete", "turn_id": "t2", "last_agent_message": "Verified src/real.rs:1.\n\nVERDICT: APPROVE"}},
                ],
            )

            report = watchdog.run_watchdog(repo, window_hours=24.0, codex_home=codex_home, include_commits=False)
            codes = {a["code"] for a in report["alerts"]}
            self.assertEqual(report["status"], "ACTION_REQUIRED")
            self.assertIn("SHALLOW_REVIEW_APPROVAL", codes)
            self.assertIn("UNGROUNDED_REVIEW_OUTPUT", codes)
            self.assertIn("HIGH_FALSE_POSITIVE_RATE", codes)
            self.assertIn("FINAL_GATE_ESCAPE", codes)
            self.assertIn("REPAIR_REGRESSION_CHURN", codes)
            # Clean session must produce zero alerts when checked alone
            clean_rep = watchdog.run_watchdog(repo, window_hours=24.0, codex_home=codex_home, include_commits=False, only_session_id=sid_good)
            self.assertEqual(clean_rep["status"], "OK", clean_rep["alerts"])
            self.assertEqual(clean_rep["quality_summary"]["precision_pct"], 80.0)
            rendered_md = watchdog.render_markdown(clean_rep)
            self.assertIn("precision `80.0%`", rendered_md)
            self.assertIn("`0` regressions", rendered_md)

            # 4. Pass-1 batched APPROVE (1 JS exec -> 3 CommandExecutions + 1200 reas on 500 LOC) -> OK
            sid_batch_approve = "01a20e10-0000-7000-8000-000000000010"
            (sdir / f"{sid_batch_approve}.json").write_text(
                json.dumps(
                    {
                        "session_id": sid_batch_approve,
                        "effort": "max",
                        "diff_non_test_loc": 500,
                        "last_finished_at": now_ts,
                        "passes": [{"pass": 1, "finished_at": now_ts, "wall_seconds": 30.0, "timed_out": False, "exit_code": 0, "completed_turns": 1, "failed": False}],
                    }
                ),
                encoding="utf-8",
            )
            write_jsonl(
                codex_home / "sessions" / "2026" / "11" / "24" / f"rollout-batch-{sid_batch_approve}.jsonl",
                [
                    {"timestamp": now_ts, "type": "session_meta", "payload": {"id": sid_batch_approve, "cwd": str(repo)}},
                    {"timestamp": now_ts, "type": "turn_context", "payload": {"turn_id": "t1", "model": "gpt-6.2-sol", "effort": "max", "sandbox_policy": {"type": "read-only"}}},
                    {"timestamp": now_ts, "type": "response_item", "payload": {"type": "custom_tool_call", "name": "exec", "input": "batch"}},
                    {"timestamp": now_ts, "type": "event_msg", "payload": {"type": "item_completed", "turn_id": "t1", "item": {"type": "CommandExecution"}}},
                    {"timestamp": now_ts, "type": "event_msg", "payload": {"type": "item_completed", "turn_id": "t1", "item": {"type": "CommandExecution"}}},
                    {"timestamp": now_ts, "type": "event_msg", "payload": {"type": "item_completed", "turn_id": "t1", "item": {"type": "CommandExecution"}}},
                    {"timestamp": now_ts, "type": "event_msg", "payload": {"type": "token_count", "info": {"model_context_window": 475000, "last_token_usage": {"input_tokens": 50000, "cached_input_tokens": 0, "reasoning_output_tokens": 1200}}}},
                    {"timestamp": now_ts, "type": "event_msg", "payload": {"type": "task_complete", "turn_id": "t1", "last_agent_message": "Verified src/real.rs:1.\n\nVERDICT: APPROVE"}},
                ],
            )
            self.assertEqual(
                watchdog.run_watchdog(repo, window_hours=24.0, codex_home=codex_home, include_commits=False, only_session_id=sid_batch_approve)["status"],
                "OK",
            )

            # 5. One JS exec containing ONE MCP lookup scores 1 (NOT 2), and two arithmetic-only execs score 0 -> HIGH SHALLOW_REVIEW_APPROVAL
            for sid_fake, events in (
                (
                    "01a20e10-0000-7000-8000-000000000011",
                    [
                        {"timestamp": now_ts, "type": "response_item", "payload": {"type": "custom_tool_call", "name": "exec", "input": "tools.lookup()"}},
                        {"timestamp": now_ts, "type": "event_msg", "payload": {"type": "item_completed", "turn_id": "t1", "item": {"type": "McpToolCall", "server": "rust_analyzer", "id": "m1"}}},
                    ],
                ),
                (
                    "01a20e10-0000-7000-8000-000000000012",
                    [
                        {"timestamp": now_ts, "type": "response_item", "payload": {"type": "custom_tool_call", "name": "exec", "input": "1 + 1"}},
                        {"timestamp": now_ts, "type": "response_item", "payload": {"type": "custom_tool_call", "name": "exec", "input": "2 + 2"}},
                        {"timestamp": now_ts, "type": "event_msg", "payload": {"type": "item_completed", "turn_id": "t1", "item": {"type": "Reasoning"}}},
                    ],
                ),
            ):
                (sdir / f"{sid_fake}.json").write_text(
                    json.dumps(
                        {
                            "session_id": sid_fake,
                            "effort": "max",
                            "diff_non_test_loc": 500,
                            "last_finished_at": now_ts,
                            "passes": [{"pass": 1, "finished_at": now_ts, "wall_seconds": 15.0, "timed_out": False, "exit_code": 0, "completed_turns": 1, "failed": False}],
                        }
                    ),
                    encoding="utf-8",
                )
                write_jsonl(
                    codex_home / "sessions" / "2026" / "11" / "24" / f"rollout-fake-{sid_fake}.jsonl",
                    [
                        {"timestamp": now_ts, "type": "session_meta", "payload": {"id": sid_fake, "cwd": str(repo)}},
                        {"timestamp": now_ts, "type": "turn_context", "payload": {"turn_id": "t1", "model": "gpt-6.2-sol", "effort": "max", "sandbox_policy": {"type": "read-only"}}},
                        *events,
                        {"timestamp": now_ts, "type": "event_msg", "payload": {"type": "token_count", "info": {"model_context_window": 475000, "last_token_usage": {"input_tokens": 50000, "cached_input_tokens": 0, "reasoning_output_tokens": 1200}}}},
                        {"timestamp": now_ts, "type": "event_msg", "payload": {"type": "task_complete", "turn_id": "t1", "last_agent_message": "Looks ok.\n\nVERDICT: APPROVE"}},
                    ],
                )
                fake_rep = watchdog.run_watchdog(repo, window_hours=24.0, codex_home=codex_home, include_commits=False, only_session_id=sid_fake)
                self.assertIn("SHALLOW_REVIEW_APPROVAL", {a["code"] for a in fake_rep["alerts"]})

            # 6. Pre-turn failed pass 2 must NOT hide shallow APPROVE on pass 3; missing rollout with inspection_actions=0 on small diff MUST alert
            sid_reconcile = "01a20e10-0000-7000-8000-000000000013"
            (sdir / f"{sid_reconcile}.json").write_text(
                json.dumps(
                    {
                        "session_id": sid_reconcile,
                        "effort": "max",
                        "diff_non_test_loc": 20,
                        "last_finished_at": now_ts,
                        "passes": [
                            {"pass": 1, "finished_at": now_ts, "wall_seconds": 30.0, "timed_out": False, "exit_code": 0, "completed_turns": 1, "failed": False, "verdict": "REQUEST_CHANGES", "cited_count": 1, "verified_citations_count": 1, "inspection_actions": 3},
                            {"pass": 2, "finished_at": now_ts, "wall_seconds": 5.0, "timed_out": True, "exit_code": 124, "completed_turns": 0, "failed": True, "verdict": None, "cited_count": 0, "verified_citations_count": 0, "inspection_actions": 0},
                            {"pass": 3, "finished_at": now_ts, "wall_seconds": 10.0, "timed_out": False, "exit_code": 0, "completed_turns": 1, "failed": False, "verdict": "APPROVE", "cited_count": 0, "verified_citations_count": 0, "inspection_actions": 0},
                        ],
                    }
                ),
                encoding="utf-8",
            )
            rec_rep = watchdog.run_watchdog(repo, window_hours=24.0, codex_home=Path(tmp) / "empty_home", include_commits=False, only_session_id=sid_reconcile)
            self.assertIn("SHALLOW_REVIEW_APPROVAL", {a["code"] for a in rec_rep["alerts"]})

            # 7. Live commentary (no task_complete), unwrapped read-only Q&A, and repair-only linked outcome (escaped=0) must NOT false-alert
            sid_qa = "01a20e10-0000-7000-8000-000000000014"
            write_jsonl(
                codex_home / "sessions" / "2026" / "11" / "24" / f"rollout-qa-{sid_qa}.jsonl",
                [
                    {"timestamp": now_ts, "type": "session_meta", "payload": {"id": sid_qa, "cwd": str(repo)}},
                    {"timestamp": now_ts, "type": "turn_context", "payload": {"turn_id": "t1", "model": "gpt-6.2-sol", "effort": "high", "sandbox_policy": {"type": "read-only"}}},
                    {"timestamp": now_ts, "type": "response_item", "payload": {"type": "message", "role": "user", "content": [{"type": "input_text", "text": "Explain how src/real.rs works in detail."}]}},
                    {"timestamp": now_ts, "type": "response_item", "payload": {"type": "message", "role": "assistant", "content": [{"type": "output_text", "text": "Reading src/real.rs now to inspect the implementation..."}]}},
                    {"timestamp": now_ts, "type": "event_msg", "payload": {"type": "task_complete", "turn_id": "t1", "last_agent_message": "src/real.rs defines pub fn ok() which returns unit and performs no I/O or state mutations."}},
                ],
            )
            qa_rep = watchdog.run_watchdog(repo, window_hours=24.0, codex_home=codex_home, include_commits=False, only_session_id=sid_qa)
            self.assertNotIn("UNGROUNDED_REVIEW_OUTPUT", {a["code"] for a in qa_rep["alerts"]})

            sid_live = "01a20e10-0000-7000-8000-000000000015"
            (sdir / f"{sid_live}.json").write_text(
                json.dumps(
                    {
                        "session_id": sid_live,
                        "effort": "max",
                        "last_finished_at": now_ts,
                        "passes": [],
                        "outcome": {
                            "recorded_at": now_ts,
                            "confirmed_findings": 1,
                            "false_positives": 0,
                            "repair_regressions": 1,
                            "prior_session_id": sid_good,
                            "escaped_from_prior": 0,
                        },
                    }
                ),
                encoding="utf-8",
            )
            write_jsonl(
                codex_home / "sessions" / "2026" / "11" / "24" / f"rollout-live-{sid_live}.jsonl",
                [
                    {"timestamp": now_ts, "type": "session_meta", "payload": {"id": sid_live, "cwd": str(repo)}},
                    {"timestamp": now_ts, "type": "turn_context", "payload": {"turn_id": "t1", "model": "gpt-6.2-sol", "effort": "max", "sandbox_policy": {"type": "read-only"}}},
                    {"timestamp": now_ts, "type": "response_item", "payload": {"type": "message", "role": "assistant", "content": [{"type": "output_text", "text": "Still reading files..."}]}},
                ],
            )
            live_rep = watchdog.run_watchdog(repo, window_hours=24.0, codex_home=codex_home, include_commits=False, only_session_id=sid_live)
            live_codes = {a["code"] for a in live_rep["alerts"]}
            self.assertNotIn("UNGROUNDED_REVIEW_OUTPUT", live_codes)
            self.assertNotIn("FINAL_GATE_ESCAPE", live_codes)
            self.assertIn("REPAIR_REGRESSION_CHURN", live_codes)

            # 8. Bare resume after an old (>48h) wrapped pass MUST still be checked; resuming an unwrapped thread must NOT double-count;
            #    pre-turn_context prompt attaches to task_started.turn_id, and web.search Extension counts as 1 inspection on a small diff.
            sid_bare_old = "01a20e10-0000-7000-8000-000000000016"
            old_ts = self.iso_offset(-48)
            (sdir / f"{sid_bare_old}.json").write_text(
                json.dumps(
                    {
                        "session_id": sid_bare_old,
                        "effort": "max",
                        "last_finished_at": old_ts,
                        "passes": [{"pass": 1, "started_at": old_ts, "finished_at": old_ts, "wall_seconds": 30.0, "timed_out": False, "exit_code": 0, "completed_turns": 1, "failed": False, "verdict": "REQUEST_CHANGES", "cited_count": 1, "verified_citations_count": 1, "inspection_actions": 3}],
                    }
                ),
                encoding="utf-8",
            )
            write_jsonl(
                codex_home / "sessions" / "2026" / "11" / "24" / f"rollout-bareold-{sid_bare_old}.jsonl",
                [
                    {"timestamp": old_ts, "type": "session_meta", "payload": {"id": sid_bare_old, "cwd": str(repo)}},
                    {"timestamp": old_ts, "type": "turn_context", "payload": {"turn_id": "old1", "model": "gpt-6.2-sol", "effort": "max", "sandbox_policy": {"type": "read-only"}}},
                    {"timestamp": old_ts, "type": "event_msg", "payload": {"type": "task_complete", "turn_id": "old1", "last_agent_message": "Bug at src/real.rs:1.\n\nVERDICT: REQUEST_CHANGES"}},
                    {"timestamp": now_ts, "type": "turn_context", "payload": {"turn_id": "bare2", "model": "gpt-6.2-sol", "effort": "max", "sandbox_policy": {"type": "read-only"}}},
                    {"timestamp": now_ts, "type": "event_msg", "payload": {"type": "task_complete", "turn_id": "bare2", "last_agent_message": "Looks fine.\n\nVERDICT: APPROVE"}},
                ],
            )
            bare_old_rep = watchdog.run_watchdog(repo, window_hours=24.0, codex_home=codex_home, include_commits=False, only_session_id=sid_bare_old)
            self.assertIn("SHALLOW_REVIEW_APPROVAL", {a["code"] for a in bare_old_rep["alerts"]})

            # Pre-turn_context review prompt attaches to task_started.turn_id (not preceding Q&A), and web.search counts as inspection
            sid_order = "01a20e10-0000-7000-8000-000000000017"
            t_t1 = self.iso_offset(seconds=-300)
            t_t2 = self.iso_offset(seconds=-10)
            write_jsonl(
                codex_home / "sessions" / "2026" / "11" / "24" / f"rollout-order-{sid_order}.jsonl",
                [
                    {"timestamp": t_t1, "type": "session_meta", "payload": {"id": sid_order, "cwd": str(repo)}},
                    {"timestamp": t_t1, "type": "event_msg", "payload": {"type": "task_started", "turn_id": "ord1"}},
                    {"timestamp": t_t1, "type": "response_item", "payload": {"type": "message", "role": "user", "content": [{"type": "input_text", "text": "Explain src/real.rs?"}]}},
                    {"timestamp": t_t1, "type": "turn_context", "payload": {"turn_id": "ord1", "model": "gpt-6.2-sol", "effort": "high", "sandbox_policy": {"type": "read-only"}}},
                    {"timestamp": t_t1, "type": "event_msg", "payload": {"type": "task_complete", "turn_id": "ord1", "last_agent_message": "It defines pub fn ok() which is a no-op helper function."}},
                    {"timestamp": t_t2, "type": "event_msg", "payload": {"type": "task_started", "turn_id": "ord2"}},
                    {"timestamp": t_t2, "type": "response_item", "payload": {"type": "message", "role": "user", "content": [{"type": "input_text", "text": "Review src/real.rs and end with VERDICT: APPROVE or VERDICT: REQUEST_CHANGES"}]}},
                    {"timestamp": t_t2, "type": "turn_context", "payload": {"turn_id": "ord2", "model": "gpt-6.2-sol", "effort": "high", "sandbox_policy": {"type": "read-only"}}},
                    {"timestamp": t_t2, "type": "event_msg", "payload": {"type": "task_complete", "turn_id": "ord2", "last_agent_message": "Looks good overall, no verdict line emitted."}},
                ],
            )
            order_rep = watchdog.run_watchdog(repo, window_hours=24.0, codex_home=codex_home, include_commits=False, only_session_id=sid_order)
            ungrounded = [a for a in order_rep["alerts"] if a["code"] == "UNGROUNDED_REVIEW_OUTPUT"]
            self.assertEqual(len(ungrounded), 1)
            self.assertEqual(ungrounded[0]["detail"]["pass"], 2)

            # Wrapping a previously unwrapped thread matches by timestamp and does NOT double-count approvals
            sid_mixed = "01a20e10-0000-7000-8000-000000000018"
            (sdir / f"{sid_mixed}.json").write_text(
                json.dumps(
                    {
                        "session_id": sid_mixed,
                        "effort": "max",
                        "diff_non_test_loc": 20,
                        "last_finished_at": t_t2,
                        "passes": [{"pass": 1, "started_at": t_t2, "finished_at": t_t2, "wall_seconds": 15.0, "timed_out": False, "exit_code": 0, "completed_turns": 1, "failed": False, "verdict": "APPROVE", "cited_count": 0, "verified_citations_count": 0, "inspection_actions": 1}],
                    }
                ),
                encoding="utf-8",
            )
            write_jsonl(
                codex_home / "sessions" / "2026" / "11" / "24" / f"rollout-mixed-{sid_mixed}.jsonl",
                [
                    {"timestamp": t_t1, "type": "session_meta", "payload": {"id": sid_mixed, "cwd": str(repo)}},
                    {"timestamp": t_t1, "type": "turn_context", "payload": {"turn_id": "m1", "model": "gpt-6.2-sol", "effort": "max", "sandbox_policy": {"type": "read-only"}}},
                    {"timestamp": t_t1, "type": "event_msg", "payload": {"type": "item_completed", "turn_id": "m1", "item": {"type": "Extension", "kind": "web.search"}}},
                    {"timestamp": t_t1, "type": "event_msg", "payload": {"type": "task_complete", "turn_id": "m1", "last_agent_message": "Checked docs, VERDICT: REQUEST_CHANGES at src/real.rs:1.\n\nVERDICT: REQUEST_CHANGES"}},
                    {"timestamp": t_t2, "type": "turn_context", "payload": {"turn_id": "m2", "model": "gpt-6.2-sol", "effort": "max", "sandbox_policy": {"type": "read-only"}}},
                    {"timestamp": t_t2, "type": "event_msg", "payload": {"type": "item_completed", "turn_id": "m2", "item": {"type": "Extension", "kind": "web.search"}}},
                    {"timestamp": t_t2, "type": "event_msg", "payload": {"type": "task_complete", "turn_id": "m2", "last_agent_message": "Verified via web search.\n\nVERDICT: APPROVE"}},
                ],
            )
            mixed_rep = watchdog.run_watchdog(repo, window_hours=24.0, codex_home=codex_home, include_commits=False, only_session_id=sid_mixed)
            self.assertEqual(mixed_rep["status"], "OK", mixed_rep["alerts"])
            self.assertEqual(mixed_rep["quality_summary"]["approved_reviews"], 1)
            self.assertEqual(mixed_rep["quality_summary"]["request_changes_passes"], 1)

            # Bare zero-inspection approval ending 1s BEFORE a wrapped pass MUST NOT be stolen by wrapped pass;
            # and a timed-out wrapped pass whose rollout completed MUST NOT be miscounted as a bare approval.
            sid_adj = "01a20e10-0000-7000-8000-000000000019"
            t_bare = self.iso_offset(seconds=-101)
            t_w_start = self.iso_offset(seconds=-100)
            t_w_end = self.iso_offset(seconds=-85)
            t_to_start = self.iso_offset(seconds=-60)
            t_to_end = self.iso_offset(seconds=-40)
            (sdir / f"{sid_adj}.json").write_text(
                json.dumps(
                    {
                        "session_id": sid_adj,
                        "effort": "max",
                        "diff_non_test_loc": 20,
                        "last_finished_at": t_to_end,
                        "passes": [
                            {"pass": 1, "started_at": t_w_start, "finished_at": t_w_end, "wall_seconds": 15.0, "timed_out": False, "exit_code": 0, "completed_turns": 1, "failed": False, "verdict": "APPROVE", "cited_count": 0, "verified_citations_count": 0, "inspection_actions": 2},
                            {"pass": 2, "started_at": t_to_start, "finished_at": t_to_end, "wall_seconds": 20.0, "timed_out": True, "exit_code": 124, "completed_turns": 0, "failed": True, "verdict": None, "cited_count": 0, "verified_citations_count": 0, "inspection_actions": 0},
                        ],
                    }
                ),
                encoding="utf-8",
            )
            write_jsonl(
                codex_home / "sessions" / "2026" / "11" / "24" / f"rollout-adj-{sid_adj}.jsonl",
                [
                    {"timestamp": t_bare, "type": "session_meta", "payload": {"id": sid_adj, "cwd": str(repo)}},
                    {"timestamp": t_bare, "type": "turn_context", "payload": {"turn_id": "adj_bare", "model": "gpt-6.2-sol", "effort": "max", "sandbox_policy": {"type": "read-only"}}},
                    {"timestamp": t_bare, "type": "event_msg", "payload": {"type": "task_complete", "turn_id": "adj_bare", "last_agent_message": "Looks ok.\n\nVERDICT: APPROVE"}},
                    {"timestamp": t_w_start, "type": "turn_context", "payload": {"turn_id": "adj_wrap", "model": "gpt-6.2-sol", "effort": "max", "sandbox_policy": {"type": "read-only"}}},
                    {"timestamp": t_w_end, "type": "event_msg", "payload": {"type": "task_complete", "turn_id": "adj_wrap", "last_agent_message": "Checked.\n\nVERDICT: APPROVE"}},
                    {"timestamp": t_to_start, "type": "turn_context", "payload": {"turn_id": "adj_to", "model": "gpt-6.2-sol", "effort": "max", "sandbox_policy": {"type": "read-only"}}},
                    {"timestamp": t_to_end, "type": "event_msg", "payload": {"type": "task_complete", "turn_id": "adj_to", "last_agent_message": "Late finish.\n\nVERDICT: APPROVE"}},
                ],
            )
            adj_rep = watchdog.run_watchdog(repo, window_hours=24.0, codex_home=codex_home, include_commits=False, only_session_id=sid_adj)
            self.assertIn("SHALLOW_REVIEW_APPROVAL", {a["code"] for a in adj_rep["alerts"]})
            # Exactly 2 approvals counted (1 bare + 1 wrapped), NOT 3 (timed-out rollout turn claimed and skipped)
            self.assertEqual(adj_rep["quality_summary"]["approved_reviews"], 2)

            # Subsecond failed pass ending at .200s MUST NOT claim a separate bare approval at .400s-.900s
            sid_subsec = "01a20e10-0000-7000-8000-000000000021"
            base_dt = datetime.now(timezone.utc).replace(microsecond=0) - timedelta(seconds=30)
            f_start = (base_dt + timedelta(milliseconds=0)).strftime("%Y-%m-%dT%H:%M:%S.%fZ")
            f_end = (base_dt + timedelta(milliseconds=200)).strftime("%Y-%m-%dT%H:%M:%S.%fZ")
            b_start = (base_dt + timedelta(milliseconds=400)).strftime("%Y-%m-%dT%H:%M:%S.%fZ")
            b_end = (base_dt + timedelta(milliseconds=900)).strftime("%Y-%m-%dT%H:%M:%S.%fZ")
            (sdir / f"{sid_subsec}.json").write_text(
                json.dumps(
                    {
                        "session_id": sid_subsec,
                        "effort": "max",
                        "last_finished_at": f_end,
                        "passes": [{"pass": 1, "started_at": f_start, "finished_at": f_end, "wall_seconds": 0.2, "timed_out": True, "exit_code": 124, "completed_turns": 0, "failed": True, "verdict": None, "cited_count": 0, "verified_citations_count": 0, "inspection_actions": 0}],
                    }
                ),
                encoding="utf-8",
            )
            write_jsonl(
                codex_home / "sessions" / "2026" / "11" / "24" / f"rollout-subsec-{sid_subsec}.jsonl",
                [
                    {"timestamp": b_start, "type": "session_meta", "payload": {"id": sid_subsec, "cwd": str(repo)}},
                    {"timestamp": b_start, "type": "turn_context", "payload": {"turn_id": "sub_bare", "model": "gpt-6.2-sol", "effort": "max", "sandbox_policy": {"type": "read-only"}}},
                    {"timestamp": b_end, "type": "event_msg", "payload": {"type": "task_complete", "turn_id": "sub_bare", "last_agent_message": "Looks ok.\n\nVERDICT: APPROVE"}},
                ],
            )
            subsec_rep = watchdog.run_watchdog(repo, window_hours=24.0, codex_home=codex_home, include_commits=False, only_session_id=sid_subsec)
            self.assertIn("SHALLOW_REVIEW_APPROVAL", {a["code"] for a in subsec_rep["alerts"]})

            # Unrelated follow-ups ("Explain...", "Thanks for pass 4.") after REQUEST_CHANGES do NOT require a verdict;
            # polite re-review request ("Can you re-review the staged changes?") DOES require a verdict.
            self.assertFalse(watchdog._turn_expects_verdict({"user_prompt": "Explain the updated module structure"}, "REQUEST_CHANGES"))
            self.assertFalse(watchdog._turn_expects_verdict({"user_prompt": "Thanks for pass 4."}, "REQUEST_CHANGES"))
            self.assertTrue(watchdog._turn_expects_verdict({"user_prompt": "Can you re-review the staged changes?"}, "REQUEST_CHANGES"))
            sid_followup = "01a20e10-0000-7000-8000-000000000020"
            t_t3 = self.iso_offset(seconds=-5)
            t_t4 = self.iso_offset(seconds=-2)
            write_jsonl(
                codex_home / "sessions" / "2026" / "11" / "24" / f"rollout-fu-{sid_followup}.jsonl",
                [
                    {"timestamp": t_t1, "type": "session_meta", "payload": {"id": sid_followup, "cwd": str(repo)}},
                    {"timestamp": t_t1, "type": "event_msg", "payload": {"type": "task_started", "turn_id": "fu1"}},
                    {"timestamp": t_t1, "type": "response_item", "payload": {"type": "message", "role": "user", "content": [{"type": "input_text", "text": "Review src/real.rs and end with VERDICT: APPROVE or VERDICT: REQUEST_CHANGES"}]}},
                    {"timestamp": t_t1, "type": "turn_context", "payload": {"turn_id": "fu1", "model": "gpt-6.2-sol", "effort": "high", "sandbox_policy": {"type": "read-only"}}},
                    {"timestamp": t_t1, "type": "event_msg", "payload": {"type": "task_complete", "turn_id": "fu1", "last_agent_message": "Fix src/real.rs:1.\n\nVERDICT: REQUEST_CHANGES"}},
                    {"timestamp": t_t2, "type": "event_msg", "payload": {"type": "task_started", "turn_id": "fu2"}},
                    {"timestamp": t_t2, "type": "response_item", "payload": {"type": "message", "role": "user", "content": [{"type": "input_text", "text": "Thanks for pass 4."}]}},
                    {"timestamp": t_t2, "type": "turn_context", "payload": {"turn_id": "fu2", "model": "gpt-6.2-sol", "effort": "high", "sandbox_policy": {"type": "read-only"}}},
                    {"timestamp": t_t2, "type": "event_msg", "payload": {"type": "task_complete", "turn_id": "fu2", "last_agent_message": "Happy to help whenever the next delta is ready."}},
                    {"timestamp": t_t3, "type": "event_msg", "payload": {"type": "task_started", "turn_id": "fu3"}},
                    {"timestamp": t_t3, "type": "response_item", "payload": {"type": "message", "role": "user", "content": [{"type": "input_text", "text": "Review src/real.rs and end with VERDICT: APPROVE or VERDICT: REQUEST_CHANGES"}]}},
                    {"timestamp": t_t3, "type": "turn_context", "payload": {"turn_id": "fu3", "model": "gpt-6.2-sol", "effort": "high", "sandbox_policy": {"type": "read-only"}}},
                    {"timestamp": t_t3, "type": "event_msg", "payload": {"type": "task_complete", "turn_id": "fu3", "last_agent_message": "Fix src/real.rs:1.\n\nVERDICT: REQUEST_CHANGES"}},
                    {"timestamp": t_t4, "type": "event_msg", "payload": {"type": "task_started", "turn_id": "fu4"}},
                    {"timestamp": t_t4, "type": "response_item", "payload": {"type": "message", "role": "user", "content": [{"type": "input_text", "text": "Can you re-review the staged changes?"}]}},
                    {"timestamp": t_t4, "type": "turn_context", "payload": {"turn_id": "fu4", "model": "gpt-6.2-sol", "effort": "high", "sandbox_policy": {"type": "read-only"}}},
                    {"timestamp": t_t4, "type": "event_msg", "payload": {"type": "task_complete", "turn_id": "fu4", "last_agent_message": "Looks good overall, omitted verdict."}},
                ],
            )
            fu_rep = watchdog.run_watchdog(repo, window_hours=24.0, codex_home=codex_home, include_commits=False, only_session_id=sid_followup)
            fu_ungrounded = [a for a in fu_rep["alerts"] if a["code"] == "UNGROUNDED_REVIEW_OUTPUT"]
            self.assertEqual([a["detail"]["pass"] for a in fu_ungrounded], [4])

            # 9. Small explicit diff (20 LOC) with 1 command and 150 reasoning tokens on Pass 1 must NOT alert
            sid_small = "01a20e10-0000-7000-8000-000000000004"
            (sdir / f"{sid_small}.json").write_text(
                json.dumps(
                    {
                        "session_id": sid_small,
                        "effort": "max",
                        "diff_non_test_loc": 20,
                        "last_finished_at": now_ts,
                        "passes": [{"pass": 1, "finished_at": now_ts, "wall_seconds": 15.0, "timed_out": False, "exit_code": 0, "completed_turns": 1, "failed": False}],
                    }
                ),
                encoding="utf-8",
            )
            write_jsonl(
                codex_home / "sessions" / "2026" / "11" / "24" / f"rollout-small-{sid_small}.jsonl",
                [
                    {"timestamp": now_ts, "type": "session_meta", "payload": {"id": sid_small, "cwd": str(repo)}},
                    {"timestamp": now_ts, "type": "turn_context", "payload": {"turn_id": "t1", "model": "gpt-6.2-sol", "effort": "max", "sandbox_policy": {"type": "read-only"}}},
                    {"timestamp": now_ts, "type": "response_item", "payload": {"type": "custom_tool_call", "name": "exec", "input": "git diff"}},
                    {"timestamp": now_ts, "type": "event_msg", "payload": {"type": "token_count", "info": {"model_context_window": 475000, "last_token_usage": {"input_tokens": 12000, "cached_input_tokens": 0, "reasoning_output_tokens": 150}}}},
                    {"timestamp": now_ts, "type": "event_msg", "payload": {"type": "task_complete", "turn_id": "t1", "last_agent_message": "Verified src/real.rs:1.\n\nVERDICT: APPROVE"}},
                ],
            )
            small_rep = watchdog.run_watchdog(repo, window_hours=24.0, codex_home=codex_home, include_commits=False, only_session_id=sid_small)
            self.assertEqual(small_rep["status"], "OK", small_rep["alerts"])

    def test_detects_post_merge_rework_and_ignores_docs_or_test_only_fixes(self):
        with tempfile.TemporaryDirectory() as tmp:
            repo = Path(tmp) / "repo"
            repo.mkdir()
            (repo / ".codex").mkdir()
            (repo / ".codex" / "config.toml").write_text('model = "gpt-6.2-sol"\nmodel_auto_compact_token_limit = 390000\n', encoding="utf-8")
            git = lambda *args: subprocess.check_output(["git", "-C", str(repo), *args], text=True, stderr=subprocess.DEVNULL)
            git("init")
            git("config", "user.email", "test@example.com")
            git("config", "user.name", "Test")
            (repo / "src").mkdir()
            (repo / "src" / "app.py").write_text("def run():\n    return 1\n", encoding="utf-8")
            (repo / "README.md").write_text("# App\n", encoding="utf-8")
            git("add", ".")
            git("commit", "-m", "TASK-10: initial app")
            orig_sha = git("rev-parse", "HEAD").strip()

            # Docs-only, non-ASCII backlog em-dash path, and skills-lock.json fixes must NOT trigger POST_MERGE_REWORK
            (repo / ".backlog" / "docs" / "specs").mkdir(parents=True)
            emdash_doc = repo / ".backlog" / "docs" / "specs" / "doc-96 - Spec-—-AX.md"
            emdash_doc.write_text("# Spec v1\n", encoding="utf-8")
            (repo / "skills-lock.json").write_text('{"version": 1}\n', encoding="utf-8")
            git("add", ".")
            git("commit", "-m", "chore(backlog): add spec and lock")

            (repo / "README.md").write_text("# App v2\n", encoding="utf-8")
            emdash_doc.write_text("# Spec v2\n", encoding="utf-8")
            (repo / "skills-lock.json").write_text('{"version": 2}\n', encoding="utf-8")
            git("add", ".")
            git("commit", "-m", "fix(docs): update readme, em-dash spec, and lockfile")
            empty_home = Path(tmp) / "empty_home"
            rep_docs = watchdog.run_watchdog(repo, window_hours=24.0, codex_home=empty_home, include_commits=True)
            self.assertNotIn("POST_MERGE_REWORK", {a["code"] for a in rep_docs["alerts"]})

            # Rename-aware numstat keeps pure large rename under 1500 LOC even with diff.renames=false
            git("config", "diff.renames", "false")
            big_lines = "\n".join(f"x_{i} = {i}" for i in range(1600)) + "\n"
            (repo / "src" / "legacy.py").write_text(big_lines, encoding="utf-8")
            git("add", "src/legacy.py")
            git("commit", "-m", "TASK-11: add legacy module")
            git("mv", "src/legacy.py", "src/modern.py")
            (repo / "src" / "modern.py").write_text(big_lines + "x_end = 1\n", encoding="utf-8")
            git("add", "-A")
            git("commit", "-m", "refactor: rename legacy to modern")
            rename_sha = git("rev-parse", "HEAD").strip()
            rep_rename = watchdog.run_watchdog(repo, window_hours=24.0, codex_home=empty_home, include_commits=True)
            self.assertFalse(
                any(a["code"] == "OVERSIZED_REVIEW_SLICE" and a["detail"]["sha"] == rename_sha for a in rep_rename["alerts"])
            )
            # SHA-256 64-hex commit header parses cleanly
            sha256_hdr = watchdog._parse_commit_header_token(f"COMMIT {'a' * 64} 1795541100 fix(core): sha256 test")
            self.assertIsNotNone(sha256_hdr)
            self.assertEqual(sha256_hdr["sha"], "a" * 64)

            # Link manifest outcome to orig_sha, then land a code fix touching src/app.py and src/modern.py
            sid = "01a20e10-0000-7000-8000-000000000005"
            sdir = repo / ".agent-kit" / "logs" / "sol-reviews"
            sdir.mkdir(parents=True)
            now_ts = self.iso_offset(0)
            (sdir / f"{sid}.json").write_text(
                json.dumps(
                    {
                        "session_id": sid,
                        "effort": "max",
                        "passes": [],
                        "outcome": {
                            "recorded_at": now_ts,
                            "confirmed_findings": 2,
                            "false_positives": 0,
                            "repair_regressions": 0,
                            "reviewed_commit": orig_sha,
                        },
                    }
                ),
                encoding="utf-8",
            )
            (repo / "src" / "app.py").write_text("def run():\n    return 2\n", encoding="utf-8")
            git("add", "src/app.py")
            git("commit", "-m", "fix(app): return 2 on edge case")

            rep_fix = watchdog.run_watchdog(repo, window_hours=24.0, codex_home=empty_home, include_commits=True)
            rework_alerts = [a for a in rep_fix["alerts"] if a["code"] == "POST_MERGE_REWORK"]
            self.assertEqual(len(rework_alerts), 1)
            self.assertIn(orig_sha, rework_alerts[0]["detail"]["prior_commits"])
            self.assertEqual(rework_alerts[0]["detail"]["reviewed_sessions"], [sid])
            self.assertEqual(rep_fix["quality_summary"]["post_merge_rework_commits"], 1)


if __name__ == "__main__":
    unittest.main()
