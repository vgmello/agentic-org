"""Provision the fixed Repowise stack and prove its configured MCP lookup.

No model calls or global profile edits. The adoption agent supplies one known
symbol and its source file; this is project evidence, not a tool-selection menu.
"""
import json
from pathlib import Path
import shutil
import subprocess
import sys

from kit_config import digest, protected_name
from kit_context import (DEFAULT_EXECUTABLE, DEFAULT_REASON, PINS, SERVER,
                         configure, load_config, lock, refresh, run, safe, tool, write)
from kit_evidence import git, timestamp


def index_python():
    for candidate in dict.fromkeys((sys.executable, shutil.which("python3.12"))):
        if not candidate:
            continue
        result = subprocess.run([candidate, "-c", "import sys;print('%d.%d'%sys.version_info[:2])"],
                                capture_output=True, text=True, timeout=10, check=True)
        if result.stdout.strip() == "3.12":
            return candidate
    raise ValueError("Install Python 3.12 with venv/pip support using the host's package workflow, then rerun setup")


def install_runtime(repo, config):
    destination = repo / Path(DEFAULT_EXECUTABLE).parents[1]
    safe(destination)
    requirements = repo / ".agents/agent-kit/policy/repowise-requirements.txt"
    safe(requirements)
    checksum = digest(requirements.read_bytes())
    marker = destination / ".starter-kit-runtime.json"
    safe(marker)
    identity = {"backend": "repowise", "version": PINS["repowise"], "root": str(repo)}
    previous = None
    if destination.exists():
        if not marker.is_file():
            raise ValueError("Unmanaged tool directory exists; inspect it before setup: " + str(destination))
        previous = json.loads(marker.read_text())
        if not isinstance(previous, dict) or previous.get("identity") != identity:
            raise ValueError("Tool environment belongs to a different checkout/version; rebuild it deliberately")
    if previous and previous.get("status") == "INSTALLED" and previous.get("requirements_sha256") == checksum:
        tool(repo, config)
        return {"managed": True, "reused": True, "requirements_sha256": checksum}
    python = index_python()
    write(marker, {"identity": identity, "status": "INSTALLING", "requirements_sha256": checksum})
    run(repo, [python, "-m", "venv", str(destination)], 120)
    interpreter = str(destination / "bin/python")
    run(repo, [interpreter, "-m", "pip", "install", "--disable-pip-version-check", "--no-input",
               "--require-hashes", "-r", str(requirements)], 900)
    run(repo, [interpreter, "-m", "pip", "check"], 60)
    freeze = run(repo, [interpreter, "-m", "pip", "freeze", "--all"], 60)
    tool(repo, config)
    write(marker, {"identity": identity, "status": "INSTALLED", "requirements_sha256": checksum,
                   "installed_at": timestamp(), "resolved_packages": freeze["stdout"]})
    return {"managed": True, "reused": False, "requirements_sha256": checksum,
            "resolved_packages": freeze["stdout"]}


def probe(repo, config, query, expected):
    requests = [
        {"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {
            "protocolVersion": "2024-11-05", "capabilities": {},
            "clientInfo": {"name": "starter-kit-setup", "version": "1"}}},
        {"jsonrpc": "2.0", "method": "notifications/initialized"},
        {"jsonrpc": "2.0", "id": 2, "method": "tools/list"},
        {"jsonrpc": "2.0", "id": 3, "method": "tools/call", "params": {
            "name": "lookup", "arguments": {"repo_root": str(repo), "kind": "search", "query": query}}},
    ]
    # Use the exact configured launcher, just as an MCP client would. Both
    # harness configs have the same entry; their trust/login still needs a pilot.
    entry = json.loads((repo / ".agents/agent-kit/context-mcp.json").read_text())
    result = run(repo, [entry["command"], *entry["args"]], 2 * config["timeout_seconds"] + 60,
                 "\n".join(map(json.dumps, requests)) + "\n")
    replies = [json.loads(line) for line in Path(result["stdout"]).read_text().splitlines()]
    if (len(replies) != 3 or [r.get("id") for r in replies] != [1, 2, 3] or
            any("error" in r for r in replies) or
            [t["name"] for t in replies[1]["result"]["tools"]] != ["lookup", "recall"]):
        raise ValueError("MCP initialization/tool-list failed; inspect " + result["stdout"])
    response = replies[2]["result"]
    if response.get("isError"):
        raise ValueError("MCP lookup failed: " + response["content"][0]["text"])
    value = json.loads(response["content"][0]["text"])
    if value["repo_root"] != str(repo) or value["backend"] != "repowise":
        raise ValueError("MCP lookup returned the wrong checkout/backend")
    # Inspect complete structured results even if the model-facing display was
    # truncated. Merely echoing the query is not a successful source lookup.
    payload = json.loads(Path(value["raw_output"]).read_text())
    if not isinstance(payload, dict) or not isinstance(payload.get("results"), list):
        raise ValueError("MCP lookup did not return structured search results")
    if not any(isinstance(item, dict) and item.get("path") == expected for item in payload["results"]):
        raise ValueError("Lookup did not locate the required source file; inspect the query/language coverage before claiming readiness")
    return {"query": query, "expected_source": expected, "source_fingerprint": value["source_fingerprint"],
            "mcp_roundtrip": "passed", "result": value["raw_output"], "protocol_log": result["stdout"]}


def setup(repo, query, expected, executable=None, adopt_existing=False):
    if not isinstance(query, str) or not query.strip() or len(query) > 2000 or query.startswith("-") or "\x00" in query:
        raise ValueError("setup requires --query naming a known project symbol")
    if not isinstance(expected, str) or not expected.strip():
        raise ValueError("setup requires --expect-path naming that symbol's source file")
    source = Path(expected)
    if source.is_absolute() or ".." in source.parts or ".git" in source.parts or any(protected_name(p) for p in source.parts):
        raise ValueError("Expected source must be a non-secret repository-relative file")
    safe(repo / source)
    if not (repo / source).is_file() or git(repo, "check-ignore", str(repo / source), optional=True) is not None:
        raise ValueError("Expected source must exist and must not be ignored")
    git(repo, "rev-parse", "--verify", "HEAD")
    receipt = repo / ".agents/agent-kit/logs/context/setup.json"
    record = {"status": "SETTING_UP", "backend": "repowise", "version": PINS["repowise"],
              "root": str(repo), "started_at": timestamp(), "live_harnesses_validated": False, "model_calls": 0}
    try:
        with lock(repo):
            config = load_config(repo)
            if git(repo, "ls-files", "--", ".agents/agent-kit/tools"):
                raise ValueError("Derived tool environments must not be tracked")
            configure(repo, "repowise", executable or DEFAULT_EXECUTABLE, DEFAULT_REASON, limits=config)
            config = load_config(repo)
            write(receipt, record)
            record["runtime"] = install_runtime(repo, config) if not executable else {"managed": False, "executable": tool(repo, config)}
            records, fingerprint, freshness = refresh(repo, config, tool(repo, config), adopt_existing, initialize=True)
            record["prepare"] = {"source_fingerprint": fingerprint, "freshness": freshness, "commands": records}
        record["probe"] = probe(repo, config, query, source.as_posix())
        record.update(status="CONTEXT_READY", finished_at=timestamp(),
                      config_sha256=digest((repo / ".agents/agent-kit/context-tools.json").read_bytes()))
        write(receipt, record)
        return {"status": record["status"], "backend": "repowise", "version": PINS["repowise"],
                "receipt": str(receipt), "source": source.as_posix(), "mcp_roundtrip": "passed",
                "next": "Restart/trust the project in the selected harness(es); prove actual lookup and complete project verification/adoption."}
    except (OSError, ValueError, TypeError, KeyError, subprocess.SubprocessError) as exc:
        record.update(status="BLOCKED", error=str(exc), finished_at=timestamp())
        write(receipt, record)
        raise
