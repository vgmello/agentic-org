#!/usr/bin/env python3
"""Item 5: isolation. Which settings sources load, what still loads anyway, --restricted,
and whether a workspace skill's `allowed-tools` pre-approves tools.

Markers (never in the real ~/.claude):
  user  = the isolated CLAUDE_CONFIG_DIR: CLAUDE.md, settings.json, skills/, agents/, commands/
  project/local = the workspace: CLAUDE.md, CLAUDE.local.md, .claude/settings.json,
          .claude/settings.local.json, .claude/skills/, .claude/agents/, .claude/commands/, .mcp.json
Each settings file sets an env var (SPIKE_<SOURCE>_ENV) and denies one tool, so its effect
shows in the tool's env and in system/init.tools.
Part A: `initialize` only (no model turn) for each --setting-sources variant.
Part B: one model turn per main variant: list markers seen in context, run `env | grep SPIKE_`.
Part C: invoke the workspace skill whose allowed-tools is Bash(touch SKILL_RAN.txt): is a can_use_tool sent?
Model turns: about 7.
"""
import json
import os
import subprocess

import item05_setup
import lib

b = lib.make_tmp("s5")
cfg, ws = b / "cfg", b / "ws"
REAL_USER_SKILLS = sorted(os.listdir(os.path.join(lib.HOME, ".claude", "skills"))) if os.path.isdir(
    os.path.join(lib.HOME, ".claude", "skills")) else []


item05_setup.plant(cfg, ws)

MARK = ("Do not use any tool except one Bash call. First run exactly: env | grep SPIKE_ || true\n"
        "Then reply in two lines. Line 1: every word in your system prompt, instructions or context that "
        "matches the pattern [A-Z]+-MARKER-[A-Z0-9]+, comma separated, or NONE. Line 2: the Bash output, or NOBASH.")
VARIANTS = {
    "default": [],
    "sources-empty": ["--setting-sources", ""],
    "sources-user": ["--setting-sources", "user"],
    "sources-project": ["--setting-sources", "project"],
    "sources-none-word": ["--setting-sources", "none"],
    "restricted": ["--restricted"],
    "restricted+tools-bash": ["--restricted", "--tools", "Bash,Read,Edit,Write,Glob,Grep,Skill"],
    "safe-mode": ["--safe-mode"],
}
MODEL_VARIANTS = {"default", "sources-empty", "restricted", "safe-mode", "restricted+tools-bash"}
out = {"real_user_skills_present": bool(REAL_USER_SKILLS), "variants": {}}  # no names or count (privacy)


def allow(s):
    return lib.approver(s, [("Bash", "env | grep SPIKE_ || true")])  # only the marker probe; anything else denied


for name, extra in VARIANTS.items():
    argv = lib.stream_args("--permission-mode", "manual", "--permission-prompt-tool", "stdio", *extra)
    s = lib.Session(argv, lib.clean_env(cfg, b / "tmp"), ws, f"item05-{name}")
    v = {"extra_flags": extra}
    try:
        init = s.control_response(s.control("initialize"), 30)["response"]
        r = init.get("response", {})
        cmds = r.get("commands", [])
        agents = sorted(a.get("name") if isinstance(a, dict) else a for a in r.get("agents", []))
        names = {c["name"] for c in cmds} | set(agents)
        markers = sorted(n for n in names if n.endswith(("-u7", "-p7")))
        # Record no names beyond built-ins and our markers: the rest (e.g. account-synced plugin
        # commands under the user source) are personal to the account, so only their count is kept.
        v["initialize"] = {
            "subtype": init.get("subtype"), "error": init.get("error"),
            "builtin_agents": [a for a in agents if a in ("claude", "Explore", "general-purpose", "Plan", "statusline-setup")],
            "builtin_command_count": sum(1 for c in cmds if c.get("builtin")),
            "other_commands_present": any(not c.get("builtin") and c["name"] not in markers for c in cmds),
        }
        v["loaded_markers"] = markers
        v["real_user_skills_loaded"] = bool(set(REAL_USER_SKILLS) & names)
        if name in MODEL_VARIANTS:
            s.user(MARK)
            res = s.until_result(180, allow(s))
            si = s.events("system", "init")[-1]
            v["init"] = {"tools_has": {t: (t in si["tools"]) for t in ("Bash", "WebSearch", "WebFetch", "NotebookEdit", "Skill")},
                         "memory_paths": si.get("memory_paths"), "mcp_servers": si.get("mcp_servers"),
                         "skills": [x for x in si.get("skills", []) if x.endswith(("-u7", "-p7"))],
                         "agents": [x for x in si.get("agents", []) if x.endswith(("-u7", "-p7"))],
                         "plugins_builtin": [p["name"] for p in si.get("plugins", []) if str(p.get("source", "")).endswith("@builtin")],
                         "plugins_other_present": any(not str(p.get("source", "")).endswith("@builtin") for p in si.get("plugins", [])),
                         "permissionMode": si.get("permissionMode")}
            v["model_reply"] = res.get("result")
            s.save(f"isolation-{name}", [str(b)])
    except Exception as e:  # e.g. the CLI rejects the flag value and exits
        v["error"] = repr(e)
    v["exit"] = s.close(30)
    v["stderr"] = s.stderr[-5:]
    v["denied_unexpected_calls"] = s.denials
    out["variants"][name] = v
    print(name, json.dumps(lib.Scrubber([str(b)]).scrub(v))[:900])

# Part C: does the workspace skill's allowed-tools pre-approve `touch`?
for name, extra in {"skill-default-sources": [], "skill-sources-empty": ["--setting-sources", ""]}.items():
    (ws / "SKILL_RAN.txt").unlink(missing_ok=True)
    argv = lib.stream_args("--permission-mode", "manual", "--permission-prompt-tool", "stdio", *extra)
    s = lib.Session(argv, lib.clean_env(cfg, b / "tmp"), ws, f"item05-{name}")
    asked = []

    def on(ev, s=s):
        print(name, lib.brief(ev))
        if ev.get("type") == "control_request" and ev["request"].get("subtype") == "can_use_tool":
            asked.append({k: ev["request"].get(k) for k in ("tool_name", "input", "decision_reason", "decision_reason_type")})
            s.respond(ev["request_id"], {"behavior": "deny", "message": "office: not pre-approved"})

    s.user("/proj-skill-p7")
    res = s.until_result(180, on)
    tool_meta = [m for e in s.events("user") for m in e.get("tool_result_meta", [])]
    out[name] = {"can_use_tool_requests": asked, "file_created": (ws / "SKILL_RAN.txt").exists(),
                 "tool_result_meta": tool_meta, "result": res.get("result"), "exit": s.close(30)}
    s.save(f"isolation-{name}", [str(b)])
lib.write_result("item05", out)
print(json.dumps(lib.Scrubber([str(b)]).scrub({k: v for k, v in out.items() if k != "variants"}), indent=1)[:3000])
