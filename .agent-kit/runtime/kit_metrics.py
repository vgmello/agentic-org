#!/usr/bin/env python3
"""Local whole-task journal and descriptive, paired efficiency scorecards.

No model calls, billing estimates, transcript ingestion or policy changes. Local
observations are evidence-backed declarations, not authenticated telemetry.
"""

import argparse
from datetime import datetime
import hashlib
import json
import math
import os
from pathlib import Path
import statistics
import sys
import time
import uuid

from kit_codex import checkout, current_run, locked, metrics, read_json, relative, write
from kit_config import KIT_VERSION, digest
from kit_evidence import git, timestamp
from kit_hooks import protected_name

TOKEN_KEYS = ("input_tokens", "cached_input_tokens", "output_tokens")
QUALITY_KEYS = ("confirmed_findings", "false_positives", "repair_regressions",
                "origin_reversals", "final_review_findings", "escaped_defects")
COVERAGES = {"unknown", "partial", "complete"}


def blank_observations():
    return {"version": 2,
            "usage": {"coverage": "unknown", **dict.fromkeys(TOKEN_KEYS),
                      "models_and_effort": [], "evidence": []},
            "cost": {"coverage": "unknown", "basis": None, "usd": None, "evidence": []},
            "human_seconds": None, "repair_attempts": None,
            "quality": dict.fromkeys(QUALITY_KEYS), "quality_evidence": [],
            "observed_through": None, "quality_window_days": None, "notes": ""}


def number(value, name, integer=False):
    if value is not None and (type(value) not in (int, float) or not math.isfinite(value)
                              or value < 0 or (integer and type(value) is not int)):
        raise ValueError(f"{name} must be a finite nonnegative {'integer' if integer else 'number'} or null")


def moment(value):
    if not isinstance(value, str):
        raise ValueError("Expected an ISO timestamp string")
    parsed = datetime.fromisoformat(value)
    if parsed.tzinfo is None:
        raise ValueError("Timestamps need an explicit timezone")
    return parsed


def fields(value, expected):
    if not isinstance(value, dict) or set(value) != set(expected):
        raise ValueError(f"Expected exactly these fields: {', '.join(expected)}")


def strings(value, name):
    if not isinstance(value, list) or any(not isinstance(x, str) or not x.strip() for x in value):
        raise ValueError(f"{name} must be a list of nonempty strings")


def validate_observations(value):
    fields(value, blank_observations())
    if value["version"] != 2:
        raise ValueError("Use PILOT_RUN.json version 2")
    fields(value["usage"], blank_observations()["usage"])
    fields(value["cost"], blank_observations()["cost"])
    fields(value["quality"], QUALITY_KEYS)
    for name in ("usage", "cost"):
        item = value[name]
        if item["coverage"] not in COVERAGES:
            raise ValueError("Coverage must be unknown, partial or complete")
        strings(item["evidence"], "evidence")
        if item["coverage"] == "complete" and not item["evidence"]:
            raise ValueError("Complete measurements require evidence references")
    usage, cost = value["usage"], value["cost"]
    for key in TOKEN_KEYS:
        number(usage[key], key, integer=True)
    strings(usage["models_and_effort"], "models_and_effort")
    if usage["coverage"] == "complete" and (usage["input_tokens"] is None or
            usage["output_tokens"] is None or not usage["models_and_effort"]):
        raise ValueError("Complete task usage needs input/output totals and observed model/effort IDs")
    if usage["coverage"] == "unknown" and any(usage[k] is not None for k in TOKEN_KEYS):
        raise ValueError("Known token counters need partial or complete coverage")
    if usage["cached_input_tokens"] is not None and (usage["input_tokens"] is None or
            usage["cached_input_tokens"] > usage["input_tokens"]):
        raise ValueError("Cached input is a subset of total input")
    number(cost["usd"], "usd")
    if cost["basis"] not in (None, "actual_usd", "api_equivalent_usd"):
        raise ValueError("Keep actual bills and API-equivalent estimates separate; no subscription conversion")
    if cost["coverage"] == "complete" and (cost["usd"] is None or cost["basis"] is None):
        raise ValueError("Complete cost needs an amount and basis, independent of token completeness")
    if cost["coverage"] == "unknown" and cost["usd"] is not None:
        raise ValueError("Known cost needs partial or complete coverage")
    if cost["usd"] is not None and cost["basis"] is None:
        raise ValueError("A cost amount needs its basis")
    number(value["human_seconds"], "human_seconds")
    number(value["repair_attempts"], "repair_attempts", integer=True)
    for key in QUALITY_KEYS:
        number(value["quality"][key], key, integer=True)
    strings(value["quality_evidence"], "quality_evidence")
    for ref in value["quality_evidence"] + usage["evidence"] + cost["evidence"]:
        if relative(ref) != ref:
            raise ValueError("Use canonical checkout-relative evidence paths")
    if (value["human_seconds"] is not None or value["repair_attempts"] is not None or
            any(x is not None for x in value["quality"].values())) and not value["quality_evidence"]:
        raise ValueError("Quality, repair and human-work observations need evidence")
    if value["observed_through"] is not None:
        moment(value["observed_through"])
    number(value["quality_window_days"], "quality_window_days", integer=True)
    if not isinstance(value["notes"], str) or len(value["notes"]) > 4000:
        raise ValueError("Keep notes under 4000 characters; reference longer evidence")
    return value


def safe_file(repo, value):
    path = Path(value)
    if ".." in path.parts:
        raise ValueError("Evidence cannot traverse parent directories")
    if not path.is_absolute():
        path = repo / relative(str(value))
    if not path.is_relative_to(repo):
        raise ValueError("Evidence must be inside this checkout")
    for part in (path, *path.parents):
        if part.is_symlink() or protected_name(part.name):
            raise ValueError("Evidence must not traverse links or protected names")
        if part == repo:
            break
    if not path.is_file() or path.stat().st_size > 2_000_000:
        raise ValueError(f"Evidence needs a regular file under 2 MB: {path}")
    return path


def evidence(repo, paths):
    return [{"path": str(p.relative_to(repo)), "sha256": digest(p.read_bytes())}
            for p in (safe_file(repo, value) for value in sorted(set(paths)))]


def application_fingerprint(repo, paths):
    """Only declared application inputs; kit/policy treatment is recorded separately."""
    strings(paths, "application_paths")
    if not paths or any(relative(p) == "." for p in paths):
        raise ValueError("List specific invariant application paths, not the whole checkout")
    raw = git(repo, "--literal-pathspecs", "ls-files", "--cached", "--others",
              "--exclude-standard", "-z", "--", *paths)
    names = sorted(set(raw.split(b"\0")) - {b""})
    if not names:
        raise ValueError("Application paths matched no files")
    result, deadline = hashlib.sha256(), time.monotonic() + 20
    for name in names:
        path = repo / os.fsdecode(name)
        if any(p.is_symlink() for p in (path, *path.parents) if p.is_relative_to(repo)):
            raise ValueError("Application comparison does not support links/submodules")
        result.update(name + b"\0")
        if not path.exists():
            result.update(b"deleted\0")
            continue
        if not path.is_file():
            raise ValueError("Application comparison requires regular files")
        result.update(str(path.stat().st_mode & 0o777).encode() + b"\0")
        content = hashlib.sha256()
        with path.open("rb") as source:
            for chunk in iter(lambda: source.read(1024 * 1024), b""):
                if time.monotonic() > deadline:
                    raise ValueError("Application fingerprint exceeded 20 seconds")
                content.update(chunk)
        result.update(content.digest())
    return result.hexdigest()


def validate_experiment(value):
    fields(value, ("version", "id", "hypothesis", "baseline", "candidate", "cases",
                   "minimum_pairs", "objective", "minimum_improvement_fraction",
                   "quality_window_days", "budget", "rollback"))
    if value["version"] != 1 or value["objective"] not in ("cost_per_accepted", "median_accepted_seconds"):
        raise ValueError("Experiment version/objective unsupported")
    for key in ("id", "hypothesis", "baseline", "candidate", "budget", "rollback"):
        if not isinstance(value[key], str) or not value[key].strip():
            raise ValueError(f"Experiment requires {key}")
    for key in ("minimum_pairs", "quality_window_days"):
        number(value[key], key, integer=True)
        if value[key] is None or value[key] < 1:
            raise ValueError(f"Set {key} to at least 1 before the experiment")
    number(value["minimum_improvement_fraction"], "minimum_improvement_fraction")
    if value["minimum_improvement_fraction"] is None or not 0 < value["minimum_improvement_fraction"] < 1:
        raise ValueError("Predeclare an improvement fraction between 0 and 1")
    if not isinstance(value["cases"], dict) or not value["cases"]:
        raise ValueError("Predeclare cases and trials")
    for case in value["cases"].values():
        fields(case, ("category", "contract_sha256", "application_paths", "trials"))
        if not isinstance(case["category"], str) or not case["category"].strip():
            raise ValueError("Each case needs a task category")
        sha = case["contract_sha256"]
        if not isinstance(sha, str) or len(sha) != 64 or any(c not in "0123456789abcdef" for c in sha):
            raise ValueError("Predeclare the immutable request/acceptance-contract SHA256")
        strings(case["application_paths"], "application_paths")
        if not case["application_paths"] or any(relative(p) == "." for p in case["application_paths"]):
            raise ValueError("Predeclare specific application paths")
        strings(case["trials"], "trials")
        if not case["trials"] or len(set(case["trials"])) != len(case["trials"]):
            raise ValueError("Trial IDs must be nonempty and unique per case")
    return value


def journal(repo):
    root = repo / ".agent-kit/logs/task-metrics"
    if any(p.is_symlink() for p in (root, *root.parents)):
        raise ValueError("Task journal must not traverse a symlink")
    if git(repo, "ls-files", "--", ".agent-kit/logs/task-metrics") or git(
            repo, "check-ignore", ".agent-kit/logs/task-metrics/.probe", optional=True) is None:
        raise ValueError("Task journals must be untracked and ignored; install the kit first")
    return root


def begin(args):
    repo, started = args.repo.resolve(), timestamp()
    with locked(repo):
        source = checkout(repo)
        contract = safe_file(repo, args.contract)
        contract_sha = digest(contract.read_text().encode())
        profile = evidence(repo, [p for p in (
            ".agent-kit/policy/model-routing.json", ".agent-kit/context-tools.json",
            ".codex/config.toml", ".codex/hooks.json", ".claude/settings.json",
            ".agent-kit/verification.json") if (repo / p).is_file()])
        exp, application = None, None
        if args.experiment:
            exp = validate_experiment(read_json(args.experiment))
            case = exp["cases"].get(args.case)
            if (not case or args.trial not in case["trials"] or args.variant not in ("baseline", "candidate")
                    or case["category"] != args.category or case["contract_sha256"] != contract_sha):
                raise ValueError("Run does not match its predeclared case/trial/contract/category")
            application = application_fingerprint(repo, case["application_paths"])
        elif any((args.case, args.trial, args.variant)):
            raise ValueError("Case/trial/variant require an experiment manifest")
        for item in (args.task, args.category, args.environment):
            if not item.strip():
                raise ValueError("Task, category and environment labels must be nonempty")
        record = {"version": 1, "id": uuid.uuid4().hex, "repo": str(repo),
                  "task": args.task, "category": args.category, "environment": args.environment,
                  "kit_version": KIT_VERSION, "profile": profile, "started_at": started,
                  "registration": args.registration, "start_head": source["head"],
                  "branch": source["branch"], "contract": str(contract.relative_to(repo)),
                  "contract_sha256": contract_sha, "experiment": exp, "case": args.case,
                  "trial": args.trial, "variant": args.variant, "application_sha256": application,
                  "status": "running", "finished_at": None, "elapsed_seconds": None,
                  "acceptance": None, "components": {}, "observations": []}
        root = journal(repo)
        # Retries belong to one task run, never a replacement success-only sample.
        if exp:
            for path in root.glob("*.json"):
                old = read_json(path)
                if old.get("experiment") and old["experiment"]["id"] == exp["id"]:
                    if old["experiment"] != exp:
                        raise ValueError("Experiment changed; use a new experiment ID")
                    if (old["case"], old["trial"], old["variant"]) == (args.case, args.trial, args.variant):
                        raise ValueError("Trial already registered; keep retries in its existing run")
        path = root / f"{record['id']}.json"
        write(path, record)
        print(path)


def load_record(path):
    record = validate_record(read_json(path))
    repo = Path(record["repo"])
    if path.absolute() != journal(repo) / f"{record['id']}.json":
        raise ValueError("Update the original checkout journal, not a copied report")
    return repo, record


def observe(repo, record, path):
    value = validate_observations(read_json(path))
    now = timestamp()
    if value["observed_through"] is not None:
        observed = moment(value["observed_through"])
        if not moment(record["started_at"]) <= observed <= moment(now):
            raise ValueError("Observation must fall between task start and now")
        prior = record["observations"][-1]["value"]["observed_through"] if record["observations"] else None
        if prior and observed < moment(prior):
            raise ValueError("Observation windows cannot go backwards")
    refs = [*value["usage"]["evidence"], *value["cost"]["evidence"], *value["quality_evidence"]]
    record["observations"].append({"recorded_at": now, "value": value, "evidence": evidence(repo, refs)})


def update(args):
    path = args.run.absolute()
    repo, _ = load_record(path)
    with locked(repo):
        _, record = load_record(path)
        if args.action in ("collect", "finish") and record["status"] != "running":
            raise ValueError("Task already finalized; only append quality/measurement observations")
        if args.action == "collect":
            directory = args.review.absolute()
            state = read_json(directory / "run.json")
            if (directory.parent != repo / ".agent-kit/reviews" or state["repo"] != str(repo)
                    or state["task_sha256"] != record["contract_sha256"] or state["branch"] != record["branch"]):
                raise ValueError("Review belongs to a different checkout, branch or contract")
            # Replacement makes repeat collection idempotent, including resumed runs.
            record["components"][directory.name] = {
                "kind": "sol_review_only", "phase": state["phase"], "head": state["head"],
                "metrics": metrics(directory, state["rounds"])}
        elif args.action == "observe":
            observe(repo, record, args.observations)
        else:
            if not args.note.strip():
                raise ValueError("Finalization requires an outcome note")
            try:
                contract = safe_file(repo, record["contract"])
                contract_changed = digest(contract.read_text().encode()) != record["contract_sha256"]
            except (OSError, ValueError):
                if args.outcome == "accepted":
                    raise
                contract_changed = True
            if args.outcome == "accepted" and contract_changed:
                raise ValueError("Acceptance contract changed; record scope change separately, do not finish this sample")
            if args.outcome == "accepted":
                if not args.review:
                    raise ValueError("Accepted outcome requires a current ACCEPTED review run")
                state = current_run(args.review.absolute())
                if (state["phase"] != "ACCEPTED" or state["repo"] != str(repo) or
                        state["branch"] != record["branch"] or state["task_sha256"] != record["contract_sha256"]):
                    raise ValueError("Acceptance needs this branch, contract and the existing review gates")
                if record["registration"] == "before-planning" and moment(state["created_at"]) < moment(record["started_at"]):
                    raise ValueError("Review predates task registration; register honestly as late")
                record["acceptance"] = {"head": state["head"], "source_sha256": state["source"]["fingerprint"],
                                        "review": str(args.review.absolute().relative_to(repo)),
                                        "run_sha256": digest((args.review / "run.json").read_bytes())}
            elif args.review:
                raise ValueError("Use collect for failed review components; --review here attests acceptance")
            ended = timestamp()
            elapsed = (moment(ended) - moment(record["started_at"])).total_seconds()
            if elapsed < 0:
                raise ValueError("Clock moved backwards; inspect timestamps before finishing")
            if args.observations:
                observe(repo, record, args.observations)
            record.update(status=args.outcome, finished_at=ended, elapsed_seconds=elapsed,
                          outcome_note=args.note, contract_changed=contract_changed)
        validate_record(record)
        write(path, record)
        print(json.dumps({"run": str(path), "status": record["status"]}))


def latest(record):
    return record["observations"][-1]["value"] if record["observations"] else blank_observations()


def validate_record(record):
    """Check portable evidence consistency without claiming identity authentication."""
    if (not isinstance(record, dict) or record.get("version") != 1 or
            record.get("status") not in ("running", "accepted", "rejected", "abandoned")):
        raise ValueError("Invalid task record")
    for key in ("id", "category", "registration", "started_at", "environment"):
        if not isinstance(record[key], str) or not record[key].strip():
            raise ValueError(f"Missing record field: {key}")
    if record["registration"] not in ("before-planning", "late"):
        raise ValueError("Invalid registration timing")
    if not isinstance(record["components"], dict) or not isinstance(record["observations"], list):
        raise ValueError("Invalid component/observation records")
    started, now = moment(record["started_at"]), moment(timestamp())
    if started > now:
        raise ValueError("Task starts in the future")
    if record["status"] == "running":
        if record["finished_at"] is not None or record["elapsed_seconds"] is not None or record["acceptance"] is not None:
            raise ValueError("Running task contradicts finalization fields")
    else:
        finished = moment(record["finished_at"])
        number(record["elapsed_seconds"], "elapsed_seconds")
        if not started <= finished <= now or record["elapsed_seconds"] is None or abs(
                record["elapsed_seconds"] - (finished - started).total_seconds()) > .001:
            raise ValueError("Elapsed time contradicts start/finish timestamps")
    if record["status"] == "accepted":
        acceptance = record["acceptance"]
        if not isinstance(acceptance, dict) or any(not acceptance.get(k) for k in (
                "head", "source_sha256", "review", "run_sha256")):
            raise ValueError("Accepted task is missing review evidence")
    prior, cutoff = started, None
    for obs in record["observations"]:
        fields(obs, ("recorded_at", "value", "evidence"))
        validate_observations(obs["value"])
        recorded = moment(obs["recorded_at"])
        if not prior <= recorded <= now:
            raise ValueError("Observation recording timestamps are out of order or in the future")
        through = obs["value"]["observed_through"]
        if through is not None:
            observed = moment(through)
            if not started <= observed <= recorded or (cutoff and observed < cutoff):
                raise ValueError("Observation cutoff is inconsistent with its recording timestamp/history")
            cutoff = observed
        if not isinstance(obs["evidence"], list):
            raise ValueError("Invalid observation evidence hashes")
        referenced = set(obs["value"]["usage"]["evidence"] + obs["value"]["cost"]["evidence"] + obs["value"]["quality_evidence"])
        for ref in obs["evidence"]:
            fields(ref, ("path", "sha256"))
            if not isinstance(ref["sha256"], str) or len(ref["sha256"]) != 64:
                raise ValueError("Evidence reference is missing its SHA256")
        if referenced != {ref["path"] for ref in obs["evidence"]}:
            raise ValueError("Observation evidence references do not match captured hashes")
        prior = recorded
    if record["experiment"]:
        validate_experiment(record["experiment"])
        for key in ("case", "trial", "variant", "contract_sha256", "application_sha256"):
            if not isinstance(record[key], str):
                raise ValueError(f"Missing experiment field: {key}")
    return record


def percentile(values, fraction):
    return sorted(values)[max(0, math.ceil(len(values) * fraction) - 1)] if values else None


def summary(records):
    accepted = [r for r in records if r["status"] == "accepted"]
    terminal = [r for r in records if r["status"] != "running"]
    costs = [latest(r)["cost"] for r in records]
    cost_complete = bool(records) and len(terminal) == len(records) and all(
        c["coverage"] == "complete" for c in costs) and len({c["basis"] for c in costs}) == 1
    usage = [latest(r)["usage"] for r in records]
    usage_complete = bool(records) and len(terminal) == len(records) and all(u["coverage"] == "complete" for u in usage)
    elapsed = [r["elapsed_seconds"] for r in accepted]
    observations = [latest(r) for r in records]
    def known_total(values):
        return sum(values) if values and all(v is not None for v in values) else None
    return {"registered": len(records), "accepted": len(accepted),
            "rejected": sum(r["status"] == "rejected" for r in records),
            "abandoned": sum(r["status"] == "abandoned" for r in records),
            "running": len(records) - len(terminal),
            "late_registrations": sum(r["registration"] != "before-planning" for r in records),
            "accepted_median_seconds": statistics.median(elapsed) if elapsed else None,
            "accepted_p90_seconds": percentile(elapsed, .9),
            "terminal_total_seconds": sum(r["elapsed_seconds"] for r in terminal),
            "failed_total_seconds": sum(r["elapsed_seconds"] for r in terminal if r["status"] != "accepted"),
            "cost_complete": cost_complete, "cost_basis": costs[0]["basis"] if cost_complete else None,
            "total_usd": sum(c["usd"] for c in costs) if cost_complete else None,
            "usd_per_accepted": sum(c["usd"] for c in costs) / len(accepted) if cost_complete and accepted else None,
            "cost_missing_runs": sum(c["coverage"] != "complete" for c in costs),
            "task_usage_complete": usage_complete,
            "task_tokens": {k: sum(u[k] for u in usage) if usage_complete and all(u[k] is not None for u in usage)
                            else None for k in TOKEN_KEYS},
            "quality_missing_runs": sum(any(v is None for v in latest(r)["quality"].values()) for r in records),
            "human_seconds_missing_runs": sum(latest(r)["human_seconds"] is None for r in records),
            "human_seconds": known_total([o["human_seconds"] for o in observations]),
            "repair_attempts": known_total([o["repair_attempts"] for o in observations]),
            "quality_totals": {k: known_total([o["quality"][k] for o in observations]) for k in QUALITY_KEYS},
            "sol_component_count": sum(len(r["components"]) for r in records)}


def comparison(records, experiment):
    """Completeness is distinct from meeting an objective; never auto-adopt."""
    experiment = validate_experiment(experiment)
    rows = [r for r in records if r["experiment"] and r["experiment"]["id"] == experiment["id"]]
    issues, cells, pairs, jointly_accepted = [], {}, [], []
    for row in rows:
        key = (row["case"], row["trial"], row["variant"])
        if key in cells:
            issues.append(f"Duplicate trial: {key}")
        cells[key] = row
        case = experiment["cases"].get(row["case"])
        if row["experiment"] != experiment or not case or row["trial"] not in case["trials"]:
            issues.append(f"Manifest drift: {row['id']}")
        elif row["contract_sha256"] != case["contract_sha256"] or row["category"] != case["category"]:
            issues.append(f"Contract/category drift: {row['id']}")
        if row["registration"] != "before-planning" or row["status"] == "running":
            issues.append(f"Late or unfinished task: {row['id']}")
        if row["variant"] not in ("baseline", "candidate") or row.get("contract_changed"):
            issues.append(f"Variant/contract drift: {row['id']}")
    for case, spec in experiment["cases"].items():
        for trial in spec["trials"]:
            base, candidate = (cells.get((case, trial, v)) for v in ("baseline", "candidate"))
            if base is None or candidate is None:
                issues.append(f"Missing pair: {case}/{trial}")
                continue
            if (not base["application_sha256"] or base["application_sha256"] != candidate["application_sha256"]
                    or base["environment"] != candidate["environment"]):
                issues.append(f"Different application/environment baseline: {case}/{trial}")
            pairs.append({"case": case, "trial": trial, "baseline_status": base["status"],
                          "candidate_status": candidate["status"],
                          "elapsed_delta_seconds": candidate["elapsed_seconds"] - base["elapsed_seconds"]
                          if base["elapsed_seconds"] is not None and candidate["elapsed_seconds"] is not None else None})
            if base["status"] == candidate["status"] == "accepted":
                jointly_accepted.append((base["elapsed_seconds"], candidate["elapsed_seconds"]))
    if len(pairs) < experiment["minimum_pairs"]:
        issues.append("Fewer than the predeclared minimum pairs")
    groups = {v: [r for r in rows if r["variant"] == v] for v in ("baseline", "candidate")}
    totals = {v: summary(rs) for v, rs in groups.items()}
    for row in rows:
        obs = latest(row)
        if obs["human_seconds"] is None or any(obs["quality"][k] is None for k in QUALITY_KEYS):
            issues.append(f"Missing quality/human observations: {row['id']}")
        # Compare the same elapsed observation window, not a mature baseline to a new candidate.
        if (row["status"] == "accepted" and (obs["quality_window_days"] != experiment["quality_window_days"] or
                not obs["observed_through"] or
                (moment(obs["observed_through"]) - moment(row["finished_at"])).total_seconds()
                < experiment["quality_window_days"] * 86400)):
            issues.append(f"Quality window not observed: {row['id']}")
    baseline, candidate = totals["baseline"], totals["candidate"]
    metric = "usd_per_accepted" if experiment["objective"] == "cost_per_accepted" else "accepted_median_seconds"
    if metric == "usd_per_accepted" and (not baseline["cost_complete"] or not candidate["cost_complete"]
            or baseline["cost_basis"] != candidate["cost_basis"]):
        issues.append("Cost coverage/basis is incomplete or incompatible")
    if metric == "accepted_median_seconds":
        if len(jointly_accepted) < experiment["minimum_pairs"]:
            issues.append("Fewer than the minimum jointly accepted pairs for latency")
        a, b = (statistics.median([p[i] for p in jointly_accepted]) if jointly_accepted else None for i in (0, 1))
    else:
        a, b = baseline[metric], candidate[metric]
    if a is None or b is None or a <= 0:
        issues.append("Objective has no usable accepted-task baseline")
    quality_ok = all(
        sum(latest(r)["quality"][key] or 0 for r in groups["candidate"]) <=
        sum(latest(r)["quality"][key] or 0 for r in groups["baseline"])
        for key in ("escaped_defects", "repair_regressions"))
    human_ok = sum(latest(r)["human_seconds"] or 0 for r in groups["candidate"]) <= sum(
        latest(r)["human_seconds"] or 0 for r in groups["baseline"])
    acceptance_ok = all(p["candidate_status"] == "accepted" for p in pairs if p["baseline_status"] == "accepted")
    improvement = (a - b) / a if a is not None and b is not None and a > 0 else None
    return {"experiment": experiment["id"], "data_complete": not issues,
            "issues": sorted(set(issues)), "pairs": pairs, "variants": totals,
            "objective_values": {"baseline": a, "candidate": b, "jointly_accepted_pairs": len(jointly_accepted)},
            "improvement_fraction": improvement,
            "objective_and_guardrails_met": (improvement >= experiment["minimum_improvement_fraction"] and
                acceptance_ok and candidate["accepted"] >= baseline["accepted"] and quality_ok and human_ok) if not issues else None,
            "decision": "Independent assessment required; this command never changes policy"}


def report(args):
    records, errors, ids = [], [], {}
    paths = list(args.input)
    for directory in args.directory:
        if not directory.is_dir():
            errors.append(f"Missing journal directory: {directory}")
        paths.extend(sorted(directory.glob("*.json")))
    for path in paths:
        try:
            data = read_json(path)
            if not isinstance(data, dict):
                raise ValueError("Expected a task record or snapshot object")
            if data.get("kind") == "task-metrics-snapshot":
                if data.get("version") != 1 or not isinstance(data.get("records"), list):
                    raise ValueError("Invalid snapshot envelope")
                strings(data["input_errors"], "inherited input_errors")
                errors.extend(f"{path}: inherited input error: {error}" for error in data["input_errors"])
                values = data["records"]
            else:
                values = [data]
            for record in values:
                validate_record(record)
                if record["id"] in ids:
                    if ids[record["id"]] != record:
                        raise ValueError("Conflicting copies of one run; supply its latest snapshot only")
                    continue
                ids[record["id"]] = record
                records.append(record)
        except (OSError, ValueError, KeyError, TypeError) as exc:
            errors.append(f"{path}: {exc}")
    result = {"kind": "task-metrics-snapshot", "version": 1, "generated_at": timestamp(),
              "limitations": "Descriptive local evidence, not a causal or statistical proof. Latency is registration-to-finish wall time including waits. P90 uses nearest rank. Sol components are subsets, never added to whole-task totals.",
              "input_errors": errors, "summary": summary(records),
              "by_category": {c: summary([r for r in records if r["category"] == c])
                              for c in sorted({r["category"] for r in records})},
              "comparison": comparison(records, read_json(args.experiment)) if args.experiment else None,
              "records": records}
    if errors and result["comparison"]:
        result["comparison"].update(data_complete=False, objective_and_guardrails_met=None)
        result["comparison"]["issues"].append("Unreadable/conflicting input records")
    if args.output:
        # A portable snapshot freezes the evidence before ignored worktrees disappear.
        write(args.output.absolute(), result)
        print(json.dumps({"snapshot": str(args.output), "sha256": digest(args.output.read_bytes())}))
    print(json.dumps({k: v for k, v in result.items() if k != "records"}, indent=2))
    return 1 if errors else 0


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest="action", required=True)
    start = sub.add_parser("begin")
    start.add_argument("--repo", type=Path, default=Path.cwd())
    for key in ("task", "category", "environment"):
        start.add_argument(f"--{key}", required=True)
    start.add_argument("--contract", type=Path, required=True)
    start.add_argument("--registration", choices=("before-planning", "late"), default="before-planning")
    start.add_argument("--experiment", type=Path)
    for key in ("case", "trial", "variant"):
        start.add_argument(f"--{key}")
    for action in ("collect", "observe", "finish"):
        command = sub.add_parser(action)
        command.add_argument("--run", type=Path, required=True)
        if action in ("collect", "finish"):
            command.add_argument("--review", type=Path, required=action == "collect")
        if action in ("observe", "finish"):
            command.add_argument("--observations", type=Path, required=action == "observe")
        if action == "finish":
            command.add_argument("--outcome", choices=("accepted", "rejected", "abandoned"), required=True)
            command.add_argument("--note", required=True)
    output = sub.add_parser("report")
    output.add_argument("--input", type=Path, action="append", default=[])
    output.add_argument("--directory", type=Path, action="append", default=[])
    output.add_argument("--experiment", type=Path)
    output.add_argument("--output", type=Path)
    args = parser.parse_args()
    try:
        if args.action == "begin":
            begin(args)
        elif args.action == "report":
            return report(args)
        else:
            update(args)
        return 0
    except (OSError, ValueError, KeyError, TypeError) as exc:
        print(f"Metrics not recorded: {exc}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    sys.exit(main())
