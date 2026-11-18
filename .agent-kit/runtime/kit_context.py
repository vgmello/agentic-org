#!/usr/bin/env python3
"""One bounded, checkout-bound retrieval interface for Claude and Codex.

Uses the fixed Repowise runtime, never implements a graph or calls a model. Provider
refresh is evidence of a completed refresh, not proof of semantic completeness.
"""

import argparse
from contextlib import contextmanager
import fcntl
import json
import os
from pathlib import Path
import re
import shutil
import signal
import subprocess
import sys
import tempfile
import time
import uuid

from kit_evidence import git, snapshot, timestamp
from kit_config import protected_name, installed_mode

PINS = {"repowise": "0.61.0"}
INDEX = {"repowise": ".repowise"}
DEFAULT_EXECUTABLE = f".agent-kit/tools/repowise-{PINS['repowise']}/bin/repowise"
DEFAULT_REASON = "Fixed stack: Repowise for bounded structural retrieval and repository history; shared by Claude and Codex."
SERVER = "starter_kit_context"
BEGIN, END = "# BEGIN STARTER KIT CONTEXT", "# END STARTER KIT CONTEXT"
# Resolve the actual launch checkout, including linked worktrees; no baked-in
# path to the original checkout survives copying these configs to another branch.
LAUNCHER = ("import pathlib,runpy,subprocess,sys; "
            "p=pathlib.Path(subprocess.check_output(['git','rev-parse','--show-toplevel'],text=True).strip()); "
            "sys.argv.extend(['--repo',str(p)]); "
            "sys.path.insert(0,str(p/'.agent-kit/runtime')); "
            "runpy.run_path(str(p/'.agent-kit/runtime/kit_context.py'),run_name='__main__')")


def safe(path):
    if any(p.is_symlink() for p in (path, *path.parents)):
        raise ValueError(f"Linked context state/config path: {path}")


def write(path, value):
    safe(path)
    path.parent.mkdir(parents=True, exist_ok=True)
    data = value if isinstance(value, str) else json.dumps(value, indent=2) + "\n"
    fd, tmp = tempfile.mkstemp(dir=path.parent, prefix=".context-")
    try:
        with os.fdopen(fd, "w") as f:
            f.write(data)
        os.replace(tmp, path)
    finally:
        if os.path.exists(tmp):
            os.unlink(tmp)


def root(path):
    actual = Path(os.fsdecode(git(path, "rev-parse", "--show-toplevel")).strip()).resolve()
    if path.resolve() != actual:
        raise ValueError("--repo must name the exact Git checkout root")
    return actual


def load_config(repo):
    safe(repo / ".agent-kit/context-tools.json")
    c = json.loads((repo / ".agent-kit/context-tools.json").read_text())
    required = {"schema_version", "profile", "backend", "executable", "expected_version", "max_chars", "timeout_seconds", "selection_reason"}
    if (not isinstance(c, dict) or set(c) != required or type(c["schema_version"]) is not int or
            c["schema_version"] != 1 or c["profile"] != "large-project" or
            c["backend"] not in {*PINS, "unconfigured"} or
            any(not isinstance(c[k], str) for k in ("executable", "expected_version", "selection_reason")) or
            not c["selection_reason"].strip() or type(c["max_chars"]) is not int or not 1000 <= c["max_chars"] <= 64000 or
            type(c["timeout_seconds"]) is not int or not 1 <= c["timeout_seconds"] <= 3600):
        raise ValueError("Invalid context-tools.json")
    if c["backend"] in PINS and (not c["executable"] or c["expected_version"] != PINS[c["backend"]]):
        raise ValueError("Use the tested provider version or revalidate and update the adapter pins")
    return c


def configure(repo, backend, executable, reason, limits=None):
    if backend != "repowise":
        raise ValueError("The supported stack uses Repowise only")
    if not reason.strip():
        raise ValueError("Record why this backend fits the project")
    c = {"schema_version": 1, "profile": "large-project", "backend": backend,
         "executable": executable or DEFAULT_EXECUTABLE,
         "expected_version": PINS.get(backend, ""), "max_chars": 12000,
         "timeout_seconds": 600, "selection_reason": reason}
    if limits:
        c.update({k: limits[k] for k in ("max_chars", "timeout_seconds")})
    mcp_path, codex_path = repo / ".mcp.json", repo / ".codex/config.toml"
    hybrid = installed_mode(repo) == "hybrid"
    mcp = json.loads(mcp_path.read_text()) if hybrid and mcp_path.exists() else {}
    if not isinstance(mcp, dict) or not isinstance(mcp.get("mcpServers", {}), dict):
        raise ValueError("Invalid existing .mcp.json; preserve and repair it first")
    entry = {"command": "python3", "args": ["-c", LAUNCHER, "serve"]}
    servers = mcp.setdefault("mcpServers", {})
    legacy_entry = {"command": "python3", "args": ["-c", LAUNCHER.replace(".agent-kit/runtime", ".claude/hooks"), "serve"]}
    if SERVER in servers and servers[SERVER] not in (entry, legacy_entry):
        raise ValueError("Existing starter_kit_context server differs; reconcile it before configuring")
    servers[SERVER] = entry
    toml = codex_path.read_text() if codex_path.exists() else ""
    if BEGIN in toml or END in toml:
        if toml.count(BEGIN) != 1 or toml.count(END) != 1 or toml.index(BEGIN) > toml.index(END):
            raise ValueError("Malformed managed Codex context block")
        toml = toml[:toml.index(BEGIN)] + toml[toml.index(END) + len(END):]
    if re.search(r"\[mcp_servers\.(?:starter_kit_context|\"starter_kit_context\"|'starter_kit_context')\]", toml):
        raise ValueError("An unmanaged starter_kit_context Codex entry already exists")
    block = (f"{BEGIN}\n[mcp_servers.{SERVER}]\ncommand = \"python3\"\n"
             f"args = {json.dumps(entry['args'])}\nenabled_tools = [\"lookup\", \"recall\"]\n"
             'env_vars = ["AGENT_KIT_MEMORY_ACCESS", "AGENT_KIT_MEMORY_BACKEND"]\n'
             f"tool_timeout_sec = {2 * c['timeout_seconds'] + 60}\n{END}\n")
    ignore = repo / ".gitignore"
    lines = ignore.read_text().splitlines() if ignore.exists() else []
    for pattern in ("/.repowise/", "/.agent-kit/tools/"):
        if pattern not in lines:
            lines.append(pattern)
    changes = {repo / ".agent-kit/context-tools.json": json.dumps(c, indent=2) + "\n",
               repo / ".agent-kit/context-mcp.json": json.dumps(entry, indent=2) + "\n", codex_path: toml.rstrip() + "\n\n" + block,
               ignore: "\n".join(lines) + "\n"}
    if hybrid:
        changes[mcp_path] = json.dumps(mcp, indent=2) + "\n"
    for path in changes:
        safe(path)
    for path, data in changes.items():
        if path.exists() and path.read_text() == data:
            continue
        if path.exists():
            backup = repo / ".agent-kit/.starter-kit-backups" / uuid.uuid4().hex / path.relative_to(repo)
            write(backup, path.read_text())
        write(path, data)
    return {"configured": backend, "mcp_server": SERVER, "restart_required": True,
            "next": "Run prepare, then test lookup in each actual harness; no live model call was made."}


def environment(repo):
    return {**os.environ, "REPOWISE_OFFLINE": "1", "REPOWISE_SKIP_EDITOR_SETUP": "1",
            "REPOWISE_NO_SAVE_KEY": "1", "REPOWISE_NO_HINTS": "1",
            "REPOWISE_CONFIG_DIR": str(repo / ".agent-kit/logs/context/provider"),
            "DO_NOT_TRACK": "1", "NO_COLOR": "1"}


@contextmanager
def lock(repo):
    path = repo / ".agent-kit/logs/context/index.lock"
    safe(path)
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open("a+") as f:
        try:
            fcntl.flock(f, fcntl.LOCK_EX | fcntl.LOCK_NB)
        except BlockingIOError as exc:
            raise ValueError("Another context request owns this checkout; retry after it finishes") from exc
        try:
            yield
        finally:
            fcntl.flock(f, fcntl.LOCK_UN)


def run(repo, argv, timeout, input_text=None):
    folder = repo / ".agent-kit/logs/context" / uuid.uuid4().hex
    safe(folder)
    folder.mkdir(parents=True, mode=0o700)
    stdout, stderr = folder / "stdout.txt", folder / "stderr.txt"
    started = time.monotonic()
    proc, reason = None, "error"
    try:
        with stdout.open("wb") as out, stderr.open("wb") as err, tempfile.TemporaryFile() as input_file:
            os.chmod(stdout, 0o600); os.chmod(stderr, 0o600)
            if input_text is not None:
                input_file.write(input_text.encode()); input_file.seek(0)
            proc = subprocess.Popen(argv, cwd=repo, env=environment(repo), stdin=input_file if input_text is not None else subprocess.DEVNULL,
                                    stdout=out, stderr=err, start_new_session=True)
            try:
                proc.wait(timeout=timeout)
            except (subprocess.TimeoutExpired, KeyboardInterrupt) as exc:
                reason = "timeout" if isinstance(exc, subprocess.TimeoutExpired) else "interrupted"
                os.killpg(proc.pid, signal.SIGKILL); proc.wait()
                raise ValueError(f"Context command timed out/interrupted; inspect {folder}") from exc
        reason = "completed" if proc.returncode == 0 else "failed"
    finally:
        record = {"argv": argv, "exit_code": proc.returncode if proc else None,
                  "duration_seconds": round(time.monotonic() - started, 3), "termination_reason": reason,
                  "stdout": str(stdout), "stderr": str(stderr),
                  "stdout_bytes": stdout.stat().st_size if stdout.exists() else None}
        write(folder / "execution.json", record)
    if proc.returncode:
        raise ValueError(f"Context command exited {proc.returncode}; inspect {folder}; use source search until resolved")
    return record


def tool(repo, c):
    candidate = Path(c["executable"])
    if "/" in c["executable"] and not candidate.is_absolute():
        candidate = repo / candidate
    exe = shutil.which(str(candidate))
    if not exe:
        raise ValueError(f"Install {c['backend']} {c['expected_version']} and configure its executable")
    p = subprocess.run([exe, "--version"], env=environment(repo), cwd=repo,
                       capture_output=True, text=True, timeout=10, check=True)
    if not re.search(r"(?<![\d.])" + re.escape(c["expected_version"]) + r"(?![\d.\w-])", p.stdout):
        raise ValueError("Indexer version differs from the tested pin; revalidate before upgrading")
    return exe


def refresh(repo, c, exe, adopt_existing=False, initialize=False):
    folder = repo / INDEX[c["backend"]]
    safe(folder)
    if git(repo, "ls-files", "--", INDEX[c["backend"]]):
        raise ValueError("Derived index must not be tracked")
    if git(repo, "check-ignore", str(folder / ".probe"), optional=True) is None:
        raise ValueError("Ignore the derived index before preparing it; run configure")
    owner = repo / ".agent-kit/logs/context/index-owner.json"
    safe(owner)
    identity = {"root": str(repo), "git_dir": os.fsdecode(git(repo, "rev-parse", "--absolute-git-dir")).strip(),
                "backend": c["backend"], "version": c["expected_version"]}
    if owner.exists() and json.loads(owner.read_text()) != identity:
        raise ValueError("Index ownership differs from this checkout/backend; inspect before reconfiguring")
    if not owner.exists() and folder.exists() and not adopt_existing:
        raise ValueError("Existing unmanaged index: validate it, then run prepare --adopt-existing")
    if not folder.exists() and not initialize:
        raise ValueError("Index missing; run prepare before starting agent work")
    state_path = owner.with_name("index-state.json")
    safe(state_path)
    before = snapshot(repo)["fingerprint"]
    if owner.exists() and folder.exists() and state_path.exists():
        state = json.loads(state_path.read_text())
        if state.get("fingerprint") == before and state.get("identity") == identity:
            return [], before, "Reused completed refresh: tracked and nonignored source contents, index and HEAD match"
    # A failed update may have changed part of the index. Its previous success
    # must not become reusable if the user subsequently reverts their source.
    state_path.unlink(missing_ok=True)
    records = []
    if not folder.exists():
        args = ["init", str(repo), "--yes", "--no-prose", "--mode", "fast", "--no-editor-setup",
                 "--no-save-key", "--no-agents", "--no-claude-md", "--no-hook", "--no-distill-hook",
                 "--no-codex", "--no-workspace", "--progress", "json"]
        records.append(run(repo, [exe, *args], c["timeout_seconds"]))
    args = ["update", str(repo), "--no-workspace", "--index-only", "--working-tree", "--progress", "json"]
    records.append(run(repo, [exe, *args], c["timeout_seconds"]))
    events = [json.loads(line) for line in Path(records[-1]["stdout"]).read_text().splitlines() if line.strip()]
    done = [e for e in events if e.get("event") == "done"]
    if len(done) != 1 or done[0].get("ok") is not True or done[0].get("degraded"):
        raise ValueError("Repowise refresh was incomplete/degraded; inspect its saved output")
    if snapshot(repo)["fingerprint"] != before:
        raise ValueError("Checkout changed during index refresh; retry after edits settle")
    write(owner, identity)
    write(state_path, {"fingerprint": before, "identity": identity, "refreshed_at": timestamp()})
    return records, before, "Provider working-tree refresh completed for the current source fingerprint"


def lookup(repo, kind, query, expected_root):
    if Path(expected_root).resolve() != repo:
        raise ValueError("Requested worktree differs from the server's checkout")
    if kind not in {"search", "context", "symbol", "why"} or not isinstance(query, str) or not query.strip() or len(query) > 2000 or query.startswith("-") or "\x00" in query:
        raise ValueError("Use a supported lookup kind and a bounded, non-option query")
    if kind in {"context", "symbol"} and (Path(query).is_absolute() or ".." in Path(query).parts or
                                         ".git" in Path(query).parts or any(protected_name(p) for p in Path(query).parts)):
        raise ValueError("Context/symbol targets must be non-secret repository-relative paths or symbol names")
    c = load_config(repo)
    if c["backend"] not in PINS:
        raise ValueError("Index is not configured; use bounded rg/source reads and record this coverage gap")
    with lock(repo):
        exe = tool(repo, c)
        records, fingerprint, freshness = refresh(repo, c, exe)
        args = (["search", query, str(repo), "--mode", "auto", "--limit", "6"] if kind == "search"
                else [kind, query])
        args += ["--format", "json", "--no-workspace"]
        record = run(repo, [exe, *args], c["timeout_seconds"])
        if snapshot(repo)["fingerprint"] != fingerprint:
            raise ValueError("Checkout changed during retrieval; discard the result and retry")
        with Path(record["stdout"]).open(errors="replace") as f:
            text = f.read(c["max_chars"] + 1)
        truncated = len(text) > c["max_chars"]
        result = {"repo_root": str(repo), "head": os.fsdecode(git(repo, "rev-parse", "HEAD")).strip(),
                  "backend": c["backend"], "version": c["expected_version"], "kind": kind,
                  "freshness": freshness, "source_fingerprint": fingerprint,
                  "content": text[:c["max_chars"]], "truncated": truncated,
                  "raw_output": record["stdout"], "returned_characters": min(len(text), c["max_chars"]),
                  "duration_seconds": round(sum(r["duration_seconds"] for r in [*records, record]), 3),
                  "coverage": "Static graph is partial; verify relevant current source. Empty results do not prove no callers.",
                  "cache_note": "This is shared retrieval, not shared model context. Each fresh session receives its own evidence."}
        write(Path(record["stdout"]).parent / "lookup.json", result)
        return result


def serve(repo):
    """Bounded code/memory retrieval only; no shell/edit/approval or retain tools."""
    schema = {"type": "object", "properties": {"repo_root": {"type": "string"},
              "kind": {"type": "string", "enum": ["search", "context", "symbol", "why"]},
              "query": {"type": "string", "maxLength": 2000}},
              "required": ["repo_root", "kind", "query"], "additionalProperties": False}
    memory_schema = {"type": "object", "properties": {"repo_root": {"type": "string"},
        "query": {"type": "string", "minLength": 1, "maxLength": 2000},
        "paths": {"type": "array", "items": {"type": "string"}, "minItems": 1, "maxItems": 32},
        "phase": {"type": "string", "enum": ["implementation", "review"]}},
        "required": ["repo_root", "query", "paths", "phase"], "additionalProperties": False}
    for line in sys.stdin:
        request = {}
        try:
            if len(line) > 100_000:
                raise ValueError("Oversized MCP request")
            request = json.loads(line)
            if not isinstance(request, dict):
                raise ValueError("MCP request must be an object")
            if "id" not in request:
                continue
            method, params = request.get("method"), request.get("params", {})
            if request.get("jsonrpc") != "2.0" or not isinstance(method, str) or not isinstance(params, dict):
                raise ValueError("Invalid MCP request shape")
            if method == "initialize":
                result = {"protocolVersion": "2024-11-05",
                          "capabilities": {"tools": {}}, "serverInfo": {"name": SERVER, "version": "1.1.0"},
                          "instructions": "Use lookup for code and recall for reviewed Git memory. Supply the actual checkout root; fresh reviews use phase=review. Only derived index/logs are written; no source or project memory edits."}
            elif method == "ping":
                result = {}
            elif method == "tools/list":
                result = {"tools": [
                    {"name": "lookup", "description": "Query this checkout's refreshed code index. Find source, callers, structure or recorded rationale without scanning whole files.", "inputSchema": schema},
                    {"name": "recall", "description": "Discover scoped, reviewed Git memory through local Hindsight. Bounded to 4000 characters; no memory writes. Read full governing records before relying on excerpts.", "inputSchema": memory_schema}]}
            elif method == "tools/call" and params.get("name") in {"lookup", "recall"}:
                a = params.get("arguments", {})
                selected = schema if params["name"] == "lookup" else memory_schema
                if not isinstance(a, dict) or set(a) != set(selected["required"]):
                    raise ValueError("Retrieval arguments do not match the selected tool's schema")
                try:
                    if params["name"] == "lookup":
                        value = lookup(repo, a["kind"], a["query"], a["repo_root"])
                        text = json.dumps(value)
                    else:
                        if Path(a["repo_root"]).resolve() != repo:
                            raise ValueError("Requested worktree differs from the server's checkout")
                        from kit_memory import compact, discover
                        text = compact(discover(repo, a["query"], a["paths"], phase=a["phase"]))
                    result = {"content": [{"type": "text", "text": text}], "isError": False}
                except (OSError, ValueError, TypeError, KeyError, subprocess.SubprocessError) as exc:
                    result = {"content": [{"type": "text", "text": str(exc)}], "isError": True}
            else:
                raise ValueError("Unsupported MCP method/tool")
            response = {"jsonrpc": "2.0", "id": request["id"], "result": result}
        except (OSError, ValueError, TypeError, KeyError, subprocess.SubprocessError) as exc:
            response = {"jsonrpc": "2.0", "id": request.get("id") if isinstance(request, dict) else None,
                        "error": {"code": -32602, "message": str(exc)}}
        print(json.dumps(response), flush=True)


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument("action", choices=("setup", "configure", "prepare", "query", "serve", "status"))
    p.add_argument("--repo", type=Path, default=Path.cwd())
    p.add_argument("--backend", choices=tuple(PINS), default="repowise")
    p.add_argument("--executable")
    p.add_argument("--reason", default="")
    p.add_argument("--adopt-existing", action="store_true")
    p.add_argument("--kind", choices=("search", "context", "symbol", "why"), default="search")
    p.add_argument("--query")
    p.add_argument("--expect-path", help="setup: known source file the lookup must find")
    a = p.parse_args()
    try:
        repo = root(a.repo)
        if a.action == "setup":
            from kit_context_setup import setup
            result = setup(repo, a.query, a.expect_path, a.executable, a.adopt_existing)
        elif a.action == "configure":
            result = configure(repo, a.backend, a.executable, a.reason or DEFAULT_REASON)
        elif a.action == "serve":
            serve(repo); return 0
        elif a.action == "query":
            result = lookup(repo, a.kind, a.query, str(repo))
        else:
            c = load_config(repo)
            if a.action == "status":
                result = {"config": c, "index_exists": c["backend"] in INDEX and (repo / INDEX[c["backend"]]).is_dir(),
                          "note": "Existence is not freshness, language coverage or proof of actual harness access"}
            else:
                if c["backend"] not in PINS:
                    raise ValueError("Run setup to provision the fixed Repowise runtime before preparing it")
                with lock(repo):
                    records, fingerprint, freshness = refresh(repo, c, tool(repo, c), a.adopt_existing, initialize=True)
                    result = {"refresh": records, "source_fingerprint": fingerprint, "freshness": freshness,
                              "repo_root": str(repo), "prepared_at": timestamp()}
        print(json.dumps(result, indent=2)); return 0
    except (OSError, ValueError, TypeError, KeyError, subprocess.SubprocessError) as exc:
        print(f"Context setup/query failed: {exc}", file=sys.stderr); return 1


if __name__ == "__main__":
    sys.exit(main())
