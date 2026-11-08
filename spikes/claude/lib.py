"""Shared harness for the Claude Code headless spike (S0, Claude).

Python 3 standard library only. Every run is isolated:
  - CLAUDE_CONFIG_DIR=<temp>/cfg  (transcripts, .claude.json, user settings, skills, agents)
  - CLAUDE_SECURESTORAGE_CONFIG_DIR=""  (empty: the keychain item keeps its default
    name "Claude Code-credentials", so the user's existing sign-in authenticates the run)
  - an env built from an allowlist (no CLAUDE_CODE_*, AI_AGENT, SSH_AUTH_SOCK, ...)
  - --strict-mcp-config with an empty --mcp-config
  - --model haiku and --max-budget-usd
Each spawned child gets its own process group (start_new_session=True). On close or at exit a
live claude gets SIGTERM, then its group SIGKILL while it is still unreaped, then it's reaped.
Temp dirs come from tempfile.mkdtemp (0700) and are removed at exit unless --keep is passed or
CLAUDE_SPIKE_KEEP=1. Tool approvals: use approver(), which allows only the scenario's exact
tool call and denies (and logs) everything else.
"""
from __future__ import annotations

import atexit
import json
import os
import pathlib
import queue
import re
import shutil
import signal
import subprocess
import sys
import tempfile
import threading
import time
import uuid

CLAUDE = os.path.realpath(shutil.which("claude") or "/opt/homebrew/bin/claude")
SPIKE = pathlib.Path(__file__).resolve().parent
FIXTURES = SPIKE / "fixtures"
RESULTS = SPIKE / "results"
HOME = os.environ["HOME"]
PATH = "/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin"
EMPTY_MCP = '{"mcpServers":{}}'

KEEP_TMP = "--keep" in sys.argv or os.environ.get("CLAUDE_SPIKE_KEEP") == "1"
TURNS: list[str] = []  # model turns sent by this process (printed at exit)
# Account-synced plugins and skills are detected by structure only, never by name (see synced_names):
# a plugin entry with source "<name>@synced", or a non-builtin command aliased "<namespace>:<skill>".
# Their names, commands and skills are personal to the account, so the scrubber drops them.
# Built-in commands whose descriptions hint at the account's plan or org: descriptions are blanked.
PLAN_HINT_COMMANDS = {"usage-credits", "extra-usage", "upgrade", "usage"}

_live: list["Session"] = []
_tmpdirs: list[str] = []
_sandbox_spawned = False  # set when a session starts with a sandbox-enabled --settings (see _kill_all)


def _exit_on_signal(signum, _frame):
    """SIGTERM/SIGHUP: exit normally so the atexit cleanup (kill children, remove temp dirs) runs."""
    sys.exit(128 + signum)


for _sig in (signal.SIGTERM, signal.SIGHUP):
    signal.signal(_sig, _exit_on_signal)


def deadline(seconds: int) -> None:
    """Wall-clock limit for the whole script: SIGALRM exits normally, so atexit cleanup still runs."""
    signal.signal(signal.SIGALRM, lambda *_: sys.exit("wall-clock limit reached"))
    signal.alarm(seconds)


def _uses_sandbox(args: list[str]) -> bool:
    for i, a in enumerate(args[:-1]):
        if a == "--settings":
            v = args[i + 1]
            text = v if v.lstrip().startswith("{") else (open(v).read() if os.path.isfile(v) else "")
            if '"sandbox"' in text:
                return True
    return False


def make_tmp(name: str) -> pathlib.Path:
    """A fresh private dir /tmp/claude-spike-<name>-XXXXXXXX (0700) with cfg/, ws/, tmp/.
    Short, under /tmp, so Unix socket paths in it stay well under the 104-byte limit."""
    base = pathlib.Path(tempfile.mkdtemp(prefix=f"claude-spike-{name}-", dir="/tmp"))
    _tmpdirs.append(str(base))
    for sub in ("cfg", "ws", "tmp"):
        (base / sub).mkdir()
    return base


def clean_env(cfg: str | os.PathLike, tmpdir: str | os.PathLike | None = None,
              extra: dict | None = None) -> dict:
    env = {
        "PATH": PATH,
        "HOME": HOME,
        "USER": os.environ.get("USER", ""),
        "LOGNAME": os.environ.get("USER", ""),
        "LANG": "en_US.UTF-8",
        "SHELL": "/bin/zsh",
        "TMPDIR": str(tmpdir or "/tmp"),
        "CLAUDE_CONFIG_DIR": str(cfg),
        "CLAUDE_SECURESTORAGE_CONFIG_DIR": "",
        "DISABLE_AUTOUPDATER": "1",
    }
    if extra:
        env.update(extra)
    return env


def stream_args(*extra: str, model: str = "haiku", budget: str = "0.50", verbose: bool = True,
                sources: str | None = "", tools: str | None = "Bash,Read") -> list[str]:
    """Base argv. By default no settings file loads (--setting-sources "") and only Bash and Read
    exist (--tools Bash,Read). Pass sources=None / tools=None to leave a flag out (item 5 measures
    exactly those), or give the flag in `extra` to override it. The recorded runs predate both defaults."""
    a = ["-p", "--input-format", "stream-json", "--output-format", "stream-json"]
    if verbose:
        a.append("--verbose")
    a += ["--model", model, "--max-budget-usd", budget,
          "--strict-mcp-config", "--mcp-config", EMPTY_MCP]
    if sources is not None and "--setting-sources" not in extra:
        a += ["--setting-sources", sources]
    if tools is not None and "--tools" not in extra and "--restricted" not in extra:
        a += ["--tools", tools]
    return a + list(extra)


def log_run(what: str) -> None:
    TURNS.append(what)
    print(f"[model turn {len(TURNS)}] {what}", file=sys.stderr)


def approver(s: "Session", allowed: list[tuple[str, str]], on_allowed=None):
    """An event handler for can_use_tool: allow only an exact (tool_name, input.command) pair from
    `allowed` (and never one asking to disable the sandbox); deny everything else, recording it in
    s.denials and on stderr. on_allowed(ev) replaces the plain allow when given."""
    def on(ev):
        if ev.get("type") != "control_request" or ev.get("request", {}).get("subtype") != "can_use_tool":
            return
        req = ev["request"]
        inp = req.get("input") or {}
        if (req.get("tool_name"), inp.get("command")) in allowed and not inp.get("dangerouslyDisableSandbox"):
            if on_allowed:
                on_allowed(ev)
            else:
                s.respond(ev["request_id"], {"behavior": "allow"})
            return
        s.denials.append({"tool_name": req.get("tool_name"), "input": inp})
        print(f"{s.label}: DENIED unexpected tool call {req.get('tool_name')} {json.dumps(inp)[:200]}", file=sys.stderr)
        s.respond(ev["request_id"], {"behavior": "deny",
                                     "message": "spike harness: only this scenario's exact command is allowed"})
    return on


def rss_kb(pid: int) -> int | None:
    out = subprocess.run(["ps", "-o", "rss=", "-p", str(pid)], capture_output=True, text=True).stdout.strip()
    return int(out) if out else None


def tree_rss_kb(pid: int) -> int:
    """RSS of pid plus all its descendants."""
    rows = subprocess.run(["ps", "-A", "-o", "pid=,ppid=,rss="], capture_output=True, text=True).stdout.split("\n")
    kids: dict[int, list[int]] = {}
    rss: dict[int, int] = {}
    for r in rows:
        p = r.split()
        if len(p) == 3:
            kids.setdefault(int(p[1]), []).append(int(p[0]))
            rss[int(p[0])] = int(p[2])
    total, stack = 0, [pid]
    while stack:
        x = stack.pop()
        total += rss.get(x, 0)
        stack += kids.get(x, [])
    return total


class Session:
    """One claude child speaking stream-json over pipes."""

    def __init__(self, args: list[str], env: dict, cwd: str | os.PathLike, label: str = "",
                 stdin=subprocess.PIPE):
        global _sandbox_spawned
        self.label = label
        self.argv = [CLAUDE] + args
        _sandbox_spawned = _sandbox_spawned or _uses_sandbox(args)
        self.t0 = time.monotonic()
        self.p = subprocess.Popen(self.argv, stdin=stdin, stdout=subprocess.PIPE,
                                  stderr=subprocess.PIPE, env=env, cwd=str(cwd),
                                  start_new_session=True)
        self.pgid = os.getpgid(self.p.pid)
        self.log: list[dict] = []  # {"dir": "in"|"out", "t_ms": int, "msg": obj}
        self.q: "queue.Queue[dict | None]" = queue.Queue()
        self.stderr: list[str] = []
        self.denials: list[dict] = []  # tool calls denied by approver()
        self._rid = 0
        threading.Thread(target=self._read_out, daemon=True).start()
        threading.Thread(target=self._read_err, daemon=True).start()
        _live.append(self)

    def _t(self) -> int:
        return int((time.monotonic() - self.t0) * 1000)

    def _read_out(self):
        for raw in self.p.stdout:
            line = raw.decode("utf-8", "replace").rstrip("\n")
            if not line:
                continue
            try:
                obj = json.loads(line)
            except ValueError:
                obj = {"type": "_non_json", "line": line[:2000]}
            self.log.append({"dir": "out", "t_ms": self._t(), "msg": obj})
            self.q.put(obj)
        self.q.put(None)

    def _read_err(self):
        for raw in self.p.stderr:
            self.stderr.append(raw.decode("utf-8", "replace").rstrip("\n"))

    # ---- writing -------------------------------------------------------
    def send(self, obj: dict) -> None:
        # json.dumps with ensure_ascii=True escapes U+2028/U+2029 and every non-ASCII char.
        line = json.dumps(obj, ensure_ascii=True) + "\n"
        self.log.append({"dir": "in", "t_ms": self._t(), "msg": obj})
        self.p.stdin.write(line.encode())
        self.p.stdin.flush()

    def user(self, text: str) -> None:
        log_run(f"{self.label}: {text[:60]!r}")
        self.send({"type": "user", "message": {"role": "user", "content": text}})

    def control(self, subtype: str, **kw) -> str:
        self._rid += 1
        rid = f"host-{self._rid}"
        self.send({"type": "control_request", "request_id": rid, "request": {"subtype": subtype, **kw}})
        return rid

    def respond(self, request_id: str, response: dict) -> None:
        self.send({"type": "control_response",
                   "response": {"subtype": "success", "request_id": request_id, "response": response}})

    def respond_error(self, request_id: str, error: str) -> None:
        self.send({"type": "control_response",
                   "response": {"subtype": "error", "request_id": request_id, "error": error}})

    # ---- reading -------------------------------------------------------
    def next(self, timeout: float) -> dict | None:
        """Next stdout object, or None at EOF. Raises queue.Empty on timeout."""
        return self.q.get(timeout=timeout)

    def until(self, pred, timeout: float = 120, on=None) -> dict:
        """Read events until pred(ev) is true. `on(ev)` sees every event first."""
        end = time.monotonic() + timeout
        while True:
            left = end - time.monotonic()
            if left <= 0:
                raise TimeoutError(f"{self.label}: timeout waiting")
            try:
                ev = self.q.get(timeout=left)
            except queue.Empty:
                raise TimeoutError(f"{self.label}: timeout waiting")
            if ev is None:
                raise EOFError(f"{self.label}: stdout closed (exit={self.p.poll()}) stderr={self.stderr[-5:]}")
            if on:
                on(ev)
            if pred(ev):
                return ev

    def until_result(self, timeout: float = 180, on=None) -> dict:
        return self.until(lambda e: e.get("type") == "result", timeout, on)

    def control_response(self, rid: str, timeout: float = 30) -> dict:
        return self.until(lambda e: e.get("type") == "control_response"
                          and e.get("response", {}).get("request_id") == rid, timeout)

    # ---- stopping ------------------------------------------------------
    def close_stdin(self) -> None:
        try:
            self.p.stdin.close()
        except Exception:
            pass

    def wait_exit(self, timeout: float = 60) -> int | None:
        try:
            return self.p.wait(timeout=timeout)
        except subprocess.TimeoutExpired:
            return None

    def kill_group(self, sig=signal.SIGKILL) -> None:
        try:
            os.killpg(self.pgid, sig)
        except (ProcessLookupError, PermissionError):  # macOS: EPERM when only the zombie leader is left
            pass

    def state(self) -> str:
        """'running', 'zombie' (exited, not yet reaped: pid and group id still reserved) or 'reaped'.
        Never reaps (waitid with WNOWAIT)."""
        if self.p.returncode is not None:
            return "reaped"
        try:
            r = os.waitid(os.P_PID, self.p.pid, os.WEXITED | os.WNOHANG | os.WNOWAIT)
        except ChildProcessError:
            return "reaped"
        return "running" if r is None else "zombie"

    def wait_unreaped(self, timeout: float) -> str:
        """Wait up to timeout for claude to exit WITHOUT reaping it; checks at least once.
        Returns state(). While it's a zombie, a killpg can't hit a recycled group."""
        end = time.monotonic() + timeout
        while True:
            st = self.state()
            if st != "running" or time.monotonic() >= end:
                return st
            time.sleep(0.05)

    def stop(self, timeout: float) -> None:
        """SIGTERM claude if it doesn't exit by itself (it then kills its tools, which run in their own
        sessions, item 7); SIGKILL its group only while the leader is unreaped or alive; then reap.
        A leader that's already reaped is left alone: its group id may belong to someone else."""
        st = self.wait_unreaped(timeout)
        if st == "running":
            try:
                os.kill(self.p.pid, signal.SIGTERM)  # not Popen.terminate(): that polls, and may reap
            except ProcessLookupError:
                pass
            st = self.wait_unreaped(10)
        if st == "reaped":
            return
        self.kill_group()
        self.wait_exit(5)

    def close(self, timeout: float = 60) -> int | None:
        self.close_stdin()
        self.stop(timeout)
        code = self.p.returncode
        if self in _live:
            _live.remove(self)
        return code

    # ---- recording -----------------------------------------------------
    def events(self, typ: str | None = None, sub: str | None = None) -> list[dict]:
        out = []
        for r in self.log:
            m = r["msg"]
            if r["dir"] == "out" and (typ is None or m.get("type") == typ) and (sub is None or m.get("subtype") == sub):
                out.append(m)
        return out

    def save(self, name: str, scrub_paths: list[str] | None = None, start: int = 0, end: int | None = None) -> pathlib.Path:
        FIXTURES.mkdir(parents=True, exist_ok=True)
        path = FIXTURES / f"{name}.jsonl"
        sc = Scrubber(scrub_paths or [], corpus=self.log)
        with path.open("w") as f:
            for r in self.log[start:end]:
                f.write(json.dumps(sc.scrub(r), ensure_ascii=False) + "\n")
        return path


_pty_children: list[int] = []  # pids from pty.fork (item 10)
# Folders the CLI creates in the real ~/.claude regardless of CLAUDE_CONFIG_DIR (README fact 11).
_HOME_CLAUDE_DIRS = [os.path.join(HOME, ".claude", d) for d in ("ssh", "bridge-spawn")]
_preexisting = {d for d in _HOME_CLAUDE_DIRS if os.path.exists(d)}


@atexit.register
def _kill_all():
    for s in list(_live):
        try:
            s.stop(0)  # skips sessions already reaped; signals a group only while its leader is unreaped
        except Exception:
            pass
    for pid in _pty_children:
        try:  # skip children already reaped: their pid may belong to someone else now
            if os.waitpid(pid, os.WNOHANG) != (0, 0):
                continue
        except ChildProcessError:
            continue
        for sig in (signal.SIGTERM, signal.SIGKILL):
            try:
                os.kill(pid, sig)
                time.sleep(1)
            except ProcessLookupError:
                break
    for d in _HOME_CLAUDE_DIRS:  # only after a sandboxed session (fact 11); only empty folders this run created
        if _sandbox_spawned and d not in _preexisting and os.path.isdir(d) and not os.listdir(d):
            os.rmdir(d)
    for d in _tmpdirs:
        if KEEP_TMP:
            print(f"kept {d}", file=sys.stderr)
        else:
            shutil.rmtree(d, ignore_errors=True)
    if TURNS:
        print(f"model turns sent by this script: {len(TURNS)}", file=sys.stderr)


UUID_RE = re.compile(r"[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}")
EMAIL_RE = re.compile(r"[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Za-z]{2,}")
TOKEN_RE = re.compile(r"(sk-ant-[A-Za-z0-9_-]{8,}|gh[pousr]_[A-Za-z0-9]{20,}|eyJ[A-Za-z0-9_-]{20,})")
PERSONAL_KEYS = {"subscriptionType", "email", "accountUuid", "organizationUuid", "orgId", "orgName", "account_uuid",
                 "organization_uuid", "userID", "user_id", "emailAddress", "displayName"}


BLOB_KEYS = {"signature"}  # opaque thinking-block signatures: no value for the adapter, so blanked
RATE_KEYS = {"rate_limit_info", "overageStatus", "overageDisabledReason", "isUsingOverage", "utilization",
             "resetsAt"}
# Synthetic, type-preserving values for the account's rate-limit data (README "Synthetic values").
SYNTH_RATE = {"status": "allowed", "rateLimitType": "five_hour", "resetsAt": 1800000000,
              "overageStatus": "allowed", "overageDisabledReason": "synthetic_reason", "isUsingOverage": False,
              "utilization": 0.42}
SYNTH_WINDOW = {"five_hour": {"utilization": 0.42, "resetsAt": 1800000000, "status": "allowed"},
                "seven_day": {"utilization": 0.17, "resetsAt": 1800500000, "status": "allowed"}}


def _synth_leaf(key, value, window=None):
    if window in SYNTH_WINDOW and key in SYNTH_WINDOW[window]:
        return SYNTH_WINDOW[window][key]
    if key in SYNTH_RATE:
        return SYNTH_RATE[key]
    # an unknown field: keep its JSON type with a neutral value
    if isinstance(value, bool):
        return False
    if isinstance(value, (int, float)):
        return 0
    if value is None:
        return None
    return "synthetic"


def _synth_rate(o, key=None, window=None):
    """Same keys and shape; every leaf replaced by a synthetic value of the same JSON type."""
    if isinstance(o, dict):
        return {k: _synth_rate(v, k, k if key == "unifiedWindows" else window) for k, v in o.items()}
    if isinstance(o, list):
        return [_synth_rate(v, key, window) for v in o]
    return _synth_leaf(key, o, window)


def _is_used_model(m) -> bool:
    """initialize's models list: only the spike's model (the `haiku` alias) is kept."""
    return isinstance(m, dict) and m.get("value") == "haiku"


def synced_names(corpus) -> tuple[set[str], set[str]]:
    """(namespaces, plain names) of account-synced plugins/skills found anywhere in corpus."""
    ns, plain = set(), set()

    def find_ns(o):
        if isinstance(o, dict):
            if isinstance(o.get("source"), str) and o["source"].endswith("@synced") and isinstance(o.get("name"), str):
                ns.add(o["name"])
            # account skills can load without a plugin entry: a non-builtin command whose aliases are
            # "<namespace>:<skill>" marks that namespace (structural detection; no names are hard-coded)
            if isinstance(o.get("name"), str) and not o.get("builtin") and isinstance(o.get("aliases"), list):
                ns.update(a.split(":", 1)[0] for a in o["aliases"] if isinstance(a, str) and ":" in a)
            for v in o.values():
                find_ns(v)
        elif isinstance(o, list):
            for v in o:
                find_ns(v)

    def find_plain(o):
        if isinstance(o, dict):
            if isinstance(o.get("name"), str) and any(isinstance(a, str) and a.split(":", 1)[0] in ns
                                                      for a in o.get("aliases") or []):
                plain.add(o["name"])
            for v in o.values():
                find_plain(v)
        elif isinstance(o, list):
            for v in o:
                find_plain(v)

    find_ns(corpus)
    find_plain(corpus)
    return ns, plain


class Scrubber:
    """Replace temp paths, the home path, uuids, emails and tokens with stable placeholders; replace the
    account's rate-limit and overage values with synthetic, type-preserving ones (keys kept); drop
    account-synced plugins (source "…@synced") and their commands and skills; keep only haiku in models.
    corpus: everything that will be scrubbed with this instance, used to find the synced names."""

    def __init__(self, paths: list[str], corpus=None):
        roots = set(_tmpdirs) | {os.path.realpath(d) for d in _tmpdirs}
        self.paths = sorted({p for p in paths if p} | {os.path.realpath(p) for p in paths if p} | roots
                            | {"/private/tmp/claude-spike-", "/tmp/claude-spike-"}, key=len, reverse=True)
        self.ids: dict[str, str] = {}
        self.ns, self.plain = synced_names(corpus) if corpus is not None else (set(), set())

    def synced(self, x) -> bool:
        if isinstance(x, dict):
            return (isinstance(x.get("source"), str) and x["source"].endswith("@synced")) or self.synced(x.get("name"))
        return isinstance(x, str) and (x in self.plain or (":" in x and x.split(":", 1)[0] in self.ns))

    def _s(self, s: str) -> str:
        for p in self.paths:
            s = s.replace(p, "<tmp>/" if p.endswith("claude-spike-") else "<tmp>")
        s = s.replace(HOME, "<home>")
        s = EMAIL_RE.sub("<email>", s)  # before the $USER pass, so a user-named email is still caught whole
        s = TOKEN_RE.sub("<token>", s)
        if os.environ.get("USER"):
            s = re.sub(rf"\b{re.escape(os.environ['USER'])}\b", "<user>", s)
        return UUID_RE.sub(lambda m: self.ids.setdefault(m.group(0), f"<uuid-{len(self.ids) + 1}>"), s)

    def scrub(self, o):
        if isinstance(o, dict):
            out = {}
            plan_hint = o.get("name") in PLAN_HINT_COMMANDS and "description" in o
            for k, v in o.items():
                if plan_hint and k == "description":
                    out[k] = "<scrubbed>"
                elif k == "account" and isinstance(v, dict):
                    out[k] = {kk: ("<scrubbed>" if vv not in (None, "", [], {}) else vv)
                              for kk, vv in v.items() if kk != "organization"}
                elif k == "models" and isinstance(v, list):  # initialize's model list hints at the plan:
                    out[k] = [{kk: ("<scrubbed>" if kk in ("description", "displayName") else self.scrub(vv))
                               for kk, vv in m.items()} for m in v if _is_used_model(m)]  # keep only haiku
                elif k in BLOB_KEYS and isinstance(v, str) and v:
                    out[k] = "<scrubbed>"
                elif k == "rate_limit_info":
                    out[k] = _synth_rate(v)
                elif k in RATE_KEYS:
                    out[k] = _synth_leaf(k, v)
                elif k in PERSONAL_KEYS and v:
                    out[k] = "<scrubbed>"
                else:
                    out[k] = self.scrub(v)
            return out
        if isinstance(o, list):
            return [self.scrub(v) for v in o if not self.synced(v)]
        if isinstance(o, str):
            return self._s(o)
        return o


def write_result(name: str, data) -> pathlib.Path:
    RESULTS.mkdir(parents=True, exist_ok=True)
    p = RESULTS / f"{name}.json"
    p.write_text(json.dumps(Scrubber([], corpus=data).scrub(data), indent=2, ensure_ascii=False) + "\n")
    return p


TUI_NOTICE_RE = re.compile(r"(Getto|Get to|notice hidden|usage-credits|extra-usage|from your admin|Switch ?any ?time)", re.I)


def crop_tui_notices(text: str) -> str:
    """Drop TUI notice lines that hint at the account's plan or org (an upsell line, the hidden-notices
    count, the /usage-credits help line) and blank the plan name in the banner."""
    text = re.sub(r"Claude (Pro|Max|Team|Enterprise|Ultra)\b", "Claude <plan>", text)
    return "\n".join(line for line in text.splitlines() if not TUI_NOTICE_RE.search(line)) + "\n"


def brief(ev: dict) -> str:
    """One-line summary of an event, for console output."""
    t = ev.get("type")
    if t == "assistant":
        parts = []
        for c in ev.get("message", {}).get("content", []):
            if c.get("type") == "text":
                parts.append("text:" + c["text"][:80])
            elif c.get("type") == "tool_use":
                parts.append(f"tool_use:{c['name']}:{json.dumps(c.get('input'))[:100]}")
            else:
                parts.append(c.get("type"))
        return "assistant " + " | ".join(parts)
    if t == "user":
        parts = []
        for c in ev.get("message", {}).get("content", []) if isinstance(ev.get("message", {}).get("content"), list) else []:
            if c.get("type") == "tool_result":
                cc = c.get("content")
                parts.append(f"tool_result(err={c.get('is_error')}):{json.dumps(cc)[:120]}")
        return "user " + " | ".join(parts)
    if t == "result":
        return (f"result {ev.get('subtype')} is_error={ev.get('is_error')} terminal={ev.get('terminal_reason')} "
                f"result={str(ev.get('result'))[:80]!r} cost={ev.get('total_cost_usd')} queued={ev.get('queued_turn_count')}")
    if t == "control_request":
        r = ev.get("request", {})
        return f"control_request {r.get('subtype')} {json.dumps(r)[:200]}"
    if t == "control_response":
        return f"control_response {json.dumps(ev.get('response'))[:200]}"
    if t == "system":
        return f"system/{ev.get('subtype')}"
    return str(t)


def seed_config(cfg: pathlib.Path, trusted: list[str] | None = None) -> None:
    """Pre-seed a fresh config dir so an interactive TUI skips onboarding and the trust dialog."""
    data = {"hasCompletedOnboarding": True, "theme": "dark", "projects": {}}
    for d in trusted or []:
        data["projects"][d] = {"hasTrustDialogAccepted": True, "hasCompletedProjectOnboarding": True}
    (cfg / ".claude.json").write_text(json.dumps(data))


def new_session_id() -> str:
    return str(uuid.uuid4())
