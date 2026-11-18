#!/usr/bin/env python3
"""Evidence-backed memory proposals, lead promotion and bounded recall.

Meaning is assessed by agents. Local records are not authenticated approvals;
hashes detect source changes, not whether an architectural decision remains valid.
"""

import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys
import uuid

from kit_context import root, safe, write
from kit_evidence import git, timestamp
from kit_config import codex_instruction_file, protected_name


def evidence(repo, name):
    p = Path(name)
    if not name or p.is_absolute() or ".." in p.parts or ".git" in p.parts or any(protected_name(x) for x in p.parts):
        raise ValueError("Memory sources must be non-secret repository-relative files")
    file = repo / p
    safe(file)
    if git(repo, "check-ignore", str(file), optional=True) is not None:
        raise ValueError("Do not promote ignored artifacts or dependencies into shared memory")
    h = hashlib.sha256()
    with file.open("rb") as f:
        for chunk in iter(lambda: f.read(1024 * 1024), b""):
            h.update(chunk)
    return {"path": p.as_posix(), "sha256": h.hexdigest()}


def propose(repo, data):
    if not isinstance(data, dict) or set(data) != {"title", "kind", "summary", "rationale", "sources"}:
        raise ValueError("Proposal requires title, kind, summary, rationale and sources")
    if data["kind"] not in {"decision", "map", "learning"}:
        raise ValueError("Unsupported knowledge kind")
    for k in ("title", "summary", "rationale"):
        if not isinstance(data[k], str) or not data[k].strip() or len(data[k]) > 4000:
            raise ValueError(f"Proposal {k} must be nonempty and bounded")
    if not isinstance(data["sources"], list) or not 1 <= len(data["sources"]) <= 20 or any(not isinstance(s, str) for s in data["sources"]):
        raise ValueError("Supply 1–20 source file references")
    record = {**data, "version": 1, "id": uuid.uuid4().hex, "status": "PROPOSED", "root": str(repo),
              "created_at": timestamp(), "head": git(repo, "rev-parse", "HEAD").decode().strip(),
              "sources": [evidence(repo, s) for s in dict.fromkeys(data["sources"])]}
    path = repo / ".agent-kit/logs/memory/proposals" / (record["id"] + ".json")
    write(path, record)
    return {"proposal": str(path), "status": "PROPOSED", "next": "Lead reviews against intent and current source; no automatic promotion."}


def currency(repo, record, cache=None):
    cache = {} if cache is None else cache
    changed = []
    for source in record["sources"]:
        name = source["path"]
        if name not in cache:
            try:
                cache[name] = evidence(repo, name)["sha256"]
            except (OSError, ValueError):
                cache[name] = None
        if cache[name] != source["sha256"]:
            changed.append(name)
    return changed


def promote(repo, path, reviewer, rationale):
    safe(path)
    if path.resolve().parent != repo / ".agent-kit/logs/memory/proposals":
        raise ValueError("Promote a proposal from this checkout's proposal directory")
    record = json.loads(path.read_text())
    if record["root"] != str(repo) or record["status"] != "PROPOSED" or not reviewer.strip() or not rationale.strip():
        raise ValueError("Promotion requires the original checkout, lead session and review rationale")
    if currency(repo, record):
        raise ValueError("Proposal source changed; re-evaluate and capture a fresh proposal")
    destination = repo / ".agent-kit/knowledge" / (path.stem + ".json")
    if destination.exists():
        raise ValueError("Already promoted; update/supersede the existing record deliberately")
    record.update(status="ACCEPTED", reviewed_by=reviewer, review_rationale=rationale, reviewed_at=timestamp())
    # Accepted records are portable across checkouts; the source hashes retain
    # their evidence. The capture root remains provenance, not a recall filter.
    write(destination, record)
    return {"accepted_record": str(destination), "next": "Commit this record; curate governing decisions/maps and supersede conflicts. Local declaration, not identity proof."}


def compact(value):
    return json.dumps(value, ensure_ascii=False, separators=(",", ":"))


def excerpt(value, limit):
    return value if len(value) <= limit else value[:max(0, limit - 1)] + ("…" if limit else "")


def recall(repo, paths, max_chars, offset=0, phase="implementation", notice=None):
    from kit_hindsight import review_access
    if not paths:
        raise ValueError("Recall requires --path for the affected area; use --path . only for a deliberate repository-wide lookup")
    if not 1000 <= max_chars <= 32000 or offset < 0:
        raise ValueError("Recall budget must be 1000–32000 characters and offset nonnegative")
    for name in paths:
        if not name.strip() or Path(name).is_absolute() or ".." in Path(name).parts:
            raise ValueError("Recall scope must be repository-relative")
    candidates = []
    for file in sorted((repo / ".agent-kit/knowledge").glob("*.json")):
        safe(file)
        record = json.loads(file.read_text())
        if record["status"] != "ACCEPTED":
            continue
        if review_access(phase) and record["kind"] not in {"decision", "map"}:
            continue
        refs = [s["path"] for s in record["sources"]]
        distances = [abs(len(Path(ref).parts) - len(Path(p).parts)) for ref in refs for p in paths
                     if Path(ref).is_relative_to(p) or Path(p).is_relative_to(ref)]
        if not distances:
            continue
        candidates.append((file, record, min(distances)))
    # Stable ID breaks ties only. Governing decisions come first, then closer
    # source scopes, then the latest lead review. Recency never resolves conflicts.
    candidates.sort(key=lambda x: x[1].get("reviewed_at", ""), reverse=True)
    candidates.sort(key=lambda x: ({"decision": 0, "map": 1, "learning": 2}.get(x[1]["kind"], 3), x[2]))
    if offset > len(candidates):
        raise ValueError("Recall offset exceeds current matches; restart the scoped lookup")

    def page(items):
        end = offset + len(items)
        return {"backend": "local", "status": "DEGRADED" if notice else "CURRENT", "notice": notice,
                "records": items, "matched_records": len(candidates), "offset": offset,
                "next_offset": end if end < len(candidates) else None,
                "omitted_records": len(candidates) - len(items),
                "note": "Discovery excerpts only. Read relevant full records for rationale/constraints and active DECISIONS.md. NEEDS_REVIEW requires source revalidation. Narrow scope or use next_offset; restart pagination after memory edits."}

    def fits(items):
        # Include the JSON envelope, escaping and CLI's terminating newline.
        return len(compact(page(items))) + 1 <= max_chars

    results, cache = [], {}
    for file, record, _ in candidates[offset:]:
        changed = currency(repo, record, cache)
        refs = [s["path"] for s in record["sources"]]
        shown, size = [], 0
        for name in refs:
            if len(shown) < 3 and size + len(name) <= 320:
                shown.append(name); size += len(name)
        item = {"record": str(file.relative_to(repo)), "kind": record["kind"],
                "title": excerpt(record["title"], 160), "summary": excerpt(record["summary"], 800),
                "sources": shown, "source_count": len(refs),
                "currency": "NEEDS_REVIEW" if changed else "SOURCE_UNCHANGED",
                "changed_source_count": len(changed),
                "excerpted": len(record["title"]) > 160 or len(record["summary"]) > 800 or len(shown) < len(refs)}
        if not fits([*results, item]):
            if results:
                break
            # Even a large valid record must be discoverable on a small page.
            # Excerpts are explicit and never replace the complete record on disk.
            item.update(sources=[], excerpted=True)
            for field in ("summary", "title"):
                original = item[field]
                low, high = 0, len(original)
                while low < high:
                    middle = (low + high + 1) // 2
                    item[field] = excerpt(original, middle)
                    if fits([item]):
                        low = middle
                    else:
                        high = middle - 1
                item[field] = excerpt(original, low)
                if fits([item]):
                    break
            if not fits([item]):
                raise ValueError("Record metadata exceeds recall budget; increase --max-chars")
        results.append(item)
    return page(results)


def audit(repo):
    """Read-only growth signals; never truncate or delete governing knowledge."""
    def characters(path):
        safe(path)
        with path.open(encoding="utf-8") as source:
            return sum(len(chunk) for chunk in iter(lambda: source.read(65536), ""))

    guidance = [repo / "CLAUDE.md", codex_instruction_file(repo), repo / ".agent-kit/WORKFLOW.md",
                *(repo / ".claude/rules").rglob("*.md")]
    total = sum(characters(p) for p in guidance if p.is_file())
    warnings = []
    if total > 12000:
        warnings.append({"path": "Root guidance + shared workflow + Claude rules", "characters": total,
                         "soft_limit": 12000, "action": "Keep global constraints here; move task-specific detail to linked references."})
    tracked = [(repo / ".agent-kit/PROJECT_MAP.md", 4000), (repo / ".agent-kit/DECISIONS.md", 8000)]
    tracked += [(p, 6000) for p in sorted((repo / ".agent-kit/tasks").glob("*.md"))]
    tracked += [(p, 8000) for p in sorted((repo / ".agent-kit/maps").glob("*.md"))]
    for path, limit in tracked:
        if path.is_file():
            size = characters(path)
            if size > limit:
                warnings.append({"path": str(path.relative_to(repo)), "characters": size, "soft_limit": limit,
                                 "action": "Lead-curate; link scoped detail/archive completed history. Preserve every active constraint and unresolved issue."})
    return {"root_guidance_characters": total, "warning_count": len(warnings), "warnings": warnings[:10],
            "omitted_warnings": max(0, len(warnings) - 10),
            "note": "Character heuristics, not model token counts. Skills, roles, tool schemas, user/global instructions and session history add context. Inspect actual /context or harness usage. No files were changed."}


def checkpoint(payload):
    found = git(Path(payload.get("cwd", ".")), "rev-parse", "--show-toplevel", optional=True)
    if found is None:
        return "Workspace memory: save intent in task records; enter each Git checkout for scoped knowledge capture/recall."
    repo = root(Path(found.decode().strip()))
    tasks = sorted(str(p.relative_to(repo)) for p in (repo / ".agent-kit/tasks").glob("*.md"))
    proposals = [p for p in (repo / ".agent-kit/logs/memory/proposals").glob("*.json")
                 if not (repo / ".agent-kit/knowledge" / p.name).exists()]
    head = git(repo, "rev-parse", "--verify", "HEAD", optional=True)
    record = {"event": payload.get("hook_event_name", "manual"), "session": payload.get("session_id"),
              "head": head.decode().strip() if head else None, "created_at": timestamp(),
              "task_records": tasks, "proposal_count": len(proposals),
              "note": "Checkpoint records pointers, not an inferred summary of the model's thoughts."}
    write(repo / ".agent-kit/logs/memory/checkpoints" / (uuid.uuid4().hex + ".json"), record)
    return "Project memory: read relevant DECISIONS.md, PROJECT_MAP.md and task checkpoints. Use project-memory for scoped recall and reviewed updates. " + str(len(tasks)) + " task records; " + str(len(proposals)) + " captured proposals. Save intent and next step before compaction; a hook cannot recover unwritten reasoning."


def discover(repo, query, paths, max_chars=4000, phase="implementation"):
    """Shared bounded semantic/local discovery for CLI and the MCP gateway."""
    import kit_hindsight
    from kit_skills import relative
    if not isinstance(query, str) or not query.strip() or len(query) > 2000:
        raise ValueError("Semantic recall needs a 1–2000 character query")
    if not isinstance(paths, list) or not 1 <= len(paths) <= 32 or any(not isinstance(p, str) for p in paths):
        raise ValueError("Memory discovery requires 1–32 repository-relative paths")
    paths = [relative(p) for p in paths]
    kit_hindsight.review_access(phase)
    treatment = os.environ.get("AGENT_KIT_MEMORY_BACKEND", "hindsight")
    if treatment not in {"local", "hindsight"}:
        raise ValueError("Invalid pilot memory backend; expected local or hindsight")
    try:
        enabled = kit_hindsight.load_config(repo, configured=False)["enabled"]
        if treatment == "local" or not enabled:
            return recall(repo, paths, max_chars, phase=phase)
        return kit_hindsight.recall(repo, query, paths, max_chars, phase)
    except (OSError, ValueError, subprocess.SubprocessError) as exc:
        return recall(repo, paths, max_chars, phase=phase, notice=str(exc)[:240])


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument("action", choices=("propose", "promote", "recall", "audit", "hook", "setup", "sync", "status", "usage"))
    p.add_argument("--repo", type=Path, default=Path.cwd())
    p.add_argument("--input", type=Path)
    p.add_argument("--reviewer-session", default="")
    p.add_argument("--rationale", default="")
    p.add_argument("--path", action="append", default=[])
    p.add_argument("--max-chars", type=int, default=4000)
    p.add_argument("--offset", type=int, default=0, help="recall: next_offset from an unchanged scoped result")
    p.add_argument("--query", help="Semantic lookup through the managed Hindsight backend")
    p.add_argument("--phase", choices=("implementation", "review"), default="implementation")
    a = p.parse_args()
    try:
        if a.action == "hook":
            print(checkpoint(json.load(sys.stdin))); return 0
        repo = root(a.repo)
        from kit_hindsight import require_writer
        if a.action in {"propose", "promote", "setup", "sync"}:
            require_writer()
        if a.action == "propose":
            result = propose(repo, json.loads(a.input.read_text()))
        elif a.action == "promote":
            result = promote(repo, a.input.absolute(), a.reviewer_session, a.rationale)
        elif a.action == "audit":
            result = audit(repo)
        elif a.action in {"setup", "sync", "status", "usage"}:
            import kit_hindsight
            result = getattr(kit_hindsight, a.action)(repo)
        elif a.query is not None:
            if a.offset:
                raise ValueError("Narrow semantic queries instead of using offsets")
            result = discover(repo, a.query, a.path, a.max_chars, a.phase)
        else:
            result = recall(repo, a.path, a.max_chars, a.offset, a.phase)
        print(compact(result) if a.action == "recall" else json.dumps(result, indent=2)); return 0
    except (OSError, ValueError, TypeError, KeyError, AttributeError, subprocess.SubprocessError) as exc:
        print(f"Memory operation failed: {exc}", file=sys.stderr); return 1


if __name__ == "__main__":
    sys.exit(main())
