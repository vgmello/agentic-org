#!/usr/bin/env python3
"""Item 11 (MCP isolation): do `--strict-mcp-config --mcp-config '{"mcpServers":{}}'` and
`--setting-sources ""` keep out config-file MCP servers and the account's claude.ai connectors?

Isolated, NOT the user's real ~/.claude: spawning claude always writes its config dir, and the controls
would start the user's own MCP servers. Instead, the isolated config dir's .claude.json gets one planted
user-scope server (`spike-stub`, command /usr/bin/false: it can only fail), standing in for the servers
in a real ~/.claude.json. The account's claude.ai connectors come from the sign-in itself.

  A  office config: --strict-mcp-config --mcp-config '{"mcpServers":{}}' --setting-sources ""
  B  control: the same without --strict-mcp-config and --mcp-config
  C  B with the default setting sources, run when B shows no server OR doesn't list the planted stub
     (then it tells whether the stub loads at all, i.e. whether --setting-sources "" kept it out)
  --only-c runs just C (used once, after A and B had run).
Every run: fresh mkdtemp cwd, --model haiku, --max-budget-usd 0.25, --tools "", deny-all approver,
prompt "hi". Recorded: counts and statuses only, plus whether any server name contains "claude.ai".
Server and tool names never reach results or fixtures (replaced by <server-N> / <mcp-tool-N>; checked
in-process before saving). Model turns: 2 or 3.
"""
import json
import re

import lib

out = {"runs": {}}
names_seen: set[str] = set()  # held in memory only, to verify the scrub; never written or printed


def scrub_mcp(records, servers):
    """Replace every MCP server name and mcp__ tool/command name with numbered placeholders."""
    smap = {n: f"<server-{i + 1}>" for i, n in enumerate(sorted(servers, key=len, reverse=True))}
    tmap: dict[str, str] = {}

    def fix(o):
        if isinstance(o, dict):
            return {k: fix(v) for k, v in o.items()}
        if isinstance(o, list):
            return [fix(v) for v in o]
        if isinstance(o, str):
            if o.startswith("mcp__"):
                return tmap.setdefault(o, f"<mcp-tool-{len(tmap) + 1}>")
            for n, ph in smap.items():
                o = o.replace(n, ph)
            return re.sub(r"mcp__[A-Za-z0-9_.-]+", lambda m: tmap.setdefault(m.group(0), f"<mcp-tool-{len(tmap) + 1}>"), o)
        return o
    return [fix(r) for r in records]


def run(name, flags):
    b = lib.make_tmp(f"s11{name[0].lower()}")
    # one planted user-scope server in the isolated config (stands in for real ~/.claude.json servers)
    (b / "cfg" / ".claude.json").write_text(json.dumps({"mcpServers": {"spike-stub": {"command": "/usr/bin/false"}}}))
    a = ["-p", "--input-format", "stream-json", "--output-format", "stream-json", "--verbose",
         "--model", "haiku", "--max-budget-usd", "0.25", "--tools", "", "--permission-mode", "manual",
         "--permission-prompt-tool", "stdio", *flags]
    s = lib.Session(a, lib.clean_env(b / "cfg", b / "tmp"), b / "ws", f"item11-{name}")
    s.user("hi")
    r = s.until_result(120, lib.approver(s, []))
    si = s.events("system", "init")[-1]
    servers = [m.get("name", "") for m in si.get("mcp_servers", [])]
    names_seen.update(servers)
    names_seen.update(t for t in si.get("tools", []) if t.startswith("mcp__"))
    v = {"flags": [f if f != lib.EMPTY_MCP else "<empty mcpServers>" for f in flags],
         "mcp_server_count": len(servers),
         "mcp_server_statuses": sorted(m.get("status", "?") for m in si.get("mcp_servers", [])),
         "planted_stub_listed": "spike-stub" in servers,
         "any_name_contains_claude_ai": any("claude.ai" in n.lower() or "claude_ai" in n.lower() for n in servers),
         "tool_count": len(si.get("tools", [])),
         "mcp_tool_count": sum(1 for t in si.get("tools", []) if t.startswith("mcp__")),
         "result_subtype": r.get("subtype"), "exit": s.close(30), "denied_unexpected_calls": s.denials}
    recs = scrub_mcp([lib.Scrubber([str(b)], corpus=s.log).scrub(x) for x in s.log], servers)
    blob = json.dumps(recs)
    leaked = [n for n in names_seen if n and n != "spike-stub" and n in blob]
    assert not leaked, "an MCP name survived the scrub; fixture not written"
    (lib.FIXTURES / f"mcp-isolation-{name}.jsonl").write_text("".join(json.dumps(x, ensure_ascii=False) + "\n" for x in recs))
    out["runs"][name] = v
    print(name, json.dumps(v))
    return v


import sys  # noqa: E402

RESULT = lib.RESULTS / "item11_mcp.json"
if "--only-c" in sys.argv:
    out = json.loads(RESULT.read_text())
    run("C-no-strict-default-sources", [])
else:
    run("A-office-strict-empty", ["--strict-mcp-config", "--mcp-config", lib.EMPTY_MCP, "--setting-sources", ""])
    vb = run("B-no-strict-sources-empty", ["--setting-sources", ""])
    if vb["mcp_server_count"] == 0 or not vb["planted_stub_listed"]:
        run("C-no-strict-default-sources", [])
lib.write_result("item11_mcp", out)
