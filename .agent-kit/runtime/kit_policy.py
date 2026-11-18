#!/usr/bin/env python3
"""Deterministic model selection from an agent's evidence-backed task assessment.

The evaluator enforces the selected profile, not the truth of task classification.
Unknown facts cannot qualify for familiar implementation; evidence is required.
"""

import argparse
import json
from pathlib import Path
import sys

from kit_config import ancestors_in_repo, digest

POLICY = Path(__file__).resolve().parents[1] / "policy/model-routing.json"
# Sol models the review/fix rounds may run on; the policy names one.
SOL_REVIEW_MODELS = ("gpt-6.1-sol", "gpt-6.2-sol")
FACTS = ("familiar_pattern", "precise_scope", "localized_change", "deterministic_checks",
         "low_risk", "no_prior_failure", "ambiguity_resolved")


def load_policy(path=None):
    if path is None:
        roots = list(ancestors_in_repo(Path.cwd()))
        path = next((p / ".agent-kit/policy/model-routing.json" for p in roots
                     if (p / ".agent-kit/policy/model-routing.json").is_file()), None)
        source = Path(__file__).resolve().parents[2]
        if path is None and (roots[-1] / ".git").exists() and not (
                roots[-1] == source and (source / "profiles/hybrid.json").is_file()):
            raise ValueError("Configure this checkout/worktree explicitly; no local routing profile")
        path = path or POLICY
        if not Path(path).exists() and not POLICY.exists():
            path = Path(__file__).resolve().parents[2] / "profiles/hybrid.json"
    raw = Path(path).read_bytes()
    value = json.loads(raw)
    if (not isinstance(value, dict) or type(value.get("version")) is not int or value.get("version") != 2 or
            value.get("mode") not in {"hybrid", "codex-only"} or
            value["implementation"]["familiar_requires_all"] != list(FACTS)):
        raise ValueError("Unsupported routing policy or eligibility criteria")
    codex = value["mode"] == "codex-only"
    lead, helper = ("gpt-6.1-sol", "gpt-6.1-sol") if codex else ("opus", "haiku")
    normal, risk = ("high", "xhigh") if codex else ("medium", "high")
    review = value["review"]
    if (type(review["default_rounds"]) is not int or type(review["maximum_rounds"]) is not int or
            not 1 <= review["default_rounds"] <= review["maximum_rounds"] <= 5 or
            review["model"] not in SOL_REVIEW_MODELS or review["effort"] != "high" or
            review["optional_effort"] != "xhigh" or review["original_author_required"] is not True or
            review["stop_on_repeated_findings"] is not True or review["final_model"] != lead or
            review["final_default_effort"] != normal or review["final_risk_effort"] != risk):
        raise ValueError("Routing policy violates supported review boundaries")
    expected = {"familiar": {"model": "gpt-6.1-sol" if codex else "sonnet", "effort": "medium"},
                "default": {"model": lead, "effort": normal},
                "difficult": {"model": lead, "effort": risk}}
    if any(value["implementation"][k] != v for k, v in expected.items()):
        raise ValueError("Unsupported implementation model mapping")
    if (value["planner"] != {"model": lead, "default_effort": normal, "risk_effort": risk} or
            value["architecture_review"] != {"model": lead, "effort": risk} or
            value["implementation"]["role"] != "fullstack-engineer" or
            value["lookup"] != {"role": "codebase-scout", "model": helper, "effort": "low", "implementation_allowed": False}):
        raise ValueError("Unsupported planning/helper policy")
    return value, digest(raw)


def route(assessment, policy=None):
    policy, checksum = policy or load_policy()
    if not isinstance(assessment, dict) or set(assessment) != {"version", "task_id", "kind", "facts", "evidence"}:
        raise ValueError("Assessment requires version, task_id, kind, facts and evidence")
    if (type(assessment["version"]) is not int or assessment["version"] != 1 or
            not isinstance(assessment["task_id"], str) or not assessment["task_id"].strip() or
            assessment["kind"] not in {"implementation", "lookup"}):
        raise ValueError("Invalid task identity/kind")
    facts, evidence = assessment["facts"], assessment["evidence"]
    if not isinstance(facts, dict) or set(facts) != set(FACTS) or not isinstance(evidence, dict) or set(evidence) != set(FACTS):
        raise ValueError("Supply every assessment fact and its evidence (unknown is null)")
    for key in FACTS:
        if facts[key] is not None and type(facts[key]) is not bool:
            raise ValueError(f"{key} must be true, false or null")
        refs = evidence[key]
        if not isinstance(refs, list) or any(not isinstance(ref, str) or not ref.strip() for ref in refs):
            raise ValueError(f"{key} evidence must be a list of source references and explanations")
        if facts[key] is not None and not refs:
            raise ValueError(f"Known fact {key} requires evidence; otherwise use null")
    difficult = facts["low_risk"] is not True or facts["ambiguity_resolved"] is not True or facts["no_prior_failure"] is False
    planner = {"model": policy["planner"]["model"],
               "effort": policy["planner"]["risk_effort" if difficult else "default_effort"]}
    if assessment["kind"] == "lookup":
        if facts["precise_scope"] is not True or facts["low_risk"] is not True or facts["ambiguity_resolved"] is not True:
            raise ValueError("Lookup requires precise bounded extraction with resolved ambiguity and low risk")
        worker = dict(policy["lookup"])
        review = None
        reason = "Read-only lookup/extraction; no implementation or acceptance delegation"
    else:
        eligible = all(facts[key] is True for key in FACTS)
        level = "familiar" if eligible else "difficult" if difficult else "default"
        worker = {"role": policy["implementation"]["role"], **policy["implementation"][level], "implementation_allowed": True}
        review = {**policy["review"], "final_effort": policy["review"]["final_risk_effort" if difficult else "final_default_effort"]}
        reason = "Every familiarity criterion is evidenced" if eligible else "Stronger implementation route required: " + ", ".join(key for key in FACTS if facts[key] is not True)
    return {"version": 2, "task_id": assessment["task_id"], "kind": assessment["kind"], "mode": policy["mode"],
            "policy_id": policy["policy_id"], "policy_sha256": checksum,
            "assessment_sha256": digest(json.dumps(assessment, sort_keys=True).encode()),
            "planner": planner, "architecture_review": policy["architecture_review"], "worker": worker, "review": review, "reason": reason,
            "classification_source": "Agent assessment; evidence is recorded, not independently authenticated"}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--assessment", required=True, type=Path)
    parser.add_argument("--policy", type=Path, help="Defaults to the installed project profile")
    args = parser.parse_args()
    try:
        print(json.dumps(route(json.loads(args.assessment.read_text()), load_policy(args.policy)), indent=2))
        return 0
    except (OSError, ValueError, TypeError, KeyError) as exc:
        print(f"Routing failed: {exc}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    sys.exit(main())
