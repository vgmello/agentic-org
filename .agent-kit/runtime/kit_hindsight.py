#!/usr/bin/env python3
"""Managed local Hindsight for semantic discovery of approved Git records.

Git remains authoritative for this adapter's reviewed-record bank. Its chunk-only
retention and retrieval do not call an LLM. A separate native coding-agent bank
uses the owner-approved Codex provider on the same local embed daemon.
"""

from contextlib import contextmanager
import json
import os
from pathlib import Path
import re
import subprocess
import time
from urllib.error import HTTPError, URLError
from urllib.parse import quote, urlsplit
from urllib.request import build_opener, HTTPRedirectHandler, ProxyHandler, Request
import uuid

from kit_config import digest
from kit_context import safe, write
from kit_evidence import git, timestamp

CONFIG = ".agent-kit/memory-tools.json"
POLICY = ".agent-kit/policy/hindsight"
LOG = ".agent-kit/logs/memory/hindsight"
VERSION = "1.2.1"
MISSION = "Starter kit reviewed Git records v1. Git records and their source evidence remain authoritative."
BANK_CONFIG = {"retain_extraction_mode": "chunks",
               "enable_observations": False, "enable_auto_consolidation": False,
               "enable_graph_retrieval": False, "enable_temporal_retrieval": False}
REVIEW_ENV = "AGENT_KIT_MEMORY_ACCESS"


def load_config(repo, configured=True):
    path = repo / CONFIG
    safe(path)
    c = json.loads(path.read_text())
    if (not isinstance(c, dict) or set(c) != {"schema_version", "backend", "profile", "enabled",
            "project_id", "endpoint", "max_tokens", "timeout_seconds"} or
            type(c["schema_version"]) is not int or c["schema_version"] != 1 or
            c["backend"] != "hindsight" or c["profile"] != "reviewed-git-records" or
            type(c["enabled"]) is not bool or type(c["max_tokens"]) is not int or not 256 <= c["max_tokens"] <= 2000 or
            type(c["timeout_seconds"]) is not int or not 1 <= c["timeout_seconds"] <= 60):
        raise ValueError("Invalid memory-tools.json; use the fixed reviewed-records profile")
    if c["project_id"] is not None and not re.fullmatch(r"[0-9a-f]{32}", str(c["project_id"])):
        raise ValueError("Invalid memory project identity; rerun memory setup")
    if c["endpoint"] is not None:
        url = urlsplit(c["endpoint"])
        if (url.scheme != "http" or url.hostname != "127.0.0.1" or not url.port or url.username or
                url.password or url.path or url.query or url.fragment):
            raise ValueError("Hindsight must use its managed loopback endpoint")
    if configured and (c["project_id"] is None or c["endpoint"] is None):
        raise ValueError("Hindsight setup pending: run kit_memory.py setup")
    return c


def policy_identity(repo):
    folder = repo / POLICY
    files = ("runtime.json", "embed.json")
    hashes = {}
    for name in files:
        safe(folder / name)
        hashes[name] = digest((folder / name).read_bytes())
    runtime = json.loads((folder / "runtime.json").read_text())
    if runtime["version"] != VERSION:
        raise ValueError("Hindsight adapter/runtime versions differ; reinstall the kit")
    return digest(json.dumps(hashes, sort_keys=True).encode()), runtime


def project_identity(repo):
    remote = git(repo, "config", "--get", "remote.origin.url", optional=True)
    if remote:
        value = remote.decode().strip()
        if "://" not in value and re.match(r"[^/]+@[^:]+:", value):
            value = "ssh://" + value.replace(":", "/", 1)
        url = urlsplit(value)
        if url.hostname:
            # Never persist credentials from a remote URL. HTTPS and SSH clones
            # of the same repository deliberately share the project bank.
            identity = url.hostname.lower() + "/" + url.path.strip("/").removesuffix(".git")
        else:
            identity = str((repo / value).resolve())
    else:
        common = git(repo, "rev-parse", "--git-common-dir").decode().strip()
        identity = str((repo / common).resolve())
    return digest(identity.encode())[:32]


def review_access(phase="implementation"):
    if phase not in {"implementation", "review"}:
        raise ValueError("Memory phase must be implementation or review")
    return phase == "review" or os.environ.get(REVIEW_ENV) == "review"


def require_writer():
    if review_access():
        raise ValueError("Review sessions may only read governing memory; setup/sync are lead operations")


class NoRedirect(HTTPRedirectHandler):
    def redirect_request(self, *args, **kwargs):
        raise ValueError("Memory service redirects are refused")


class Client:
    def __init__(self, config, repo=None):
        self.config = config
        self.repo = repo
        self.bank = "kit-" + config["project_id"]
        self.requests = 0
        self.received_bytes = 0
        self.input_characters = 0
        self.verified_no_llm = False
        self.opener = build_opener(ProxyHandler({}), NoRedirect())

    def request(self, method, path, value=None, missing_ok=False):
        body = None if value is None else json.dumps(value).encode()
        self.input_characters += len(body.decode()) if body else 0
        self.requests += 1
        request = Request(self.config["endpoint"] + path, data=body, method=method,
                          headers={"Content-Type": "application/json"})
        try:
            with self.opener.open(request, timeout=self.config["timeout_seconds"]) as response:
                raw = response.read(2_000_001)
        except HTTPError as exc:
            if exc.code == 404 and missing_ok:
                return None
            raise ValueError(f"Hindsight HTTP {exc.code}; run memory status/setup and inspect the service") from exc
        except (URLError, TimeoutError, OSError) as exc:
            raise ValueError("Hindsight is unavailable; run kit_memory.py setup to recover the managed service") from exc
        self.received_bytes += len(raw)
        if len(raw) > 2_000_000:
            raise ValueError("Hindsight response exceeded the bounded transport limit")
        return json.loads(raw) if raw else {}

    def path(self, suffix="", bank=None):
        return "/v1/default/banks/" + quote(bank or self.bank, safe="") + suffix

    def verify(self):
        verify_service(self.repo, self.config)
        version = self.request("GET", "/version")
        if version.get("api_version") != VERSION:
            raise ValueError("Hindsight version/profile drift; rerun setup with the pinned service")
        value = self.request("GET", self.path("/config"))
        config = value.get("config", {})
        if config.get("retain_mission") != MISSION or any(config.get(k) != v for k, v in BANK_CONFIG.items()):
            raise ValueError("Hindsight reviewed-record bank configuration drift; preserve its chunk-only profile")
        self.verified_no_llm = True
        return version


@contextmanager
def measured(repo, operation, client):
    event = {"version": 1, "operation": operation, "started_at": timestamp(),
             "project_id": client.config["project_id"], "status": "FAILED"}
    started = time.monotonic()
    try:
        yield event
        event["status"] = "COMPLETED"
    except (ValueError, OSError, subprocess.SubprocessError) as exc:
        event["error"] = str(exc)
        raise
    finally:
        event.update(duration_seconds=round(time.monotonic() - started, 4), requests=client.requests,
                     transport_bytes=client.received_bytes, request_characters=client.input_characters,
                     llm_input_tokens=0 if client.verified_no_llm else None,
                     llm_output_tokens=0 if client.verified_no_llm else None,
                     cost_usd=None, note="Local CPU/storage costs and agent consumption are not measured here.")
        write(repo / LOG / "events" / (uuid.uuid4().hex + ".json"), event)


def provision(repo):
    from kit_hindsight_embed import provision as provision_embed
    checksum, runtime = policy_identity(repo)
    return provision_embed(repo, checksum, runtime)


def verify_service(repo, config):
    from kit_hindsight_embed import verify as verify_embed
    checksum, runtime = policy_identity(repo)
    verify_embed(repo, config, checksum, runtime)


def configured(repo, check_project=True):
    c = load_config(repo)
    if check_project and c["project_id"] != project_identity(repo):
        raise ValueError("Memory configuration belongs to another project; rerun setup here")
    safe(repo / LOG / "setup.json")
    setup = json.loads((repo / LOG / "setup.json").read_text())
    if (setup.get("status") != "HINDSIGHT_READY" or setup.get("root") != str(repo) or
            setup.get("config_sha256") != digest((repo / CONFIG).read_bytes()) or
            setup.get("policy_sha256") != policy_identity(repo)[0] or
            setup.get("probe") != "retain-recall-delete-passed"):
        raise ValueError("Hindsight setup evidence is stale/incomplete; rerun kit_memory.py setup")
    return c, setup


def ensure_bank(client, bank):
    path = client.path(bank=bank)
    existing = client.request("GET", path + "/config", missing_ok=True)
    if existing is None:
        client.request("PUT", path, {"name": "Starter kit reviewed memory", "retain_mission": MISSION,
            **{k: v for k, v in BANK_CONFIG.items() if k != "enable_auto_consolidation"}})
    elif existing.get("config", {}).get("retain_mission") != MISSION:
        raise ValueError("Existing bank is not owned by the starter-kit profile; preserve it and resolve identity")


def retain(client, bank, document_id, content, kind):
    response = client.request("POST", client.path("/memories", bank), {"async": False, "items": [{
        "content": content, "document_id": document_id, "tags": ["kit:accepted", "kind:" + kind],
        "metadata": {"document_id": document_id, "profile": "reviewed-git-records"},
        "context": MISSION}]})
    if response.get("success") is not True or response.get("async") is not False or response.get("items_count") != 1:
        raise ValueError("Hindsight did not confirm synchronous retention")
    usage = response.get("usage")
    if usage and any(usage.get(k, 0) for k in ("input_tokens", "output_tokens", "total_tokens", "thoughts_tokens")):
        raise ValueError("Unexpected LLM consumption in the no-LLM memory profile")


def provider_recall(client, query, review=False, bank=None):
    groups = [{"tags": ["kit:accepted"], "match": "all_strict"}]
    if review:
        groups.append({"tags": ["kind:decision", "kind:map"], "match": "any_strict"})
    result = client.request("POST", client.path("/memories/recall", bank), {
        "query": query, "types": ["world", "experience"], "budget": "low",
        "max_tokens": client.config["max_tokens"], "tag_groups": groups,
        "include": {"entities": None, "chunks": None, "source_facts": None}})
    if not isinstance(result, dict) or not isinstance(result.get("results"), list):
        raise ValueError("Malformed Hindsight recall response")
    return result["results"]


def probe(client):
    """Exercise actual storage/retrieval in a disposable bank, never project data."""
    bank = client.bank + "-probe-" + uuid.uuid4().hex[:12]
    try:
        ensure_bank(client, bank)
        retain(client, bank, "kit-probe", "Invoice reconciliation uses integer cents to prevent floating-point rounding errors.", "decision")
        results = provider_recall(client, "Why are monetary amounts stored as integer minor units?", bank=bank)
        if not any(item.get("document_id") == "kit-probe" for item in results):
            raise ValueError("Hindsight probe returned no matching retained evidence")
    finally:
        client.request("DELETE", client.path(bank=bank), missing_ok=True)
    return "retain-recall-delete-passed"


def health(repo):
    require_writer()
    c, _ = configured(repo)
    if not c["enabled"]:
        raise ValueError("Hindsight is disabled; the fixed memory setup is incomplete")
    client = Client(c, repo)
    with measured(repo, "health-probe", client):
        client.request("GET", "/health/ready")
        client.verify()
        return {"status": "PASSED", "probe": probe(client), "service_llm_provider": "openai-codex", "profile_model_calls": 0}


def mcp_probe(repo):
    from kit_context import run
    path = repo / ".agent-kit/context-mcp.json"
    safe(path)
    entry = json.loads(path.read_text())
    requests = [
        {"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {"protocolVersion": "2024-11-05", "capabilities": {}, "clientInfo": {"name": "memory-health", "version": "1"}}},
        {"jsonrpc": "2.0", "id": 2, "method": "tools/list"},
        {"jsonrpc": "2.0", "id": 3, "method": "tools/call", "params": {"name": "recall", "arguments": {
            "repo_root": str(repo), "query": "Governing project constraints", "paths": ["."], "phase": "review"}}}]
    result = run(repo, [entry["command"], *entry["args"]], 120,
                 "\n".join(map(json.dumps, requests)) + "\n")
    replies = [json.loads(line) for line in Path(result["stdout"]).read_text().splitlines()]
    if (len(replies) != 3 or [r.get("id") for r in replies] != [1, 2, 3] or
            any("error" in r for r in replies) or
            [t["name"] for t in replies[1]["result"]["tools"]] != ["lookup", "recall"]):
        raise ValueError("Memory MCP handshake failed; rerun kit_context.py setup")
    response = replies[2]["result"]
    text = response["content"][0]["text"]
    if response.get("isError") or len(text) + 1 > 4000:
        raise ValueError("Memory MCP retrieval failed or exceeded its budget; inspect " + result["stdout"])
    value = json.loads(text)
    if value.get("backend") != "hindsight" or value.get("status") != "CURRENT":
        raise ValueError("Memory MCP is degraded/disabled; inspect " + result["stdout"])
    return {"mcp_roundtrip": "passed", "protocol_log": result["stdout"], "returned_characters": len(text) + 1}


def setup(repo):
    require_writer()
    c = load_config(repo, configured=False)
    started = time.monotonic()
    receipt = {"version": 1, "root": str(repo), "created_at": timestamp(), "status": "BLOCKED"}
    try:
        service = provision(repo)
        c.update(project_id=project_identity(repo), endpoint=service["endpoint"])
        write(repo / CONFIG, c)
        receipt.update(service, config_sha256=digest((repo / CONFIG).read_bytes()))
        client = Client(c, repo)
        deadline = time.monotonic() + 300
        while True:
            try:
                client.request("GET", "/health/ready")
                break
            except ValueError:
                if time.monotonic() >= deadline:
                    raise ValueError("Hindsight readiness timed out; inspect the local daemon logs and rerun setup")
                time.sleep(1)
        ensure_bank(client, client.bank)
        client.verify()
        with measured(repo, "setup-probe", client):
            probe(client)
        receipt.update(status="HINDSIGHT_READY", probe="retain-recall-delete-passed",
                       service_llm_provider="openai-codex", memory_profile="reviewed-git-records",
                       profile_model_calls=0, storage="embedded-pg0", efficiency_validated=False)
        write(repo / LOG / "setup.json", receipt)
        receipt["sync"] = sync(repo)
        receipt["setup_duration_seconds"] = round(time.monotonic() - started, 4)
        write(repo / LOG / "setup.json", receipt)
        return receipt
    except (OSError, ValueError, subprocess.SubprocessError) as exc:
        receipt.update(status="BLOCKED", error=str(exc), setup_duration_seconds=round(time.monotonic() - started, 4))
        write(repo / LOG / "setup.json", receipt)
        raise


def records(repo, paths=None, phase="implementation"):
    """Only committed, unchanged accepted records enter the external derived store."""
    from kit_skills import relative
    scopes = [relative(p) for p in paths] if paths else None
    result = {}
    omitted = 0
    for path in sorted((repo / ".agent-kit/knowledge").glob("*.json")):
        safe(path)
        if path.stat().st_size > 64_000:
            raise ValueError("Split oversized memory records before syncing")
        raw = path.read_bytes()
        item = json.loads(raw)
        if item.get("status") != "ACCEPTED":
            continue
        if (item.get("version") != 1 or item.get("kind") not in {"decision", "map", "learning"} or
                any(not isinstance(item.get(k), str) or not item[k].strip() for k in
                    ("id", "title", "summary", "rationale", "reviewed_by", "review_rationale")) or
                not isinstance(item.get("sources"), list) or not 1 <= len(item["sources"]) <= 20):
            raise ValueError(f"Malformed accepted memory record: {path.name}")
        for source in item["sources"]:
            relative(source["path"])
            if not re.fullmatch(r"[0-9a-f]{64}", source["sha256"]):
                raise ValueError(f"Invalid source evidence in {path.name}")
        if review_access(phase) and item["kind"] not in {"decision", "map"}:
            continue
        if scopes and not any(Path(s["path"]).is_relative_to(p) or Path(p).is_relative_to(s["path"])
                              for s in item["sources"] for p in scopes):
            continue
        name = path.relative_to(repo).as_posix()
        if git(repo, "show", "HEAD:" + name, optional=True) != raw:
            omitted += 1
            continue
        doc_id = "kit-" + digest(raw)
        result[doc_id] = {"path": name, "record": item, "sha256": digest(raw)}
    return result, omitted


def document(item):
    record = item["record"]
    return (f"Approved {record['kind']}: {record['title']}\n\n{record['summary']}\n\n"
            f"Rationale: {record['rationale']}\n\nSources: " + ", ".join(s["path"] for s in record["sources"]))


def sync(repo):
    require_writer()
    c, _ = configured(repo)
    client = Client(c, repo)
    with measured(repo, "sync", client) as event:
        client.verify()
        accepted, omitted = records(repo)
        saved, reused = 0, 0
        for doc_id, item in accepted.items():
            content = document(item)
            existing = client.request("GET", client.path("/documents/" + doc_id), missing_ok=True)
            if existing is not None:
                if existing.get("original_text") != content or existing.get("memory_unit_count", 0) < 1:
                    raise ValueError("Indexed memory differs from its content identity; preserve evidence and investigate")
                reused += 1
                continue
            if saved >= 100:
                break
            retain(client, client.bank, doc_id, content, item["record"]["kind"])
            saved += 1
        event.update(synced_records=saved, reused_records=reused, uncommitted_records=omitted,
                     remaining_records=len(accepted) - saved - reused)
        return {k: event[k] for k in ("synced_records", "reused_records", "uncommitted_records", "remaining_records")}


def recall(repo, query, paths, max_chars=4000, phase="implementation"):
    from kit_memory import compact, currency, excerpt
    if not isinstance(query, str) or not query.strip() or len(query) > 2000 or not paths:
        raise ValueError("Semantic recall needs a 1–2000 character query and explicit --path (use . deliberately)")
    if not 1000 <= max_chars <= 32000:
        raise ValueError("Recall budget must be 1000–32000 characters")
    c, _ = configured(repo)
    client = Client(c, repo)
    with measured(repo, "recall", client) as event:
        event.update(query_sha256=digest(query.encode()), phase="review" if review_access(phase) else "implementation")
        client.verify()
        accepted, uncommitted = records(repo, paths, phase)
        found = provider_recall(client, query, review_access(phase))
        result = {"backend": "hindsight", "status": "CURRENT", "records": [], "discarded_results": 0,
                  "uncommitted_records": uncommitted, "omitted_results": 0,
                  "note": "Discovery excerpts from current committed Git records. Read full records and active decisions; revalidate NEEDS_REVIEW evidence. Earlier review learnings are excluded in review mode."}
        seen, cache = set(), {}
        for hit in found:
            doc_id = hit.get("document_id") if isinstance(hit, dict) else None
            if doc_id not in accepted:
                result["discarded_results"] += 1
                continue
            if doc_id in seen:
                continue
            seen.add(doc_id)
            item = accepted[doc_id]
            record = item["record"]
            changed = currency(repo, record, cache)
            # Provider text never enters agent context: resolve the returned ID
            # to the current approved record in this exact checkout instead.
            candidate = {"record": item["path"], "kind": record["kind"], "title": excerpt(record["title"], 160),
                         "summary": excerpt(record["summary"], 800), "record_sha256": item["sha256"],
                         "currency": "NEEDS_REVIEW" if changed else "SOURCE_UNCHANGED", "excerpted": True}
            result["records"].append(candidate)
            if len(compact(result)) + 1 > max_chars:
                result["records"].pop()
                result["omitted_results"] += 1
        # Counts can grow after the last append; preserve the complete-envelope cap.
        while len(compact(result)) + 1 > max_chars and result["records"]:
            result["records"].pop(); result["omitted_results"] += 1
        if len(compact(result)) + 1 > max_chars:
            raise ValueError("Memory response metadata exceeds its budget")
        event.update(returned_records=len(result["records"]), returned_characters=len(compact(result)) + 1,
                     discarded_results=result["discarded_results"])
        return result


def status(repo, live=True):
    c, setup = configured(repo)
    if live:
        Client(c, repo).verify()
    return {"status": "HINDSIGHT_READY", "enabled": c["enabled"], "version": VERSION,
            "storage": "embedded-pg0", "service_llm_provider": "openai-codex",
            "profile_model_calls": 0, "service": setup["profile"],
            "live_checked": live, "efficiency_validated": False}


def usage(repo):
    events = []
    for path in sorted((repo / LOG / "events").glob("*.json")):
        safe(path)
        value = json.loads(path.read_text())
        events.append(value)
    return {"operations": len(events), "failed_operations": sum(e["status"] != "COMPLETED" for e in events),
            "duration_seconds": round(sum(e["duration_seconds"] for e in events), 4),
            "returned_characters": sum(e.get("returned_characters", 0) for e in events),
            "llm_input_tokens": sum(e["llm_input_tokens"] for e in events) if all(e["llm_input_tokens"] is not None for e in events) else None,
            "llm_output_tokens": sum(e["llm_output_tokens"] for e in events) if all(e["llm_output_tokens"] is not None for e in events) else None,
            "cost_usd": None, "evidence_directory": LOG + "/events",
            "note": "Memory-operation subset only. Include agent usage, cold setup, CPU/storage and failures in whole-task comparisons; fewer returned characters do not prove savings."}
