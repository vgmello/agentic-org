#!/usr/bin/env python3
"""Same-checkout Sol review/fix cycles, author validation and profile-bound final review.

Local evidence, not a trusted attestation service. Requires exclusive checkout
ownership, a clean task checkpoint, configured checks and an authenticated Codex.
"""

import argparse
from contextlib import contextmanager
import fcntl
import json
import os
from pathlib import Path
import signal
import subprocess
import sys
import tempfile
import time
import uuid

from kit_config import CheckError, codex_instruction_file, digest, load_checks
from kit_evidence import git, inspect_receipt, snapshot, timestamp
from kit_hooks import protected_name, verify
from kit_policy import load_policy, route
from kit_skills import current as current_skills, resolve as resolve_skills

MODEL = "gpt-6.2-sol"


def obj(properties):
    return {"type": "object", "properties": properties, "required": list(properties),
            "additionalProperties": False}


STRING = {"type": "string"}
STRINGS = {"type": "array", "items": STRING}
FINDING = obj({"id": STRING, "priority": {"enum": ["P0", "P1", "P2", "P3"], "type": "string"},
               "file": STRING, "line": {"type": "integer", "minimum": 1},
               "title": STRING, "detail": STRING})
REVIEW_SCHEMA = obj({"verdict": {"enum": ["clean", "issues", "incomplete"], "type": "string"},
                     "summary": STRING, "findings": {"type": "array", "items": FINDING}})
FIX_SCHEMA = obj({"status": {"enum": ["fixed", "blocked"], "type": "string"},
                  "summary": STRING, "addressed_ids": STRINGS, "unaddressed_ids": STRINGS})


def write(path, data):
    """Atomic, private artifacts; never follow a linked output directory."""
    for parent in (path, *path.parents):
        if parent.is_symlink():
            raise ValueError(f"Artifact path traverses a symlink: {path}")
    path.parent.mkdir(parents=True, exist_ok=True, mode=0o700)
    fd, name = tempfile.mkstemp(dir=path.parent, prefix=".review-")
    try:
        with os.fdopen(fd, "wb") as stream:
            if not isinstance(data, bytes):
                data = (data if isinstance(data, str) else json.dumps(data, indent=2) + "\n").encode()
            stream.write(data)
        os.replace(name, path)
    finally:
        if os.path.exists(name):
            os.unlink(name)


def read_json(path):
    if path.stat().st_size > 2_000_000:
        raise ValueError(f"JSON artifact too large: {path}")
    return json.loads(path.read_text())


def relative(value):
    if not isinstance(value, str) or not value or "\0" in value:
        raise ValueError("Expected a nonempty repository-relative path")
    path = Path(value)
    if path.is_absolute() or ".." in path.parts or value.startswith(":"):
        raise ValueError(f"Not a repository-relative path: {value}")
    return path.as_posix()


def validate_review(value):
    if not isinstance(value, dict) or set(value) != set(REVIEW_SCHEMA["properties"]):
        raise ValueError("Invalid review fields")
    if value["verdict"] not in {"clean", "issues", "incomplete"}:
        raise ValueError("Invalid review verdict")
    if not isinstance(value["summary"], str) or not value["summary"].strip():
        raise ValueError("Review needs a summary")
    if not isinstance(value["findings"], list):
        raise ValueError("Findings must be an array")
    ids = set()
    for finding in value["findings"]:
        if not isinstance(finding, dict) or set(finding) != set(FINDING["properties"]):
            raise ValueError("Invalid finding fields")
        for key in ("id", "file", "title", "detail"):
            if not isinstance(finding[key], str) or not finding[key].strip():
                raise ValueError(f"Finding needs {key}")
        relative(finding["file"])
        if (finding["priority"] not in {"P0", "P1", "P2", "P3"} or
                type(finding["line"]) is not int or finding["line"] < 1 or finding["id"] in ids):
            raise ValueError("Invalid priority, line or duplicate finding ID")
        ids.add(finding["id"])
    if (value["verdict"] == "clean" and ids) or (value["verdict"] == "issues" and not ids):
        raise ValueError("Review verdict contradicts findings")
    return value


def validate_fix(value, review):
    if not isinstance(value, dict) or set(value) != set(FIX_SCHEMA["properties"]):
        raise ValueError("Invalid fix fields")
    if value["status"] not in {"fixed", "blocked"} or not isinstance(value["summary"], str) or not value["summary"].strip():
        raise ValueError("Invalid fix status/summary")
    for key in ("addressed_ids", "unaddressed_ids"):
        values = value[key]
        if not isinstance(values, list) or any(not isinstance(v, str) for v in values) or len(values) != len(set(values)):
            raise ValueError("Invalid fix finding IDs")
    addressed, remaining = set(value["addressed_ids"]), set(value["unaddressed_ids"])
    if addressed & remaining or addressed | remaining != {f["id"] for f in review["findings"]}:
        raise ValueError("Fix must account for every finding exactly once")
    if value["status"] == "fixed" and remaining:
        raise ValueError("Fixed verdict has unresolved findings")
    return value


def checkout(repo, branch=None, head=None, clean=False):
    state = snapshot(repo)
    if not state.get("bound") or not state.get("head") or not state.get("branch"):
        raise ValueError("Use a Git branch with an initial commit, not a detached/unborn checkout")
    if Path(state["root"]) != repo:
        raise ValueError("--repo must be the checkout root")
    if branch is not None and state["branch"] != branch:
        raise ValueError("Branch changed during the handoff")
    if head is not None and state["head"] != head:
        raise ValueError("HEAD changed outside the runner; inspect before continuing")
    if clean and state["status_porcelain_z"]:
        raise ValueError("Commit the task checkpoint first; checkout/index must be clean")
    return state


@contextmanager
def locked(repo):
    common = Path(os.fsdecode(git(repo, "rev-parse", "--git-common-dir")).strip())
    if not common.is_absolute():
        common = repo / common
    path = common / "starter-kit-review.lock"
    if path.is_symlink():
        raise ValueError("Review lock must not be a symlink")
    with path.open("a+") as lock:
        try:
            fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
        except BlockingIOError as exc:
            raise ValueError("Another starter-kit review owns this repository") from exc
        try:
            yield
        finally:
            fcntl.flock(lock, fcntl.LOCK_UN)


def save_run(directory, state):
    state["updated_at"] = timestamp()
    state["metrics"] = metrics(directory, state["rounds"])
    write(directory / "metrics.json", state["metrics"])
    write(directory / "run.json", state)
    lines = ["# Review handoff", "", f"Status: **{state['phase']}**", "",
             f"Task base: `{state['base']}`", f"Initial implementation: `{state['initial_head']}`",
             f"Current recorded HEAD: `{state.get('head', state['initial_head'])}`",
             f"Branch: `{state['branch']}`", f"Original session: `{state['origin_session']}`",
             f"Profile: `{state['mode']}`; Sol: `{MODEL}` / `{state['effort']}`; final: `{state['final_model']}` / `{state['final_effort']}`", "",
             f"Reported cache-read fraction: {state['metrics']['cached_input_fraction']} (null means unknown).",
             "See metrics.json for model-turn durations and reported usage; session reuse is not proof of caching.", "",
             "Read task.md, each review.json / review.md, fix.json, fix.diff and verification.json.",
             "Read the selected project-role/skill references in review-skills.json (implementation-skills.json for repairs); missing guidance blocks coverage.",
             "Validate the final diff against the original intent in the ORIGINAL author session.",
             "Only then request a NEW read-only final review with the recorded profile's model/effort, covering behavior, outputs and acceptance criteria.", ""]
    for round_ in state["rounds"]:
        lines += [f"## Round {round_['number']}", f"Session: `{round_.get('session_id', 'not started')}`",
                  f"Before: `{round_['before']}`; after: `{round_.get('after', 'no verified fix commit')}`",
                  f"Artifacts: `{round_['directory']}/`", f"Verdict: {round_.get('verdict', 'pending')}", ""]
    if state.get("failure"):
        lines += ["Unresolved: " + state["failure"], ""]
    lines += ["Acceptance records are local declarations, not authenticated identity proofs.",
              "No automatic push, PR, merge, reset or checkout is performed.", ""]
    write(directory / "HANDOFF.md", "\n".join(lines))


def metrics(directory, rounds):
    """Codex emits thread totals: subtract the review snapshot on exact-session resume.

    Confirmed in rust-v0.167.0-alpha.4's exec JSONL usage_from_last_total().
    Preserve raw snapshots; missing fields and counter resets are not zero usage.
    """
    turns = []
    token_keys = ("input_tokens", "cached_input_tokens", "output_tokens")
    final = [{"number": "final", "directory": "final"}] if (directory / "final/final-prompt.txt").exists() else []
    for round_ in [*rounds, *final]:
        previous, previous_session = None, None
        for phase in (("final",) if round_["number"] == "final" else ("review", "fix")):
            folder = directory / round_["directory"]
            if not (folder / f"{phase}-prompt.txt").exists():
                continue
            entry = {"round": round_["number"], "phase": phase, "duration_seconds": None,
                     "input_tokens": None, "cached_input_tokens": None, "output_tokens": None,
                     "reported_thread_usage": None,
                     "usage_basis": "resumed_session_delta" if phase == "fix" else "fresh_session_total"}
            try:
                execution = read_json(folder / f"{phase}-execution.json")
                duration = execution.get("duration_seconds")
                if type(duration) in {int, float} and duration >= 0:
                    entry["duration_seconds"] = duration
                usages = execution.get("usage", [])
                sessions = execution.get("session_ids", [])
                session = sessions[0] if len(sessions) == 1 and isinstance(sessions[0], str) and sessions[0] else None
                raw = usages[0] if isinstance(usages, list) and len(usages) == 1 and isinstance(usages[0], dict) else {}
                observed = {k: raw[k] if type(raw.get(k)) is int and raw[k] >= 0 else None for k in token_keys}
                entry["reported_thread_usage"] = observed
                # The CLI can emit default zero counters when no usage update arrived.
                measurable = session and not (observed["input_tokens"] == observed["output_tokens"] == 0)
                if phase != "fix":
                    if measurable:
                        entry.update(observed)
                        previous, previous_session = observed, session
                elif measurable and previous is not None and session == previous_session:
                    reset = any(observed[k] is not None and previous[k] is not None and observed[k] < previous[k] for k in token_keys)
                    if not reset:
                        for key in token_keys:
                            if observed[key] is not None and previous[key] is not None:
                                entry[key] = observed[key] - previous[key]
                if (entry["cached_input_tokens"] is not None and entry["input_tokens"] is not None and
                        entry["cached_input_tokens"] > entry["input_tokens"]):
                    entry["cached_input_tokens"] = None
            except (OSError, ValueError, TypeError, AttributeError):
                pass  # Failed or truncated turns stay explicitly unknown.
            turns.append(entry)
    total = {}
    for key in ("duration_seconds", "input_tokens", "cached_input_tokens", "output_tokens"):
        values = [t[key] for t in turns]
        total[key] = sum(values) if values and all(v is not None for v in values) else None
    fraction = (total["cached_input_tokens"] / total["input_tokens"]
                if total["input_tokens"] and total["cached_input_tokens"] is not None else None)
    return {"turns": turns, "totals": total, "cached_input_fraction": fraction,
            "usage_semantics": "Codex thread-cumulative snapshots; exact-session fix deltas",
            "api_cost_usd": None, "note": "Codex-reported input includes cached input. Missing data is null. "
            "Resumed fixes subtract the preceding review's thread totals, not sum them. "
            "Counter regression, ambiguous zero reports or session mismatch leave usage unknown. "
            "Output includes reasoning when the provider reports it there; do not add it again. "
            "Cache-write categories/billing tier are not established; no dollar estimate. "
            "Durations cover wrapped Codex turns, including a Codex-only final pass when invoked here, not the complete task or external author/reviewer work."}


def finding_signature(review):
    return sorted((f["file"], f["priority"], f["title"].strip(), f["detail"].strip()) for f in review["findings"])


def invoke(repo, directory, phase, prompt, effort, timeout, session=None, executable="codex"):
    schema = FIX_SCHEMA if phase == "fix" else REVIEW_SCHEMA
    write(directory / f"{phase}-schema.json", schema)
    write(directory / f"{phase}-prompt.txt", prompt)
    result_path = directory / f"{phase}.json"
    args = [executable, "exec"] + (["resume"] if session else [])
    args += ["-m", MODEL, "-c", f'model_reasoning_effort="{effort}"',
             "-c", "agents.enabled=false",
             "-c", "features.hooks=false", "-c", "mcp_servers.hindsight.enabled=false",
             "-c", 'approval_policy="never"', "-c", f'sandbox_mode="{"workspace-write" if session else "read-only"}"',
             "--json", "--output-schema", str(directory / f"{phase}-schema.json"),
             "--output-last-message", str(result_path)]
    if session:
        args.append(session)
    args.append("-")
    write(directory / f"{phase}-command.json", {"argv": args, "cwd": str(repo)})
    started = time.monotonic()
    for name in (f"{phase}-events.jsonl", f"{phase}-stderr.txt"):
        write(directory / name, "")
    process, reason = None, "error"
    ids, completed, failed, usage = [], 0, False, []
    try:
        with (directory / f"{phase}-events.jsonl").open("w") as events, (directory / f"{phase}-stderr.txt").open("w") as errors:
            process = subprocess.Popen(args, cwd=repo, stdin=subprocess.PIPE, stdout=events,
                                       stderr=errors, text=True, start_new_session=True,
                                       env={**os.environ, "AGENT_KIT_MEMORY_ACCESS": "review",
                                            "HINDSIGHT_DISABLED": "true"})
            try:
                process.communicate(prompt, timeout=timeout)
            except (subprocess.TimeoutExpired, KeyboardInterrupt) as exc:
                reason = "timeout" if isinstance(exc, subprocess.TimeoutExpired) else "interrupted"
                os.killpg(process.pid, signal.SIGKILL)
                process.wait()
                raise ValueError(f"Codex {phase} interrupted/timed out; no approval") from exc
        with (directory / f"{phase}-events.jsonl").open() as events:
            for line in events:
                if len(line) > 2_000_000:
                    raise ValueError("Oversized Codex event")
                event = json.loads(line)
                if not isinstance(event, dict):
                    raise ValueError("Codex event must be an object")
                kind = event.get("type")
                if kind == "thread.started":
                    ids.append(str(uuid.UUID(event["thread_id"])))
                elif kind == "turn.completed":
                    completed += 1
                    usage.append(event.get("usage", {}))
                elif kind in {"turn.failed", "error"}:
                    failed = True
        if process.returncode or failed or completed != 1 or len(ids) != 1:
            raise ValueError(f"Codex {phase} did not complete successfully; inspect private events/stderr")
        if session is not None and ids[0] != session:
            raise ValueError("Fix did not resume the review session")
        result = read_json(result_path)
        reason = "completed"
        return result, ids[0]
    finally:
        # Failures consume real time even when the provider never supplies usage.
        write(directory / f"{phase}-execution.json", {"exit_code": process.returncode if process else None,
              "duration_seconds": round(time.monotonic() - started, 3), "session_ids": ids,
              "usage": usage, "requested_model": MODEL, "requested_effort": effort,
              "termination_reason": reason})


def checked(repo, config, directory):
    _, receipt, path = verify(repo, config)
    if not inspect_receipt(path)["current"] or not any(s.get("root") == str(repo) for s in receipt["sources_after"]):
        raise ValueError("Checks must produce current evidence for this checkout")
    write(directory / "verification.json", receipt)
    return str(path)


def changed_paths(repo):
    raw = git(repo, "diff", "--no-ext-diff", "--no-textconv", "--name-only", "--no-renames", "-z", "HEAD", "--")
    raw += git(repo, "ls-files", "--others", "--exclude-standard", "-z")
    return sorted({os.fsdecode(p) for p in raw.split(b"\0") if p})


def start(args):
    repo = args.repo.resolve()
    policy = load_policy(repo / ".agent-kit/policy/model-routing.json")
    assessment = read_json(args.assessment)
    selection = route(assessment, policy)
    if selection["kind"] != "implementation":
        raise ValueError("Read-only lookup does not need an implementation review loop")
    review_policy = selection["review"]
    args.rounds = args.rounds if args.rounds is not None else review_policy["default_rounds"]
    if args.rounds > review_policy["maximum_rounds"]:
        raise ValueError("Requested rounds exceed the installed policy")
    args.effort = args.effort or review_policy["effort"]
    allowed = {review_policy["final_default_effort"], review_policy["final_risk_effort"]}
    if args.final_effort and (args.final_effort not in allowed or
            (review_policy["final_effort"] == review_policy["final_risk_effort"] and args.final_effort != review_policy["final_effort"])):
        raise ValueError("Task assessment/profile forbids downgrading or substituting final-review effort")
    args.final_effort = args.final_effort or review_policy["final_effort"]
    with locked(repo):
        before = checkout(repo, clean=True)
        base = git(repo, "rev-parse", "--verify", "--end-of-options", f"{args.base}^{{commit}}").decode().strip()
        git(repo, "merge-base", "--is-ancestor", base, before["head"])
        changed = git(repo, "diff", "--name-only", "--no-renames", "-z", base, "HEAD", "--")
        if any(protected_name(Path(os.fsdecode(p)).name) for p in changed.split(b"\0") if p):
            raise ValueError("Task diff includes a protected secret filename; inspect separately before review")
        if git(repo, "ls-files", "--", ".agent-kit/reviews"):
            raise ValueError("Review artifacts must not be tracked")
        if git(repo, "check-ignore", ".agent-kit/reviews/.probe", optional=True) is None:
            raise ValueError("Ignore .agent-kit/reviews/ before starting (the installer supplies this)")
        config = repo / ".agent-kit/verification.json"
        checks, _ = load_checks(config, repo)
        if any(snapshot(directory).get("root") != str(repo) for directory, _ in checks):
            raise ValueError("Run one review per checkout; cross-repo verification belongs in final integration")
        config_hash = digest(config.read_bytes())
        task = args.task.read_text()
        if not task.strip() or len(task) > 128_000:
            raise ValueError("Supply a nonempty task/acceptance contract under 128k characters")
        scopes = [relative(p) for p in args.scope]
        skill_paths = sorted(set(scopes + [relative(os.fsdecode(p)) for p in changed.split(b"\0") if p]))
        skill_packets = {phase: resolve_skills(repo, skill_paths, args.risk,
                         "code-reviewer" if phase == "review" else "fullstack-engineer", phase)
                         for phase in ("review", "implementation")}
        if skill_packets["review"]["critical_areas"] and assessment["facts"]["low_risk"] is True:
            raise ValueError("Critical project skill scope conflicts with low_risk=true; reassess before review")
        version = subprocess.run([args.codex, "--version"], capture_output=True, text=True, timeout=10, check=True).stdout.strip()
        directory = repo / ".agent-kit/reviews" / uuid.uuid4().hex
        state = {"version": 2, "mode": selection["mode"], "final_model": review_policy["final_model"],
                 "phase": "RUNNING", "created_at": timestamp(), "repo": str(repo),
                 "base": base, "initial_head": before["head"], "head": before["head"], "branch": before["branch"],
                 "origin_session": args.origin_session, "origin_model": args.origin_model,
                 "effort": args.effort, "final_effort": args.final_effort, "codex_version": version,
                 "scopes": scopes, "max_rounds": args.rounds, "rounds": [], "source": before,
                 "routing": selection, "skill_packets": skill_packets,
                 "config_sha256": config_hash, "task_sha256": digest(task.encode())}
        write(directory / "task.md", task)
        write(directory / "assessment.json", assessment)
        write(directory / "routing.json", selection)
        for phase, packet in skill_packets.items():
            write(directory / f"{phase}-skills.json", packet)
        save_run(directory, state)
        print(f"Review artifacts: {directory}", flush=True)
        deadline = time.monotonic() + args.budget
        previous_findings = None
        try:
            state["receipt"] = checked(repo, config, directory)
            for number in range(1, args.rounds + 1):
                if time.monotonic() >= deadline:
                    raise ValueError("Review run time budget exhausted")
                current = checkout(repo, state["branch"], state["head"], clean=True)
                check_inputs(repo, directory, state)
                round_dir = directory / f"round-{number:02d}"
                record = {"number": number, "directory": round_dir.name, "before": current["head"]}
                state["rounds"].append(record)
                save_run(directory, state)
                prompt = (f"Review only. Do not edit, stage, commit, push, delegate, or invoke the review runner. "
                          f"Read {codex_instruction_file(repo).name} and relevant project guidance. Review the complete task diff from {base} "
                          f"to HEAD ({current['head']}) and relevant surrounding source, tests and user-facing outputs. "
                          f"Read the task contract at {directory / 'task.md'}. Use search and configured navigation tools. "
                          f"Read {directory / 'review-skills.json'} and its required project-role/skill sources. "
                          "Apply the listed critical-area review procedures; missing skill evidence means incomplete. "
                          "Do not read earlier review-round artifacts: independently inspect the code in this fresh session. "
                          "Identify actionable bugs, regressions and unmet acceptance criteria with precise file/line evidence. "
                          "Do not invent stylistic issues to fill a quota. Report incomplete if coverage is insufficient. "
                          "Return only the required JSON. Clean means no actionable findings; it is not final acceptance.")
                review, session = invoke(repo, round_dir, "review", prompt, args.effort,
                                         min(args.timeout, max(1, deadline - time.monotonic())), executable=args.codex)
                validate_review(review)
                if session in {r.get("session_id") for r in state["rounds"]}:
                    raise ValueError("Each review round must start a fresh session")
                record.update(session_id=session, verdict=review["verdict"])
                write(round_dir / "review.md", "# Sol review\n\n" + review["summary"] + "\n\n" + "\n\n".join(
                    f"- {f['id']} [{f['priority']}] {f['file']}:{f['line']} — {f['title']}\n  {f['detail']}" for f in review["findings"]))
                after_review = checkout(repo, state["branch"], state["head"], clean=True)
                if after_review["fingerprint"] != current["fingerprint"]:
                    raise ValueError("Review changed source contents")
                check_inputs(repo, directory, state)
                if review["verdict"] == "incomplete":
                    raise ValueError("Sol review is incomplete")
                if review["verdict"] == "clean":
                    state["receipt"] = checked(repo, config, round_dir)
                    check_inputs(repo, directory, state)
                    state.update(phase="AWAITING_ORIGIN", source=checkout(repo, state["branch"], state["head"], clean=True))
                    break
                signature = finding_signature(review)
                if review_policy["stop_on_repeated_findings"] and signature == previous_findings:
                    raise ValueError("The same findings recurred after a claimed fix; return to the original author before spending another repair turn")
                previous_findings = signature
                if time.monotonic() >= deadline:
                    raise ValueError("Review run time budget exhausted before fixes")
                prompt = ("Now verify each finding against the task contract and apply confirmed fixes in this SAME session. "
                          f"Read {round_dir / 'review.json'} if needed. Authorized paths: {json.dumps(scopes)}. "
                          f"Read {directory / 'implementation-skills.json'} and its required role/skill sources before repairs. "
                          "Make only fixes necessary for reproducible or clearly evidenced bugs, including meaningful regression checks. "
                          "Do not implement stylistic preferences or speculative refactors. If your finding is a false positive "
                          "or conflicts with intended behavior, mark it unaddressed and explain why in the summary for the original author. "
                          "Do not commit, stage, push, change branches, delegate, modify the verification configuration, "
                          "weaken checks or edit review artifacts. The runner verifies and commits. "
                          "If a finding needs broader scope or cannot be fixed, report it unaddressed. "
                          "Account for every finding ID in the required JSON; fixed is not an approval verdict.")
                fix, _ = invoke(repo, round_dir, "fix", prompt, args.effort,
                                min(args.timeout, max(1, deadline - time.monotonic())), session, args.codex)
                validate_fix(fix, review)
                checkout(repo, state["branch"], state["head"])
                if digest(config.read_bytes()) != config_hash:
                    raise ValueError("Fix changed verification configuration; inspect manually")
                paths = changed_paths(repo)
                for name in paths:
                    path = Path(name)
                    if not any(scope == "." or name == scope or path.is_relative_to(scope) for scope in scopes):
                        raise ValueError(f"Fix exceeded authorized paths: {name}")
                    if (protected_name(path.name) or (repo / path).is_symlink() or
                            any(p.is_symlink() for p in (repo / path).parents)):
                        raise ValueError(f"Fix changed a protected/linked path: {name}")
                if fix["status"] != "fixed" or fix["unaddressed_ids"]:
                    raise ValueError("Sol left unresolved findings; inspect fix.json")
                if not paths:
                    raise ValueError("Sol reported fixes without changes; resolve disputed findings with the author")
                check_inputs(repo, directory, state)
                git(repo, "--literal-pathspecs", "add", "--all", "--", *paths)
                write(round_dir / "fix.diff", git(repo, "diff", "--no-ext-diff", "--no-textconv", "--binary", "HEAD", "--"))
                expected_tree = git(repo, "write-tree").strip()
                verified = checked(repo, config, round_dir)
                checkout(repo, state["branch"], state["head"])
                if not inspect_receipt(verified)["current"]:
                    raise ValueError("Source changed between verification and commit")
                check_inputs(repo, directory, state)
                git(repo, "commit", "-m", f"fix: address Sol review round {number}")
                committed = checkout(repo, state["branch"], clean=True)
                record["after"] = committed["head"]
                state.update(head=committed["head"], source=committed)
                if (git(repo, "rev-parse", "HEAD^{tree}").strip() != expected_tree or
                        git(repo, "rev-parse", "HEAD^").decode().strip() != record["before"]):
                    raise ValueError("Commit hooks changed the verified tree or parent; inspect the recorded commit")
                # A commit changes source identity. Recheck on the committed revision.
                state["receipt"] = checked(repo, config, round_dir)
                check_inputs(repo, directory, state)
                save_run(directory, state)
            else:
                raise ValueError("Round limit reached; the last fixes still need an independent clean review")
        except (CheckError, OSError, ValueError, KeyError, TypeError, subprocess.SubprocessError, KeyboardInterrupt) as exc:
            state.update(phase="BLOCKED", failure=str(exc) or "Interrupted")
            save_run(directory, state)
            raise ValueError(f"{state['failure']}; handoff: {directory / 'HANDOFF.md'}") from exc
        save_run(directory, state)
        print(f"AWAITING_ORIGIN: {directory / 'HANDOFF.md'}")
        return 0


def check_inputs(repo, directory, state):
    # Ignored review artifacts are outside the source fingerprint. Bind every
    # model/acceptance boundary to the contract copied when this run started.
    try:
        contract_hash = digest((directory / "task.md").read_bytes())
    except OSError as exc:
        raise ValueError("Review task contract is missing or unreadable; start a new run") from exc
    if contract_hash != state.get("task_sha256"):
        raise ValueError("Review task contract changed; start a new run with the current contract")
    packets = state.get("skill_packets")
    if not isinstance(packets, dict) or set(packets) != {"implementation", "review"}:
        raise ValueError("Review lacks project skill routing; start a new run after capability adoption")
    for phase, packet in packets.items():
        if json.loads((directory / f"{phase}-skills.json").read_text()) != packet:
            raise ValueError("Review skill packet artifact changed; start a new run with current guidance")
        current_skills(repo, packet)


def current_run(directory):
    state = read_json(directory / "run.json")
    repo = Path(state["repo"])
    if directory.parent != repo / ".agent-kit/reviews":
        raise ValueError("Run must remain in its original checkout artifact folder")
    policy, checksum = load_policy(repo / ".agent-kit/policy/model-routing.json")
    if state.get("mode") != policy["mode"] or state["routing"]["policy_sha256"] != checksum:
        raise ValueError("Review profile/policy changed; do not transfer acceptance across modes")
    check_inputs(repo, directory, state)
    current = checkout(repo, state["branch"], state["head"], clean=True)
    if current["fingerprint"] != state["source"]["fingerprint"]:
        raise ValueError("Review source evidence is stale")
    if not inspect_receipt(state["receipt"])["current"]:
        raise ValueError("Verification receipt is stale")
    return state


def record_final(directory, state, result):
    if state["phase"] != "AWAITING_FINAL":
        raise ValueError("Final review requires original-author acceptance first")
    required = {"head", "session_id", "model", "effort", "review"}
    if not isinstance(result, dict) or set(result) != required:
        raise ValueError("Final result requires head, session_id, model, effort and review")
    model = result["model"]
    model_ok = isinstance(model, str) and (model == MODEL if state["mode"] == "codex-only" else model.startswith("claude-opus-"))
    allowed = ("high", "xhigh") if state["mode"] == "codex-only" else ("medium", "high")
    if (result["head"] != state["head"] or not model_ok or result["effort"] not in allowed or
            (state["final_effort"] == allowed[-1] and result["effort"] != allowed[-1])):
        raise ValueError("Final review has the wrong revision, profile model or effort")
    sessions = {state["origin_session"], *(r["session_id"] for r in state["rounds"])}
    if not isinstance(result["session_id"], str) or not result["session_id"].strip() or result["session_id"] in sessions:
        raise ValueError("Final review must use a new session/context")
    validate_review(result["review"])
    write(directory / "final-review.json", result)
    state["phase"] = "ACCEPTED" if result["review"]["verdict"] == "clean" else "CHANGES_REQUIRED"


def final_review(args):
    directory = args.run.absolute()
    initial = read_json(directory / "run.json")
    repo = Path(initial["repo"])
    with locked(repo):
        state = current_run(directory)
        if state["mode"] != "codex-only" or state["phase"] != "AWAITING_FINAL":
            raise ValueError("Automatic Sol final review requires codex-only and original-author acceptance")
        prompt = (f"Independent final code AND output review only. Do not edit, commit, delegate or call the review runner. "
                  f"Read {directory / 'review-skills.json'} and its required project role/skill sources. "
                  f"Inspect the original contract {directory / 'task.md'}, active project decisions and complete diff "
                  f"from {state['base']} to {state['head']}. Verify observable acceptance criteria, actual output evidence "
                  "and regression risks. First inspect source/outputs independently, then inspect the review round "
                  f"artifacts and {directory / 'origin-validation.md'} to evaluate repairs against intent. "
                  "A clean earlier review is not evidence of correctness. Report incomplete if required output/UI "
                  "evidence is unavailable. Return the required JSON with only actionable findings.")
        try:
            review, session = invoke(repo, directory / "final", "final", prompt,
                                     state["final_effort"], args.timeout, executable=args.codex)
            state = current_run(directory)
            record_final(directory, state, {"head": state["head"], "session_id": session,
                         "model": MODEL, "effort": state["final_effort"], "review": review})
        except (CheckError, OSError, ValueError, KeyError, TypeError, subprocess.SubprocessError, KeyboardInterrupt) as exc:
            state.update(phase="BLOCKED", failure=f"Final review failed: {exc}")
            save_run(directory, state)
            raise ValueError(f"{state['failure']}; inspect {directory / 'HANDOFF.md'}") from exc
        save_run(directory, state)
        print(state["phase"])
        return 0 if state["phase"] == "ACCEPTED" else 2


def attest(args):
    directory = args.run.absolute()
    initial = read_json(directory / "run.json")
    with locked(Path(initial["repo"])):
        state = current_run(directory)
        if args.action == "origin":
            if state["phase"] != "AWAITING_ORIGIN" or args.session != state["origin_session"]:
                raise ValueError("Origin validation requires the recorded original session and a clean Sol handoff")
            notes = args.notes.read_text()
            if not notes.strip():
                raise ValueError("Origin validation needs reasoning about intent and the actual fixes")
            write(directory / "origin-validation.md", notes)
            state["origin_validation"] = {"session": args.session, "decision": args.decision,
                                          "head": state["head"], "notes_sha256": digest(notes.encode())}
            state["phase"] = "AWAITING_FINAL" if args.decision == "accept" else "CHANGES_REQUIRED"
        else:
            record_final(directory, state, read_json(args.result))
        save_run(directory, state)
        print(state["phase"])
        return 0 if state["phase"] in {"AWAITING_FINAL", "ACCEPTED"} else 2


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest="action", required=True)
    run = sub.add_parser("start")
    run.add_argument("--repo", type=Path, default=Path.cwd())
    run.add_argument("--base", required=True)
    run.add_argument("--task", type=Path, required=True)
    run.add_argument("--assessment", type=Path, required=True, help="Evidence-backed task facts; routing is recomputed from installed policy")
    run.add_argument("--scope", action="append", required=True, help="Authorized file/directory; repeat for multiple scopes")
    run.add_argument("--risk", action="append", default=[], help="Semantic risk tag from the project skill catalog; repeat as needed")
    run.add_argument("--origin-session", required=True)
    run.add_argument("--origin-model", required=True)
    run.add_argument("--effort", choices=("high", "xhigh"), help="Default from installed policy")
    run.add_argument("--final-effort", choices=("medium", "high", "xhigh"), help="Allowed by the profile and at least the assessed effort")
    run.add_argument("--rounds", type=int, choices=range(1, 6), help="Default 3 from policy; hard ceiling 5")
    run.add_argument("--timeout", type=int, default=1200, help="Seconds per Codex turn, maximum 3600")
    run.add_argument("--budget", type=int, default=7200, help="Total model-turn scheduling budget in seconds")
    run.add_argument("--codex", default="codex", help="Codex executable path")
    origin = sub.add_parser("origin")
    origin.add_argument("--run", type=Path, required=True)
    origin.add_argument("--session", required=True)
    origin.add_argument("--decision", choices=("accept", "reject"), required=True)
    origin.add_argument("--notes", type=Path, required=True)
    final = sub.add_parser("final")
    final.add_argument("--run", type=Path, required=True)
    final.add_argument("--result", type=Path, required=True)
    auto = sub.add_parser("final-review", help="Run a fresh read-only Sol final review in codex-only mode")
    auto.add_argument("--run", type=Path, required=True)
    auto.add_argument("--codex", default="codex")
    auto.add_argument("--timeout", type=int, default=1200)
    status = sub.add_parser("status")
    status.add_argument("--run", type=Path, required=True)
    args = parser.parse_args()
    try:
        if args.action == "final-review":
            if not 1 <= args.timeout <= 3600:
                raise ValueError("Use timeout 1..3600 seconds")
            return final_review(args)
        if args.action == "start":
            if not 1 <= args.timeout <= 3600 or not 1 <= args.budget <= 18000:
                raise ValueError("Use timeout 1..3600 and total budget 1..18000 seconds")
            if not args.origin_session.strip() or not args.origin_model.strip():
                raise ValueError("Record the original author session and model")
            return start(args)
        if args.action == "status":
            state = current_run(args.run.absolute())
            print(json.dumps({"phase": state["phase"], "head": state["head"], "current": True}))
            return 0 if state["phase"] == "ACCEPTED" else 2
        return attest(args)
    except (CheckError, OSError, ValueError, KeyError, TypeError, subprocess.SubprocessError) as exc:
        print(f"Review not accepted: {exc}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    sys.exit(main())
