#!/usr/bin/env python3
"""Item 5, part b: control for the skill test. The same markers as item05 (item05_setup.plant),
and the same flags as its skill-default-sources run, but `touch` is asked for directly, without
the skill. If this asks (can_use_tool) while the skill run didn't, the skill's allowed-tools
pre-approved it. Model turns: 1.
"""
import json

import item05_setup
import lib

b = lib.make_tmp("s5b")
ws = b / "ws"
item05_setup.plant(b / "cfg", ws)
s = lib.Session(lib.stream_args("--permission-mode", "manual", "--permission-prompt-tool", "stdio",
                                sources=None, tools=None),  # same flags as item05's skill-default-sources
                lib.clean_env(b / "cfg", b / "tmp"), ws, "item05b-control")
asked = []


def on(ev):
    print(lib.brief(ev))
    if ev.get("type") == "control_request" and ev["request"].get("subtype") == "can_use_tool":
        asked.append({k: ev["request"].get(k) for k in ("tool_name", "input")})
        s.respond(ev["request_id"], {"behavior": "deny", "message": "office: control run, denied"})  # never allowed


s.user("Use the Bash tool to run exactly: touch CONTROL.txt  If it is denied, reply DENIED.")
r = s.until_result(180, on)
out = {"can_use_tool_requests": asked, "file_created": (ws / "CONTROL.txt").exists(), "result": r.get("result"),
       "exit": s.close(30)}
s.save("isolation-skill-control-no-skill", [str(b)])
lib.write_result("item05b", out)
print(json.dumps(out, indent=1))
