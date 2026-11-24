"""Unit tests for kit_sol_review.py (cache-safe read-only Sol review wrapper)."""

import io
import json
import os
from pathlib import Path
import stat
import sys
import tempfile
import tomllib
import unittest
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "runtime"))
import kit_sol_review


class SolReviewWrapperTests(unittest.TestCase):
    def make_configs(self, tmp):
        codex_home = Path(tmp) / "codex_home"
        codex_home.mkdir()
        (codex_home / "config.toml").write_text(
            '[mcp_servers.hindsight]\ncommand = "node"\n'
            '[mcp_servers.livekit-docs]\nurl = "https://docs.livekit.io/mcp"\n'
            '[mcp_servers.rust-lsp-plugin]\ncommand = "rust-lsp-plugin"\n'
            '[mcp_servers."dotted.mcp"]\ncommand = "echo"\n'
            '[plugins."warp@codex-warp"]\nenabled = true\n'
            '[plugins."svelte@svelte"]\nenabled = true\n',
            encoding="utf-8",
        )
        repo = Path(tmp) / "repo"
        (repo / ".codex").mkdir(parents=True)
        (repo / ".codex" / "config.toml").write_text(
            '[mcp_servers.starter_kit_context]\ncommand = "python3"\n'
            '[mcp_servers.rust_analyzer]\ncommand = "sh"\n'
            '[mcp_servers.svelte]\ncommand = "sh"\n'
            '[mcp_servers.svelte_lsp]\ncommand = "sh"\n'
            '[mcp_servers.typescript_lsp]\ncommand = "sh"\n',
            encoding="utf-8",
        )
        return repo, codex_home

    def test_rust_scope_bundles_all_excluded_mcps_and_plugins_in_single_inline_tables(self):
        with tempfile.TemporaryDirectory() as tmp:
            repo, codex_home = self.make_configs(tmp)
            overrides = kit_sol_review.discover_disable_overrides(repo, "rust", codex_home=codex_home)
            values = [overrides[i + 1] for i in range(0, len(overrides), 2)]
            # Exactly one mcp_servers={...} table and one plugins={...} table so dotted names never clobber bare ones.
            self.assertEqual(len(values), 2)
            parsed = tomllib.loads("\n".join(values))
            self.assertEqual(
                set(parsed["mcp_servers"].keys()),
                {"dotted.mcp", "hindsight", "livekit-docs", "rust-lsp-plugin", "svelte", "svelte_lsp", "typescript_lsp"},
            )
            self.assertTrue(all(v == {"enabled": False} for v in parsed["mcp_servers"].values()))
            self.assertEqual(
                set(parsed["plugins"].keys()),
                {"svelte@svelte", "warp@codex-warp"},
            )
            self.assertTrue(all(v == {"enabled": False} for v in parsed["plugins"].values()))
            self.assertNotIn("starter_kit_context", parsed["mcp_servers"])
            self.assertNotIn("rust_analyzer", parsed["mcp_servers"])

    def test_ui_scope_keeps_svelte_and_ts_and_disables_rust_and_hindsight(self):
        with tempfile.TemporaryDirectory() as tmp:
            repo, codex_home = self.make_configs(tmp)
            overrides = kit_sol_review.discover_disable_overrides(repo, "ui", codex_home=codex_home)
            values = [overrides[i + 1] for i in range(0, len(overrides), 2)]
            parsed = tomllib.loads("\n".join(values))
            self.assertEqual(
                set(parsed["mcp_servers"].keys()),
                {"dotted.mcp", "hindsight", "livekit-docs", "rust-lsp-plugin", "rust_analyzer"},
            )
            self.assertNotIn("svelte", parsed["mcp_servers"])
            self.assertNotIn("svelte_lsp", parsed["mcp_servers"])
            self.assertNotIn("typescript_lsp", parsed["mcp_servers"])
            self.assertNotIn("starter_kit_context", parsed["mcp_servers"])

    def test_missing_user_config_emits_no_invalid_hindsight_override(self):
        with tempfile.TemporaryDirectory() as tmp:
            repo = Path(tmp) / "repo"
            (repo / ".codex").mkdir(parents=True)
            (repo / ".codex" / "config.toml").write_text(
                '[mcp_servers.starter_kit_context]\ncommand = "python3"\n',
                encoding="utf-8",
            )
            empty_home = Path(tmp) / "empty_home"
            overrides = kit_sol_review.discover_disable_overrides(repo, "rust", codex_home=empty_home)
            self.assertEqual(overrides, [])

    def test_start_and_resume_argv_pin_read_only_hooks_multi_agent_v2_and_compact_limit(self):
        with tempfile.TemporaryDirectory() as tmp:
            repo, codex_home = self.make_configs(tmp)
            start_argv = kit_sol_review.build_argv(
                mode="start",
                repo=repo,
                model="gpt-6.2-sol",
                effort="xhigh",
                scope="rust",
                compact_limit=390000,
                last_message_path=Path(tmp) / "out.txt",
                codex_home=codex_home,
            )
            self.assertIn("--sandbox", start_argv)
            self.assertIn("read-only", start_argv)
            self.assertIn('model_reasoning_effort="xhigh"', start_argv)
            self.assertIn("model_auto_compact_token_limit=390000", start_argv)
            self.assertIn("features.hooks=false", start_argv)
            self.assertIn("agents.enabled=false", start_argv)
            self.assertIn("features.multi_agent_v2=false", start_argv)
            self.assertNotIn("--ephemeral", start_argv)

            sid = "01a20dec-7cf7-7dd2-b7b5-0ba962e7b137"
            resume_argv = kit_sol_review.build_argv(
                mode="resume",
                repo=repo,
                model="gpt-6.2-sol",
                effort="max",
                scope="rust",
                compact_limit=390000,
                last_message_path=Path(tmp) / "out2.txt",
                session_id=sid,
                codex_home=codex_home,
            )
            self.assertEqual(resume_argv[:3], ["codex", "exec", "resume"])
            self.assertIn('sandbox_mode="read-only"', resume_argv)
            self.assertIn('model_reasoning_effort="max"', resume_argv)
            self.assertIn("model_auto_compact_token_limit=390000", resume_argv)
            self.assertIn("features.multi_agent_v2=false", resume_argv)
            self.assertEqual(resume_argv[-2:], [sid, "-"])
            self.assertNotIn("-C", resume_argv)

    def test_resume_refuses_effort_drift_and_missing_manifest_without_effort(self):
        parser = kit_sol_review.build_parser()
        sid = "01a20dec-7cf7-7dd2-b7b5-0ba962e7b137"
        saved = {
            "session_id": sid,
            "model": "gpt-6.2-sol",
            "effort": "max",
            "scope": "rust",
            "compact_limit": 390000,
            "passes": [],
        }
        drift_args = parser.parse_args(["resume", sid, "--effort", "xhigh"])
        with self.assertRaisesRegex(ValueError, "effort drift"):
            kit_sol_review.resolve_resume_settings(drift_args, saved)

        reuse_args = parser.parse_args(["resume", sid])
        model, effort, scope, compact, _ = kit_sol_review.resolve_resume_settings(reuse_args, saved)
        self.assertEqual((model, effort, scope, compact), ("gpt-6.2-sol", "max", "rust", 390000))

        untracked_args = parser.parse_args(["resume", sid])
        with self.assertRaisesRegex(ValueError, "No saved manifest"):
            kit_sol_review.resolve_resume_settings(untracked_args, None)

    def test_failed_pass_marks_subsequent_delta_as_aggregate_and_records_timeout(self):
        with tempfile.TemporaryDirectory() as tmp:
            sid = "01a20dec-7cf7-7dd2-b7b5-0ba962e7b137"
            p1_usage = {
                "input_tokens": 25351,
                "cached_input_tokens": 12672,
                "output_tokens": 7,
                "reasoning_output_tokens": 0,
            }
            # Direct consecutive pass -> scope == "pass"
            delta_direct, scope_direct = kit_sol_review.compute_pass_usage(
                {
                    "input_tokens": 50798,
                    "cached_input_tokens": 37888,
                    "output_tokens": 14,
                    "reasoning_output_tokens": 3,
                },
                [{"usage_available": True, "cumulative_usage": p1_usage}],
            )
            self.assertEqual(scope_direct, "pass")
            self.assertEqual(delta_direct["input_tokens"], 25447)

            # Spanning an unmeasured failed/timed-out pass -> scope == "aggregate"
            prior_with_failure = [
                {"usage_available": True, "cumulative_usage": p1_usage},
                {"usage_available": False, "cumulative_usage": None},
            ]
            delta_agg, scope_agg = kit_sol_review.compute_pass_usage(
                {
                    "input_tokens": 50798,
                    "cached_input_tokens": 37888,
                    "output_tokens": 14,
                    "reasoning_output_tokens": 3,
                },
                prior_with_failure,
            )
            self.assertEqual(scope_agg, "aggregate")
            self.assertEqual(delta_agg["input_tokens"], 25447)
            receipt = kit_sol_review.format_receipt(sid, 3, "max", "docs", 10.0, delta_agg, scope_agg, 1.0)
            self.assertIn("aggregate_includes_unmeasured_prior_pass", receipt)
            self.assertEqual(kit_sol_review.compute_pass_usage(None, prior_with_failure), (None, "unknown"))

            # Timeout run persists manifest entry with timed_out=True and 0600 message.txt
            repo = Path(tmp) / "repo"
            (repo / ".codex").mkdir(parents=True)
            (repo / ".codex" / "config.toml").write_text("", encoding="utf-8")
            fake_codex = Path(tmp) / "fake_codex.py"
            fake_codex.write_text(
                f"#!/usr/bin/env python3\nimport json, sys, time\n"
                f"print(json.dumps({{'type':'thread.started','thread_id':'{sid}'}}), flush=True)\n"
                f"time.sleep(5)\n",
                encoding="utf-8",
            )
            fake_codex.chmod(0o755)
            with patch.object(sys, "stdin", io.StringIO("timeout test")):
                rc = kit_sol_review.main(
                    [
                        "start",
                        "--effort",
                        "max",
                        "--scope",
                        "docs",
                        "--repo",
                        str(repo),
                        "--codex-home",
                        str(Path(tmp) / "empty"),
                        "--codex-bin",
                        str(fake_codex),
                        "--timeout",
                        "1",
                        "-",
                    ]
                )
            self.assertEqual(rc, 124)
            manifest = kit_sol_review.load_manifest(repo, sid)
            self.assertIsNotNone(manifest)
            self.assertTrue(manifest["passes"][0]["timed_out"])
            self.assertFalse(manifest["passes"][0]["usage_available"])
            msg_path = Path(manifest["passes"][0]["message_path"])
            self.assertEqual(stat.S_IMODE(os.stat(msg_path).st_mode), 0o600)

            # Timeout BEFORE thread.started still persists outcome.json in the invocation dir.
            repo_early = Path(tmp) / "repo_early"
            (repo_early / ".codex").mkdir(parents=True)
            (repo_early / ".codex" / "config.toml").write_text("", encoding="utf-8")
            silent_codex = Path(tmp) / "silent_codex.py"
            silent_codex.write_text("#!/usr/bin/env python3\nimport time\ntime.sleep(5)\n", encoding="utf-8")
            silent_codex.chmod(0o755)
            with patch.object(sys, "stdin", io.StringIO("early timeout")):
                rc2 = kit_sol_review.main(
                    [
                        "start",
                        "--effort",
                        "max",
                        "--scope",
                        "docs",
                        "--repo",
                        str(repo_early),
                        "--codex-home",
                        str(Path(tmp) / "empty"),
                        "--codex-bin",
                        str(silent_codex),
                        "--timeout",
                        "1",
                        "-",
                    ]
                )
            self.assertEqual(rc2, 124)
            outcomes = list(kit_sol_review.state_dir_for(repo_early).glob("*/outcome.json"))
            self.assertEqual(len(outcomes), 1)
            self.assertEqual(stat.S_IMODE(os.stat(outcomes[0]).st_mode), 0o600)
            outcome_data = json.loads(outcomes[0].read_text(encoding="utf-8"))
            self.assertIsNone(outcome_data["session_id"])
            self.assertTrue(outcome_data["timed_out"])
            self.assertFalse(outcome_data["usage_available"])

    def test_inherited_relative_codex_home_resolves_identically_and_dry_run_creates_no_dirs(self):
        with tempfile.TemporaryDirectory() as tmp:
            repo, codex_home = self.make_configs(tmp)
            subdir = Path(tmp) / "sub"
            subdir.mkdir()
            rel_home = os.path.relpath(codex_home, subdir)
            old_cwd = os.getcwd()
            try:
                os.chdir(subdir)
                env = kit_sol_review.review_env(base_env={"CODEX_HOME": rel_home})
                self.assertEqual(env["CODEX_HOME"], str(codex_home.resolve()))
                self.assertEqual(env["HINDSIGHT_DISABLED"], "true")
                self.assertEqual(env["HINDSIGHT_DISABLE_HOOKS"], "1")
            finally:
                os.chdir(old_cwd)

            buf = io.StringIO()
            with patch.object(sys, "stdin", io.StringIO("dry run test")), patch.object(sys, "stdout", buf):
                rc = kit_sol_review.main(
                    [
                        "start",
                        "--effort",
                        "xhigh",
                        "--scope",
                        "rust",
                        "--repo",
                        str(repo),
                        "--codex-home",
                        str(codex_home),
                        "--dry-run",
                        "-",
                    ]
                )
            self.assertEqual(rc, 0)
            self.assertFalse((repo / ".agent-kit" / "logs" / "sol-reviews").exists())
            parsed = json.loads(buf.getvalue())
            self.assertEqual(parsed["env_overrides"]["CODEX_HOME"], str(codex_home.resolve()))

    def test_private_permissions_unique_dirs_and_symlink_rejection(self):
        with tempfile.TemporaryDirectory() as tmp:
            repo = Path(tmp) / "repo"
            repo.mkdir()
            plan = {"pass_num": 1}
            p1 = kit_sol_review.allocate_invocation_paths(repo, plan, create_dirs=True)
            p2 = kit_sol_review.allocate_invocation_paths(repo, plan, create_dirs=True)
            self.assertNotEqual(p1[0].parent, p2[0].parent)
            self.assertEqual(stat.S_IMODE(os.stat(p1[0].parent).st_mode), 0o700)
            self.assertEqual(stat.S_IMODE(os.stat(p1[2]).st_mode), 0o600)

            sid = "01a20dec-7cf7-7dd2-b7b5-0ba962e7b137"
            kit_sol_review.save_manifest(repo, sid, {"session_id": sid, "effort": "max", "passes": []})
            mpath = kit_sol_review.manifest_path_for(repo, sid)
            self.assertEqual(stat.S_IMODE(os.stat(mpath).st_mode), 0o600)

            # Symlinked state dir must be refused before chmod/write.
            outside = Path(tmp) / "outside"
            outside.mkdir(mode=0o755)
            repo2 = Path(tmp) / "repo2"
            (repo2 / ".agent-kit" / "logs").mkdir(parents=True)
            os.symlink(outside, repo2 / ".agent-kit" / "logs" / "sol-reviews")
            with self.assertRaisesRegex(ValueError, "symlinked"):
                kit_sol_review.ensure_private_dir(repo2, kit_sol_review.state_dir_for(repo2))
            self.assertEqual(stat.S_IMODE(os.stat(outside).st_mode), 0o755)

            # Concurrent FileExistsError on os.mkdir(..., dir_fd=...) succeeds when peer created a real dir.
            repo3 = Path(tmp) / "repo3"
            repo3.mkdir()
            target3 = kit_sol_review.state_dir_for(repo3)
            orig_mkdir = os.mkdir

            def racing_mkdir(name, mode=0o777, *, dir_fd=None):
                if name == "sol-reviews":
                    orig_mkdir(name, mode, dir_fd=dir_fd)
                    raise FileExistsError("concurrent peer created dir")
                return orig_mkdir(name, mode, dir_fd=dir_fd)

            with patch.object(os, "mkdir", racing_mkdir):
                created = kit_sol_review.ensure_private_dir(repo3, target3)
            self.assertEqual(created, target3)
            self.assertEqual(stat.S_IMODE(os.stat(target3).st_mode), 0o700)

            # Directory at manifest path and FIFO at message path must fail closed without blocking.
            bad_sid = "01a20dec-7cf7-7dd2-b7b5-0ba962e7b138"
            kit_sol_review.manifest_path_for(repo, bad_sid).mkdir()
            with self.assertRaisesRegex(ValueError, "non-regular"):
                kit_sol_review.load_manifest(repo, bad_sid)

            fifo_path = p1[0].parent / "fifo_msg.txt"
            os.mkfifo(fifo_path, 0o600)
            with self.assertRaisesRegex(ValueError, "non-regular"):
                kit_sol_review.emit_last_message(repo, fifo_path)


if __name__ == "__main__":
    unittest.main()
