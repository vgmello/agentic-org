#!/usr/bin/env python3
"""Deterministic, zero-LLM efficiency watchdog for Codex/Sol sessions, review passes, and commit slices."""

import argparse
from datetime import datetime, timedelta, timezone
import fcntl
import json
import os
from pathlib import Path
import re
import subprocess
import sys
import tomllib
import uuid

from kit_sol_review import (
    INSPECTION_EXTENSION_KINDS,
    analyze_review_message,
    is_test_path,
    iso_now,
    open_private_dir_fd,
    open_private_file_at,
    parse_numstat_z_tokens,
    read_private_regular_file_at,
    resolve_codex_home,
    state_dir_for,
    write_text_atomic_at,
)

SOL_CONTEXT_WINDOW = 475_000
MIN_COMPACT_RATIO = 0.70
SHORT_GAP_MINUTES = 45.0
LOW_CACHE_HIT_PCT = 85.0
MIN_CACHE_PREFIX_TOKENS = 4096
MAX_XHIGH_PASSES = 3
MAX_ANY_PASSES = 5
MAX_REPEATED_WAIT_POLLS = 6
MAX_SINGLE_CELL_WAIT_POLLS = 4
MAX_WAIT_POLL_RATIO = 0.10
SHORT_WAIT_YIELD_MS = 5000
SHORT_WAIT_GAP_SECONDS = 15.0
MAX_NON_TEST_COMMIT_LOC = 1500
MAX_PASS_WALL_SECONDS = 900.0
MIN_SHALLOW_DIFF_LOC = 100
MIN_PASS1_INSPECTION_ACTIONS = 2
MIN_PASS1_HIGH_EFFORT_REASONING = 500
MIN_OUTCOME_FINDINGS_FOR_FP_RATE = 3
MAX_FALSE_POSITIVE_RATIO = 0.30
POST_MERGE_REWORK_DAYS = 7

HINDSIGHT_HOOK_PREFIXES = ("<hindsight_pages>", "<hindsight_memory>", "<hindsight_page_refresh>")
GEN_DIR_RE = re.compile(r"\Agen-\d{8}T\d{6}Z-[0-9a-f]{10}\Z", re.ASCII)
EXEC_TOOL_NAMES = frozenset({"exec", "exec_command", "shell", "shell_command"})
REWORK_SUBJECT_RE = re.compile(r"^(?:revert\b|(?:fix|hotfix)(?:\([^)]+\))?!?:)", re.IGNORECASE)
NON_CODE_REWORK_EXACT = frozenset(
    {
        ".agent-kit/.starter-kit-manifest.json",
        ".agent-kit/skill-catalog.json",
        "skills-lock.json",
        "cargo.lock",
        "bun.lock",
        "package-lock.json",
        "yarn.lock",
        "pnpm-lock.yaml",
    }
)


def parse_iso_utc(raw):
    if not raw or not isinstance(raw, str):
        return None
    text = raw.strip()
    if text.endswith("Z"):
        text = text[:-1] + "+00:00"
    try:
        dt = datetime.fromisoformat(text)
    except ValueError:
        return None
    return dt.replace(tzinfo=timezone.utc) if dt.tzinfo is None else dt.astimezone(timezone.utc)


def find_repo_root(explicit_repo=None):
    if explicit_repo:
        return Path(explicit_repo).resolve()
    out = subprocess.check_output(["git", "rev-parse", "--show-toplevel"], text=True).strip()
    return Path(out).resolve()


def watchdog_dir_for(repo):
    return Path(repo).resolve() / ".agent-kit" / "logs" / "efficiency-watchdog"


def _read_json_regular_at(dir_fd, name):
    try:
        raw = read_private_regular_file_at(dir_fd, name)
    except ValueError:
        return None
    if raw is None:
        return None
    try:
        data = json.loads(raw)
    except json.JSONDecodeError:
        return None
    return data if isinstance(data, dict) else None


def _validate_session_id(value):
    if value is None:
        return None
    try:
        return str(uuid.UUID(str(value)))
    except (ValueError, AttributeError, TypeError) as exc:
        raise ValueError(f"Invalid session_id UUID: {value!r}") from exc


def load_wrapper_manifests(repo, only_session_id=None):
    only_session_id = _validate_session_id(only_session_id)
    manifests = {}
    try:
        with open_private_dir_fd(repo, state_dir_for(repo), create=False) as dir_fd:
            if dir_fd is None:
                return manifests
            names = [f"{only_session_id}.json"] if only_session_id else sorted(n for n in os.listdir(dir_fd) if n.endswith(".json"))
            for name in names:
                data = _read_json_regular_at(dir_fd, name)
                sid = data.get("session_id") if data else None
                if not sid:
                    continue
                if only_session_id and sid != only_session_id:
                    continue
                manifests[sid] = data
    except (OSError, ValueError):
        return manifests
    return manifests


def load_orphan_outcomes(repo, cutoff_utc):
    """Load startup-timeout/early-failure outcome.json files non-blockingly via dir_fd."""
    sdir = state_dir_for(repo)
    outcomes = []
    dir_flags = os.O_RDONLY | os.O_DIRECTORY | getattr(os, "O_NOFOLLOW", 0)
    try:
        with open_private_dir_fd(repo, sdir, create=False) as root_fd:
            if root_fd is None:
                return outcomes
            for sub in sorted(os.listdir(root_fd)):
                try:
                    sub_fd = os.open(sub, dir_flags, dir_fd=root_fd)
                except OSError:
                    continue
                try:
                    data = _read_json_regular_at(sub_fd, "outcome.json")
                finally:
                    os.close(sub_fd)
                if not data or data.get("session_id"):
                    continue
                ts = parse_iso_utc(data.get("finished_at") or data.get("started_at"))
                if ts and ts < cutoff_utc:
                    continue
                outcomes.append((str(sdir / sub / "outcome.json"), data))
    except (OSError, ValueError):
        return outcomes
    return outcomes


def path_matches_repo(candidate_cwd, repo_resolved):
    if not candidate_cwd:
        return False
    try:
        cand = Path(candidate_cwd).resolve()
    except OSError:
        return False
    return cand == repo_resolved or repo_resolved in cand.parents


def _new_rollout_state(path):
    return {
        "path": str(path),
        "session_id": None,
        "cwd": None,
        "started_at": None,
        "last_ts": None,
        "model": None,
        "sandbox_types": set(),
        "efforts": [],
        "turns": [],
        "turns_by_id": {},
        "current_turn": None,
        "hindsight_injections": [],
        "hindsight_tool_calls": [],
        "wait_polls_by_cell": {},
        "api_calls": [],
        "compactions": [],
        "context_window": None,
        "active_task_turn_id": None,
        "prompt_expects_verdict_by_turn": {},
        "prompt_text_by_turn": {},
        "pending_prompt_expects_verdict": False,
        "pending_prompt_text": "",
    }


def _current_turn_is_read_only(state):
    turn = state.get("current_turn")
    return bool(turn and turn.get("sandbox") == "read-only")


def _consume_turn_prompt(state, turn_id):
    prompt_expects = bool(turn_id and state["prompt_expects_verdict_by_turn"].get(turn_id))
    prompt_text = state["prompt_text_by_turn"].get(turn_id, "") if turn_id else ""
    if not prompt_expects and state["pending_prompt_expects_verdict"]:
        prompt_expects = True
        state["pending_prompt_expects_verdict"] = False
    if not prompt_text and state["pending_prompt_text"]:
        prompt_text = state["pending_prompt_text"]
        state["pending_prompt_text"] = ""
    return prompt_expects, prompt_text


def _start_or_update_turn(state, payload, ts):
    effort = payload.get("effort")
    model = payload.get("model")
    sandbox = (payload.get("sandbox_policy") or {}).get("type")
    turn_id = payload.get("turn_id")
    if model:
        state["model"] = model
    if sandbox:
        state["sandbox_types"].add(sandbox)
    if effort:
        state["efforts"].append(effort)

    # Codex emits another turn_context with the same turn_id after mid-turn compaction.
    if turn_id and turn_id in state["turns_by_id"]:
        existing = state["turns_by_id"][turn_id]
        existing["last_ts"] = ts or existing["last_ts"]
        if effort:
            existing["effort"] = effort
        if sandbox:
            existing["sandbox"] = sandbox
        state["current_turn"] = existing
        return

    prev = state["current_turn"]
    idle_m = round((ts - prev["last_ts"]).total_seconds() / 60.0, 2) if (prev and prev["last_ts"] and ts) else None
    prompt_expects, prompt_text = _consume_turn_prompt(state, turn_id)
    turn = {
        "index": len(state["turns"]) + 1,
        "turn_id": turn_id,
        "sandbox": sandbox,
        "effort": effort,
        "prev_effort": prev["effort"] if prev else None,
        "prev_end_in": prev["last_total_in"] if prev else None,
        "idle_minutes": idle_m,
        "started_at": ts,
        "last_ts": ts,
        "first_call": None,
        "last_total_in": None,
        "had_compaction": False,
        "prev_had_compaction": prev["had_compaction"] if prev else False,
        "exec_calls": 0,
        "command_executions": 0,
        "other_inspection_calls": 0,
        "non_hindsight_mcp_calls": 0,
        "seen_mcp_call_ids": set(),
        "has_item_telemetry": False,
        "prompt_expects_verdict": prompt_expects,
        "user_prompt": prompt_text,
        "reasoning_tokens": 0,
        "has_reasoning_telemetry": False,
        "task_complete": False,
        "last_agent_message": None,
        "assistant_message": None,
    }
    state["turns"].append(turn)
    if turn_id:
        state["turns_by_id"][turn_id] = turn
    state["current_turn"] = turn


def _record_token_event(state, payload, ts):
    info = payload.get("info") or {}
    win = info.get("model_context_window")
    if isinstance(win, int) and win > 0:
        state["context_window"] = win
    lu = info.get("last_token_usage") or {}
    inp = lu.get("input_tokens")
    if not isinstance(inp, int) or inp <= 0:
        return
    cached = int(lu.get("cached_input_tokens") or 0)
    state["api_calls"].append(ts)
    turn = state["current_turn"]
    if turn is None:
        return
    turn["last_ts"] = ts or turn["last_ts"]
    reas = lu.get("reasoning_output_tokens")
    if isinstance(reas, int) and reas >= 0:
        turn["has_reasoning_telemetry"] = True
        turn["reasoning_tokens"] += reas
    if turn["first_call"] is None:
        turn["first_call"] = {
            "in": inp,
            "cached": cached,
            "uncached": max(0, inp - cached),
            "hit_pct": round(100.0 * cached / inp, 2),
        }
    turn["last_total_in"] = inp


def _is_verified_hindsight_injection(payload):
    """Only developer-role hook blocks starting with a real Hindsight XML tag count as injections."""
    if payload.get("role") != "developer":
        return False
    content = payload.get("content")
    if not isinstance(content, list):
        return False
    for block in content:
        if not isinstance(block, dict):
            continue
        text = block.get("text")
        if isinstance(text, str) and text.lstrip().startswith(HINDSIGHT_HOOK_PREFIXES):
            return True
    return False


def _is_direct_hindsight_tool_call(payload):
    """Match direct MCP function calls only (never regex-scan JS source inside exec)."""
    name = str(payload.get("name") or "").lower()
    return name.startswith("mcp__hindsight__") or name.startswith("hindsight_")


def _record_wait_call(state, payload, ts):
    raw_args = payload.get("arguments") or payload.get("input") or ""
    cell_id = None
    yield_ms = None
    if isinstance(raw_args, str):
        try:
            parsed = json.loads(raw_args)
            if isinstance(parsed, dict):
                if parsed.get("cell_id") is not None:
                    cell_id = str(parsed["cell_id"])
                if isinstance(parsed.get("yield_time_ms"), (int, float)):
                    yield_ms = float(parsed["yield_time_ms"])
        except json.JSONDecodeError:
            pass
    turn = state["current_turn"]
    turn_key = (turn["turn_id"] or turn["index"]) if turn else 0
    key = (turn_key, cell_id if cell_id is not None else f"anon:{len(state['wait_polls_by_cell'])}")
    state["wait_polls_by_cell"].setdefault(key, []).append({"ts": ts, "yield_ms": yield_ms})


def _extract_message_text(payload):
    content = payload.get("content")
    if not isinstance(content, list):
        return ""
    parts = [block.get("text") for block in content if isinstance(block, dict) and isinstance(block.get("text"), str)]
    return "\n".join(parts).strip()


def _record_inspection_tool_call(turn, payload):
    name = str(payload.get("name") or "").lower()
    call_id = payload.get("call_id") or payload.get("id")
    if name in EXEC_TOOL_NAMES:
        turn["exec_calls"] += 1
        return
    turn["other_inspection_calls"] += 1
    if call_id:
        turn["seen_mcp_call_ids"].add(str(call_id))


def _record_user_prompt(state, payload):
    text = _extract_message_text(payload)
    expects = "verdict:" in text.lower()
    meta_tid = (payload.get("internal_chat_message_metadata_passthrough") or {}).get("turn_id")
    target_tid = meta_tid or state.get("active_task_turn_id")
    if target_tid:
        state["prompt_text_by_turn"][target_tid] = text
        if expects:
            state["prompt_expects_verdict_by_turn"][target_tid] = True
        existing = state["turns_by_id"].get(target_tid)
        if existing is not None:
            existing["user_prompt"] = text
            if expects:
                existing["prompt_expects_verdict"] = True
        return
    turn = state["current_turn"]
    if turn is not None and not turn.get("task_complete"):
        turn["user_prompt"] = text
        if expects:
            turn["prompt_expects_verdict"] = True
        return
    state["pending_prompt_text"] = text
    if expects:
        state["pending_prompt_expects_verdict"] = True


def _record_message_item(state, payload, ts):
    if _is_verified_hindsight_injection(payload):
        state["hindsight_injections"].append((ts, _current_turn_is_read_only(state)))
        return
    role = payload.get("role")
    if role == "user":
        _record_user_prompt(state, payload)
    elif role == "assistant" and state["current_turn"] is not None:
        text = _extract_message_text(payload)
        if text:
            state["current_turn"]["assistant_message"] = text


def _record_response_item(state, payload, ts):
    ptype = payload.get("type")
    if ptype == "message":
        _record_message_item(state, payload, ts)
        return
    if ptype not in ("function_call", "custom_tool_call"):
        return
    if _is_direct_hindsight_tool_call(payload):
        state["hindsight_tool_calls"].append((ts, _current_turn_is_read_only(state)))
    elif payload.get("name") == "wait":
        _record_wait_call(state, payload, ts)
    elif state["current_turn"] is not None:
        _record_inspection_tool_call(state["current_turn"], payload)


def _record_compaction(state, ts):
    turn = state["current_turn"]
    pre_in = turn["last_total_in"] if turn else None
    win = state["context_window"]
    ratio = round(pre_in / win, 3) if (pre_in and win) else None
    state["compactions"].append({"ts": ts, "pre_in": pre_in, "window": win, "ratio": ratio})
    if turn is not None:
        turn["had_compaction"] = True


def _record_mcp_item_completed(state, turn, item, ts):
    server = str(item.get("server") or "").lower()
    if server == "hindsight":
        state["hindsight_tool_calls"].append((ts, bool(turn and turn.get("sandbox") == "read-only")))
        return
    if turn is None:
        return
    cid = item.get("call_id") or item.get("id")
    if cid and str(cid) in turn["seen_mcp_call_ids"]:
        return
    if cid:
        turn["seen_mcp_call_ids"].add(str(cid))
    turn["non_hindsight_mcp_calls"] += 1


def _record_item_completed(state, payload, ts):
    item = payload.get("item") or {}
    turn = state["turns_by_id"].get(payload.get("turn_id")) or state["current_turn"]
    if turn is not None:
        turn["has_item_telemetry"] = True
    itype = item.get("type")
    if itype == "CommandExecution":
        if turn is not None:
            turn["command_executions"] += 1
    elif itype == "McpToolCall":
        _record_mcp_item_completed(state, turn, item, ts)
    elif itype == "Extension":
        kind = str(item.get("kind") or "").lower()
        if turn is not None and kind in INSPECTION_EXTENSION_KINDS:
            turn["other_inspection_calls"] += 1


def _record_event_msg(state, payload, ts):
    etype = payload.get("type")
    if etype == "token_count":
        _record_token_event(state, payload, ts)
    elif etype == "task_started":
        tid = payload.get("turn_id")
        if tid:
            state["active_task_turn_id"] = tid
    elif etype == "context_compacted":
        _record_compaction(state, ts)
    elif etype == "task_complete":
        turn = state["turns_by_id"].get(payload.get("turn_id")) or state["current_turn"]
        if turn is not None:
            turn["task_complete"] = True
            turn["last_ts"] = ts or turn["last_ts"]
            msg = payload.get("last_agent_message")
            if isinstance(msg, str):
                turn["last_agent_message"] = msg
    elif etype == "mcp_tool_call_begin":
        server = str((payload.get("invocation") or {}).get("server") or "").lower()
        if server == "hindsight":
            state["hindsight_tool_calls"].append((ts, _current_turn_is_read_only(state)))
    elif etype == "item_completed":
        _record_item_completed(state, payload, ts)


def _process_rollout_event(state, obj):
    ts = parse_iso_utc(obj.get("timestamp"))
    if ts:
        state["started_at"] = state["started_at"] or ts
        state["last_ts"] = ts
    otype = obj.get("type")
    payload = obj.get("payload") or {}
    if otype == "session_meta":
        state["session_id"] = payload.get("id") or state["session_id"]
        state["cwd"] = payload.get("cwd") or state["cwd"]
    elif otype == "turn_context":
        _start_or_update_turn(state, payload, ts)
    elif otype == "response_item":
        _record_response_item(state, payload, ts)
    elif otype == "compacted":
        _record_compaction(state, ts)
    elif otype == "event_msg":
        _record_event_msg(state, payload, ts)


def parse_rollout_file(path):
    state = _new_rollout_state(path)
    try:
        lines = Path(path).read_text(encoding="utf-8", errors="replace").splitlines()
    except OSError:
        return None
    for raw in lines:
        if not raw.strip():
            continue
        try:
            obj = json.loads(raw)
        except json.JSONDecodeError:
            continue
        if isinstance(obj, dict):
            _process_rollout_event(state, obj)
    return state


def peek_rollout_meta(path):
    """Read only the first few lines to extract session_meta (id, cwd) without parsing megabytes."""
    try:
        with Path(path).open("r", encoding="utf-8", errors="replace") as fh:
            for _ in range(5):
                line = fh.readline()
                if not line:
                    break
                if '"session_meta"' not in line:
                    continue
                obj = json.loads(line)
                if isinstance(obj, dict) and obj.get("type") == "session_meta":
                    payload = obj.get("payload") or {}
                    return payload.get("id"), payload.get("cwd")
    except (OSError, json.JSONDecodeError):
        return None, None
    return None, None


def _rollout_candidate_matches(path, repo_resolved, cutoff_epoch, only_session_id):
    if only_session_id and only_session_id not in path.name:
        return False
    try:
        if path.stat().st_mtime < cutoff_epoch:
            return False
    except OSError:
        return False
    meta_id, meta_cwd = peek_rollout_meta(path)
    if meta_cwd and not path_matches_repo(meta_cwd, repo_resolved):
        return False
    if only_session_id and meta_id and meta_id != only_session_id:
        return False
    return True


def scan_codex_rollouts(repo, cutoff_utc, codex_home=None, only_session_id=None):
    home_dir, _ = resolve_codex_home(codex_home)
    sessions_root = home_dir / "sessions"
    if not sessions_root.is_dir():
        return []
    repo_resolved = Path(repo).resolve()
    cutoff_epoch = cutoff_utc.timestamp()
    results = []
    for path in sorted(sessions_root.rglob("rollout-*.jsonl")):
        if not _rollout_candidate_matches(path, repo_resolved, cutoff_epoch, only_session_id):
            continue
        parsed = parse_rollout_file(path)
        if not parsed or not path_matches_repo(parsed.get("cwd"), repo_resolved):
            continue
        if parsed["last_ts"] and parsed["last_ts"] < cutoff_utc:
            continue
        results.append(parsed)
    return results


def _alert(code, severity, summary, detail, action):
    return {
        "code": code,
        "severity": severity,
        "summary": summary,
        "detail": detail,
        "action": action,
    }


def _ts_in_window(ts, cutoff_utc):
    return ts is None or ts >= cutoff_utc


def analyze_configs(repo, codex_home=None):
    alerts = []
    home_dir, _ = resolve_codex_home(codex_home)
    for label, path in (("user", home_dir / "config.toml"), ("project", Path(repo) / ".codex" / "config.toml")):
        if not path.is_file():
            continue
        try:
            data = tomllib.loads(path.read_text(encoding="utf-8"))
        except (OSError, tomllib.TOMLDecodeError):
            continue
        limit = data.get("model_auto_compact_token_limit")
        model = str(data.get("model") or "")
        if isinstance(limit, int) and limit < int(SOL_CONTEXT_WINDOW * MIN_COMPACT_RATIO):
            if label == "user" or model.startswith("gpt-6.2-sol"):
                alerts.append(
                    _alert(
                        "PREMATURE_COMPACT_CONFIG",
                        "HIGH",
                        f"{label} config pins model_auto_compact_token_limit={limit} (<70% of 475k Sol window)",
                        {"path": str(path), "limit": limit, "model": model},
                        "Remove the global 250k cap in ~/.codex/config.toml and keep 390000 for gpt-6.2-sol in .codex/config.toml.",
                    )
                )
    return alerts


def _prefix_reuse_metrics(turn):
    """Compare cached tokens against expected shared prefix (min of current input and prior turn end)."""
    fc = turn["first_call"]
    expected = turn.get("prev_end_in")
    if not isinstance(expected, int) or expected < MIN_CACHE_PREFIX_TOKENS:
        return None
    shared_cap = min(fc["in"], expected)
    if shared_cap < MIN_CACHE_PREFIX_TOKENS:
        return None
    reused = min(fc["cached"], shared_cap)
    reuse_pct = round(100.0 * reused / shared_cap, 2)
    missed_prefix = max(0, shared_cap - reused)
    return {"shared_cap": shared_cap, "reuse_pct": reuse_pct, "missed_prefix": missed_prefix}


def _evaluate_rollout_turns(session, sid, cutoff_utc, alerts):
    for turn in session["turns"]:
        if turn["index"] == 1 or not turn["first_call"]:
            continue
        if not _ts_in_window(turn["started_at"], cutoff_utc):
            continue
        metrics = _prefix_reuse_metrics(turn)
        if not metrics or metrics["reuse_pct"] >= LOW_CACHE_HIT_PCT or metrics["missed_prefix"] < MIN_CACHE_PREFIX_TOKENS:
            continue
        eff = turn["effort"]
        prev_eff = turn["prev_effort"]
        idle_m = turn["idle_minutes"]
        fc = turn["first_call"]
        if eff and prev_eff and eff != prev_eff:
            alerts.append(
                _alert(
                    "EFFORT_DRIFT_CACHE_MISS",
                    "HIGH",
                    f"Session {sid[:8]} turn {turn['index']} changed effort {prev_eff}->{eff} ({metrics['reuse_pct']}% prefix reuse, {metrics['missed_prefix']} missed)",
                    {"session_id": sid, "turn": turn["index"], "from": prev_eff, "to": eff, **fc, **metrics},
                    "Always resume via python3 .agent-kit/runtime/kit_sol_review.py resume <sid> to lock identical effort.",
                )
            )
        elif idle_m is not None and idle_m <= SHORT_GAP_MINUTES:
            alerts.append(
                _alert(
                    "SHORT_GAP_CACHE_EVICTION",
                    "MEDIUM",
                    f"Session {sid[:8]} turn {turn['index']} dropped to {metrics['reuse_pct']}% prefix reuse after {idle_m}m idle ({metrics['missed_prefix']} missed)",
                    {
                        "session_id": sid,
                        "turn": turn["index"],
                        "idle_minutes": idle_m,
                        "prev_had_compaction": turn["prev_had_compaction"],
                        **fc,
                        **metrics,
                    },
                    "Inspect prefix mutation or post-compaction instability; avoid premature compaction.",
                )
            )


def _evaluate_rollout_compactions(session, sid, cutoff_utc, alerts):
    recent = [c for c in session["compactions"] if _ts_in_window(c.get("ts"), cutoff_utc)]
    premature = [c for c in recent if c["ratio"] is not None and c["ratio"] < MIN_COMPACT_RATIO]
    if premature:
        alerts.append(
            _alert(
                "PREMATURE_AUTO_COMPACTION",
                "HIGH",
                f"Session {sid[:8]} compacted {len(premature)}x below 70% context window",
                {"session_id": sid, "compactions": [{k: v for k, v in c.items() if k != "ts"} for c in premature]},
                "Keep model_auto_compact_token_limit=390000 for gpt-6.2-sol and disable Hindsight/unused MCPs on reviews.",
            )
        )
    elif len(recent) >= 2:
        alerts.append(
            _alert(
                "REPEATED_COMPACTION",
                "MEDIUM",
                f"Session {sid[:8]} compacted {len(recent)} times",
                {"session_id": sid, "count": len(recent)},
                "Start a fresh session instead of extending long-running review threads past 4 passes.",
            )
        )


def _is_short_repeated_wait(prev_entry, cur_entry):
    """A repeated wait is a short polling loop if yield_time_ms <= 5s OR observed cadence <= 15s."""
    ym = cur_entry.get("yield_ms")
    if ym is not None and ym <= SHORT_WAIT_YIELD_MS:
        return True
    prev_ts = prev_entry.get("ts")
    cur_ts = cur_entry.get("ts")
    if prev_ts and cur_ts and (cur_ts - prev_ts).total_seconds() <= SHORT_WAIT_GAP_SECONDS:
        return True
    return False


def _evaluate_rollout_waits(session, sid, cutoff_utc, alerts):
    recent_api_calls = sum(1 for ts in session["api_calls"] if _ts_in_window(ts, cutoff_utc))
    short_repeated_polls = 0
    max_cell_short_burst = 0
    for entries in session["wait_polls_by_cell"].values():
        recent = [e for e in entries if _ts_in_window(e.get("ts"), cutoff_utc)]
        cell_short = sum(1 for i in range(1, len(recent)) if _is_short_repeated_wait(recent[i - 1], recent[i]))
        if cell_short > 0:
            short_repeated_polls += cell_short
            max_cell_short_burst = max(max_cell_short_burst, cell_short + 1)

    ratio_hit = (
        short_repeated_polls >= MAX_REPEATED_WAIT_POLLS
        and recent_api_calls > 0
        and (short_repeated_polls / recent_api_calls) >= MAX_WAIT_POLL_RATIO
    )
    if ratio_hit or max_cell_short_burst >= MAX_SINGLE_CELL_WAIT_POLLS:
        alerts.append(
            _alert(
                "WAIT_POLLING_STORM",
                "MEDIUM",
                f"Session {sid[:8]} repeated {short_repeated_polls} short wait() polls (max {max_cell_short_burst}x on one cell)",
                {"session_id": sid, "short_repeated_polls": short_repeated_polls, "max_cell_burst": max_cell_short_burst, "api_calls": recent_api_calls},
                "Instruct reviewer to use yield_time_ms >= 15000 on shell calls and run Cargo checks outside Codex.",
            )
        )


def _append_pass_cap_alert(sid, n_turns, is_xhigh_only, alerts):
    if is_xhigh_only and n_turns > MAX_XHIGH_PASSES:
        alerts.append(
            _alert(
                "EXCESSIVE_XHIGH_PASSES",
                "MEDIUM",
                f"Tier-1 xhigh session {sid[:8]} ran {n_turns} passes (cap: {MAX_XHIGH_PASSES})",
                {"session_id": sid, "passes": n_turns},
                "Batch root-cause fixes per pass, add differential tests, and escalate to fresh max after 2-3 xhigh passes.",
            )
        )
    elif n_turns > MAX_ANY_PASSES:
        alerts.append(
            _alert(
                "EXCESSIVE_REVIEW_PASSES",
                "MEDIUM",
                f"Review session {sid[:8]} ran {n_turns} passes (cap: {MAX_ANY_PASSES})",
                {"session_id": sid, "passes": n_turns},
                "Slice changes smaller (~800-1,500 non-test LOC) and run all deterministic checks before review.",
            )
        )


def _new_quality_stats():
    return {
        "approved_reviews": 0,
        "request_changes_passes": 0,
        "outcomes_recorded": 0,
        "confirmed_findings": 0,
        "false_positives": 0,
        "precision_pct": None,
        "repair_regressions": 0,
        "final_gate_escapes": 0,
        "post_merge_rework_commits": 0,
    }


def _finalize_quality_stats(stats):
    judged = stats["confirmed_findings"] + stats["false_positives"]
    stats["precision_pct"] = round(100.0 * stats["confirmed_findings"] / judged, 2) if judged > 0 else None
    return stats


def _turn_inspection_actions(turn):
    underlying = turn["command_executions"] + turn["non_hindsight_mcp_calls"]
    if turn["has_item_telemetry"] or underlying > 0:
        return underlying + turn["other_inspection_calls"]
    return turn["exec_calls"] + turn["other_inspection_calls"]


def _shallow_approval_reasons(pass_num, effort, inspection_actions, reasoning_tokens, has_reasoning, diff_loc):
    if inspection_actions == 0:
        return ["0 inspection commands/tool calls"]
    non_trivial = diff_loc is None or diff_loc >= MIN_SHALLOW_DIFF_LOC
    if pass_num != 1 or not non_trivial:
        return []
    reasons = []
    if inspection_actions is not None and inspection_actions < MIN_PASS1_INSPECTION_ACTIONS:
        reasons.append(f"only {inspection_actions} inspection action(s) on Pass 1")
    if effort in ("xhigh", "max") and has_reasoning and reasoning_tokens < MIN_PASS1_HIGH_EFFORT_REASONING:
        reasons.append(f"only {reasoning_tokens} reasoning tokens at effort={effort} on Pass 1")
    return reasons


def _evaluate_pass_quality(
    *,
    sid,
    pass_num,
    effort,
    verdict,
    cited_count,
    verified_count,
    msg_len,
    inspection_actions,
    reasoning_tokens,
    has_reasoning,
    diff_loc,
    expect_verdict,
    alerts,
    quality_stats,
):
    if verdict == "APPROVE":
        quality_stats["approved_reviews"] += 1
        reasons = _shallow_approval_reasons(pass_num, effort, inspection_actions, reasoning_tokens, has_reasoning, diff_loc)
        if reasons:
            alerts.append(
                _alert(
                    "SHALLOW_REVIEW_APPROVAL",
                    "HIGH",
                    f"Review session {sid[:8]} pass {pass_num} issued APPROVE superficially ({'; '.join(reasons)})",
                    {
                        "session_id": sid,
                        "pass": pass_num,
                        "effort": effort,
                        "inspection_actions": inspection_actions,
                        "reasoning_tokens": reasoning_tokens if has_reasoning else None,
                        "diff_non_test_loc": diff_loc,
                    },
                    "Re-run a fresh review pass verifying the actual diff, touched files, and callers before accepting APPROVE.",
                )
            )
        return

    if verdict == "REQUEST_CHANGES":
        quality_stats["request_changes_passes"] += 1
        if verified_count == 0:
            alerts.append(
                _alert(
                    "UNGROUNDED_REVIEW_OUTPUT",
                    "MEDIUM",
                    f"Review session {sid[:8]} pass {pass_num} requested changes with 0 verified repo file:line citations (cited {cited_count})",
                    {"session_id": sid, "pass": pass_num, "cited_count": cited_count, "verified_citations": 0},
                    "Require concrete repo file:line proof for every REQUEST_CHANGES finding and verify against source before fixing.",
                )
            )
        return

    if expect_verdict:
        alerts.append(
            _alert(
                "UNGROUNDED_REVIEW_OUTPUT",
                "MEDIUM",
                f"Review session {sid[:8]} pass {pass_num} omitted an explicit VERDICT line",
                {"session_id": sid, "pass": pass_num, "msg_len": msg_len},
                "Require a standalone `VERDICT: APPROVE` or `VERDICT: REQUEST_CHANGES` line on every review pass.",
            )
        )


def _pass_entry_failed(pass_entry):
    if not pass_entry:
        return False
    return bool(
        pass_entry.get("timed_out")
        or pass_entry.get("failed")
        or pass_entry.get("exit_code") not in (None, 0)
        or pass_entry.get("completed_turns") == 0
    )


def _completed_ro_turns(session):
    if not session:
        return []
    return [t for t in session["turns"] if t.get("sandbox") == "read-only" and t.get("task_complete")]


def _turn_message_text(turn):
    msg = turn.get("last_agent_message")
    if msg is None:
        msg = turn.get("assistant_message")
    return msg or ""


EXPLICIT_REVIEW_CONT_RE = re.compile(
    r"\b(?:re-?review|re-?check\s+(?:the\s+)?(?:diff|delta|changes|fixes|staged|patch)|"
    r"review\s+(?:the\s+)?(?:delta|fixes|changes|staged|patch)|approve\s+or\s+request_changes)\b",
    re.IGNORECASE,
)
QA_OPENER_RE = re.compile(r"^\s*(?:explain|describe|summarize|what|why|how|where|list|show)\b", re.IGNORECASE)


def _turn_expects_verdict(turn, prev_verdict):
    if turn.get("prompt_expects_verdict"):
        return True
    if prev_verdict != "REQUEST_CHANGES":
        return False
    prompt = (turn.get("user_prompt") or "").strip()
    if not prompt or QA_OPENER_RE.match(prompt):
        return False
    return bool(EXPLICIT_REVIEW_CONT_RE.search(prompt))


def _evaluate_unwrapped_rollout_quality(repo, session, sid, cutoff_utc, alerts, quality_stats):
    completed = _completed_ro_turns(session)
    if not completed:
        return
    prev_verdict = None
    for turn in completed:
        msg_text = _turn_message_text(turn)
        analysis = analyze_review_message(repo, msg_text)
        expect_verdict = _turn_expects_verdict(turn, prev_verdict)
        if _ts_in_window(turn.get("last_ts") or turn.get("started_at"), cutoff_utc):
            _evaluate_pass_quality(
                sid=sid,
                pass_num=turn["index"],
                effort=turn.get("effort"),
                verdict=analysis["verdict"],
                cited_count=analysis["cited_count"],
                verified_count=analysis["verified_citations_count"],
                msg_len=len(msg_text.strip()),
                inspection_actions=_turn_inspection_actions(turn),
                reasoning_tokens=turn["reasoning_tokens"],
                has_reasoning=turn["has_reasoning_telemetry"],
                diff_loc=None,
                expect_verdict=expect_verdict,
                alerts=alerts,
                quality_stats=quality_stats,
            )
        prev_verdict = analysis["verdict"]


def _evaluate_rollout_hygiene_alerts(session, sid, has_ro_turn, manifests, cutoff_utc, alerts):
    recent_ro_inj = sum(1 for ts, is_ro in session["hindsight_injections"] if is_ro and _ts_in_window(ts, cutoff_utc))
    recent_ro_calls = sum(1 for ts, is_ro in session["hindsight_tool_calls"] if is_ro and _ts_in_window(ts, cutoff_utc))
    if recent_ro_inj > 0 or recent_ro_calls > 0:
        alerts.append(
            _alert(
                "HINDSIGHT_IN_READONLY_REVIEW",
                "HIGH",
                f"Read-only session {sid[:8]} incurred {recent_ro_inj} Hindsight injections and {recent_ro_calls} Hindsight calls",
                {"session_id": sid, "injections": recent_ro_inj, "tool_calls": recent_ro_calls},
                "Invoke reviews via python3 .agent-kit/runtime/kit_sol_review.py (never bare codex exec) so hooks & Hindsight stay disabled.",
            )
        )
    if has_ro_turn and sid not in manifests and _ts_in_window(session["last_ts"], cutoff_utc):
        alerts.append(
            _alert(
                "UNWRAPPED_CODEX_REVIEW",
                "MEDIUM",
                f"Read-only session {sid[:8]} bypassed kit_sol_review.py (no saved manifest)",
                {"session_id": sid, "path": session["path"]},
                "Use python3 .agent-kit/runtime/kit_sol_review.py start|resume so cache/hook/MCP hygiene stays enforced.",
            )
        )


def analyze_rollout(session, manifests, cutoff_utc, repo=None, quality_stats=None):
    alerts = []
    if quality_stats is None:
        quality_stats = _new_quality_stats()
    sid = session.get("session_id") or Path(session["path"]).stem
    has_ro_turn = any(t.get("sandbox") == "read-only" for t in session["turns"])
    _evaluate_rollout_hygiene_alerts(session, sid, has_ro_turn, manifests, cutoff_utc, alerts)
    _evaluate_rollout_turns(session, sid, cutoff_utc, alerts)
    _evaluate_rollout_compactions(session, sid, cutoff_utc, alerts)
    _evaluate_rollout_waits(session, sid, cutoff_utc, alerts)
    # Wrapped sessions reconcile per-pass quality and pass caps in analyze_manifests.
    if sid not in manifests:
        if repo is not None:
            _evaluate_unwrapped_rollout_quality(repo, session, sid, cutoff_utc, alerts, quality_stats)
        if has_ro_turn:
            efforts = set(session["efforts"])
            _append_pass_cap_alert(sid, len(session["turns"]), "xhigh" in efforts and "max" not in efforts, alerts)
    return alerts


def analyze_single_outcome(label, entry):
    alerts = []
    if entry.get("timed_out"):
        alerts.append(
            _alert(
                "REVIEW_PASS_TIMEOUT",
                "MEDIUM",
                f"Review invocation {label} timed out on pass {entry.get('pass', 1)}",
                {"label": label, "pass": entry.get("pass"), "exit_code": entry.get("exit_code")},
                "Slice the review diff smaller or supply a targeted delta manifest on resume.",
            )
        )
    if (entry.get("wall_seconds") or 0) > MAX_PASS_WALL_SECONDS:
        alerts.append(
            _alert(
                "SLOW_REVIEW_PASS",
                "MEDIUM",
                f"Review invocation {label} exceeded {int(MAX_PASS_WALL_SECONDS)}s on pass {entry.get('pass', 1)}",
                {"label": label, "pass": entry.get("pass"), "wall_seconds": entry.get("wall_seconds")},
                "Keep review slices bounded and verify deterministic checks before invoking Codex.",
            )
        )
    return alerts


def _extract_valid_outcome_counts(outcome, cutoff_utc, has_recent_passes):
    if not isinstance(outcome, dict):
        return None
    rec_ts = parse_iso_utc(outcome.get("recorded_at"))
    if rec_ts is not None and not _ts_in_window(rec_ts, cutoff_utc):
        return None
    if rec_ts is None and not has_recent_passes:
        return None
    confirmed = outcome.get("confirmed_findings")
    fp = outcome.get("false_positives")
    if not (isinstance(confirmed, int) and confirmed >= 0 and isinstance(fp, int) and fp >= 0):
        return None
    reg = outcome.get("repair_regressions")
    reg_int = reg if (isinstance(reg, int) and reg >= 0) else 0
    return confirmed, fp, reg_int


def _evaluate_manifest_outcome(sid, outcome, cutoff_utc, has_recent_passes, alerts, quality_stats):
    counts = _extract_valid_outcome_counts(outcome, cutoff_utc, has_recent_passes)
    if counts is None:
        return
    confirmed, fp, reg_int = counts
    quality_stats["outcomes_recorded"] += 1
    quality_stats["confirmed_findings"] += confirmed
    quality_stats["false_positives"] += fp
    quality_stats["repair_regressions"] += reg_int

    judged = confirmed + fp
    if judged >= MIN_OUTCOME_FINDINGS_FOR_FP_RATE and (fp / judged) > MAX_FALSE_POSITIVE_RATIO:
        fp_pct = round(100.0 * fp / judged, 1)
        alerts.append(
            _alert(
                "HIGH_FALSE_POSITIVE_RATE",
                "MEDIUM",
                f"Review session {sid[:8]} had {fp}/{judged} ({fp_pct}%) false positives (cap: {int(MAX_FALSE_POSITIVE_RATIO * 100)}%)",
                {"session_id": sid, "confirmed_findings": confirmed, "false_positives": fp, "false_positive_pct": fp_pct},
                "Tighten review prompt scope, contracts, and source grounding so reviewers verify behavior before raising findings.",
            )
        )
    prior_sid = outcome.get("prior_session_id")
    escaped = outcome.get("escaped_from_prior")
    if prior_sid and isinstance(escaped, int) and escaped > 0:
        quality_stats["final_gate_escapes"] += 1
        alerts.append(
            _alert(
                "FINAL_GATE_ESCAPE",
                "MEDIUM",
                f"Escalated session {sid[:8]} caught {escaped} prior-revision finding(s) missed by session {str(prior_sid)[:8]}",
                {"session_id": sid, "prior_session_id": str(prior_sid), "escaped_from_prior": escaped, "confirmed_findings": confirmed},
                "Inspect why Tier-1 missed these bugs, add regression tests, and tighten the Tier-1 review checklist.",
            )
        )
    if reg_int > 0:
        alerts.append(
            _alert(
                "REPAIR_REGRESSION_CHURN",
                "MEDIUM",
                f"Review session {sid[:8]} recorded {reg_int} repair regression(s) across fix passes",
                {"session_id": sid, "repair_regressions": reg_int},
                "Run full targeted tests after every repair pass and verify root causes before re-submitting to review.",
            )
        )


def _resolve_manifest_pass_analysis(repo, entry, matched_turn):
    if "verdict" in entry:
        return (
            entry.get("verdict"),
            int(entry.get("cited_count") or 0),
            int(entry.get("verified_citations_count") or 0),
            0,
        )
    if matched_turn is not None and repo is not None:
        msg_text = _turn_message_text(matched_turn)
        analysis = analyze_review_message(repo, msg_text)
        return (
            analysis["verdict"],
            analysis["cited_count"],
            analysis["verified_citations_count"],
            len(msg_text.strip()),
        )
    if repo is None or not entry.get("message_path"):
        return None
    msg_text = _read_repo_text_nonblocking(repo, entry["message_path"])
    if msg_text is None:
        return None
    analysis = analyze_review_message(repo, msg_text)
    return (
        analysis["verdict"],
        analysis["cited_count"],
        analysis["verified_citations_count"],
        len(msg_text.strip()),
    )


def _resolve_pass_telemetry(entry, matched_turn):
    insp = entry.get("inspection_actions")
    if not isinstance(insp, int):
        insp = _turn_inspection_actions(matched_turn) if matched_turn is not None else None
    reas = entry.get("reasoning_output_tokens")
    if isinstance(reas, int) and reas >= 0:
        return insp, reas, True
    if matched_turn is not None and matched_turn["has_reasoning_telemetry"]:
        return insp, matched_turn["reasoning_tokens"], True
    return insp, 0, False


def _has_subsecond(raw_ts):
    return isinstance(raw_ts, str) and "." in raw_ts


def _bounded_window_contains(p_start, p_end, raw_start, raw_end, t_start, t_end, allow_second_slack):
    if _has_subsecond(raw_start) and _has_subsecond(raw_end):
        return p_start <= t_start and t_end <= p_end
    if allow_second_slack:
        return t_start >= p_start and t_end < p_end + timedelta(seconds=1)
    return t_start >= p_start and t_end <= p_end


def _turn_contained_in_pass_window(pass_entry, turn, allow_second_slack=True):
    """Match a rollout turn strictly inside [started_at, finished_at] (subsecond exact, no slack for failed passes)."""
    raw_start, raw_end = pass_entry.get("started_at"), pass_entry.get("finished_at")
    p_start, p_end = parse_iso_utc(raw_start), parse_iso_utc(raw_end)
    t_start = turn.get("started_at")
    t_end = turn.get("last_ts") or t_start
    if (p_start is None and p_end is None) or t_start is None:
        return False
    if p_start is not None and p_end is not None:
        return _bounded_window_contains(p_start, p_end, raw_start, raw_end, t_start, t_end, allow_second_slack)
    if not allow_second_slack:
        return False
    anchor = p_end or p_start
    return abs((t_end - anchor).total_seconds()) < 1.0


def _pick_unique_turn_candidate(pass_entry, candidates, require_unique=False):
    if not candidates:
        return None, None
    if len(candidates) == 1:
        return candidates[0]
    if require_unique:
        return None, None
    p_end = parse_iso_utc(pass_entry.get("finished_at") or pass_entry.get("started_at"))
    ranked = sorted(
        candidates,
        key=lambda pair: abs(((pair[1].get("last_ts") or pair[1]["started_at"]) - p_end).total_seconds()),
    )
    d0 = abs(((ranked[0][1].get("last_ts") or ranked[0][1]["started_at"]) - p_end).total_seconds())
    d1 = abs(((ranked[1][1].get("last_ts") or ranked[1][1]["started_at"]) - p_end).total_seconds())
    if d0 == d1 and ranked[0][1].get("started_at") != ranked[1][1].get("started_at"):
        return None, None
    return ranked[0]


def _find_matching_turn(pass_entry, completed_turns, matched_idxs, allow_second_slack=True, require_unique=False):
    pass_tid = pass_entry.get("turn_id")
    if pass_tid:
        for i, turn in enumerate(completed_turns):
            if i not in matched_idxs and turn.get("turn_id") == pass_tid:
                matched_idxs.add(i)
                return i, turn
        return None, None
    candidates = [
        (i, turn)
        for i, turn in enumerate(completed_turns)
        if i not in matched_idxs and _turn_contained_in_pass_window(pass_entry, turn, allow_second_slack=allow_second_slack)
    ]
    idx, turn = _pick_unique_turn_candidate(pass_entry, candidates, require_unique=require_unique)
    if idx is not None:
        matched_idxs.add(idx)
    return idx, turn


def _claim_failed_pass_turns(failed_passes, completed_turns, matched_idxs, matched_verdicts):
    """Account for failed/timed-out wrapped passes only on unambiguous in-window matches (zero post-end slack)."""
    for p in failed_passes:
        turn_idx, _ = _find_matching_turn(p, completed_turns, matched_idxs, allow_second_slack=False, require_unique=True)
        if turn_idx is not None:
            matched_verdicts[turn_idx] = None


def _evaluate_unmatched_rollout_turns(repo, sid, data, completed_turns, matched_verdicts, cutoff_utc, diff_loc, alerts, quality_stats):
    if repo is None or not completed_turns:
        return
    prev_verdict = None
    for i, turn in enumerate(completed_turns):
        if i in matched_verdicts:
            prev_verdict = matched_verdicts[i]
            continue
        msg_text = _turn_message_text(turn)
        analysis = analyze_review_message(repo, msg_text)
        expect_verdict = _turn_expects_verdict(turn, prev_verdict)
        if _ts_in_window(turn.get("last_ts") or turn.get("started_at"), cutoff_utc):
            _evaluate_pass_quality(
                sid=sid,
                pass_num=turn["index"],
                effort=turn.get("effort") or data.get("effort"),
                verdict=analysis["verdict"],
                cited_count=analysis["cited_count"],
                verified_count=analysis["verified_citations_count"],
                msg_len=len(msg_text.strip()),
                inspection_actions=_turn_inspection_actions(turn),
                reasoning_tokens=turn["reasoning_tokens"],
                has_reasoning=turn["has_reasoning_telemetry"],
                diff_loc=diff_loc,
                expect_verdict=expect_verdict,
                alerts=alerts,
                quality_stats=quality_stats,
            )
        prev_verdict = analysis["verdict"]


def _evaluate_wrapped_passes_quality(repo, sid, data, all_passes, cutoff_utc, rollout, alerts, quality_stats):
    diff_loc = data.get("diff_non_test_loc")
    completed_turns = _completed_ro_turns(rollout)
    completed_passes = [p for p in all_passes if not _pass_entry_failed(p)]
    failed_passes = [p for p in all_passes if _pass_entry_failed(p)]
    matched_idxs = set()
    matched_verdicts = {}
    for idx, p in enumerate(completed_passes):
        turn_idx, matched_turn = _find_matching_turn(p, completed_turns, matched_idxs)
        resolved = _resolve_manifest_pass_analysis(repo, p, matched_turn)
        if turn_idx is not None:
            matched_verdicts[turn_idx] = resolved[0] if resolved is not None else None
        if not _ts_in_window(parse_iso_utc(p.get("finished_at") or p.get("started_at")), cutoff_utc):
            continue
        if resolved is None:
            continue
        verdict, cited_count, verified_count, msg_len = resolved
        insp, reas, has_reas = _resolve_pass_telemetry(p, matched_turn)
        _evaluate_pass_quality(
            sid=sid,
            pass_num=int(p.get("pass") or (idx + 1)),
            effort=p.get("effort") or data.get("effort"),
            verdict=verdict,
            cited_count=cited_count,
            verified_count=verified_count,
            msg_len=msg_len,
            inspection_actions=insp,
            reasoning_tokens=reas,
            has_reasoning=has_reas,
            diff_loc=diff_loc,
            expect_verdict=bool(p.get("hygiene_header", True)),
            alerts=alerts,
            quality_stats=quality_stats,
        )
    _claim_failed_pass_turns(failed_passes, completed_turns, matched_idxs, matched_verdicts)
    _evaluate_unmatched_rollout_turns(
        repo, sid, data, completed_turns, matched_verdicts, cutoff_utc, diff_loc, alerts, quality_stats
    )


def _append_manifest_runtime_alerts(sid, data, all_passes, recent_passes, alerts):
    if not recent_passes:
        return
    timeouts = [p["pass"] for p in recent_passes if p.get("timed_out")]
    if timeouts:
        alerts.append(
            _alert(
                "REVIEW_PASS_TIMEOUT",
                "MEDIUM",
                f"Session {sid[:8]} timed out on pass(es) {timeouts}",
                {"session_id": sid, "timed_out_passes": timeouts},
                "Slice the review diff smaller or supply a targeted delta manifest on resume.",
            )
        )
    slow = [p["pass"] for p in recent_passes if (p.get("wall_seconds") or 0) > MAX_PASS_WALL_SECONDS]
    if slow:
        alerts.append(
            _alert(
                "SLOW_REVIEW_PASS",
                "MEDIUM",
                f"Session {sid[:8]} exceeded {int(MAX_PASS_WALL_SECONDS)}s on pass(es) {slow}",
                {"session_id": sid, "slow_passes": slow},
                "Keep review slices bounded and verify deterministic checks before invoking Codex.",
            )
        )
    _append_pass_cap_alert(sid, len(all_passes), data.get("effort") == "xhigh", alerts)


def analyze_manifests(manifests, cutoff_utc, repo=None, rollouts_by_sid=None, quality_stats=None):
    alerts = []
    if quality_stats is None:
        quality_stats = _new_quality_stats()
    by_sid = rollouts_by_sid or {}
    for sid, data in sorted(manifests.items()):
        all_passes = data.get("passes") or []
        recent_passes = [
            p
            for p in all_passes
            if _ts_in_window(parse_iso_utc(p.get("finished_at") or p.get("started_at")), cutoff_utc)
        ]
        _evaluate_manifest_outcome(sid, data.get("outcome"), cutoff_utc, bool(recent_passes), alerts, quality_stats)
        _append_manifest_runtime_alerts(sid, data, all_passes, recent_passes, alerts)
        rollout = by_sid.get(sid)
        if recent_passes or rollout is not None:
            _evaluate_wrapped_passes_quality(repo, sid, data, all_passes, cutoff_utc, rollout, alerts, quality_stats)
    return alerts


def _is_code_rework_path(path_str):
    if not path_str or is_test_path(path_str):
        return False
    norm = Path(path_str).as_posix().lower()
    if norm.startswith(".backlog/") or norm in NON_CODE_REWORK_EXACT:
        return False
    if norm.endswith((".md", ".lock")):
        return False
    return True


def _reviewed_sessions_by_commit(manifests):
    by_sha = {}
    for sid, data in (manifests or {}).items():
        outcome = data.get("outcome")
        if isinstance(outcome, dict) and outcome.get("reviewed_commit"):
            by_sha.setdefault(str(outcome["reviewed_commit"]), []).append(sid)
    return by_sha


HEX_DIGITS = frozenset("0123456789abcdefABCDEF")


def _parse_commit_header_token(tok):
    clean = tok.lstrip("\n")
    if not clean.startswith("COMMIT "):
        return None
    parts = clean.split(" ", 3)
    if len(parts) < 3 or len(parts[1]) not in (40, 64) or not all(c in HEX_DIGITS for c in parts[1]) or not parts[2].isdigit():
        return None
    return {
        "sha": parts[1],
        "commit_ts": int(parts[2]),
        "subject": parts[3] if len(parts) > 3 else "",
        "non_test_loc": 0,
        "total_loc": 0,
        "code_paths": set(),
    }


def _apply_commit_numstat(current, added, deleted, paths):
    if not (added and deleted and added.isdigit() and deleted.isdigit() and paths):
        return
    churn = int(added) + int(deleted)
    current["total_loc"] += churn
    if not all(is_test_path(p) for p in paths):
        current["non_test_loc"] += churn
    for p in paths:
        if _is_code_rework_path(p):
            current["code_paths"].add(p)


def _parse_recent_commits(repo, since_utc):
    since_arg = since_utc.strftime("%Y-%m-%dT%H:%M:%SZ")
    try:
        raw = subprocess.check_output(
            [
                "git",
                "-C",
                str(repo),
                "log",
                "--no-merges",
                "--numstat",
                "-z",
                "--find-renames",
                f"--since={since_arg}",
                "--format=COMMIT %H %ct %s",
            ],
            stderr=subprocess.DEVNULL,
        )
    except (OSError, subprocess.SubprocessError):
        return []

    tokens = raw.split(b"\x00")
    commits = []
    current = None
    idx = 0
    while idx < len(tokens):
        if not tokens[idx]:
            idx += 1
            continue
        tok = os.fsdecode(tokens[idx])
        header = _parse_commit_header_token(tok)
        if header is not None:
            current = header
            commits.append(current)
            idx += 1
            continue
        added, deleted, paths, idx = parse_numstat_z_tokens(tokens, idx)
        if current is not None:
            _apply_commit_numstat(current, added, deleted, paths)
    return commits


def _find_rework_priors(item, earlier_commits, rework_window_s, reviewed_by_sha):
    prior_shas = []
    seen_shas = set()
    overlap_files = set()
    linked_sessions = set()
    for prior in earlier_commits:
        age = item["commit_ts"] - prior["commit_ts"]
        if age < 0 or age > rework_window_s or prior["subject"].lower().startswith("chore(backlog)"):
            continue
        shared = item["code_paths"] & prior["code_paths"]
        if not shared:
            continue
        if prior["sha"] not in seen_shas:
            seen_shas.add(prior["sha"])
            prior_shas.append(prior["sha"])
        overlap_files.update(shared)
        linked_sessions.update(reviewed_by_sha.get(prior["sha"], ()))
    return prior_shas, overlap_files, linked_sessions


def analyze_recent_commits(repo, cutoff_utc, manifests=None, quality_stats=None):
    alerts = []
    if quality_stats is None:
        quality_stats = _new_quality_stats()
    lookback_utc = cutoff_utc - timedelta(days=POST_MERGE_REWORK_DAYS)
    commits = _parse_recent_commits(repo, lookback_utc)
    cutoff_epoch = int(cutoff_utc.timestamp())
    rework_window_s = POST_MERGE_REWORK_DAYS * 86400
    reviewed_by_sha = _reviewed_sessions_by_commit(manifests)

    for idx, item in enumerate(commits):
        if item["commit_ts"] < cutoff_epoch:
            continue
        if item["non_test_loc"] > MAX_NON_TEST_COMMIT_LOC:
            alerts.append(
                _alert(
                    "OVERSIZED_REVIEW_SLICE",
                    "MEDIUM",
                    f"Commit {item['sha'][:8]} changed {item['non_test_loc']} non-test LOC (target <= {MAX_NON_TEST_COMMIT_LOC})",
                    {k: v for k, v in item.items() if k != "code_paths"},
                    "Split multi-surface work into sequential <=1,500 non-test LOC increments so review converges in 2-3 passes.",
                )
            )
        if not REWORK_SUBJECT_RE.match(item["subject"]) or not item["code_paths"]:
            continue
        prior_shas, overlap_files, linked_sessions = _find_rework_priors(
            item, commits[idx + 1 :], rework_window_s, reviewed_by_sha
        )
        if prior_shas:
            quality_stats["post_merge_rework_commits"] += 1
            short_priors = ", ".join(sha[:8] for sha in prior_shas[:5])
            alerts.append(
                _alert(
                    "POST_MERGE_REWORK",
                    "MEDIUM",
                    f"Fix/revert commit {item['sha'][:8]} modified {len(overlap_files)} code file(s) changed within {POST_MERGE_REWORK_DAYS}d by {short_priors}",
                    {
                        "fix_sha": item["sha"],
                        "subject": item["subject"],
                        "overlapping_files": sorted(overlap_files),
                        "prior_commits": prior_shas[:5],
                        "reviewed_sessions": sorted(linked_sessions),
                    },
                    "Inspect why this defect escaped prior review/tests and add a targeted regression test.",
                )
            )
    return alerts


def deduplicate_alerts(alerts):
    seen = set()
    unique = []
    for item in alerts:
        key = (item["code"], item["summary"])
        if key not in seen:
            seen.add(key)
            unique.append(item)
    return unique


def _relative_outcome_path(repo, outcome_path):
    """Extract repo-relative path without resolving symlinks inside repo (handles macOS /var -> /private/var)."""
    raw_path = Path(outcome_path)
    rel = None
    if not raw_path.is_absolute():
        rel = raw_path
    else:
        for base in (Path(repo), Path(repo).resolve()):
            try:
                rel = raw_path.relative_to(base)
                break
            except ValueError:
                continue
    if rel is None:
        raise ValueError(f"Outcome path is outside repository: {outcome_path}")
    if ".." in rel.parts:
        raise ValueError(f"Refusing parent-traversal component in outcome path: {outcome_path}")
    return rel


def _read_repo_text_nonblocking(repo, target_path):
    """Read an in-repo regular file safely without following symlinks or blocking on FIFOs."""
    repo_root = Path(repo).resolve()
    try:
        rel = _relative_outcome_path(repo, target_path)
        target = repo_root / rel
        with open_private_dir_fd(repo_root, target.parent, create=False) as dir_fd:
            if dir_fd is None:
                return None
            return read_private_regular_file_at(dir_fd, target.name)
    except (OSError, ValueError):
        return None


def _read_single_outcome_nonblocking(repo, outcome_path):
    """Keep original in-repo path components (no .resolve()) so open_private_dir_fd/O_NOFOLLOW catch symlinks."""
    raw = _read_repo_text_nonblocking(repo, outcome_path)
    if raw is None:
        return None
    try:
        data = json.loads(raw)
    except json.JSONDecodeError:
        return None
    return data if isinstance(data, dict) else None


def _collect_outcome_and_commit_alerts(repo, cutoff_utc, include_commits, only_session_id, outcome_path, manifests, quality_stats, alerts):
    if outcome_path:
        entry = _read_single_outcome_nonblocking(repo, outcome_path)
        if entry and not entry.get("session_id"):
            alerts.extend(analyze_single_outcome(Path(outcome_path).parent.name, entry))
        return
    if only_session_id:
        return
    for opath, entry in load_orphan_outcomes(repo, cutoff_utc):
        alerts.extend(analyze_single_outcome(Path(opath).parent.name, entry))
    if include_commits:
        alerts.extend(analyze_recent_commits(repo, cutoff_utc, manifests=manifests, quality_stats=quality_stats))


def run_watchdog(
    repo,
    window_hours=24.0,
    codex_home=None,
    include_commits=True,
    only_session_id=None,
    outcome_path=None,
):
    only_session_id = _validate_session_id(only_session_id)
    cutoff_utc = datetime.now(timezone.utc) - timedelta(hours=window_hours)
    if outcome_path and not only_session_id:
        rollouts, manifests = [], {}
    else:
        manifests = load_wrapper_manifests(repo, only_session_id=only_session_id)
        rollouts = scan_codex_rollouts(repo, cutoff_utc, codex_home=codex_home, only_session_id=only_session_id)
        if only_session_id:
            rollouts = [r for r in rollouts if r.get("session_id") == only_session_id]

    quality_stats = _new_quality_stats()
    alerts = list(analyze_configs(repo, codex_home=codex_home))
    rollouts_by_sid = {}
    for session in rollouts:
        if session.get("session_id"):
            rollouts_by_sid[session["session_id"]] = session
        alerts.extend(analyze_rollout(session, manifests, cutoff_utc, repo=repo, quality_stats=quality_stats))
    alerts.extend(
        analyze_manifests(manifests, cutoff_utc, repo=repo, rollouts_by_sid=rollouts_by_sid, quality_stats=quality_stats)
    )
    _collect_outcome_and_commit_alerts(
        repo, cutoff_utc, include_commits, only_session_id, outcome_path, manifests, quality_stats, alerts
    )

    alerts = deduplicate_alerts(alerts)
    high_count = sum(1 for a in alerts if a["severity"] == "HIGH")
    med_count = sum(1 for a in alerts if a["severity"] == "MEDIUM")
    return {
        "generated_at": iso_now(),
        "repo": str(Path(repo).resolve()),
        "window_hours": window_hours,
        "only_session_id": only_session_id,
        "sessions_scanned": len(rollouts),
        "manifests_scanned": len(manifests),
        "high_alerts": high_count,
        "medium_alerts": med_count,
        "quality_summary": _finalize_quality_stats(quality_stats),
        "status": "ACTION_REQUIRED" if high_count > 0 else ("WARNINGS" if med_count > 0 else "OK"),
        "alerts": alerts,
    }


def render_markdown(report):
    lines = [
        f"# Efficiency Watchdog Report ({report['status']})",
        "",
        f"- Generated: `{report['generated_at']}`",
        f"- Window: last `{report['window_hours']}h` | Rollouts scanned: `{report['sessions_scanned']}` | Wrapper manifests: `{report['manifests_scanned']}`",
        f"- Alerts: **{report['high_alerts']} HIGH**, **{report['medium_alerts']} MEDIUM**",
    ]
    qs = report.get("quality_summary")
    if isinstance(qs, dict):
        prec = f"{qs['precision_pct']}%" if qs.get("precision_pct") is not None else "n/a"
        lines.append(
            f"- Review quality: `{qs.get('approved_reviews', 0)}` approved | `{qs.get('request_changes_passes', 0)}` changes-requested | "
            f"outcomes: `{qs.get('outcomes_recorded', 0)}` (`{qs.get('confirmed_findings', 0)}` confirmed, "
            f"`{qs.get('false_positives', 0)}` false positives, precision `{prec}`, "
            f"`{qs.get('repair_regressions', 0)}` regressions, `{qs.get('final_gate_escapes', 0)}` escapes, "
            f"`{qs.get('post_merge_rework_commits', 0)}` rework)"
        )
    lines.append("")
    if not report["alerts"]:
        lines.append("No efficiency, cache-hygiene, or review-quality regressions detected.")
        return "\n".join(lines) + "\n"
    for item in report["alerts"]:
        lines.append(f"- **[{item['severity']}] `{item['code']}`** — {item['summary']}")
        lines.append(f"  - Action: {item['action']}")
    return "\n".join(lines) + "\n"


def _atomic_symlink_at(dir_fd, link_name, target_rel):
    tmp_link = f".{link_name}.{os.getpid()}.{uuid.uuid4().hex[:8]}.lnk"
    os.symlink(target_rel, tmp_link, dir_fd=dir_fd)
    try:
        os.replace(tmp_link, link_name, src_dir_fd=dir_fd, dst_dir_fd=dir_fd)
    except BaseException:
        try:
            os.unlink(tmp_link, dir_fd=dir_fd)
        except OSError:
            pass
        raise


def _new_gen_name():
    return f"gen-{datetime.now(timezone.utc).strftime('%Y%m%dT%H%M%SZ')}-{uuid.uuid4().hex[:10]}"


def _remove_generation_at(dir_fd, gen_name):
    """Remove a tool-created generation directory strictly via dir_fd (no absolute paths)."""
    if not GEN_DIR_RE.fullmatch(gen_name):
        return
    dir_flags = os.O_RDONLY | os.O_DIRECTORY | getattr(os, "O_NOFOLLOW", 0)
    try:
        gen_fd = os.open(gen_name, dir_flags, dir_fd=dir_fd)
    except OSError:
        return
    try:
        for child in os.listdir(gen_fd):
            try:
                os.unlink(child, dir_fd=gen_fd)
            except OSError:
                pass
    finally:
        os.close(gen_fd)
    try:
        os.rmdir(gen_name, dir_fd=dir_fd)
    except OSError:
        pass


def _stage_generation_at(dir_fd, gen_name, json_payload, md_payload):
    dir_flags = os.O_RDONLY | os.O_DIRECTORY | getattr(os, "O_NOFOLLOW", 0)
    os.mkdir(gen_name, 0o700, dir_fd=dir_fd)
    gen_fd = os.open(gen_name, dir_flags, dir_fd=dir_fd)
    try:
        os.fchmod(gen_fd, 0o700)
        if json_payload is not None:
            write_text_atomic_at(gen_fd, "latest.json", json_payload)
        if md_payload is not None:
            write_text_atomic_at(gen_fd, "latest.md", md_payload)
    except BaseException:
        os.close(gen_fd)
        _remove_generation_at(dir_fd, gen_name)
        raise
    os.close(gen_fd)


def _is_managed_alias_at(dir_fd, name, expected_target):
    try:
        return os.readlink(name, dir_fd=dir_fd) == expected_target
    except OSError:
        return False


def _read_legacy_regular_if_unmanaged(dir_fd, name, expected_target):
    if _is_managed_alias_at(dir_fd, name, expected_target):
        return None
    return read_private_regular_file_at(dir_fd, name)


def _current_target_at(dir_fd):
    try:
        return os.readlink("current", dir_fd=dir_fd)
    except OSError:
        return None


def _has_current_at(dir_fd):
    try:
        os.lstat("current", dir_fd=dir_fd)
        return True
    except FileNotFoundError:
        return False


def _ensure_report_aliases_at(dir_fd):
    """Install latest.json/latest.md aliases idempotently, preserving any existing legacy regular files."""
    has_current = _has_current_at(dir_fd)
    if not has_current:
        legacy_json = _read_legacy_regular_if_unmanaged(dir_fd, "latest.json", "current/latest.json")
        legacy_md = _read_legacy_regular_if_unmanaged(dir_fd, "latest.md", "current/latest.md")
        if legacy_json is not None or legacy_md is not None:
            legacy_gen = _new_gen_name()
            _stage_generation_at(dir_fd, legacy_gen, legacy_json, legacy_md)
            _atomic_symlink_at(dir_fd, "current", legacy_gen)

    if not _is_managed_alias_at(dir_fd, "latest.json", "current/latest.json"):
        _atomic_symlink_at(dir_fd, "latest.json", "current/latest.json")
    if not _is_managed_alias_at(dir_fd, "latest.md", "current/latest.md"):
        _atomic_symlink_at(dir_fd, "latest.md", "current/latest.md")


def _cleanup_dangling_aliases_at(dir_fd):
    if _has_current_at(dir_fd):
        return
    for alias in ("latest.json", "latest.md"):
        if _is_managed_alias_at(dir_fd, alias, f"current/{alias}"):
            try:
                os.unlink(alias, dir_fd=dir_fd)
            except OSError:
                pass


def _prune_old_generations_at(dir_fd, keep_name):
    for name in os.listdir(dir_fd):
        if name != keep_name and GEN_DIR_RE.fullmatch(name):
            _remove_generation_at(dir_fd, name)


def persist_report(repo, report):
    """Stage latest.json and latest.md in one private generation dir, then atomically flip `current`."""
    wdir = watchdog_dir_for(repo)
    json_payload = json.dumps(report, indent=2) + "\n"
    md_payload = render_markdown(report)
    gen_name = _new_gen_name()
    with open_private_dir_fd(repo, wdir, create=True) as dir_fd:
        lock_fd = open_private_file_at(dir_fd, "latest.lock", os.O_RDWR | os.O_CREAT)
        try:
            fcntl.flock(lock_fd, fcntl.LOCK_EX)
            _stage_generation_at(dir_fd, gen_name, json_payload, md_payload)
            try:
                _ensure_report_aliases_at(dir_fd)
                # Single atomic generation switch guarantees latest.json and latest.md always match.
                _atomic_symlink_at(dir_fd, "current", gen_name)
            except BaseException:
                if _current_target_at(dir_fd) != gen_name:
                    _remove_generation_at(dir_fd, gen_name)
                _cleanup_dangling_aliases_at(dir_fd)
                raise
            _prune_old_generations_at(dir_fd, gen_name)
        finally:
            try:
                fcntl.flock(lock_fd, fcntl.LOCK_UN)
            finally:
                os.close(lock_fd)
    return wdir / "latest.json", wdir / "latest.md"


def check_session_inline(repo, session_id, outcome_path=None, codex_home=None):
    """Lightweight post-pass check invoked automatically by kit_sol_review.py; writes stderr alerts only."""
    if not session_id and not outcome_path:
        return None
    report = run_watchdog(
        repo,
        window_hours=24.0,
        codex_home=codex_home,
        include_commits=False,
        only_session_id=session_id,
        outcome_path=outcome_path,
    )
    if report["alerts"]:
        for item in report["alerts"]:
            print(
                f"[efficiency-watchdog] {item['severity']} {item['code']}: {item['summary']} -> {item['action']}",
                file=sys.stderr,
            )
    return report


def build_parser():
    parser = argparse.ArgumentParser(description="Deterministic zero-LLM efficiency watchdog for Codex/Sol workflows")
    parser.add_argument("--repo", help="Repository root (defaults to git toplevel)")
    parser.add_argument("--codex-home", help="Override CODEX_HOME for rollout/config scanning")
    parser.add_argument("--window-hours", type=float, default=24.0, help="Lookback window in hours (default: 24)")
    parser.add_argument("--session-id", help="Inspect only one Codex session UUID")
    parser.add_argument("--json", action="store_true", help="Emit JSON report to stdout")
    parser.add_argument("--no-write", action="store_true", help="Do not persist latest.json/latest.md")
    parser.add_argument("--quiet-ok", action="store_true", help="Emit nothing when status is OK")
    parser.add_argument("--strict", action="store_true", help="Exit 2 when any HIGH alert is present")
    return parser


def main(argv=None):
    args = build_parser().parse_args(argv)
    try:
        repo = find_repo_root(args.repo)
        report = run_watchdog(
            repo,
            window_hours=args.window_hours,
            codex_home=args.codex_home,
            include_commits=not bool(args.session_id),
            only_session_id=args.session_id,
        )
        if not args.no_write:
            persist_report(repo, report)
    except ValueError as exc:
        print(f"[efficiency-watchdog] ERROR: {exc}", file=sys.stderr)
        return 2

    if args.quiet_ok and report["status"] == "OK":
        return 0
    if args.json:
        print(json.dumps(report, indent=2))
    else:
        sys.stdout.write(render_markdown(report))
    if args.strict and report["high_alerts"] > 0:
        return 2
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
