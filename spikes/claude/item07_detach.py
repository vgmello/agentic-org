#!/usr/bin/env python3
"""Item 7: do Bash-tool processes detach from claude's process group / session?

The Bash tool runs `sh hold.sh`, which records its env var names, starts
`( sleep 62 & )` (double fork), `nohup sleep 63 &`, then `sleep 61` in the foreground.
While it runs, the harness records pid/ppid/pgid/sid of claude and every descendant.
Three runs:
  R1: SIGKILL claude's pid only -> who survives? then killpg(claude pgid) -> who survives?
  R2: SIGTERM claude's pid only -> who survives after 6 s?
  R3: killpg(claude pgid, SIGKILL) directly (what org-supervise would do).
Everything left over is killed by pid at the end. Model turns: 3.
"""
import json
import os
import signal
import stat
import subprocess
import time

import lib

HOLD = """echo "shell $$" > hold.info
env | cut -d= -f1 | sort > env.txt
( sleep 62 & )
nohup sleep 63 >/dev/null 2>&1 &
sleep 61
echo done
"""


def procs():
    rows = subprocess.run(["ps", "-A", "-o", "pid=,ppid=,pgid=,command="], capture_output=True, text=True).stdout
    out = {}
    for r in rows.splitlines():
        p = r.split(None, 3)
        if len(p) == 4:
            pid = int(p[0])
            try:
                sid = os.getsid(pid)
            except OSError:
                sid = None
            out[pid] = {"pid": pid, "ppid": int(p[1]), "pgid": int(p[2]), "sid": sid, "cmd": p[3][:120]}
    return out


def descendants(root, table):
    res, stack = [], [root]
    while stack:
        x = stack.pop()
        for p in table.values():
            if p["ppid"] == x:
                res.append(p)
                stack.append(p["pid"])
    return res


def sleepers(table=None, sids=None):
    """The hold.sh sleeps only: matched by command AND by a session id seen under our claude."""
    table = table or procs()
    return [p for p in table.values() if p["cmd"] in ("sleep 61", "sleep 62", "sleep 63")
            and (sids is None or p["sid"] in sids)]


def start(label, b):
    (b / "ws" / "hold.sh").write_text(HOLD)
    s = lib.Session(lib.stream_args("--permission-mode", "manual", "--permission-prompt-tool", "stdio"),
                    lib.clean_env(b / "cfg", b / "tmp"), b / "ws", label)
    s.user("Use the Bash tool to run exactly: sh hold.sh (in the foreground, not in the background). Then reply with its output.")

    allow = lib.approver(s, [("Bash", "sh hold.sh")])  # only the scenario's exact command; anything else denied
    s.denials = denials  # share one list across R1-R3

    def our_sids():
        return {p["sid"] for p in descendants(s.p.pid, procs())}

    end = time.monotonic() + 90
    while time.monotonic() < end and len(sleepers(sids=our_sids())) < 3:
        try:
            s.until(lambda e: False, 1, allow)
        except TimeoutError:
            pass
    time.sleep(1)
    t = procs()
    desc = descendants(s.p.pid, t)
    snap = {"claude": t.get(s.p.pid), "descendants": desc, "sleepers": sleepers(t, {p["sid"] for p in desc})}
    return s, snap


def watchlist(snap):
    """pid -> (cmd, sid) for every process in the snapshot (descendants and the detached sleeps)."""
    return {p["pid"]: (p["cmd"], p["sid"]) for p in snap["descendants"] + snap["sleepers"]}


def survivors(watch):
    """Processes from the snapshot that are still alive AND still the same process (same command and
    session id), so a pid reused by an unrelated process is never reported or killed."""
    t = procs()
    return [t[p] for p, (cmd, sid) in watch.items() if p in t and t[p]["cmd"] == cmd and t[p]["sid"] == sid]


def kill_survivors(watch):
    for p in survivors(watch):
        try:
            os.kill(p["pid"], signal.SIGKILL)
        except ProcessLookupError:
            pass


def drop_sock(pid):
    """Remove claude's /tmp/cc-socks/<pid>.sock if it was left behind. Call it only while that claude is
    an unreaped zombie of ours, so the pid (and its socket name) can't belong to anyone else yet."""
    sock = f"/tmp/cc-socks/{pid}.sock"
    try:
        st = os.lstat(sock)  # lstat: never follow a symlink planted at that name
    except FileNotFoundError:
        return False
    if not stat.S_ISSOCK(st.st_mode) or st.st_uid != os.getuid():
        return False  # not a socket of ours: leave it
    os.unlink(sock)
    return True


out = {}
denials = []  # every session's s.denials (lib.approver), collected in start()
b = lib.make_tmp("s7")
all_watch = {}

# R1
s, snap = start("item07-R1", b)
out["R1_tree"] = snap
watch = watchlist(snap)
all_watch.update(watch)
sock = f"/tmp/cc-socks/{s.p.pid}.sock"
os.kill(s.p.pid, signal.SIGKILL)
s.wait_unreaped(5)  # don't reap yet: the killpg below must not hit a recycled group
time.sleep(2)
out["R1_after_sigkill_pid"] = survivors(watch)
out["R1_cc_socket_left_after_sigkill"] = os.path.exists(sock)
try:
    os.killpg(s.pgid, signal.SIGKILL)  # the group is normally empty by now (claude is dead)
except (ProcessLookupError, PermissionError):  # macOS gives EPERM when only the zombie leader is left
    out["R1_claude_group_already_empty"] = True
time.sleep(1)
out["R1_after_killpg_claude_group"] = survivors(watch)
out["env_names_seen_by_tool"] = (b / "ws" / "env.txt").read_text().split() if (b / "ws" / "env.txt").exists() else None
kill_survivors(watch)
drop_sock(s.p.pid)  # our own leftover, removed while claude is still unreaped
s.wait_exit(5)

# R2
s, snap = start("item07-R2", b)
out["R2_tree"] = snap
watch = watchlist(snap)
all_watch.update(watch)
t0 = time.monotonic()
os.kill(s.p.pid, signal.SIGTERM)
s.wait_unreaped(15)  # exited, not reaped yet
out["R2_exit_seconds"] = round(time.monotonic() - t0, 2)
time.sleep(6)
out["R2_after_sigterm_pid"] = survivors(watch)
out["R2_stdout_tail"] = [lib.brief(e["msg"]) for e in s.log if e["dir"] == "out"][-3:]
out["R2_cc_socket_left_after_sigterm"] = drop_sock(s.p.pid) if s.state() != "running" else "claude still running"
kill_survivors(watch)
out["R2_exit_code"] = s.wait_exit(5)

# R3
s, snap = start("item07-R3", b)
out["R3_tree"] = snap
watch = watchlist(snap)
all_watch.update(watch)
os.killpg(s.pgid, signal.SIGKILL)
s.wait_unreaped(5)
time.sleep(2)
out["R3_after_killpg_claude_group"] = survivors(watch)
out["R3_cc_socket_left_after_killpg"] = drop_sock(s.p.pid)  # S4: remove it like R1's, before reaping
kill_survivors(watch)
s.wait_exit(5)

time.sleep(1)
out["leftover_processes_at_end"] = survivors(all_watch)
out["denied_unexpected_calls"] = denials
lib.write_result("item07", out)
print(json.dumps(lib.Scrubber([str(b)]).scrub(out), indent=1)[:7000])
