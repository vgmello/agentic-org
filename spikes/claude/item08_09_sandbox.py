#!/usr/bin/env python3
"""Items 8 and 9: Claude's Bash sandbox (macOS seatbelt), the office socket, and git commit
in a linked worktree whose git dir is outside the workspace.

Layout under a private /tmp/claude-spike-s8-XXXXXXXX (its realpath is used everywhere; /tmp is a
symlink), built by gitfixture.build: origin/, work.git/ (bare, worktreeConfig), worktrees/t1/ (the
session's cwd), homerepo/agents/x/ (an agent home inside a repo), rt/ (gitconfig, empty hooks/, probe.sh).
  run/a.sock         listed in allowUnixSockets, exists at start
  run/b.sock         NOT listed, exists at start
  run/c.sock         listed, created only after claude has started (path didn't exist at launch)
Git env (through claude's env to its tools): GIT_CONFIG_NOSYSTEM=1, GIT_CONFIG_GLOBAL=rt/gitconfig,
GIT_CONFIG_COUNT=4: core.hooksPath=rt/hooks, core.fsmonitor=false, gc.auto=0, maintenance.auto=false.

Process A ("design roots"): sandbox on; allowWrite = the design's writable roots for the task
  worktree and for the home-in-repo (literal files that may not exist yet included);
  denyWrite = objects/info, commondir, gitdir; allowUnixSockets = [a.sock, c.sock].
Process B ("cwd only"): sandbox on, no extra roots, no sockets.
Each runs `sh rt/probe.sh` once (1 model turn each).
"""
import json
import os
import socket
import subprocess
import sys
import threading

import gitfixture
import lib

b = lib.make_tmp("s8")
B = os.path.realpath(b)
P = lambda *x: os.path.join(B, *x)  # noqa: E731
fx = gitfixture.build(B)
GIT_ENV, WT, setup = fx["env"], fx["wt"], fx["setup"]


# socket servers
def serve(path):
    srv = socket.socket(socket.AF_UNIX)
    srv.bind(path)
    srv.listen(5)

    def loop():
        while True:
            try:
                c, _ = srv.accept()
            except OSError:
                return
            c.sendall(b"PONG")
            c.close()
    threading.Thread(target=loop, daemon=True).start()
    return srv


servers = [serve(P("run/a.sock")), serve(P("run/b.sock"))]

PROBE = f"""#!/bin/sh
R={B}
probe() {{ python3 -c '
import socket,sys
p=sys.argv[1]
try:
    c=socket.socket(socket.AF_UNIX); c.settimeout(3); c.connect(p); print(sys.argv[2], "CONNECTED", c.recv(8).decode())
except Exception as e:
    print(sys.argv[2], "REFUSED", type(e).__name__, e)
' "$1" "$2"; }}
echo "== sockets"
probe $R/run/a.sock a.sock-listed
probe $R/run/b.sock b.sock-unlisted
probe $R/run/c.sock c.sock-listed-created-late
probe "$CLAUDE_CODE_MESSAGING_SOCKET" own-cc-messaging-socket
echo "== git env"
git config --show-origin --get core.hooksPath 2>&1
git config --show-origin --get user.email 2>&1
echo "== task commit"
cd $R/worktrees/t1 && date >> f.txt && git add f.txt 2>&1 && git commit -q -m spike-task 2>&1 && echo TASK-COMMIT-OK || echo TASK-COMMIT-FAIL
git -C $R/worktrees/t1 log --oneline -1 2>&1
git -C $R/worktrees/t1 config --worktree spike.x 1 2>&1 && echo WT-CONFIG-OK || echo WT-CONFIG-FAIL
git -C $R/worktrees/t1 config spike.y 1 2>&1 && echo SHARED-CONFIG-OK || echo SHARED-CONFIG-FAIL
echo "== home commit"
cd $R/homerepo/agents/x && date >> memory.md && git add memory.md 2>&1 && git commit -q -m spike-home 2>&1 && echo HOME-COMMIT-OK || echo HOME-COMMIT-FAIL
echo "== writes (non-destructive: open for append, write nothing)"
(: >> $R/outside.txt) 2>&1 && echo OUTSIDE-WRITE-OK || echo OUTSIDE-WRITE-DENIED
(: >> $R/work.git/config) 2>&1 && echo WORKGIT-CONFIG-WRITE-OK || echo WORKGIT-CONFIG-WRITE-DENIED
(: >> $R/work.git/hooks/pre-commit) 2>&1 && echo WORKGIT-HOOK-WRITE-OK || echo WORKGIT-HOOK-WRITE-DENIED
(: >> $R/work.git/objects/info/alternates) 2>&1 && echo OBJECTS-INFO-WRITE-OK || echo OBJECTS-INFO-WRITE-DENIED
(: >> $R/work.git/worktrees/t1/commondir) 2>&1 && echo COMMONDIR-WRITE-OK || echo COMMONDIR-WRITE-DENIED
(: >> $R/homerepo/.git/config) 2>&1 && echo HOME-GIT-CONFIG-WRITE-OK || echo HOME-GIT-CONFIG-WRITE-DENIED
(: >> $R/homerepo/.git/hooks/pre-commit) 2>&1 && echo HOME-GIT-HOOK-WRITE-OK || echo HOME-GIT-HOOK-WRITE-DENIED
(: >> $R/homerepo/.git/commondir) 2>&1 && echo HOME-GIT-COMMONDIR-CREATE-OK || echo HOME-GIT-COMMONDIR-CREATE-DENIED
echo "== end"
"""
open(P("rt/probe.sh"), "w").write(PROBE)

WG, HG = P("work.git"), P("homerepo/.git")
DESIGN_FS = {
    "allowWrite": [f"{WG}/objects", f"{WG}/refs", f"{WG}/logs", f"{WG}/worktrees/t1",
                   P("homerepo/agents/x"), f"{HG}/objects", f"{HG}/refs", f"{HG}/logs",
                   *[f"{HG}/{n}" for n in ("index", "index.lock", "HEAD", "HEAD.lock", "ORIG_HEAD",
                                           "COMMIT_EDITMSG", "packed-refs", "packed-refs.lock")]],
    "denyWrite": [f"{WG}/objects/info", f"{WG}/worktrees/t1/commondir", f"{WG}/worktrees/t1/gitdir",
                  f"{HG}/objects/info"],
}


def settings(fs, sockets):
    # autoAllowBashIfSandboxed off: only the exact allow rule below pre-approves the probe, and any
    # other command reaches lib.approver (denied and logged). The recorded runs had it on (README item 8).
    sb = {"enabled": True, "autoAllowBashIfSandboxed": False, "allowUnsandboxedCommands": False}
    if fs:
        sb["filesystem"] = fs
    if sockets:
        sb["network"] = {"allowUnixSockets": sockets}
    return {"sandbox": sb, "permissions": {"allow": [f"Bash(sh {P('rt/probe.sh')})"]}}


out = {"setup": setup, "runs": {}}
if "--dry" in sys.argv:  # check the git setup and the probe outside any sandbox, no model turn
    print(json.dumps(setup))
    servers.append(serve(P("run/c.sock")))
    print(subprocess.run(["sh", P("rt/probe.sh")], env={"PATH": lib.PATH, "HOME": lib.HOME, **GIT_ENV},
                         capture_output=True, text=True).stdout.replace(B, "<s8>"))
    sys.exit(0)
for name, fs, socks in (("A-design-roots", DESIGN_FS, [P("run/a.sock"), P("run/c.sock")]),
                        ("B-cwd-only", None, None)):
    sf = P(f"rt/settings-{name}.json")
    open(sf, "w").write(json.dumps(settings(fs, socks)))
    for f in ("outside.txt",):
        if os.path.exists(P(f)):
            os.unlink(P(f))
    s = lib.Session(lib.stream_args("--setting-sources", "", "--settings", sf, "--permission-mode", "manual",
                                    "--permission-prompt-tool", "stdio"),
                    lib.clean_env(P("cfg"), P("tmp"), GIT_ENV), WT, f"item08-{name}")
    s.control_response(s.control("initialize"))
    if not os.path.exists(P("run/c.sock")):
        servers.append(serve(P("run/c.sock")))  # created after claude started
    asked = []
    # only the exact probe command, never with dangerouslyDisableSandbox; anything else denied and logged
    guard = lib.approver(s, [("Bash", f"sh {P('rt/probe.sh')}")])

    def on(ev, guard=guard):
        if ev.get("type") == "control_request" and ev["request"].get("subtype") == "can_use_tool":
            asked.append(ev["request"]["input"])
        guard(ev)

    s.user(f"Use the Bash tool once to run exactly: sh {P('rt/probe.sh')}  Do not retry and do not disable the sandbox. "
           "Then reply with the word DONE.")
    r = s.until_result(240, on)
    results = [c for e in s.events("user") for c in (e["message"]["content"] if isinstance(e["message"]["content"], list) else [])
               if c.get("type") == "tool_result"]
    metas = [m for e in s.events("user") for m in e.get("tool_result_meta", [])]
    text = "\n".join(c["content"] if isinstance(c["content"], str) else json.dumps(c["content"]) for c in results)
    out["runs"][name] = {"settings": settings(fs, socks), "approvals_asked": asked, "tool_result_meta": metas,
                         "probe_output": text.replace(B, "<s8>").splitlines(), "result": r.get("result"),
                         "exit": s.close(30), "stderr": s.stderr[-5:], "denied_unexpected_calls": s.denials}
    s.save(f"sandbox-{name}", [B, str(b)])
    print(name, "\n".join(out["runs"][name]["probe_output"]))
for srv in servers:
    srv.close()
out["work_git_config_tail"] = open(P("work.git/config")).read().splitlines()[-3:]
out["worktree_config_files"] = sorted(os.path.relpath(os.path.join(d, f), B) for d, _, fs in os.walk(WG) for f in fs if f == "config.worktree")
lib.write_result("item08_09", json.loads(json.dumps(out).replace(B, "<s8>")))
