#!/usr/bin/env python3
"""Inspect setup; --live probes services/login, --checks runs configured project checks. No model calls."""

import argparse
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys

from kit_config import KIT_VERSION, MANIFEST, digest, read_manifest, read_settings, runtime_file
from kit_config import CheckError, codex_instruction_file, load_checks, installed_mode
from kit_policy import load_policy
from kit_context import load_config as load_context_config, tool as context_tool, INDEX, LAUNCHER, SERVER
from kit_memory import audit as memory_audit
from kit_skills import load as load_skills
from kit_context import safe, write
from kit_evidence import timestamp


def diagnose(project, config_dir):
    findings = []
    try:
        mode = installed_mode(project)
    except (OSError, ValueError, TypeError, AttributeError) as exc:
        raise ValueError(f"Routing policy invalid: {exc}") from exc

    def add(level, message):
        findings.append({"level": level, "message": message})

    add("info", f"Diagnostic runner {KIT_VERSION}; Python {sys.version.split()[0]}")
    try:
        memory = memory_audit(project)
        add("info", f"Root guidance: {memory['root_guidance_characters']} characters before roles/skills/tools/history; this is not a token count")
        for warning in memory["warnings"]:
            add("warning", f"Context growth: {warning['path']} has {warning['characters']} characters "
                f"(soft limit {warning['soft_limit']}). {warning['action']}")
        if memory["omitted_warnings"]:
            add("warning", f"{memory['omitted_warnings']} further context growth warnings; curate oversized task/maps files")
    except (OSError, ValueError) as exc:
        add("warning", f"Could not audit memory growth: {exc}")
    try:
        from kit_hindsight import configured
        memory_config, _ = configured(project, check_project=False)
        add("info", "Hindsight 1.2.1: local embed daemon/pg0, CPU embeddings/reranker; reviewed-record bank uses chunks; "
            "setup probe recorded. Run kit_memory.py status for current service availability.")
        if not memory_config["enabled"]:
            add("warning", "Semantic memory is explicitly disabled; scoped Git memory remains available")
    except (OSError, ValueError, KeyError, TypeError) as exc:
        add("error", f"Hindsight memory setup incomplete: {exc}")
    try:
        policy, checksum = load_policy(project / ".agent-kit/policy/model-routing.json")
        add("info", f"Routing policy {policy['policy_id']} ({checksum[:12]}): {policy['review']['default_rounds']} default review rounds; hard cap {policy['review']['maximum_rounds']}")
    except (OSError, ValueError, KeyError, TypeError) as exc:
        add("error", f"Routing policy invalid: {exc}")
    try:
        catalog, checksum, _ = load_skills(project)
        add("info", f"Project skill catalog ({checksum[:12]}): {len(catalog['skills'])} scoped skills and {len(catalog['roles'])} project role briefs; source evidence current")
    except (OSError, ValueError, KeyError, TypeError) as exc:
        add("error", f"Project capability setup invalid: {exc}")
    try:
        context = load_context_config(project)
        backend = context["backend"]
        if backend == "unconfigured":
            add("error", "Repowise setup pending: run kit_context.py setup with a known symbol and source path")
        else:
            context_tool(project, context)
            if not (project / INDEX[backend]).is_dir():
                add("error", f"{backend} index missing; run kit_context.py prepare")
            mcp = json.loads((project / ".mcp.json").read_text()) if mode == "hybrid" else {"mcpServers": {}}
            if not isinstance(mcp, dict) or not isinstance(mcp.get("mcpServers", {}), dict):
                raise ValueError("Invalid .mcp.json server mapping")
            expected = {"command": "python3", "args": ["-c", LAUNCHER, "serve"]}
            codex = (project / ".codex/config.toml").read_text()
            if (mode == "hybrid" and mcp.get("mcpServers", {}).get(SERVER) != expected) or (f"[mcp_servers.{SERVER}]" not in codex or
                    f"args = {json.dumps(expected['args'])}" not in codex or 'enabled_tools = ["lookup", "recall"]' not in codex or
                    'env_vars = ["AGENT_KIT_MEMORY_ACCESS", "AGENT_KIT_MEMORY_BACKEND"]' not in codex):
                add("error", "Context MCP wiring missing; run kit_context.py configure")
            if not (project / ".agent-kit/logs/context/index-state.json").is_file():
                add("error", "No completed managed refresh recorded; run kit_context.py prepare")
            setup = json.loads((project / ".agent-kit/logs/context/setup.json").read_text())
            if not isinstance(setup, dict) or any(not isinstance(setup.get(k), dict) for k in ("probe", "runtime")):
                raise ValueError("Malformed context setup record; rerun kit_context.py setup")
            if (setup.get("status") != "CONTEXT_READY" or setup.get("root") != str(project) or
                    setup.get("config_sha256") != digest((project / ".agent-kit/context-tools.json").read_bytes()) or
                    setup.get("probe", {}).get("mcp_roundtrip") != "passed"):
                add("error", "Context setup evidence is blocked/mismatched; rerun kit_context.py setup")
            if setup.get("runtime", {}).get("managed") is True and setup["runtime"].get("requirements_sha256") != digest((project / ".agent-kit/policy/repowise-requirements.txt").read_bytes()):
                add("error", "Context dependency lock changed; rerun kit_context.py setup")
            add("info", f"Fixed context stack: {backend} {context['expected_version']}; verify lookup/root/freshness in both harnesses")
    except (OSError, ValueError, KeyError, TypeError, subprocess.SubprocessError) as exc:
        add("error", f"Context setup invalid: {exc}")
    if sys.version_info < (3, 10):
        add("error", "Python 3.10 or later is required")
    for tool in (("bash", "git", "claude", "codex") if mode == "hybrid" else ("git", "codex")):
        executable = shutil.which(tool)
        add("info" if executable else "error", f"{tool}: {executable or 'not installed/on PATH'}")
        if tool in {"claude", "codex"} and executable:
            try:
                result = subprocess.run([executable, "--version"], capture_output=True, text=True, timeout=5)
                add("info" if result.returncode == 0 else "error",
                    f"{tool} version probe: " + (result.stdout.strip()[:200] or f"exit {result.returncode}"))
            except (OSError, subprocess.TimeoutExpired) as exc:
                add("error", f"{tool} version probe failed: {exc}")
    add("info", "Sol review default: gpt-6.2-sol / high; explicit fresh exec then resume by ID. "
        "Version probes do not establish working authentication or model availability.")
    try:
        guidance = codex_instruction_file(project)
        if guidance.is_symlink() or not guidance.is_file():
            raise ValueError(f"{guidance.name} must be a regular, unlinked instruction file")
        if ".agent-kit/WORKFLOW.md" not in guidance.read_text():
            add("error", f"Effective Codex instructions {guidance.name} lack .agent-kit/WORKFLOW.md; "
                "rerun the full installer or merge the workflow pointer while preserving project standards")
        else:
            add("info", f"Effective root Codex instructions: {guidance.name}; workflow pointer present")
    except (OSError, ValueError) as exc:
        add("error", f"Codex instruction entry point invalid: {exc}")
    for relative in (".codex/config.toml", ".agents/skills/code-navigation/SKILL.md",
                     ".agents/skills/task-routing/SKILL.md", ".agents/skills/review-cycle/SKILL.md",
                     ".agents/skills/cost-aware-execution/SKILL.md", ".agent-kit/reference/ADOPT.md",
                     ".agents/skills/indexed-context/SKILL.md", ".agents/skills/project-memory/SKILL.md",
                     ".agents/skills/setup-health/SKILL.md", ".agent-kit/reference/HINDSIGHT.md",
                     ".agent-kit/reference/CONTEXT_TOOLS.md"):
        if not (project / relative).is_file():
            add("warning", f"Cross-harness setup missing: {relative}")
    add("info", "Navigation stack: Repowise symbol/context plus bounded source search and the project's compiler/type checks. "
        "No extra navigation server is required. Structural retrieval does not prove typed LSP parity or actual harness access.")

    try:
        hooks = read_settings(project / ".codex/hooks.json")["hooks"]
        manifest = read_manifest(project / ".agent-kit" / MANIFEST)
        expected_hash = manifest["files"].get(".codex/hooks.json")
        if not expected_hash or digest((project / ".codex/hooks.json").read_bytes()) != expected_hash:
            add("error", "Codex hooks changed since installation; reconcile and review their trust")
        for event, matcher, action, timeout in (
            ("PreToolUse", "apply_patch|Edit|Write", "guard", 10),
            ("PostToolUse", "apply_patch|Edit|Write", "format", 40),
            ("SubagentStop", "^fullstack-engineer$", "verify", 360),
            ("SessionStart", "", "checkpoint", 10),
            ("PreCompact", "", "checkpoint", 10),
        ):
            matches = [hook for group in hooks.get(event, []) if group.get("matcher", "") == matcher
                       for hook in group["hooks"] if hook.get("type") == "command" and
                       ".agent-kit/runtime/kit_codex_hooks.py" in hook.get("command", "") and
                       hook["command"].endswith(" " + action)]
            if len(matches) != 1:
                add("error", f"Codex {event}: expected exactly one starter-kit hook; found {len(matches)}")
            elif matches[0].get("async") or matches[0].get("timeout", 0) < timeout:
                add("error", f"Codex {event}: hook must run synchronously with timeout >= {timeout}s")
        roles = ("staff-engineer", "fullstack-engineer", "code-reviewer", "architecture-reviewer", "codebase-scout") if mode == "codex-only" else ("code-reviewer",)
        for role in roles:
            if not (project / ".codex/agents" / (role + ".toml")).is_file():
                add("error", f"Native Codex role missing: {role}")
        add("warning", "Codex /hooks trust is required on the actual host; static files do not prove hooks ran. Explicit checks and review gates are mandatory.")
    except (OSError, ValueError, KeyError, TypeError) as exc:
        add("error", f"Codex adapter invalid: {exc}")

    for root, manifest_dir in (((config_dir, config_dir), (project, project / ".agent-kit")) if mode == "hybrid" else ((project, project / ".agent-kit"),)):
        path = manifest_dir / MANIFEST
        if not path.is_file():
            if root == config_dir and all((project / ".claude/agents" / f"{role}.md").is_file() for role in
                                         ("staff-engineer", "fullstack-engineer", "code-reviewer", "codebase-scout", "test-runner")):
                add("info", "Project-local roles installed; a starter-kit global profile is not required")
                continue
            add("error", f"No installation manifest: {path}; install/upgrade this scope first")
            continue
        try:
            manifest = read_manifest(path)
            add("info", f"{manifest_dir}: recorded kit version {manifest.get('kit_version', 'unknown')}")
            for relative, checksum in manifest["files"].items():
                if runtime_file(relative):
                    installed = root / relative
                    if not installed.is_file() or installed.is_symlink() or digest(installed.read_bytes()) != checksum:
                        add("error", f"Missing/modified managed runtime: {installed}; review before upgrading")
        except (ValueError, OSError) as exc:
            add("error", str(exc))

    if mode == "hybrid":
        layers = []
        for path in (config_dir / "settings.json", project / ".claude/settings.json",
                     project / ".claude/settings.local.json"):
            try:
                if path.exists():
                    layers.append((path, read_settings(path)))
            except (ValueError, OSError) as exc:
                add("error", str(exc))
        environment, selections = {}, {}
        hook_groups = {}
        for path, settings in layers:
            environment.update(settings.get("env", {}))
            selections.update({k: v for k, v in settings.items() if k in {"model", "agent"}})
            if settings.get("disableAllHooks") is True:
                add("error", f"Hooks disabled by {path}")
            for event, groups in settings.get("hooks", {}).items():
                hook_groups.setdefault(event, []).extend(groups)
        for name in ("CLAUDE_CODE_SUBAGENT_MODEL", "CLAUDE_CODE_SUBAGENT_MODEL_FORCE",
                     "CLAUDE_CODE_MAX_SUBAGENT_SPAWN_DEPTH", "CLAUDE_CODE_DISABLE_EXPLORE_PLAN_AGENTS", "ANTHROPIC_MODEL",
                     "CLAUDE_CODE_EFFORT_LEVEL"):
            if name in os.environ:
                environment[name] = os.environ[name]
            if name in environment:
                add("warning" if name in {"CLAUDE_CODE_SUBAGENT_MODEL_FORCE", "ANTHROPIC_MODEL", "CLAUDE_CODE_DISABLE_EXPLORE_PLAN_AGENTS", "CLAUDE_CODE_EFFORT_LEVEL"} else "info",
                    f"Configured {name}={environment[name]}")
        for name in ("DISABLE_PROMPT_CACHING", "DISABLE_PROMPT_CACHING_OPUS", "DISABLE_PROMPT_CACHING_SONNET",
                     "DISABLE_PROMPT_CACHING_HAIKU", "FORCE_PROMPT_CACHING_5M", "CLAUDE_CODE_PROMPT_CACHE_TTL",
                     "CLAUDE_CODE_SUBAGENT_PROMPT_CACHE_TTL"):
            value = os.environ.get(name, environment.get(name))
            if value is not None:
                add("warning" if value == "1" and name.startswith("DISABLE_") else "info", f"Cache setting {name}={value}")
        for path, settings in layers:
            for name in ("promptCacheTtl", "subagentPromptCacheTtl"):
                if name in settings:
                    add("info" if isinstance(settings[name], str) and settings[name] in {"5m", "1h"} else "warning", f"{path}: {name}={settings[name]}")
        add("info", "Caching remains provider-managed. Inspect Claude /usage and Sol metrics.json; static diagnostics cannot prove cache hits.")
        if environment.get("CLAUDE_CODE_MAX_SUBAGENT_SPAWN_DEPTH") != "2":
            add("warning", "Configured nesting depth differs from the starter-kit default of 2")
        add("info", f"Settings selections: agent={selections.get('agent', 'default')}, model={selections.get('model', 'default')}")
        if selections.get("agent") != "staff-engineer" or selections.get("model") != "opus":
            add("warning", "Existing agent/model selections differ from starter-kit defaults")

        for event, matcher, filename, timeout in (
            ("PreToolUse", "Edit|Write", "guard-secrets.sh", 10),
            ("PostToolUse", "Edit|Write", "post-edit-format.sh", 40),
            ("SubagentStop", "^fullstack-engineer$", "subagent-quality-gate.sh", 360),
            ("SessionStart", "", "memory-checkpoint.sh", 10),
            ("PreCompact", "", "memory-checkpoint.sh", 10),
        ):
            expected = f'bash "$CLAUDE_PROJECT_DIR"/.claude/hooks/{filename}'
            matches = [hook for group in hook_groups.get(event, []) if group.get("matcher", "") == matcher
                       for hook in group["hooks"] if hook.get("type") == "command" and hook.get("command") == expected]
            if len(matches) != 1:
                add("error", f"{event}: expected exactly one starter-kit hook; found {len(matches)}")
            elif matches[0].get("async") or matches[0].get("timeout", 0) < timeout:
                add("error", f"{event}: hook must run synchronously with timeout >= {timeout}s")
            if not (project / ".claude/hooks" / filename).is_file():
                add("error", f"Missing hook script {filename}")

        for role in ("staff-engineer", "fullstack-engineer", "code-reviewer", "codebase-scout", "test-runner"):
            candidates = [project / ".claude/agents" / f"{role}.md", config_dir / "agents" / f"{role}.md"]
            path = next((p for p in candidates if p.is_file()), None)
            if path is None:
                add("error", f"Missing role definition: {role}")
                continue
            fields = {}
            for line in path.read_text().split("---", 2)[1:2]:
                for field in line.splitlines():
                    key, sep, value = field.partition(":")
                    if sep and key in {"model", "effort", "maxTurns"}:
                        fields[key] = value.strip()
            add("info", f"Role {role}: {fields}; source={path}")
            if path == candidates[0]:
                add("warning", f"Project role overrides global role: {role}")

    try:
        checks, timeout = load_checks(project / ".agent-kit/verification.json", project)
        add("info", f"{len(checks)} configured checks; total budget {timeout}s (commands were not run)")
        for directory, argv in checks:
            executable = str(directory / argv[0]) if "/" in argv[0] else shutil.which(argv[0])
            available = executable and Path(executable).is_file() and os.access(executable, os.X_OK)
            add("info" if available else "error", f"Check tool {argv[0]} in {directory}: {'available' if available else 'missing/not executable'}")
    except (CheckError, OSError) as exc:
        add("error", str(exc))
    add("warning", "Live validation pending: confirm active roles, trusted hooks, observed model/effort and the full review cycle in the selected harness(es). "
        "Managed policy, CLI flags and runtime model resolution are not proven by this diagnostic.")
    return {"version": 1, "kit_version": KIT_VERSION, "mode": mode, "project": str(project),
            "configuration_ready": not any(f["level"] == "error" for f in findings),
            "live_validated": False, "findings": findings}


def live_checks(project, report, run_checks=False):
    """Failures stay independent and visible; never repair settings during diagnosis."""
    results = {}

    def check(name, action):
        try:
            results[name] = {"status": "PASSED", "evidence": action()}
        except (OSError, ValueError, KeyError, TypeError, CheckError, subprocess.SubprocessError) as exc:
            results[name] = {"status": "FAILED", "error": str(exc)[:1000]}

    def context_probe():
        from kit_context_setup import probe
        path = project / ".agent-kit/logs/context/setup.json"
        safe(path)
        prior = json.loads(path.read_text())["probe"]
        return probe(project, load_context_config(project), prior["query"], prior["expected_source"])

    def auth(tool):
        argv = [tool, "login", "status"] if tool == "codex" else [tool, "auth", "status", "--json"]
        result = subprocess.run(argv, capture_output=True, text=True, timeout=15)
        if result.returncode:
            raise ValueError(f"{tool} login status failed; run its login/status command interactively")
        billing = "unknown"
        if tool == "claude":
            value = json.loads(result.stdout)
            if value.get("loggedIn") is not True:
                raise ValueError("Claude reports no active login")
            if value.get("authMethod") == "claude.ai":
                billing = "subscription"
            elif value.get("authMethod") in {"api_key", "console"}:
                billing = "api"
        else:
            message = (result.stdout + result.stderr).lower()
            if "chatgpt" in message:
                billing = "subscription"
            elif "api key" in message:
                billing = "api"
        # Do not copy account identities, token material or arbitrary CLI output.
        return {"login_present": True, "observed_billing_mode": billing, "entitlement_verified": False,
                "note": "Saved login only; no model request was sent."}

    from kit_hindsight import health, mcp_probe
    check("code_lookup", context_probe)
    check("memory_storage_retrieval", lambda: health(project))
    check("memory_mcp_lookup", lambda: mcp_probe(project))
    for tool in (("codex", "claude") if report.get("mode") == "hybrid" else ("codex",)):
        check(tool + "_login", lambda tool=tool: auth(tool))
    if run_checks:
        def project_checks():
            from kit_hooks import verify
            _, receipt, path = verify(project, project / ".agent-kit/verification.json")
            if not receipt["source_bound"]:
                raise ValueError("Project checks passed but are not bound to source evidence")
            return {"receipt": str(path), "passed": receipt["passed"], "source_bound": True}
        check("project_checks", project_checks)
    report.update(health_checks=results, checked_at=timestamp(), model_calls=0,
                  services_ready=all(results[k]["status"] == "PASSED" for k in ("code_lookup", "memory_storage_retrieval", "memory_mcp_lookup")),
                  checks_verified=results.get("project_checks", {}).get("status") == "PASSED",
                  health_passed=report["configuration_ready"] and all(r["status"] == "PASSED" for r in results.values()))
    path = project / ".agent-kit/logs/health/latest.json"
    report["health_receipt"] = str(path)
    write(path, report)
    return report


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--project", type=Path, default=Path.cwd())
    parser.add_argument("--config-dir", type=Path,
                        default=Path(os.environ.get("CLAUDE_CONFIG_DIR", str(Path.home() / ".claude"))))
    parser.add_argument("--json", action="store_true")
    parser.add_argument("--live", action="store_true", help="Probe actual code lookup, memory retain/recall and saved logins; no model calls")
    parser.add_argument("--checks", action="store_true", help="Also run configured project commands (implies --live); commands can mutate files")
    args = parser.parse_args()
    try:
        report = diagnose(args.project.expanduser().resolve(), args.config_dir.expanduser().resolve())
        if args.live or args.checks:
            report = live_checks(args.project.expanduser().resolve(), report, args.checks)
    except (OSError, ValueError, TypeError) as exc:
        report = {"configuration_ready": False, "live_validated": False,
                  "findings": [{"level": "error", "message": str(exc)}]}
    if args.json:
        print(json.dumps(report, indent=2))
    else:
        for finding in report["findings"]:
            print(f"{finding['level'].upper()}: {finding['message']}")
        for name, result in report.get("health_checks", {}).items():
            print(f"{result['status']}: {name}: {result.get('error', 'probe completed')}")
        print("Configuration ready; live pilot pending." if report["configuration_ready"] else "Setup needs attention.")
    return 0 if report.get("health_passed", report["configuration_ready"]) else 1


if __name__ == "__main__":
    sys.exit(main())
