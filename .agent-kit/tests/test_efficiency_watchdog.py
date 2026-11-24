"""Unit tests for kit_efficiency_watchdog.py (deterministic Sol/Codex efficiency watchdog)."""

from datetime import datetime, timedelta, timezone
import errno
import json
import os
from pathlib import Path
import stat
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


if __name__ == "__main__":
    unittest.main()
