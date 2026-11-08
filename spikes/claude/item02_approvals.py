#!/usr/bin/env python3
"""Item 2: approvals over stdio (`--permission-prompt-tool stdio`, can_use_tool).

Process P1 (`--permission-prompt-tool stdio --permission-mode manual`), one turn per scenario:
  allow            {"behavior":"allow"} (no updatedInput)
  allow-rewrite    {"behavior":"allow","updatedInput":{...changed command...}}
  deny-message     {"behavior":"deny","message":"..."}
  deny-interrupt   {"behavior":"deny","message":"...","interrupt":true}
  deny-invalid     {"behavior":"deny"} (no message: schema-invalid)
  error-response   control_response subtype "error"
  no-answer        never answered for NO_ANSWER_S seconds, then an interrupt
Process P2: request pending, then stdin closed (no answer ever).
Process P3: no --permission-prompt-tool flag at all (does a can_use_tool still arrive?).
Fixtures: approval-<scenario>.jsonl. Model turns: 10 (8 in P1, 1 each in P2 and P3).
"""
import json
import sys
import time

import lib

NO_ANSWER_S = next((int(a) for a in sys.argv[1:] if a.isdigit()), 150)  # first numeric arg; flags like --keep ignored
PROMPT = ("Use the Bash tool to run exactly this command and nothing else: {cmd}\n"
          "If the tool call is denied or fails, do not retry and do not try anything else: "
          "reply with the word DENIED followed by the reason you were given.")

b = lib.make_tmp("s2")
ws = b / "ws"
out = {"scenarios": {}}


def run_turn(s, name, cmd, answer, wait_answer_s=0):
    """Send one prompt; answer each can_use_tool with answer(req) (None = never answer)."""
    start = len(s.log)
    s.user(PROMPT.format(cmd=cmd))
    seen = {"requests": [], "cancels": []}
    t_req = None

    def on(ev):
        nonlocal t_req
        print(name, lib.brief(ev))
        if ev.get("type") == "control_request" and ev["request"].get("subtype") == "can_use_tool":
            seen["requests"].append(ev)
            t_req = time.monotonic()
            req = ev["request"]
            if (req.get("tool_name"), (req.get("input") or {}).get("command")) != ("Bash", cmd):
                # not this scenario's exact command: deny, never apply the scenario's answer to it
                lib.approver(s, [])(ev)
                return
            a = answer(ev)
            if a is not None:
                kind, payload = a
                if kind == "ok":
                    s.respond(ev["request_id"], payload)
                else:
                    s.respond_error(ev["request_id"], payload)
        if ev.get("type") == "control_cancel_request":
            seen["cancels"].append(ev)

    if wait_answer_s:
        # wait for the request, then sit on it
        s.until(lambda e: e.get("type") == "control_request", 120, on)
        try:
            ev = s.until(lambda e: e.get("type") == "result", wait_answer_s, on)
            seen["result_without_answer"] = True
        except TimeoutError:
            seen["result_without_answer"] = False
            seen["waited_s"] = round(time.monotonic() - t_req, 1)
            rid = s.control("interrupt")
            ev = s.until(lambda e: e.get("type") == "result", 60, on)
    else:
        ev = s.until_result(180, on)
    seen["result"] = {k: ev.get(k) for k in ("subtype", "is_error", "result", "terminal_reason", "permission_denials")}
    seen["files"] = sorted(p.name for p in ws.iterdir())
    out["scenarios"][name] = seen
    # deny-interrupt, deny-invalid and error-response get their real fixtures from item02b (fresh
    # processes); here the model refuses before asking, so keep these under another name.
    prefix = "approval-inconv-" if name in ("deny-interrupt", "deny-invalid", "error-response") else "approval-"
    s.save(f"{prefix}{name}", [str(b)], start=start)
    return seen


argv = lib.stream_args("--permission-prompt-tool", "stdio", "--permission-mode", "manual")
s = lib.Session(argv, lib.clean_env(b / "cfg", b / "tmp"), ws, "item02-P1")
out["P1_argv"] = argv
s.control_response(s.control("initialize"))

run_turn(s, "allow", "echo approved > allow.txt", lambda r: ("ok", {"behavior": "allow"}))
run_turn(s, "allow-rewrite", "echo original > original.txt",
         lambda r: ("ok", {"behavior": "allow", "updatedInput": {**r["request"]["input"],
                                                                  "command": "echo rewritten > rewritten.txt"}}))
run_turn(s, "deny-message", "echo nope > deny.txt",
         lambda r: ("ok", {"behavior": "deny", "message": "Office policy: writes need the owner's approval"}))
run_turn(s, "deny-interrupt", "echo nope > deny2.txt",
         lambda r: ("ok", {"behavior": "deny", "message": "Stopped by the owner", "interrupt": True}))
run_turn(s, "deny-invalid", "echo nope > deny3.txt", lambda r: ("ok", {"behavior": "deny"}))
run_turn(s, "error-response", "echo nope > deny4.txt", lambda r: ("err", "office: approval store unavailable"))
run_turn(s, "no-answer", "echo late > late.txt", lambda r: None, wait_answer_s=NO_ANSWER_S)
# after the interrupt, is the session still usable?
s.user("Reply with exactly the word ALIVE.")
deny_all = lib.approver(s, [])  # no tool expected: deny (and log) any request
r = s.until_result(120, lambda e: (print("after", lib.brief(e)), deny_all(e)))
out["alive_after_no_answer"] = r.get("result")
out["P1_exit"] = s.close(60)
out["P1_stderr"] = s.stderr[-10:]

# P2: request pending, stdin closed
s2 = lib.Session(argv, lib.clean_env(b / "cfg", b / "tmp"), ws, "item02-P2")
s2.user(PROMPT.format(cmd="echo closed > closed.txt"))
req = s2.until(lambda e: e.get("type") == "control_request", 120, lambda e: print("P2", lib.brief(e)))
t = time.monotonic()
s2.close_stdin()  # the request is never answered
tail = []
try:
    while True:
        ev = s2.next(90)
        if ev is None:
            break
        tail.append(ev)
        print("P2 after close", lib.brief(ev))
except Exception as e:  # queue.Empty
    tail.append({"_timeout": str(e)})
exit_code = s2.close(30)  # waits unreaped, group-kills only while the leader is ours, then reaps
out["P2_pending_then_stdin_closed"] = {"exit": exit_code, "seconds": round(time.monotonic() - t, 1),
                                       "events_after_close": [lib.brief(e) if "type" in e else e for e in tail],
                                       "stderr": s2.stderr[-10:], "file_written": (ws / "closed.txt").exists()}
s2.save("approval-pending-stdin-closed", [str(b)])

# P3: no --permission-prompt-tool
argv3 = lib.stream_args("--permission-mode", "manual")
s3 = lib.Session(argv3, lib.clean_env(b / "cfg", b / "tmp"), ws, "item02-P3")
s3.user(PROMPT.format(cmd="echo noflag > noflag.txt"))
evs = []
deny3 = lib.approver(s3, [])  # none is expected without the flag; if one came, it's denied (and recorded)
r3 = s3.until_result(180, lambda e: (evs.append(lib.brief(e)), print("P3", lib.brief(e)), deny3(e)))
out["P3_no_prompt_tool_flag"] = {"control_requests": [e for e in evs if e.startswith("control_request")],
                                 "result": {k: r3.get(k) for k in ("subtype", "result", "permission_denials")},
                                 "file_written": (ws / "noflag.txt").exists()}
s3.close(30)
s3.save("approval-no-prompt-tool-flag", [str(b)])

out["denied_unexpected_calls"] = s.denials + s2.denials + s3.denials
lib.write_result("item02", out)
print(json.dumps(lib.Scrubber([str(b)]).scrub(out), indent=1, default=str)[:6000])
