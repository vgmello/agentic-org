#!/usr/bin/env python3
"""Read-only tiered Sol review wrapper with cache-safe resume, hook/MCP hygiene, and telemetry."""

import argparse
from contextlib import contextmanager
from datetime import datetime, timezone
import fcntl
import json
import os
from pathlib import Path
import re
import signal
import stat
import subprocess
import sys
import time
import tomllib
from urllib.parse import unquote
import uuid

DEFAULT_MODEL = "gpt-6.2-sol"
DEFAULT_COMPACT_LIMIT = 390_000
DEFAULT_TIMEOUT = 600
IDLE_CACHE_WARN_MINUTES = 55.0
PASS_COUNT_WARN = 4

VALID_EFFORTS = ("low", "medium", "high", "xhigh", "max")
BARE_TOML_KEY = re.compile(r"^[A-Za-z0-9_-]+$")
USAGE_KEYS = ("input_tokens", "cached_input_tokens", "output_tokens", "reasoning_output_tokens")

TEST_DIR_PARTS = frozenset({"tests", "__tests__", "test", "e2e"})
TEST_EXACT_NAMES = frozenset({"tests.rs", "test.rs"})
VERDICT_LINE_RE = re.compile(
    r"^ {0,3}(?:[#*-]+\s+)?(?:\*\*|`)?VERDICT(?:\*\*|`)?\s*:(?:\*\*|`)?\s*(?:\*\*|`)?"
    r"(APPROVE|APPROVED|REQUEST_CHANGES|CHANGES_REQUESTED|REJECT|BLOCKED)(?:\*\*|`)?\s*$",
    re.IGNORECASE,
)
LIST_MARKER_RE = re.compile(r"^(?:[-*+]|\d{1,9}[.)])(?:[ \t]+|$)")
FENCE_OPEN_RE = re.compile(r"^(`{3,}|~{3,})(.*)$")
FENCE_CLOSE_RE = re.compile(r"^(`{3,}|~{3,})[ \t]*$")
MD_LINK_RE = re.compile(r"\[[^\]]*\]\((?:<([^>]+)>|([^)\s]+))\)")
LINK_TARGET_LINE_RE = re.compile(r"^(?P<path>.+?)(?::(?P<line>\d{1,7})(?:-\d{1,7})?|#L(?P<hline>\d{1,7})(?:-L?\d{1,7})?)$")
PLAIN_CITE_RE = re.compile(
    r"(?<![A-Za-z0-9_/.:-])(?P<path>"
    r"/?(?:[A-Za-z0-9_.-]+/)+[A-Za-z0-9_.-]+"
    r"|(?:[A-Za-z0-9_.-]+\.[A-Za-z0-9]{1,8})"
    r"|(?:\.[A-Za-z0-9][A-Za-z0-9_.-]*)"
    r"|(?:Makefile|Dockerfile|Justfile|Procfile|Brewfile|LICENSE)"
    r")(?::(?P<line>\d{1,7})(?:-\d{1,7})?|#L(?P<hline>\d{1,7})(?:-L?\d{1,7})?)(?![A-Za-z0-9_/-])"
)
EVENTS_INSPECTION_ITEM_TYPES = frozenset({"command_execution", "web_search", "web_fetch", "file_read", "grep", "glob"})
INSPECTION_EXTENSION_KINDS = frozenset(
    {"web.search", "web_search", "web.fetch", "web_fetch", "web.open", "web_open", "file.read", "file_read", "code.search", "code_search"}
)
MAX_CITATION_LINE_DIGITS = 7
MAX_CITATION_SCAN_BYTES = 2_000_000
MAX_UNTRACKED_LOC_BYTES = 2_000_000
MAX_OUTCOME_NOTES_CHARS = 1000

SCOPE_ALLOWED_MCPS = {
    "rust": frozenset({"starter_kit_context", "rust_analyzer"}),
    "ui": frozenset({"starter_kit_context", "svelte", "svelte_lsp", "typescript_lsp"}),
    "docs": frozenset({"starter_kit_context"}),
    "full": frozenset(
        {"starter_kit_context", "rust_analyzer", "svelte", "svelte_lsp", "typescript_lsp"}
    ),
    "minimal": frozenset({"starter_kit_context"}),
}

START_HYGIENE_HEADER = (
    "Review harness hygiene (read-only mode): "
    "Do NOT edit, patch, or commit files. "
    "Skip repo-lead startup rituals (do NOT run `backlog instructions overview`, Hindsight queries, "
    "`.agents/check-tools.sh`, or `cargo` unless explicitly requested below — deterministic check "
    "receipts are supplied by the lead). "
    "Use `yield_time_ms` >= 15000 on shell calls to avoid 1s wait-polling loops. "
    "End with a standalone line `VERDICT: APPROVE` or `VERDICT: REQUEST_CHANGES`, and cite concrete `file:line` paths for any findings. "
    "Apply `.agents/skills/caveman/SKILL.md` (delegate-only).\n\n"
)

RESUME_HYGIENE_HEADER = (
    "Review harness hygiene (read-only resume): "
    "Do NOT edit, patch, or commit files. Inspect only the reported delta and affected callers; "
    "skip `backlog`, Hindsight, and `cargo` unless explicitly requested below; "
    "use `yield_time_ms` >= 15000 on shell calls; "
    "end with a standalone `VERDICT: APPROVE` or `VERDICT: REQUEST_CHANGES` line with `file:line` citations for any findings.\n\n"
)


def utc_now():
    return datetime.now(timezone.utc)


def iso_now():
    return utc_now().strftime("%Y-%m-%dT%H:%M:%SZ")


def iso_now_precise():
    return utc_now().strftime("%Y-%m-%dT%H:%M:%S.%fZ")


def parse_iso(value):
    if not value:
        return None
    text = str(value).strip()
    if text.endswith("Z"):
        text = text[:-1] + "+00:00"
    dt = datetime.fromisoformat(text)
    return dt.replace(tzinfo=timezone.utc) if dt.tzinfo is None else dt.astimezone(timezone.utc)


def toml_key_segment(name):
    return name if BARE_TOML_KEY.match(name) else json.dumps(name)


def inline_disable_table(table_name, names):
    """Build one TOML inline-table override so every excluded key stays disabled without clobbering."""
    entries = ",".join(f"{toml_key_segment(name)}={{enabled=false}}" for name in names)
    return f"{table_name}={{{entries}}}"


def resolve_codex_home(explicit_home=None, base_env=None):
    """Return (resolved_path, must_export) so discovery and child Codex always read the same config."""
    env = os.environ if base_env is None else base_env
    if explicit_home:
        return Path(explicit_home).expanduser().resolve(), True
    env_home = env.get("CODEX_HOME")
    if env_home:
        return Path(env_home).expanduser().resolve(), True
    return (Path.home() / ".codex").resolve(), False


def load_toml_tables(path):
    if not path or not path.is_file():
        return set(), set()
    try:
        data = tomllib.loads(path.read_text(encoding="utf-8"))
    except (OSError, tomllib.TOMLDecodeError):
        return set(), set()
    if not isinstance(data, dict):
        return set(), set()
    mcps = set((data.get("mcp_servers") or {}).keys())
    plugins = set((data.get("plugins") or {}).keys())
    return mcps, plugins


def discover_disable_overrides(repo, scope, codex_home=None, base_env=None):
    """Disable all excluded MCP servers and all plugins declared in user/project config via inline tables."""
    allowed = SCOPE_ALLOWED_MCPS[scope]
    home_dir, _ = resolve_codex_home(codex_home, base_env=base_env)
    user_mcps, user_plugins = load_toml_tables(home_dir / "config.toml")
    repo_mcps, repo_plugins = load_toml_tables(Path(repo) / ".codex" / "config.toml")
    disable_mcps = sorted((user_mcps | repo_mcps) - allowed)
    disable_plugins = sorted(user_plugins | repo_plugins)
    overrides = []
    if disable_mcps:
        overrides.extend(["-c", inline_disable_table("mcp_servers", disable_mcps)])
    if disable_plugins:
        overrides.extend(["-c", inline_disable_table("plugins", disable_plugins)])
    return overrides


def compact_limit_for(model, explicit_limit):
    if explicit_limit is not None:
        return explicit_limit
    if model.startswith("gpt-6.2-sol"):
        return DEFAULT_COMPACT_LIMIT
    return None


def build_argv(
    *,
    mode,
    repo,
    model,
    effort,
    scope,
    compact_limit,
    last_message_path,
    session_id=None,
    codex_bin="codex",
    codex_home=None,
    base_env=None,
):
    disable_args = discover_disable_overrides(repo, scope, codex_home=codex_home, base_env=base_env)
    common = [
        "-m",
        model,
        "-c",
        f'model_reasoning_effort="{effort}"',
        "-c",
        'approval_policy="never"',
        "-c",
        "features.hooks=false",
        "-c",
        "agents.enabled=false",
        "-c",
        "features.multi_agent_v2=false",
    ]
    if compact_limit is not None:
        common.extend(["-c", f"model_auto_compact_token_limit={compact_limit}"])
    common.extend(disable_args)
    common.extend(["--json", "-o", str(last_message_path)])
    if mode == "start":
        return [codex_bin, "exec", "-C", str(repo), "--sandbox", "read-only", *common, "-"]
    if mode == "resume":
        if not session_id:
            raise ValueError("resume requires a session_id")
        return [
            codex_bin,
            "exec",
            "resume",
            "-c",
            'sandbox_mode="read-only"',
            *common,
            session_id,
            "-",
        ]
    raise ValueError(f"Unsupported mode: {mode}")


def review_env(base_env=None, codex_home=None):
    env = dict(os.environ if base_env is None else base_env)
    resolved_home, should_export = resolve_codex_home(codex_home, base_env=env)
    env["HINDSIGHT_DISABLED"] = "true"
    env["HINDSIGHT_DISABLE_HOOKS"] = "1"
    env["AGENT_KIT_MEMORY_ACCESS"] = "review"
    if should_export:
        env["CODEX_HOME"] = str(resolved_home)
    return env


def _open_dir_step(parent_fd, part, create):
    dir_flags = os.O_RDONLY | os.O_DIRECTORY | getattr(os, "O_NOFOLLOW", 0)
    try:
        return os.open(part, dir_flags, dir_fd=parent_fd)
    except FileNotFoundError:
        if not create:
            return None
        try:
            os.mkdir(part, 0o700, dir_fd=parent_fd)
        except FileExistsError:
            pass
        try:
            return os.open(part, dir_flags, dir_fd=parent_fd)
        except OSError as exc:
            raise ValueError(f"Refusing symlinked or non-directory review state component: {part}") from exc
    except OSError as exc:
        raise ValueError(f"Refusing symlinked or non-directory review state component: {part}") from exc


@contextmanager
def open_private_dir_fd(repo, path, create=True):
    """Pin every path component from repo root with O_DIRECTORY|O_NOFOLLOW to prevent symlink TOCTOU."""
    root_resolved = Path(repo).resolve()
    rel = Path(path).relative_to(root_resolved)
    dir_flags = os.O_RDONLY | os.O_DIRECTORY
    cur_fd = os.open(root_resolved, dir_flags)
    try:
        for part in rel.parts:
            if part in ("", ".", ".."):
                raise ValueError(f"Refusing unsafe review state path component: {part!r}")
            next_fd = _open_dir_step(cur_fd, part, create=create)
            if next_fd is None:
                os.close(cur_fd)
                cur_fd = None
                yield None
                return
            os.close(cur_fd)
            cur_fd = next_fd
        if create:
            os.fchmod(cur_fd, 0o700)
        yield cur_fd
    finally:
        if cur_fd is not None:
            os.close(cur_fd)


def ensure_private_dir(repo, path):
    with open_private_dir_fd(repo, path, create=True):
        pass
    return Path(path)


def open_private_file_at(dir_fd, name, flags):
    open_flags = flags | getattr(os, "O_NOFOLLOW", 0)
    try:
        fd = os.open(name, open_flags, 0o600, dir_fd=dir_fd)
    except OSError as exc:
        raise ValueError(f"Refusing symlinked or inaccessible review state file: {name}") from exc
    try:
        os.fchmod(fd, 0o600)
    except OSError:
        os.close(fd)
        raise
    return fd


@contextmanager
def open_private_write(repo, path):
    target = Path(path)
    with open_private_dir_fd(repo, target.parent, create=True) as dir_fd:
        fd = open_private_file_at(dir_fd, target.name, os.O_WRONLY | os.O_CREAT | os.O_TRUNC)
        with os.fdopen(fd, "w", encoding="utf-8") as fh:
            yield fh


def write_text_atomic_at(dir_fd, filename, payload):
    """Create a 0600 temp file exclusively inside dir_fd and atomically replace filename."""
    tmp_name = f".{filename}.{os.getpid()}.{uuid.uuid4().hex[:8]}.tmp"
    fd = open_private_file_at(dir_fd, tmp_name, os.O_WRONLY | os.O_CREAT | os.O_EXCL)
    try:
        with os.fdopen(fd, "w", encoding="utf-8") as fh:
            fh.write(payload)
            fh.flush()
        os.replace(tmp_name, filename, src_dir_fd=dir_fd, dst_dir_fd=dir_fd)
    except BaseException:
        try:
            os.unlink(tmp_name, dir_fd=dir_fd)
        except OSError:
            pass
        raise


def save_json_atomic(repo, path, data):
    target = Path(path)
    payload = json.dumps(data, indent=2) + "\n"
    with open_private_dir_fd(repo, target.parent, create=True) as dir_fd:
        write_text_atomic_at(dir_fd, target.name, payload)


def state_dir_for(repo):
    return Path(repo).resolve() / ".agent-kit" / "logs" / "sol-reviews"


def manifest_path_for(repo, session_id):
    return state_dir_for(repo) / f"{session_id}.json"


def read_private_regular_file_at(dir_fd, name, chmod_0600=False):
    """Open name inside dir_fd non-blockingly without following symlinks and verify S_ISREG before reading."""
    flags = os.O_RDONLY | getattr(os, "O_NOFOLLOW", 0) | getattr(os, "O_NONBLOCK", 0)
    try:
        fd = os.open(name, flags, dir_fd=dir_fd)
    except FileNotFoundError:
        return None
    except OSError as exc:
        raise ValueError(f"Refusing non-regular or symlinked review state file: {name}") from exc
    try:
        st = os.fstat(fd)
        if not stat.S_ISREG(st.st_mode):
            raise ValueError(f"Refusing non-regular review state file: {name}")
        if chmod_0600:
            os.fchmod(fd, 0o600)
        with os.fdopen(fd, "r", encoding="utf-8") as fh:
            fd = None
            return fh.read()
    finally:
        if fd is not None:
            os.close(fd)


def load_manifest(repo, session_id):
    target = manifest_path_for(repo, session_id)
    with open_private_dir_fd(repo, target.parent, create=False) as dir_fd:
        if dir_fd is None:
            return None
        raw = read_private_regular_file_at(dir_fd, target.name)
        return json.loads(raw) if raw is not None else None


def save_manifest(repo, session_id, data):
    save_json_atomic(repo, manifest_path_for(repo, session_id), data)


@contextmanager
def session_lock(repo, session_id):
    if not session_id:
        yield
        return
    with open_private_dir_fd(repo, state_dir_for(repo), create=True) as dir_fd:
        fd = open_private_file_at(dir_fd, f"{session_id}.lock", os.O_RDWR | os.O_CREAT)
        try:
            fcntl.flock(fd, fcntl.LOCK_EX)
            yield
        finally:
            try:
                fcntl.flock(fd, fcntl.LOCK_UN)
            finally:
                os.close(fd)


def _is_inspection_event_item(item):
    if not isinstance(item, dict):
        return False
    itype = str(item.get("type") or "").lower()
    if itype in EVENTS_INSPECTION_ITEM_TYPES:
        return True
    if itype == "mcp_tool_call":
        return str(item.get("server") or "").lower() != "hindsight"
    return itype == "extension" and str(item.get("kind") or "").lower() in INSPECTION_EXTENSION_KINDS


def parse_events_jsonl(events_path):
    session_ids = []
    totals = {k: 0 for k in USAGE_KEYS}
    completed = 0
    has_usage = False
    failed = False
    inspection_actions = 0
    for raw in Path(events_path).read_text(encoding="utf-8").splitlines():
        line = raw.strip()
        if not line:
            continue
        try:
            event = json.loads(line)
        except json.JSONDecodeError:
            continue
        if not isinstance(event, dict):
            continue
        kind = event.get("type")
        if kind == "thread.started" and event.get("thread_id"):
            session_ids.append(str(uuid.UUID(event["thread_id"])))
        elif kind == "turn.completed":
            completed += 1
            usage = event.get("usage")
            if isinstance(usage, dict) and usage:
                has_usage = True
                for key in USAGE_KEYS:
                    totals[key] += int(usage.get(key) or 0)
        elif kind == "item.completed" and _is_inspection_event_item(event.get("item")):
            inspection_actions += 1
        elif kind in {"turn.failed", "error"}:
            failed = True
    return session_ids, (totals if has_usage else None), completed, failed, inspection_actions


def find_usage_baseline(prior_passes):
    """Return (last_valid_cumulative, spanned_unmeasured_pass)."""
    spanned_unmeasured = False
    for entry in reversed(prior_passes or []):
        prev = entry.get("cumulative_usage")
        if isinstance(prev, dict) and all(k in prev for k in USAGE_KEYS):
            return prev, spanned_unmeasured
        spanned_unmeasured = True
    return None, spanned_unmeasured


def compute_pass_usage(cumulative, prior_passes):
    """Return (delta_dict_or_none, usage_scope) where usage_scope is 'pass', 'aggregate', or 'unknown'."""
    if cumulative is None:
        return None, "unknown"
    prev, spanned_unmeasured = find_usage_baseline(prior_passes)
    scope = "aggregate" if spanned_unmeasured else "pass"
    if prev is None:
        return dict(cumulative), scope
    delta = {}
    for key in USAGE_KEYS:
        cur_val = int(cumulative.get(key) or 0)
        prev_val = int(prev.get(key) or 0)
        delta[key] = cur_val - prev_val if cur_val >= prev_val else cur_val
    return delta, scope


def read_prompt_source(prompt_arg):
    if prompt_arg == "-" or prompt_arg is None:
        return sys.stdin.read()
    return Path(prompt_arg).read_text(encoding="utf-8")


def find_repo_root(explicit_repo=None):
    if explicit_repo:
        return Path(explicit_repo).resolve()
    out = subprocess.check_output(["git", "rev-parse", "--show-toplevel"], text=True).strip()
    return Path(out).resolve()


def is_test_path(path_str):
    parts = [p.lower() for p in Path(path_str).parts]
    if any(part in TEST_DIR_PARTS for part in parts[:-1]):
        return True
    base = parts[-1] if parts else ""
    return (
        base in TEST_EXACT_NAMES
        or base.endswith(("_tests.rs", "_test.rs", "_test.py", "_test.ts", "_test.js"))
        or base.startswith("test_")
        or ".test." in base
        or ".spec." in base
    )


def _expand_indent(text):
    lead_chars = len(text) - len(text.lstrip(" \t"))
    return len(text[:lead_chars].expandtabs(4)), text[lead_chars:]


def _strip_columns(text, cols):
    """Strip cols leading visual columns (tabs expanded to 4-col stops) from text; return remainder or None."""
    if cols <= 0:
        return text
    col = 0
    idx = 0
    while idx < len(text) and col < cols:
        ch = text[idx]
        if ch == " ":
            col += 1
        elif ch == "\t":
            col += 4 - (col % 4)
        else:
            return None
        idx += 1
    if col < cols:
        return None
    return (" " * (col - cols)) + text[idx:]


def _match_existing_containers(line, prev_containers, max_count=None):
    """Advance line through continuing outer containers in nesting order; return (matched, remaining)."""
    limit = len(prev_containers) if max_count is None else min(max_count, len(prev_containers))
    matched = []
    rest = line
    for kind, col in prev_containers[:limit]:
        if kind == "bq":
            lead_col, trimmed = _expand_indent(rest)
            if lead_col > 3 or not trimmed.startswith(">"):
                break
            rest = trimmed[1:]
            if rest.startswith(" "):
                rest = rest[1:]
            elif rest.startswith("\t"):
                rest = "   " + rest[1:]
            matched.append(("bq", 0))
        else:
            if not rest.strip():
                matched.append(("list", col))
                rest = ""
                continue
            stripped = _strip_columns(rest, col)
            if stripped is None:
                break
            rest = stripped
            matched.append(("list", col))
    return matched, rest


def _peel_new_containers(rest, containers):
    """Peel any newly opened interleaved blockquote/list containers from rest; return updated rest."""
    while rest.strip():
        lead_col, trimmed = _expand_indent(rest)
        if lead_col > 3:
            break
        if trimmed.startswith(">"):
            rest = trimmed[1:]
            if rest.startswith(" "):
                rest = rest[1:]
            elif rest.startswith("\t"):
                rest = "   " + rest[1:]
            containers.append(("bq", 0))
            continue
        match = LIST_MARKER_RE.match(trimmed)
        if not match:
            break
        marker = match.group(0)
        after = trimmed[len(marker) :]
        extra_lead, _ = _expand_indent(after)
        marker_cols = len(marker.expandtabs(4))
        content_col = lead_col + marker_cols if (after.strip() and extra_lead <= 3) else lead_col + len(marker.rstrip(" \t")) + 1
        containers.append(("list", content_col))
        rest = after
    return rest


def _match_fence_open(rest):
    lead_col, trimmed = _expand_indent(rest)
    if not (0 <= lead_col <= 3):
        return None
    match = FENCE_OPEN_RE.match(trimmed)
    if not match:
        return None
    marker, info = match.group(1), match.group(2)
    if marker[0] == "`" and "`" in info:
        return None
    return marker[0], len(marker)


def _matches_fence_close(rest, fence_char, fence_len):
    lead_col, trimmed = _expand_indent(rest)
    if not (0 <= lead_col <= 3):
        return False
    match = FENCE_CLOSE_RE.match(trimmed)
    return bool(match and match.group(1)[0] == fence_char and len(match.group(1)) >= fence_len)


def strip_fenced_code_blocks(text):
    """Remove CommonMark fenced blocks respecting interleaved blockquote/list container nesting and fence length."""
    if not text:
        return ""
    out = []
    open_fence = None  # (fence_depth, fence_char, fence_len)
    containers = []
    for line in text.splitlines():
        if open_fence is not None:
            fdepth, fchar, flen = open_fence
            matched, rest = _match_existing_containers(line, containers, max_count=fdepth)
            if len(matched) == fdepth:
                if _matches_fence_close(rest, fchar, flen):
                    open_fence = None
                    containers = matched
                continue
            open_fence = None
            containers = matched
        else:
            containers, rest = _match_existing_containers(line, containers)
        rest = _peel_new_containers(rest, containers)
        opened = _match_fence_open(rest)
        if opened is not None:
            open_fence = (len(containers), opened[0], opened[1])
            continue
        out.append(line)
    return "\n".join(out)


def extract_verdict(text):
    """Return canonical 'APPROVE', 'REQUEST_CHANGES', or None from the last standalone VERDICT line."""
    cleaned = strip_fenced_code_blocks(text)
    verdict = None
    for line in cleaned.splitlines():
        match = VERDICT_LINE_RE.match(line)
        if not match:
            continue
        raw = match.group(1).upper()
        verdict = "APPROVE" if raw in ("APPROVE", "APPROVED") else "REQUEST_CHANGES"
    return verdict


def _safe_positive_line_num(line_str):
    if not line_str or len(line_str) > MAX_CITATION_LINE_DIGITS or not line_str.isdigit():
        return None
    val = int(line_str)
    return val if val >= 1 else None


def _parse_line_target(target):
    m = LINK_TARGET_LINE_RE.match(target.strip())
    if not m:
        return None
    line_num = _safe_positive_line_num(m.group("line") or m.group("hline"))
    if line_num is None:
        return None
    return m.group("path"), line_num


def extract_file_line_citations(text):
    """Extract (raw_path, line_num) citations outside fenced code blocks from markdown links and plain tokens."""
    cleaned = strip_fenced_code_blocks(text)
    if not cleaned:
        return []
    citations = []

    def _collect_md(match):
        target = match.group(1) or match.group(2) or ""
        parsed = _parse_line_target(target)
        if parsed is not None:
            citations.append(parsed)
        return " "

    remainder = MD_LINK_RE.sub(_collect_md, cleaned)
    for line in remainder.splitlines():
        for match in PLAIN_CITE_RE.finditer(line):
            prefix = line[: match.start()]
            last_ws = max(prefix.rfind(" "), prefix.rfind("\t"), prefix.rfind("("), prefix.rfind("["))
            token_prefix = prefix[last_ws + 1 :]
            if "://" in token_prefix:
                continue
            line_num = _safe_positive_line_num(match.group("line") or match.group("hline"))
            if line_num is not None:
                citations.append((match.group("path"), line_num))
    return citations


def _repo_relative_candidate(repo, raw_path):
    candidate = raw_path.strip()
    if not candidate:
        return None
    if candidate.startswith("file://"):
        candidate = unquote(candidate[len("file://") :])
    if not candidate or "\x00" in candidate:
        return None
    p = Path(candidate)
    if p.is_absolute():
        rel = None
        for base in (Path(repo), Path(repo).resolve()):
            try:
                rel = p.relative_to(base)
                break
            except ValueError:
                continue
        if rel is None:
            return None
    else:
        rel = p
    if not rel.parts or ".." in rel.parts:
        return None
    return rel


def _scan_fd_lines_bounded(fd, max_needed_line):
    """Scan a regular file descriptor up to max_needed_line or MAX_CITATION_SCAN_BYTES; return (lines, reached_eof)."""
    with os.fdopen(fd, "rb") as fh:
        lines = 0
        scanned = 0
        saw_any = False
        ended_nl = False
        while scanned < MAX_CITATION_SCAN_BYTES:
            chunk = fh.read(min(65536, MAX_CITATION_SCAN_BYTES - scanned))
            if not chunk:
                return ((lines + (0 if ended_nl else 1)) if saw_any else 0), True
            if b"\x00" in chunk:
                return None, True
            saw_any = True
            scanned += len(chunk)
            lines += chunk.count(b"\n")
            ended_nl = chunk.endswith(b"\n")
            if max_needed_line is not None and lines >= max_needed_line:
                return lines, False
        # Byte cap reached before EOF: include trailing partial line if the chunk did not end with NL.
        return ((lines + (0 if ended_nl else 1)) if saw_any else 0), True


def _repo_file_line_count(repo_root, rel, max_needed_line=None):
    """Open rel inside repo via pinned dir_fd (O_NOFOLLOW|O_NONBLOCK), verify S_ISREG, and return (lines, reached_eof)."""
    target = repo_root / rel
    try:
        with open_private_dir_fd(repo_root, target.parent, create=False) as dir_fd:
            if dir_fd is None:
                return None, True
            flags = os.O_RDONLY | getattr(os, "O_NOFOLLOW", 0) | getattr(os, "O_NONBLOCK", 0)
            fd = os.open(target.name, flags, dir_fd=dir_fd)
            try:
                if not stat.S_ISREG(os.fstat(fd).st_mode):
                    return None, True
                bound_fd = fd
                fd = None
                return _scan_fd_lines_bounded(bound_fd, max_needed_line)
            finally:
                if fd is not None:
                    os.close(fd)
    except (OSError, ValueError):
        return None, True


def _resolve_repo_file_lines(repo, raw_path, cache, max_needed_line=None):
    repo_root = Path(repo).resolve()
    candidates = [raw_path]
    unquoted = unquote(raw_path)
    if unquoted != raw_path:
        candidates.append(unquoted)
    for cand in candidates:
        rel = _repo_relative_candidate(repo, cand)
        if rel is None:
            continue
        key = rel.as_posix()
        cached = cache.get(key)
        need_scan = cached is None or (
            cached[0] is not None and not cached[1] and (max_needed_line is None or max_needed_line > cached[0])
        )
        if need_scan:
            cache[key] = _repo_file_line_count(repo_root, rel, max_needed_line=max_needed_line)
        line_count = cache[key][0]
        if line_count is not None:
            return key, line_count
    return None, None


def is_verified_repo_file(repo, raw_path, line_num=None):
    """Return canonical repo-relative POSIX path if raw_path is a real text regular file (and line_num is in bounds)."""
    rel_key, line_count = _resolve_repo_file_lines(repo, raw_path, {}, max_needed_line=line_num)
    if rel_key is None:
        return None
    if line_num is not None and (line_num < 1 or line_num > line_count):
        return None
    return rel_key


def analyze_review_message(repo, text):
    """Extract verdict and repo-verified file:line citations (path + in-bounds line) from a review pass output."""
    verdict = extract_verdict(text)
    raw_citations = extract_file_line_citations(text)
    max_line_by_raw = {}
    for raw_path, line_num in raw_citations:
        if line_num > max_line_by_raw.get(raw_path, 0):
            max_line_by_raw[raw_path] = line_num
    verified = []
    seen = set()
    cache = {}
    for raw_path, line_num in raw_citations:
        rel, line_count = _resolve_repo_file_lines(repo, raw_path, cache, max_needed_line=max_line_by_raw[raw_path])
        if rel is not None and 1 <= line_num <= line_count:
            key = (rel, line_num)
            if key not in seen:
                seen.add(key)
                verified.append(f"{rel}:{line_num}")
    return {
        "verdict": verdict,
        "cited_count": len(raw_citations),
        "verified_citations": verified,
        "verified_citations_count": len(verified),
    }


def parse_numstat_z_tokens(tokens, start_idx=0):
    """Parse one numstat -z entry starting at tokens[start_idx]; return (added_str, deleted_str, paths, next_idx)."""
    tok = os.fsdecode(tokens[start_idx])
    if "\t" not in tok:
        return None, None, [], start_idx + 1
    added, deleted, fpath = tok.split("\t", 2)
    added = added.lstrip("\n")
    if fpath == "" and start_idx + 2 < len(tokens):
        return added, deleted, [os.fsdecode(tokens[start_idx + 1]), os.fsdecode(tokens[start_idx + 2])], start_idx + 3
    return added, deleted, [fpath], start_idx + 1


def resolve_git_commit(repo, ref):
    if not ref or not str(ref).strip():
        raise ValueError("Commit ref must not be empty")
    try:
        out = subprocess.check_output(
            ["git", "-C", str(repo), "rev-parse", "--verify", "--end-of-options", f"{ref}^{{commit}}"],
            text=True,
            stderr=subprocess.DEVNULL,
        ).strip()
    except (OSError, subprocess.SubprocessError) as exc:
        raise ValueError(f"Unknown git commit ref: {ref}") from exc
    return out


def _count_untracked_file_lines(repo_root, rel_path):
    rel = _repo_relative_candidate(repo_root, rel_path)
    if rel is None or is_test_path(rel.as_posix()):
        return 0
    target = repo_root / rel
    try:
        with open_private_dir_fd(repo_root, target.parent, create=False) as dir_fd:
            if dir_fd is None:
                return 0
            flags = os.O_RDONLY | getattr(os, "O_NOFOLLOW", 0) | getattr(os, "O_NONBLOCK", 0)
            fd = os.open(target.name, flags, dir_fd=dir_fd)
            try:
                st = os.fstat(fd)
                if not stat.S_ISREG(st.st_mode) or st.st_size > MAX_UNTRACKED_LOC_BYTES:
                    return 0
                with os.fdopen(fd, "rb") as fh:
                    fd = None
                    data = fh.read(MAX_UNTRACKED_LOC_BYTES + 1)
            finally:
                if fd is not None:
                    os.close(fd)
    except (OSError, ValueError):
        return 0
    if not data or len(data) > MAX_UNTRACKED_LOC_BYTES or b"\x00" in data:
        return 0
    return data.count(b"\n") + (0 if data.endswith(b"\n") else 1)


def _count_untracked_non_test_loc(repo_root):
    try:
        untracked_raw = subprocess.check_output(
            ["git", "-C", str(repo_root), "ls-files", "--others", "--exclude-standard", "-z"],
            stderr=subprocess.DEVNULL,
        )
    except (OSError, subprocess.SubprocessError):
        return 0
    return sum(_count_untracked_file_lines(repo_root, os.fsdecode(item)) for item in untracked_raw.split(b"\0") if item)


def detect_diff_non_test_loc(repo, base):
    """Measure rename-aware non-test LOC changed against an explicit base ref plus untracked files."""
    repo_root = Path(repo).resolve()
    ref = resolve_git_commit(repo_root, base)
    try:
        raw = subprocess.check_output(
            ["git", "-C", str(repo_root), "diff", "--numstat", "-z", "--find-renames", ref, "--"],
            stderr=subprocess.DEVNULL,
        )
    except (OSError, subprocess.SubprocessError):
        return None
    total = 0
    tokens = raw.split(b"\x00")
    idx = 0
    while idx < len(tokens):
        if not tokens[idx]:
            idx += 1
            continue
        added, deleted, paths, idx = parse_numstat_z_tokens(tokens, idx)
        if added and deleted and added.isdigit() and deleted.isdigit() and paths and not all(is_test_path(p) for p in paths):
            total += int(added) + int(deleted)
    total += _count_untracked_non_test_loc(repo_root)
    return total


def validate_resume_lock(args, saved_model, saved_effort):
    if args.allow_effort_change:
        return
    if args.effort and args.effort != saved_effort:
        raise ValueError(
            f"Refusing effort drift on resume ({saved_effort} -> {args.effort}): changing effort "
            f"invalidates Sol prompt cache. Omit --effort to reuse {saved_effort}, start a fresh "
            "session for tier escalation, or pass --allow-effort-change explicitly."
        )
    if args.model and args.model != saved_model:
        raise ValueError(
            f"Refusing model drift on resume ({saved_model} -> {args.model}). "
            "Start a fresh session or pass --allow-effort-change."
        )


def resolve_resume_settings(args, manifest):
    if manifest is None:
        if not args.effort:
            raise ValueError(
                "No saved manifest for this session; pass explicit --effort (e.g. xhigh or max) "
                "so resume never silently falls back to default effort and busts prompt cache."
            )
        model = args.model or DEFAULT_MODEL
        return model, args.effort, args.scope or "rust", compact_limit_for(model, args.compact_limit), None

    saved_model = manifest.get("model") or DEFAULT_MODEL
    saved_effort = manifest["effort"]
    validate_resume_lock(args, saved_model, saved_effort)
    compact = args.compact_limit if args.compact_limit is not None else manifest.get("compact_limit")
    return args.model or saved_model, args.effort or saved_effort, args.scope or manifest.get("scope") or "rust", compact, manifest


def emit_resume_warnings(manifest):
    if not manifest:
        return None
    idle_minutes = None
    last_done = parse_iso(manifest.get("last_finished_at"))
    if last_done is not None:
        idle_minutes = round((utc_now() - last_done).total_seconds() / 60.0, 1)
        if idle_minutes > IDLE_CACHE_WARN_MINUTES:
            print(
                f"[sol-review] WARNING: {idle_minutes}m idle since previous pass "
                f"(>{IDLE_CACHE_WARN_MINUTES:.0f}m); prompt cache may have evicted — consider starting fresh.",
                file=sys.stderr,
            )
    pass_count = len(manifest.get("passes") or [])
    if pass_count >= PASS_COUNT_WARN:
        print(
            f"[sol-review] WARNING: session already has {pass_count} passes; "
            "consider escalating to a fresh session instead of accumulating stale context.",
            file=sys.stderr,
        )
    return idle_minutes


def run_codex_turn(argv, repo, prompt_text, env, events_path, stderr_path, timeout):
    started = time.monotonic()
    process = None
    timed_out = False
    try:
        with open_private_write(repo, events_path) as events_f, open_private_write(repo, stderr_path) as err_f:
            process = subprocess.Popen(
                argv,
                cwd=repo,
                stdin=subprocess.PIPE,
                stdout=events_f,
                stderr=err_f,
                text=True,
                start_new_session=True,
                env=env,
            )
            try:
                process.communicate(prompt_text, timeout=timeout)
                rc = process.returncode
            except subprocess.TimeoutExpired:
                timed_out = True
                rc = 124
            return rc, round(time.monotonic() - started, 2), timed_out
    finally:
        if process is not None and process.poll() is None:
            try:
                os.killpg(process.pid, signal.SIGKILL)
            except (ProcessLookupError, PermissionError):
                pass
            process.wait()


def format_receipt(session_id, pass_num, effort, scope, wall_s, pass_usage, usage_scope, idle_minutes):
    idle_str = f"{idle_minutes:.1f}m" if idle_minutes is not None else "0.0m"
    if pass_usage is None:
        return (
            f"[sol-review] session={session_id} pass={pass_num} effort={effort} scope={scope} "
            f"wall={wall_s:.1f}s usage=unknown idle={idle_str}"
        )
    inp = pass_usage["input_tokens"]
    cached = pass_usage["cached_input_tokens"]
    uncached = max(0, inp - cached)
    hit_pct = (100.0 * cached / inp) if inp > 0 else 0.0
    span_tag = " [aggregate_includes_unmeasured_prior_pass]" if usage_scope == "aggregate" else ""
    return (
        f"[sol-review] session={session_id} pass={pass_num} effort={effort} scope={scope} "
        f"wall={wall_s:.1f}s in={inp} cached={cached} ({hit_pct:.1f}%) "
        f"unc={uncached} out={pass_usage['output_tokens']} "
        f"reas={pass_usage['reasoning_output_tokens']} idle={idle_str}{span_tag}"
    )


def prepare_review_plan(mode, args, repo):
    if mode == "start":
        model = args.model or DEFAULT_MODEL
        header = "" if args.no_hygiene_header else START_HYGIENE_HEADER
        if args.diff_non_test_loc is not None:
            if args.diff_non_test_loc < 0:
                raise ValueError("--diff-non-test-loc must be a non-negative integer")
            diff_loc = args.diff_non_test_loc
        elif args.base is not None:
            diff_loc = detect_diff_non_test_loc(repo, base=args.base)
        else:
            diff_loc = None
        return {
            "model": model,
            "effort": args.effort,
            "scope": args.scope,
            "compact": compact_limit_for(model, args.compact_limit),
            "manifest": None,
            "session_id": None,
            "idle_minutes": None,
            "pass_num": 1,
            "header": header,
            "hygiene_header": not args.no_hygiene_header,
            "diff_non_test_loc": diff_loc,
        }
    session_id = str(uuid.UUID(args.session_id))
    manifest = load_manifest(repo, session_id)
    model, effort, scope, compact, manifest = resolve_resume_settings(args, manifest)
    idle_minutes = emit_resume_warnings(manifest)
    header = "" if args.no_hygiene_header else RESUME_HYGIENE_HEADER
    return {
        "model": model,
        "effort": effort,
        "scope": scope,
        "compact": compact,
        "manifest": manifest,
        "session_id": session_id,
        "idle_minutes": idle_minutes,
        "pass_num": len((manifest or {}).get("passes") or []) + 1,
        "header": header,
        "hygiene_header": not args.no_hygiene_header,
        "diff_non_test_loc": (manifest or {}).get("diff_non_test_loc"),
    }


def build_usage_fields(pass_usage, usage_scope, cumulative):
    if pass_usage is None:
        return {
            "usage_available": False,
            "usage_scope": "unknown",
            "input_tokens": None,
            "cached_input_tokens": None,
            "uncached_tokens": None,
            "cache_hit_pct": None,
            "output_tokens": None,
            "reasoning_output_tokens": None,
            "cumulative_usage": None,
        }
    inp = pass_usage["input_tokens"]
    cached = pass_usage["cached_input_tokens"]
    return {
        "usage_available": True,
        "usage_scope": usage_scope,
        "input_tokens": inp,
        "cached_input_tokens": cached,
        "uncached_tokens": max(0, inp - cached),
        "cache_hit_pct": round(100.0 * cached / inp, 2) if inp > 0 else 0.0,
        "output_tokens": pass_usage["output_tokens"],
        "reasoning_output_tokens": pass_usage["reasoning_output_tokens"],
        "cumulative_usage": cumulative,
    }


def read_last_message_text(repo, last_msg_path):
    target = Path(last_msg_path)
    with open_private_dir_fd(repo, target.parent, create=False) as dir_fd:
        if dir_fd is None:
            return None
        return read_private_regular_file_at(dir_fd, target.name, chmod_0600=True)


def record_pass_result(repo, resolved_id, plan, outcome):
    prior_passes = (plan["manifest"] or {}).get("passes") or []
    cumulative = outcome["totals"]
    pass_usage, usage_scope = compute_pass_usage(cumulative, prior_passes)
    msg_analysis = analyze_review_message(repo, outcome.get("message_text") or "")
    entry = {
        "session_id": resolved_id,
        "model": plan["model"],
        "effort": plan["effort"],
        "scope": plan["scope"],
        "pass": plan["pass_num"],
        "started_at": outcome["started_at"],
        "finished_at": outcome["finished_at"],
        "idle_minutes": plan["idle_minutes"],
        "wall_seconds": outcome["wall_s"],
        "hygiene_header": plan.get("hygiene_header", True),
        **build_usage_fields(pass_usage, usage_scope, cumulative),
        "verdict": msg_analysis["verdict"],
        "cited_count": msg_analysis["cited_count"],
        "verified_citations_count": msg_analysis["verified_citations_count"],
        "inspection_actions": outcome.get("inspection_actions"),
        "exit_code": outcome["exit_code"],
        "timed_out": outcome["timed_out"],
        "completed_turns": outcome["completed"],
        "failed": outcome["failed"],
        "events_path": str(outcome["events_path"]),
        "stderr_path": str(outcome["stderr_path"]),
        "message_path": str(outcome["last_msg_path"]),
    }
    outcome_path = Path(outcome["events_path"]).parent / "outcome.json"
    save_json_atomic(repo, outcome_path, entry)
    if not resolved_id:
        return pass_usage, usage_scope, outcome_path

    record = plan["manifest"] or {
        "session_id": resolved_id,
        "repo": str(repo),
        "model": plan["model"],
        "effort": plan["effort"],
        "scope": plan["scope"],
        "compact_limit": plan["compact"],
        "diff_non_test_loc": plan.get("diff_non_test_loc"),
        "created_at": outcome["started_at"],
        "passes": [],
    }
    record.update(
        {
            "model": plan["model"],
            "effort": plan["effort"],
            "scope": plan["scope"],
            "compact_limit": plan["compact"],
            "last_finished_at": outcome["finished_at"],
        }
    )
    if record.get("diff_non_test_loc") is None and plan.get("diff_non_test_loc") is not None:
        record["diff_non_test_loc"] = plan["diff_non_test_loc"]
    record["passes"].append(entry)
    save_manifest(repo, resolved_id, record)
    return pass_usage, usage_scope, outcome_path


def emit_last_message(repo, last_msg_path, preloaded_text=None):
    text = preloaded_text if preloaded_text is not None else read_last_message_text(repo, last_msg_path)
    if not text:
        return
    sys.stdout.write(text)
    if not text.endswith("\n"):
        sys.stdout.write("\n")


def allocate_invocation_paths(repo, plan, create_dirs):
    stamp = datetime.now(timezone.utc).strftime("%Y%m%dT%H%M%SZ")
    nonce = uuid.uuid4().hex[:12]
    inv_dir = state_dir_for(repo) / f"{stamp}-p{plan['pass_num']}-{nonce}"
    events_path = inv_dir / "events.jsonl"
    stderr_path = inv_dir / "stderr.txt"
    last_msg_path = inv_dir / "message.txt"
    if create_dirs:
        with open_private_dir_fd(repo, inv_dir, create=True) as inv_fd:
            fd = open_private_file_at(inv_fd, "message.txt", os.O_WRONLY | os.O_CREAT | os.O_TRUNC)
            os.close(fd)
    return events_path, stderr_path, last_msg_path


def print_dry_run(mode, repo, plan, argv, env, no_hygiene_header):
    env_keys = ("HINDSIGHT_DISABLED", "HINDSIGHT_DISABLE_HOOKS", "AGENT_KIT_MEMORY_ACCESS")
    env_overrides = {k: env[k] for k in env_keys}
    if "CODEX_HOME" in env:
        env_overrides["CODEX_HOME"] = env["CODEX_HOME"]
    print(
        json.dumps(
            {
                "mode": mode,
                "repo": str(repo),
                "session_id": plan["session_id"],
                "model": plan["model"],
                "effort": plan["effort"],
                "scope": plan["scope"],
                "compact_limit": plan["compact"],
                "diff_non_test_loc": plan.get("diff_non_test_loc"),
                "argv": argv,
                "env_overrides": env_overrides,
                "hygiene_header_applied": not no_hygiene_header,
            },
            indent=2,
        )
    )


def run_inline_watchdog(repo, session_id, outcome_path, codex_home):
    """Run the deterministic zero-LLM efficiency & quality check on the session (stderr-only)."""
    if not session_id and not outcome_path:
        return
    try:
        import kit_efficiency_watchdog as watchdog

        watchdog.check_session_inline(repo, session_id, outcome_path=outcome_path, codex_home=codex_home)
    except Exception as exc:  # pragma: no cover - watchdog warnings must never mask a review verdict
        print(f"[efficiency-watchdog] WARNING: inline check skipped ({exc})", file=sys.stderr)


def execute_review(mode, args):
    repo = find_repo_root(args.repo)
    raw_prompt = read_prompt_source(args.prompt)
    if not raw_prompt.strip():
        raise ValueError("Review prompt is empty")

    lock_id = str(uuid.UUID(args.session_id)) if mode == "resume" else None
    lock_ctx = session_lock(repo, lock_id) if not args.dry_run else session_lock(repo, None)
    with lock_ctx:
        plan = prepare_review_plan(mode, args, repo)
        events_path, stderr_path, last_msg_path = allocate_invocation_paths(
            repo, plan, create_dirs=not args.dry_run
        )
        argv = build_argv(
            mode=mode,
            repo=repo,
            model=plan["model"],
            effort=plan["effort"],
            scope=plan["scope"],
            compact_limit=plan["compact"],
            last_message_path=last_msg_path,
            session_id=plan["session_id"],
            codex_bin=args.codex_bin,
            codex_home=args.codex_home,
        )
        env = review_env(codex_home=args.codex_home)
        if args.dry_run:
            print_dry_run(mode, repo, plan, argv, env, args.no_hygiene_header)
            return 0

        started_at = iso_now_precise()
        exit_code, wall_s, timed_out = run_codex_turn(
            argv, repo, plan["header"] + raw_prompt, env, events_path, stderr_path, args.timeout
        )
        ids, totals, completed, failed, inspection_actions = parse_events_jsonl(events_path)
        resolved_id = ids[0] if ids else plan["session_id"]
        msg_text = read_last_message_text(repo, last_msg_path)
        pass_usage, usage_scope, outcome_path = record_pass_result(
            repo,
            resolved_id,
            plan,
            {
                "started_at": started_at,
                "finished_at": iso_now_precise(),
                "wall_s": wall_s,
                "totals": totals,
                "exit_code": exit_code,
                "timed_out": timed_out,
                "completed": completed,
                "failed": failed,
                "inspection_actions": inspection_actions,
                "events_path": events_path,
                "stderr_path": stderr_path,
                "last_msg_path": last_msg_path,
                "message_text": msg_text,
            },
        )
        emit_last_message(repo, last_msg_path, preloaded_text=msg_text)
        print(
            format_receipt(
                resolved_id or "unknown",
                plan["pass_num"],
                plan["effort"],
                plan["scope"],
                wall_s,
                pass_usage,
                usage_scope,
                plan["idle_minutes"],
            ),
            file=sys.stderr,
        )
        run_inline_watchdog(repo, resolved_id, outcome_path, args.codex_home)
        if timed_out or exit_code != 0 or failed or completed != 1:
            print(
                f"[sol-review] ERROR: exit={exit_code} timed_out={timed_out} failed={failed} "
                f"completed={completed}; stderr={stderr_path}",
                file=sys.stderr,
            )
            return exit_code or 1
        return 0


def cmd_status(args):
    repo = find_repo_root(args.repo)
    session_id = str(uuid.UUID(args.session_id))
    manifest = load_manifest(repo, session_id)
    if manifest is None:
        print(f"No saved sol-review manifest for {session_id}", file=sys.stderr)
        return 1
    print(json.dumps(manifest, indent=2))
    return 0


def _validate_outcome_args(args, session_id):
    for label, value in (
        ("--confirmed", args.confirmed),
        ("--false-positives", args.false_positives),
        ("--regressions", args.regressions),
    ):
        if value < 0:
            raise ValueError(f"{label} must be a non-negative integer")
    prior_id = None
    if args.prior_session:
        prior_id = str(uuid.UUID(args.prior_session))
        if prior_id == session_id:
            raise ValueError("--prior-session must differ from session_id")
        if args.escaped is None:
            raise ValueError("--prior-session requires --escaped <N> (confirmed findings that existed at the prior reviewed revision)")
    elif args.escaped is not None:
        raise ValueError("--escaped requires --prior-session")
    if args.escaped is not None and (args.escaped < 0 or args.escaped > args.confirmed):
        raise ValueError("--escaped must be between 0 and --confirmed")
    notes = (args.notes or "").strip()
    if len(notes) > MAX_OUTCOME_NOTES_CHARS:
        raise ValueError(f"--notes must be at most {MAX_OUTCOME_NOTES_CHARS} characters")
    return prior_id, args.escaped, notes


def cmd_outcome(args):
    repo = find_repo_root(args.repo)
    session_id = str(uuid.UUID(args.session_id))
    prior_id, escaped_from_prior, notes = _validate_outcome_args(args, session_id)
    reviewed_commit = resolve_git_commit(repo, args.commit) if args.commit else None

    with session_lock(repo, session_id):
        manifest = load_manifest(repo, session_id)
        if manifest is None:
            raise ValueError(f"No saved sol-review manifest for {session_id}")
        judged = args.confirmed + args.false_positives
        precision_pct = round(100.0 * args.confirmed / judged, 2) if judged > 0 else None
        outcome = {
            "recorded_at": iso_now(),
            "confirmed_findings": args.confirmed,
            "false_positives": args.false_positives,
            "repair_regressions": args.regressions,
            "precision_pct": precision_pct,
            "prior_session_id": prior_id,
            "escaped_from_prior": escaped_from_prior,
            "reviewed_commit": reviewed_commit,
            "notes": notes,
        }
        manifest["outcome"] = outcome
        save_manifest(repo, session_id, manifest)
    print(json.dumps({"session_id": session_id, "outcome": outcome}, indent=2))
    run_inline_watchdog(repo, session_id, None, args.codex_home)
    return 0


def add_common_flags(parser):
    parser.add_argument("--repo", help="Repository root (defaults to git toplevel)")
    parser.add_argument("--model", help=f"Codex model (default: {DEFAULT_MODEL})")
    parser.add_argument("--compact-limit", type=int, help="Override model_auto_compact_token_limit")
    parser.add_argument("--timeout", type=int, default=DEFAULT_TIMEOUT, help="Timeout in seconds")
    parser.add_argument("--codex-bin", default="codex", help="Codex executable")
    parser.add_argument("--codex-home", help="Override CODEX_HOME for both discovery and Codex execution")
    parser.add_argument("--no-hygiene-header", action="store_true", help="Do not prepend hygiene header")
    parser.add_argument("--dry-run", action="store_true", help="Print resolved command without executing")


def build_parser():
    parser = argparse.ArgumentParser(description="Cache-safe read-only Sol review wrapper")
    sub = parser.add_subparsers(dest="command", required=True)

    start_p = sub.add_parser("start", help="Start a fresh read-only Sol review session")
    start_p.add_argument("--effort", required=True, choices=VALID_EFFORTS)
    start_p.add_argument("--scope", default="rust", choices=sorted(SCOPE_ALLOWED_MCPS))
    start_p.add_argument("--base", help="Base git ref for non-test diff LOC measurement")
    start_p.add_argument("--diff-non-test-loc", type=int, help="Explicit non-test diff LOC (overrides auto-detect)")
    add_common_flags(start_p)
    start_p.add_argument("prompt", nargs="?", default="-", help="Prompt file path or '-' for stdin")

    resume_p = sub.add_parser("resume", help="Resume an existing read-only Sol review session")
    resume_p.add_argument("session_id", help="Codex session UUID returned by start")
    resume_p.add_argument("--effort", choices=VALID_EFFORTS, help="Must match saved effort unless --allow-effort-change")
    resume_p.add_argument("--scope", choices=sorted(SCOPE_ALLOWED_MCPS))
    resume_p.add_argument("--allow-effort-change", action="store_true", help="Allow changing effort/model (busts cache)")
    add_common_flags(resume_p)
    resume_p.add_argument("prompt", nargs="?", default="-", help="Prompt file path or '-' for stdin")

    status_p = sub.add_parser("status", help="Show saved manifest and telemetry for a review session")
    status_p.add_argument("session_id")
    status_p.add_argument("--repo")

    outcome_p = sub.add_parser("outcome", help="Record verified lead quality outcome for a review session")
    outcome_p.add_argument("session_id", help="Codex session UUID")
    outcome_p.add_argument("--confirmed", type=int, required=True, help="Count of confirmed real findings")
    outcome_p.add_argument("--false-positives", type=int, required=True, help="Count of rejected false positives")
    outcome_p.add_argument("--regressions", type=int, default=0, help="Count of repair regressions introduced")
    outcome_p.add_argument("--prior-session", help="Prior Tier-1 session UUID if this was an escalation/final gate")
    outcome_p.add_argument("--escaped", type=int, help="Confirmed findings that existed at the prior reviewed revision (required with --prior-session)")
    outcome_p.add_argument("--commit", help="Git commit ref covered by this review session")
    outcome_p.add_argument("--notes", default="", help="Optional short notes (<=1000 chars)")
    outcome_p.add_argument("--repo", help="Repository root (defaults to git toplevel)")
    outcome_p.add_argument("--codex-home", help="Override CODEX_HOME for inline watchdog check")
    return parser


def main(argv=None):
    parser = build_parser()
    args = parser.parse_args(argv)
    try:
        if args.command == "status":
            return cmd_status(args)
        if args.command == "outcome":
            return cmd_outcome(args)
        return execute_review(args.command, args)
    except ValueError as exc:
        print(f"[sol-review] ERROR: {exc}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
