#!/usr/bin/env python3
"""Validate project-adapted roles and select bounded skill references, without model calls.

Agents assess relevance and author the project catalog. Code checks its structure,
source freshness, path coverage and declared preflight; it cannot prove competence.
"""

import argparse
import json
from pathlib import Path, PurePosixPath
import re
import sys

from kit_config import digest, protected_name

CATALOG = ".agent-kit/skill-catalog.json"
ROLES = {"staff-engineer", "fullstack-engineer", "code-reviewer", "codebase-scout"}
ALIASES = {"architecture-reviewer": "code-reviewer", "test-runner": "codebase-scout"}


def relative(value):
    if not isinstance(value, str) or not value or "\\" in value or "\0" in value:
        raise ValueError("Use a nonempty repository-relative POSIX path")
    path = PurePosixPath(value)
    if (path.is_absolute() or ".." in path.parts or any(c in value for c in "*?") or
            any(p == ".git" or protected_name(p) for p in path.parts)):
        raise ValueError("Skill paths must be literal non-secret repository paths")
    if any(path.parts[i] in {".agent-kit", ".claude"} and path.parts[i + 1] in
           {"logs", "reviews", "tools", ".starter-kit-backups"} for i in range(len(path.parts) - 1)):
        raise ValueError("Private logs, review history and derived tools cannot be skill sources")
    return path.as_posix()


def read(repo, name, maximum=2_000_000):
    name = relative(name)
    path = repo / name
    if any(p.is_symlink() for p in (path, *path.parents)) or not path.is_file():
        raise ValueError(f"Skill source missing or linked: {name}")
    if path.stat().st_size > maximum:
        raise ValueError(f"Skill source exceeds {maximum} bytes; split it into focused references: {name}")
    raw = path.read_bytes()
    return raw, {"path": name, "sha256": digest(raw)}


def words(value, label, maximum=2000):
    if not isinstance(value, str) or not value.strip() or len(value) > maximum:
        raise ValueError(f"{label} must be nonempty and at most {maximum} characters")
    return value


def items(value, label, allow_empty=False, maximum=64):
    if (not isinstance(value, list) or len(value) > maximum or (not value and not allow_empty) or
            any(not isinstance(x, str) or not x for x in value) or len(set(value)) != len(value)):
        raise ValueError(f"{label} must contain distinct strings (maximum {maximum})")
    return value


def load(repo):
    raw, identity = read(repo, CATALOG, 96_000)
    value = json.loads(raw)
    required = {"version", "status", "reviewed_by", "rationale", "project_evidence", "roles", "skills"}
    if not isinstance(value, dict) or set(value) != required or type(value["version"]) is not int or value["version"] != 1:
        raise ValueError("Unsupported project skill catalog")
    if value["status"] != "ready":
        raise ValueError("Project skills need adaptation: use project-capabilities before dispatch")
    words(value["reviewed_by"], "Observed adopting/reviewing session", 200)
    words(value["rationale"], "Project capability rationale")
    evidence = value["project_evidence"]
    if not isinstance(evidence, list) or not 1 <= len(evidence) <= 64:
        raise ValueError("Record 1–64 project evidence files with hashes")
    seen = {}
    for ref in evidence:
        if not isinstance(ref, dict) or set(ref) != {"path", "sha256"}:
            raise ValueError("Project evidence requires path and sha256")
        _, current = read(repo, ref["path"])
        if ref != current or ref["path"] in seen:
            raise ValueError(f"Project capability evidence changed or duplicated: {ref['path']}; reassess before updating hashes")
        seen[ref["path"]] = ref["sha256"]
    roles = value["roles"]
    if not isinstance(roles, dict) or set(roles) != ROLES:
        raise ValueError("Provide project briefs for lead, full-stack engineer, reviewer and scout")
    references = {}

    def instruction(name):
        normalized = relative(name)
        if normalized not in references:
            body, reference = read(repo, normalized, 24_000)
            if seen.get(normalized) != reference["sha256"]:
                raise ValueError(f"Skill/role instructions lack current reviewed evidence: {normalized}; capture guidance after assessment")
            if not body.decode("utf-8").strip():
                raise ValueError(f"Empty skill/role instructions: {normalized}")
            references[normalized] = reference
        return normalized

    for name in roles.values():
        instruction(name)
    skills = value["skills"]
    if not isinstance(skills, list) or not 1 <= len(skills) <= 64:
        raise ValueError("Record 1–64 project skills, including a baseline conventions skill")
    ids = set()
    for skill in skills:
        if not isinstance(skill, dict) or set(skill) != {"id", "description", "paths", "risks", "implementation", "review", "critical"}:
            raise ValueError("Invalid project skill fields")
        if not isinstance(skill["id"], str) or not re.fullmatch(r"[a-z][a-z0-9-]{0,79}", skill["id"]) or skill["id"] in ids:
            raise ValueError("Use distinct lowercase skill IDs")
        ids.add(skill["id"])
        words(skill["description"], "Skill description", 300)
        for path in items(skill["paths"], "Skill path prefixes"):
            if relative(path) != path:
                raise ValueError("Use normalized skill path prefixes")
        for risk in items(skill["risks"], "Risk tags", True):
            if not re.fullmatch(r"[a-z][a-z0-9-]{0,79}", risk):
                raise ValueError("Use lowercase risk tags")
        for phase in ("implementation", "review"):
            for path in items(skill[phase], f"{phase} skill references", True):
                instruction(path)
        if type(skill["critical"]) is not bool or not (skill["implementation"] or skill["review"]):
            raise ValueError("Skills require phase references and a boolean critical flag")
        if skill["critical"] and not skill["review"]:
            raise ValueError("Critical areas require focused review instructions")
    return value, identity["sha256"], references


def overlaps(left, right):
    left, right = PurePosixPath(left), PurePosixPath(right)
    return left.is_relative_to(right) or right.is_relative_to(left)


def resolve(repo, paths, risks, role, phase):
    value, checksum, references = load(repo)
    role = ALIASES.get(role, role)
    if role not in ROLES or phase not in {"implementation", "review"}:
        raise ValueError("Unsupported role or skill phase")
    requested = {relative(p) for p in items(paths, "Assignment paths", maximum=1024)}
    # Keep explicit parent scopes once, rather than repeating every changed child.
    paths = sorted(p for p in requested if not any(p != q and PurePosixPath(p).is_relative_to(q) for q in requested))
    risks = sorted(items(risks, "Assignment risks", True))
    matched = [s for s in value["skills"] if any(overlaps(p, scope) for p in paths for scope in s["paths"])
               and (not s["risks"] or set(s["risks"]) & set(risks))]
    selected = [s for s in matched if s[phase]]
    for path in paths:
        if not any(PurePosixPath(path).is_relative_to(PurePosixPath(scope))
                   for s in selected for scope in s["paths"]):
            raise ValueError(f"No {phase} skill coverage for {path}; return to the adopting/lead agent")
    for risk in risks:
        if not any(risk in s["risks"] for s in selected):
            raise ValueError(f"Uncovered risk tag {risk}; add relevant {phase} guidance before dispatch")
    names = {relative(value["roles"][role]), *(relative(p) for s in selected for p in s[phase])}
    return {"version": 1, "repo": str(repo), "catalog_sha256": checksum, "role": role, "phase": phase,
            "paths": paths, "risks": risks,
            "skills": [{"id": s["id"], "description": s["description"]} for s in selected],
            "critical_areas": [s["id"] for s in matched if s["critical"]],
            "required_sources": [references[name] for name in sorted(names)],
            "instruction": "Read the selected role/skill sources before work. Follow referenced project checks/constraints; missing context blocks readiness. No skill bodies are copied into this packet."}


def current(repo, packet):
    if not isinstance(packet, dict) or packet.get("repo") != str(repo):
        raise ValueError("Skill packet belongs to a different checkout")
    expected = resolve(repo, packet["paths"], packet["risks"], packet["role"], packet["phase"])
    if packet != expected:
        raise ValueError("Skill packet is stale; reassess, resolve and reload current guidance")
    return expected


def capture(repo, paths, include_guidance=False):
    names = set(items(paths, "Evidence paths", allow_empty=include_guidance))
    if include_guidance:
        value = json.loads(read(repo, CATALOG, 96_000)[0])
        if (not isinstance(value, dict) or not isinstance(value.get("roles"), dict) or
                not isinstance(value.get("project_evidence"), list) or not isinstance(value.get("skills"), list)):
            raise ValueError("Evidence capture requires catalog roles, project_evidence and skills")
        names.update(value["roles"].values())
        names.update(ref["path"] for ref in value["project_evidence"])
        for skill in value["skills"]:
            for phase in ("implementation", "review"):
                names.update(items(skill[phase], f"{phase} references", True))
    return [read(repo, name)[1] for name in sorted(items(list(names), "Evidence paths"))]


def preflight(repo, packet, acknowledgment):
    current(repo, packet)
    if (not isinstance(acknowledgment, dict) or set(acknowledgment) != {"version", "session_id", "read", "notes"} or
            type(acknowledgment["version"]) is not int or acknowledgment["version"] != 1):
        raise ValueError("Preflight needs version, observed session_id, read references and applicability notes")
    words(acknowledgment["session_id"], "Observed worker session", 200)
    words(acknowledgment["notes"], "Applicable constraints/checks or blockers")
    if acknowledgment["read"] != packet["required_sources"]:
        raise ValueError("Acknowledge every required source at its current hash, in packet order")
    return {"status": "DECLARED_READY", "session_id": acknowledgment["session_id"],
            "packet_sha256": digest(json.dumps(packet, sort_keys=True).encode()),
            "limit": "Local acknowledgment checks completeness/freshness, not actual reading, understanding or model identity."}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("action", choices=("validate", "evidence", "resolve", "preflight"))
    parser.add_argument("--repo", type=Path, default=Path.cwd())
    parser.add_argument("--path", action="append", default=[])
    parser.add_argument("--risk", action="append", default=[])
    parser.add_argument("--include-guidance", action="store_true", help="evidence: include current role/skill references and previously recorded basis files; does not approve them")
    parser.add_argument("--role", choices=sorted(ROLES | ALIASES.keys()), default="fullstack-engineer")
    parser.add_argument("--phase", choices=("implementation", "review"), default="implementation")
    parser.add_argument("--packet", type=Path)
    parser.add_argument("--ack", type=Path)
    args = parser.parse_args()
    try:
        repo = args.repo.absolute()
        if args.action == "evidence":
            result = capture(repo, args.path, args.include_guidance)
        elif args.action == "validate":
            value, checksum, _ = load(repo)
            result = {"status": "SKILLS_CONFIGURED", "catalog_sha256": checksum,
                      "roles": sorted(value["roles"]), "skills": len(value["skills"]),
                      "limit": "Structure, references and evidence hashes verified; semantic fit remains an agent assessment."}
        elif args.action == "resolve":
            result = resolve(repo, args.path, args.risk, args.role, args.phase)
        else:
            if not args.packet or not args.ack:
                raise ValueError("preflight requires --packet and --ack")
            result = preflight(repo, json.loads(args.packet.read_text()), json.loads(args.ack.read_text()))
        print(json.dumps(result, indent=2))
        return 0
    except (OSError, ValueError, TypeError, KeyError) as exc:
        print(f"Skill setup/routing blocked: {exc}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    sys.exit(main())
