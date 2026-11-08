#!/usr/bin/env python3
"""Item 10: take-over. Headless -> interactive `claude --resume <id>` in a PTY -> headless again.

H0 (headless, office spec, --session-id): "remember HERON".
T  (interactive TUI in a PTY via pty.fork, same spec minus the -p/stream flags): ask for the word,
   and to remember FALCON too; then /exit.
H1 (headless --resume, full spec): "which two words?"
The config dir is pre-seeded (onboarding done, workspace trusted) the way the office would.
Fixtures: takeover-headless-before.jsonl, takeover-pty.txt (ANSI stripped), takeover-headless-after.jsonl.
Model turns: 3.
"""
import fcntl
import json
import os
import pty
import re
import select
import signal
import struct
import termios
import time

import lib

# Wall-clock limit for the whole script (the PTY phase waits on a TUI): on expiry it exits normally,
# so lib's atexit cleanup still kills the PTY child and the headless sessions.
lib.deadline(600)
b = lib.make_tmp("s10")
cfg, ws, rt = b / "cfg", b / "ws", b / "runtime"
rt.mkdir()
lib.seed_config(cfg, trusted=[str(ws), os.path.realpath(ws)])
sid = lib.new_session_id()
instr = rt / "instructions.md"
instr.write_text("Office instructions: you are an office-run agent. Keep replies to one line.\n")
AGENTS = json.dumps({"zephyr": {"description": "Office-run agent Zephyr", "prompt": "You are Zephyr."}})
SPEC = ["--setting-sources", "", "--tools", "Bash,Read", "--strict-mcp-config", "--mcp-config", lib.EMPTY_MCP, "--model", "haiku",
        "--agents", AGENTS, "--agent", "zephyr", "--append-system-prompt-file", str(instr),
        "--permission-mode", "manual"]
out = {}


def headless(name, extra, prompt):
    s = lib.Session(["-p", "--input-format", "stream-json", "--output-format", "stream-json", "--verbose",
                     "--max-budget-usd", "0.50", "--permission-prompt-tool", "stdio", *SPEC, *extra],
                    lib.clean_env(cfg, b / "tmp"), ws, f"item10-{name}")
    s.user(prompt)
    r = s.until_result(150, lib.approver(s, []))  # no tool is expected: deny (and log) any request
    out[name] = {"reply": r.get("result"), "session_id_same": r.get("session_id") == sid, "exit": s.close(30),
                 "denied_unexpected_calls": s.denials}
    s.save(f"takeover-{name}", [str(b)])
    print(name, out[name])


headless("headless-before", ["--session-id", sid], "Remember the word HERON. Reply OK.")

# --- interactive take-over in a PTY ---------------------------------------------------------
ANSI = re.compile(r"\x1b\[[0-9;?<>=]*[ -/]*[@-~]|\x1b\][^\x07\x1b]*(\x07|\x1b\\)|\x1b[()][A-Za-z0-9]|\x1b[=>78DEHMc]")
env = lib.clean_env(cfg, b / "tmp", {"TERM": "xterm-256color", "COLUMNS": "120", "LINES": "40"})
argv = [lib.CLAUDE, "--resume", sid, *SPEC]
pid, fd = pty.fork()
if pid == 0:  # child: new session with the PTY as controlling terminal (pty.fork does setsid)
    try:
        os.chdir(ws)
        os.execve(lib.CLAUDE, argv, env)
    finally:  # never return into the parent's code (or its atexit cleanup) from the forked child
        os._exit(127)
lib._pty_children.append(pid)  # killed at exit if anything below fails
fcntl.ioctl(fd, termios.TIOCSWINSZ, struct.pack("HHHH", 40, 120, 0, 0))
raw = bytearray()
lib.log_run("item10-pty: interactive turn")


def pump(seconds, until=None):
    end = time.monotonic() + seconds
    while time.monotonic() < end:
        r, _, _ = select.select([fd], [], [], 0.2)
        if fd in r:
            try:
                chunk = os.read(fd, 65536)
            except OSError:
                return False
            if not chunk:
                return False
            raw.extend(chunk)
            if b"\x1b[6n" in chunk:  # cursor position query
                os.write(fd, b"\x1b[1;1R")
            if b"\x1b[c" in chunk:  # primary device attributes
                os.write(fd, b"\x1b[?1;2c")
        if until and until(text()):
            return True
    return None


def text():
    return ANSI.sub("", raw.decode("utf-8", "replace"))


def typeline(s):
    os.write(fd, s.encode())
    time.sleep(0.5)
    os.write(fd, b"\r")


t0 = time.monotonic()
ready = pump(40, lambda t: "zephyr" in t.lower() or "HERON" in t or ">" in t[-200:])
out["pty_ready_seconds"] = round(time.monotonic() - t0, 1)
pump(3)
mark = len(text())
typeline("What word did I ask you to remember? Also remember the word FALCON. Reply in one line.")
got = pump(90, lambda t: "HERON" in t[mark:] and "FALCON" in t[mark:].replace(
    "Also remember the word FALCON", ""))
pump(3)
out["pty_answer_seen"] = bool(got)
out["pty_tail"] = [l.strip() for l in lib.crop_tui_notices(text()[mark:]).splitlines() if l.strip()][-12:]
typeline("/exit")
exited = None
for _ in range(30):
    pump(1)
    wp, status = os.waitpid(pid, os.WNOHANG)
    if wp:
        exited = os.waitstatus_to_exitcode(status)
        break
if exited is None:
    os.kill(pid, signal.SIGTERM)
    time.sleep(2)
    try:
        os.kill(pid, signal.SIGKILL)
    except ProcessLookupError:
        pass
    _, status = os.waitpid(pid, 0)
    exited = f"forced ({os.waitstatus_to_exitcode(status)})"
lib._pty_children.remove(pid)  # reaped: never signal this pid again
out["pty_exit"] = exited
os.close(fd)
(lib.FIXTURES / "takeover-pty.txt").write_text(lib.crop_tui_notices(lib.Scrubber([str(b)])._s(text())))

headless("headless-after", ["--resume", sid], "Which two words did I ask you to remember? One line.")
out["transcripts"] = sorted(str(p.relative_to(cfg)) for p in (cfg / "projects").rglob("*.jsonl"))
lib.write_result("item10", json.loads(json.dumps(lib.Scrubber([str(b)]).scrub(out))))
print(json.dumps(lib.Scrubber([str(b)]).scrub(out), indent=1))
