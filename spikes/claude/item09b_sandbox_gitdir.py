#!/usr/bin/env python3
"""Item 9, part b: what does Claude's sandbox make writable in a linked worktree's git dir
when the office lists NO extra roots (cwd only)? It builds its own repos with gitfixture.build in
a private temp dir. One model turn. (--debug-file was meant to capture the seatbelt profile; in
2.1.311 it logged none.)

Each probe opens a path for append and writes nothing (`: >>`), so an allowed probe only
creates an empty file where none existed.
"""
import json
import os
import re

import sys

import gitfixture
import lib

# --deny: the office adds denyWrite entries for what the design says must stay read-only,
# and the probe also commits, to see whether denyWrite beats the sandbox's own git-dir allowance.
DENY = "--deny" in sys.argv
TAG = "B3-cwd-only-plus-denywrite" if DENY else "B2-cwd-only-gitdir-probe"

b = lib.make_tmp("s9b")
B = os.path.realpath(b)
fx = gitfixture.build(B)
WT = fx["wt"]
PATHS = {
    "work.git/refs/heads/main (another branch)": f"{B}/work.git/refs/heads/main",
    "work.git/refs/heads/task/t1 (own branch)": f"{B}/work.git/refs/heads/task/t1",
    "work.git/HEAD (the bare repo HEAD)": f"{B}/work.git/HEAD",
    "work.git/packed-refs": f"{B}/work.git/packed-refs",
    "work.git/info/exclude": f"{B}/work.git/info/exclude",
    "work.git/description": f"{B}/work.git/description",
    "work.git/config.worktree": f"{B}/work.git/config.worktree",
    "work.git/logs/HEAD": f"{B}/work.git/logs/HEAD",
    "work.git/FETCH_HEAD": f"{B}/work.git/FETCH_HEAD",
    "work.git/objects/<existing pack dir>/probe": f"{B}/work.git/objects/pack/probe",
    "work.git/worktrees/t1/HEAD": f"{B}/work.git/worktrees/t1/HEAD",
    "work.git/worktrees/t1/gitdir": f"{B}/work.git/worktrees/t1/gitdir",
    "work.git/worktrees/t1/config.worktree": f"{B}/work.git/worktrees/t1/config.worktree",
    "work.git/worktrees/t1/index": f"{B}/work.git/worktrees/t1/index",
    "origin/README (unrelated repo)": f"{B}/origin/README",
    "cwd: .git file (worktree pointer)": f"{WT}/.git",
    "cwd: .claude/settings.json": f"{WT}/.claude/settings.json",
    "cwd: .claude/skills/x/SKILL.md": f"{WT}/.claude/skills/x/SKILL.md",
    "cwd: .gitattributes": f"{WT}/.gitattributes",
    "cwd: plain file": f"{WT}/plain.txt",
}
lines = ["#!/bin/sh", f"rm -rf {WT}/.claude 2>/dev/null; mkdir -p {WT}/.claude/skills/x 2>/dev/null"]
for label, p in PATHS.items():
    lines.append(f"(: >> '{p}') 2>/dev/null && echo 'ALLOW  {label}' || echo 'DENY   {label}'")
if DENY:
    lines += [f"cd {WT} && date >> f.txt && git add f.txt 2>&1 && git commit -q -m spike-b3 2>&1 && echo TASK-COMMIT-OK || echo TASK-COMMIT-FAIL"]
probe = f"{B}/rt/probe-gitdir{'-deny' if DENY else ''}.sh"
open(probe, "w").write("\n".join(lines) + "\n")
existed = {label: os.path.exists(p) for label, p in PATHS.items()}

# autoAllowBashIfSandboxed off: only the exact allow rule pre-approves the probe; anything else reaches
# lib.approver (denied and logged). The recorded runs had it on (README item 9).
sandbox = {"enabled": True, "autoAllowBashIfSandboxed": False, "allowUnsandboxedCommands": False}
if DENY:
    sandbox["filesystem"] = {"denyWrite": [f"{B}/work.git/refs/heads/main", f"{B}/work.git/HEAD", f"{B}/work.git/packed-refs",
                                           f"{B}/work.git/info", f"{B}/work.git/description", f"{B}/work.git/FETCH_HEAD",
                                           f"{B}/work.git/worktrees/t1/gitdir"]}
settings = f"{B}/rt/settings-{TAG}.json"
open(settings, "w").write(json.dumps({"sandbox": sandbox, "permissions": {"allow": [f"Bash(sh {probe})"]}}))
debug = f"{B}/rt/debug-{TAG}.log"
s = lib.Session(lib.stream_args("--setting-sources", "", "--settings", settings, "--permission-mode", "manual",
                                "--permission-prompt-tool", "stdio", "--debug-file", debug),
                lib.clean_env(f"{B}/cfg", f"{B}/tmp", fx["env"]), WT, "item09b")
# Only the probe runs, pre-approved by the settings' allow rule plus the sandbox; any can_use_tool
# that still arrives is denied and logged. (The recorded B2/B3 runs predate fx["env"]: their git
# used the user's global git config for the commit identity. See README item 9.)
on = lib.approver(s, [])


s.user(f"Use the Bash tool once to run exactly: sh {probe}  Do not retry and do not disable the sandbox. Then reply DONE.")
r = s.until_result(240, on)
text = "\n".join(c["content"] if isinstance(c["content"], str) else json.dumps(c["content"])
                 for e in s.events("user") for c in (e["message"]["content"] if isinstance(e["message"]["content"], list) else [])
                 if c.get("type") == "tool_result")
s.close(30)
s.save(f"sandbox-{TAG}", [B, str(b)])
# pull the write rules out of the seatbelt profile in the debug log
dbg = open(debug, errors="replace").read() if os.path.exists(debug) else ""
write_rules = sorted(set(re.findall(r'\(allow file-write[^\n]{0,400}', dbg)))[:40]
deny_rules = sorted(set(re.findall(r'\(deny file-write[^\n]{0,400}', dbg)))[:60]
sandbox_lines = [l for l in dbg.splitlines() if re.search(r"[Ss]andbox", l)][:60]
out = {"probe_output": text.replace(B, "<s8>").splitlines(), "existed_before": existed, "denied_unexpected_calls": s.denials,
       "seatbelt_allow_write": [x.replace(B, "<s8>") for x in write_rules],
       "seatbelt_deny_write": [x.replace(B, "<s8>") for x in deny_rules],
       "debug_sandbox_lines": [x.replace(B, "<s8>")[:300] for x in sandbox_lines]}
lib.write_result("item09c" if DENY else "item09b", out)
print("\n".join(out["probe_output"]))
print(len(dbg), "debug bytes;", len(write_rules), "allow rules;", len(deny_rules), "deny rules")
