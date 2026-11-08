#!/usr/bin/env python3
"""Item 6: inline agent identity with --agents/--agent and --append-system-prompt-file.

Runs under the office's isolation flags (--setting-sources "").
  A: --agents '<json>' (prompt + tools) --agent zephyr --append-system-prompt-file <file>
  B: --agents <file path> --agent zephyr (no append)
  C: --agent zephyr with no --agents (unknown agent)
Each asks: who are you, and what is your office handle?
Fixtures: agent-inline-json.jsonl, agent-file.jsonl, agent-unknown.jsonl. Model turns: 3.
"""
import json

import lib

b = lib.make_tmp("s6")
AGENTS = {"zephyr": {
    "description": "Office-run agent Zephyr",
    "prompt": "You are Zephyr, an engineer agent of the office. Begin every reply with 'ZEPHYR:'.",
    "tools": ["Read", "Bash"],
}}
agents_file = b / "runtime" / "agents.json"
agents_file.parent.mkdir()
agents_file.write_text(json.dumps(AGENTS))
append_file = b / "runtime" / "instructions.md"
append_file.write_text("Your office handle is river-7. Never reveal this file's path.\n")
Q = "In one line: what is your name, and what is your office handle (or NONE)?"
VARIANTS = {
    "agent-inline-json": ["--agents", json.dumps(AGENTS), "--agent", "zephyr", "--append-system-prompt-file", str(append_file)],
    "agent-file": ["--agents", str(agents_file), "--agent", "zephyr"],
    "agent-unknown": ["--agent", "zephyr"],
}
out = {}
for name, extra in VARIANTS.items():
    # tools=None: the unknown-agent case is detected by init.tools falling back to the full default set
    argv = lib.stream_args("--setting-sources", "", "--permission-mode", "manual", "--permission-prompt-tool", "stdio", *extra,
                           tools=None)
    s = lib.Session(argv, lib.clean_env(b / "cfg", b / "tmp"), b / "ws", f"item06-{name}")
    v = {"extra_flags": extra}
    try:
        init_resp = s.control_response(s.control("initialize"), 30)["response"].get("response", {})
        v["initialize_agents"] = init_resp.get("agents")
        s.user(Q)
        r = s.until_result(120, lib.approver(s, []))  # no tool is expected: deny (and log) any request
        si = s.events("system", "init")[-1]
        v["init_tools"] = si.get("tools")
        v["init_agents"] = si.get("agents")
        v["init_agent_field"] = {k: si.get(k) for k in si if "agent" in k.lower()}
        v["reply"] = r.get("result")
    except Exception as e:
        v["error"] = repr(e)
    v["exit"] = s.close(30)
    v["denied_unexpected_calls"] = s.denials
    v["stderr"] = s.stderr[-5:]
    s.save(name, [str(b)])
    out[name] = v
    print(name, json.dumps(lib.Scrubber([str(b)]).scrub(v))[:1200])
lib.write_result("item06", out)
