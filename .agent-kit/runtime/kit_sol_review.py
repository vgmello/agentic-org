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
import uuid

DEFAULT_MODEL = "gpt-6.2-sol"
DEFAULT_COMPACT_LIMIT = 390_000
DEFAULT_TIMEOUT = 600
IDLE_CACHE_WARN_MINUTES = 55.0
PASS_COUNT_WARN = 4

VALID_EFFORTS = ("low", "medium", "high", "xhigh", "max")
BARE_TOML_KEY = re.compile(r"^[A-Za-z0-9_-]+$")
USAGE_KEYS = ("input_tokens", "cached_input_tokens", "output_tokens", "reasoning_output_tokens")

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
    "Apply `.agents/skills/caveman/SKILL.md` (delegate-only).\n\n"
)

RESUME_HYGIENE_HEADER = (
    "Review harness hygiene (read-only resume): "
    "Do NOT edit, patch, or commit files. Inspect only the reported delta and affected callers; "
    "skip `backlog`, Hindsight, and `cargo` unless explicitly requested below; "
    "use `yield_time_ms` >= 15000 on shell calls.\n\n"
)


def utc_now():
    return datetime.now(timezone.utc)


def iso_now():
    return utc_now().strftime("%Y-%m-%dT%H:%M:%SZ")


def parse_iso(value):
    if not value:
        return None
    return datetime.strptime(value, "%Y-%m-%dT%H:%M:%SZ").replace(tzinfo=timezone.utc)


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


def parse_events_jsonl(events_path):
    session_ids = []
    totals = {k: 0 for k in USAGE_KEYS}
    completed = 0
    has_usage = False
    failed = False
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
        elif kind in {"turn.failed", "error"}:
            failed = True
    return session_ids, (totals if has_usage else None), completed, failed


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


def record_pass_result(repo, resolved_id, plan, outcome):
    prior_passes = (plan["manifest"] or {}).get("passes") or []
    cumulative = outcome["totals"]
    pass_usage, usage_scope = compute_pass_usage(cumulative, prior_passes)
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
        **build_usage_fields(pass_usage, usage_scope, cumulative),
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
    record["passes"].append(entry)
    save_manifest(repo, resolved_id, record)
    return pass_usage, usage_scope, outcome_path


def emit_last_message(repo, last_msg_path):
    target = Path(last_msg_path)
    with open_private_dir_fd(repo, target.parent, create=False) as dir_fd:
        if dir_fd is None:
            return
        text = read_private_regular_file_at(dir_fd, target.name, chmod_0600=True)
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
                "argv": argv,
                "env_overrides": env_overrides,
                "hygiene_header_applied": not no_hygiene_header,
            },
            indent=2,
        )
    )


def run_inline_watchdog(repo, session_id, outcome_path, codex_home):
    """Run the deterministic zero-LLM efficiency check on the just-completed pass (stderr-only)."""
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

        started_at = iso_now()
        exit_code, wall_s, timed_out = run_codex_turn(
            argv, repo, plan["header"] + raw_prompt, env, events_path, stderr_path, args.timeout
        )
        ids, totals, completed, failed = parse_events_jsonl(events_path)
        resolved_id = ids[0] if ids else plan["session_id"]
        pass_usage, usage_scope, outcome_path = record_pass_result(
            repo,
            resolved_id,
            plan,
            {
                "started_at": started_at,
                "finished_at": iso_now(),
                "wall_s": wall_s,
                "totals": totals,
                "exit_code": exit_code,
                "timed_out": timed_out,
                "completed": completed,
                "failed": failed,
                "events_path": events_path,
                "stderr_path": stderr_path,
                "last_msg_path": last_msg_path,
            },
        )
        emit_last_message(repo, last_msg_path)
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
    return parser


def main(argv=None):
    parser = build_parser()
    args = parser.parse_args(argv)
    try:
        if args.command == "status":
            return cmd_status(args)
        return execute_review(args.command, args)
    except ValueError as exc:
        print(f"[sol-review] ERROR: {exc}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
