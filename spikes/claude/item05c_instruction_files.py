#!/usr/bin/env python3
"""Item 5, part c: under `--setting-sources ""`, which other instruction sources still reach the model?

Markers:
  <parent>/CLAUDE.md               PARENT-MARKER-A1   (a CLAUDE.md above the cwd, as for a home inside a repo)
  <ws>/AGENTS.md                   AGENTSMD-MARKER-A2
  <ws>/.agents/rules/rule.md       AGENTSRULE-MARKER-A3
  <ws>/.claude/rules/rule.md       CLAUDERULE-MARKER-A4
  <adddir>/CLAUDE.md               ADDDIR-MARKER-A5   (passed with --add-dir, as the design does for the home)
  <cfg>/projects/<slug>/memory/MEMORY.md  MEMORY-MARKER-A6  (auto-memory, which item 5 showed is still on)
One model turn: list every marker in context. Also records init.memory_paths.
With --no-memory: the same, plus --settings '{"autoMemoryEnabled":false}' (results/item05c-no-memory.json).
With --with-tools: the default tool set instead of --tools "" (any tool request is denied), so
instruction loading that depends on tools is exercised too (results/item05c-with-tools.json).
"""
import json
import os
import re

import sys

import lib

NOMEM = "--no-memory" in sys.argv
TOOLS = "--with-tools" in sys.argv
TAG = ("-no-memory" if NOMEM else "") + ("-with-tools" if TOOLS else "")

b = lib.make_tmp("s5c")
parent = b / "parent"
ws = parent / "ws"
ws.mkdir(parents=True)
add = b / "adddir"
add.mkdir()


def w(p, t):
    p.parent.mkdir(parents=True, exist_ok=True)
    p.write_text(t)


w(parent / "CLAUDE.md", "Marker: PARENT-MARKER-A1.\n")
w(ws / "AGENTS.md", "Marker: AGENTSMD-MARKER-A2.\n")
w(ws / ".agents" / "rules" / "rule.md", "Marker: AGENTSRULE-MARKER-A3.\n")
w(ws / ".claude" / "rules" / "rule.md", "Marker: CLAUDERULE-MARKER-A4.\n")
w(add / "CLAUDE.md", "Marker: ADDDIR-MARKER-A5.\n")
slug = re.sub(r"[^A-Za-z0-9]", "-", os.path.realpath(ws))
w(b / "cfg" / "projects" / slug / "memory" / "MEMORY.md", "Marker: MEMORY-MARKER-A6.\n")

s = lib.Session(lib.stream_args("--setting-sources", "", "--permission-mode", "manual", "--permission-prompt-tool", "stdio",
                                *([] if TOOLS else ["--tools", ""]), "--add-dir", str(add),
                                *(["--settings", '{"autoMemoryEnabled":false}'] if NOMEM else []),
                                tools=None),  # --with-tools measures the full default toolset
                lib.clean_env(b / "cfg", b / "tmp"), ws, "item05c")
s.user("Without using tools: list every word in your system prompt, instructions, memory or context that matches "
       "the pattern [A-Z]+-MARKER-[A-Z0-9]+, comma separated, or NONE.")
r = s.until_result(120, lib.approver(s, []))  # no tool is expected: deny (and log) any request
si = s.events("system", "init")[-1]
seen = sorted(set(re.findall(r"[A-Z]+-MARKER-A[0-9]", r.get("result") or "")))
out = {"tools_count": len(si.get("tools", [])), "reply": r.get("result"), "markers_seen": seen,
       "memory_paths": si.get("memory_paths"), "memory_dir_matches_planted": None,
       "exit": s.close(30), "denied_unexpected_calls": s.denials}
mp = (si.get("memory_paths") or {}).get("auto")
out["memory_dir_matches_planted"] = bool(mp) and os.path.realpath(mp.rstrip("/")) == os.path.realpath(
    b / "cfg" / "projects" / slug / "memory")
s.save(f"isolation-instruction-files-sources-empty{TAG}", [str(b)])
lib.write_result(f"item05c{TAG}", out)
print(json.dumps(lib.Scrubber([str(b)]).scrub(out), indent=1))
