#!/usr/bin/env python3
"""Item 3: interrupt mid-turn with the `interrupt` control request.

One process with --include-partial-messages:
  A. during a Bash tool call (`sh wait47.sh`, pre-allowed; a bare `sleep 47` is blocked by
     Claude Code itself): interrupt once the sleep runs; check whether it survives.
  B. during streamed text: interrupt at the first text delta. (The recorded run predates this
     predicate and matched any content_block_delta, so it interrupted during thinking.)
  C. a follow-up turn to show the session is still usable.
  D. an interrupt while idle (no turn running).
Fixtures: interrupt-during-tool.jsonl, interrupt-during-text.jsonl. Model turns: 3.
"""
import json
import subprocess
import time

import lib

b = lib.make_tmp("s3")
(b / "ws" / "wait47.sh").write_text("sleep 47\necho done\n")
argv = lib.stream_args("--permission-mode", "manual", "--permission-prompt-tool", "stdio",
                       "--allowedTools", "Bash(sh wait47.sh)", "--include-partial-messages")
s = lib.Session(argv, lib.clean_env(b / "cfg", b / "tmp"), b / "ws", "item03")
out = {"argv": argv}


def sleepers():
    ps = subprocess.run(["ps", "-A", "-o", "pid=,ppid=,pgid=,command="], capture_output=True, text=True).stdout
    return [l.strip() for l in ps.splitlines() if "sleep 47" in l and "ps -A" not in l]


# A: during a tool call
start = len(s.log)
s.user("Use the Bash tool to run exactly: sh wait47.sh (in the foreground, not in the background). Then reply with the output.")
approvals = []
guard = lib.approver(s, [("Bash", "sh wait47.sh")])  # only the scenario's exact command; anything else denied


def allow(ev):
    if ev.get("type") == "control_request" and ev["request"].get("subtype") == "can_use_tool":
        approvals.append(ev["request"])
    guard(ev)


s.until(lambda e: e.get("type") == "assistant" and any(c.get("type") == "tool_use" for c in e["message"]["content"]), 120, allow)
try:  # keep answering approvals for 4 s while the tool starts
    s.until(lambda e: False, 4, allow)
except TimeoutError:
    pass
out["A_approvals_asked"] = approvals
out["A_sleepers_before"] = sleepers()
t = time.monotonic()
rid = s.control("interrupt")
resp = s.control_response(rid, 30)
out["A_interrupt_ack_ms"] = int((time.monotonic() - t) * 1000)
out["A_interrupt_response"] = resp["response"]
r = s.until_result(60, lambda e: (guard(e), print("A", lib.brief(e)) if e.get("type") != "stream_event" else None))
out["A_result_ms_after_interrupt"] = int((time.monotonic() - t) * 1000)
out["A_result"] = {k: r.get(k) for k in ("subtype", "is_error", "result", "terminal_reason", "stop_reason")}
time.sleep(2)
out["A_sleepers_after"] = sleepers()
s.save("interrupt-during-tool", [str(b)], start=start)

# B: during streamed text
start = len(s.log)
s.user("Write a 400-word story about a lighthouse keeper. No preamble.")
s.until(lambda e: e.get("type") == "stream_event" and e.get("event", {}).get("type") == "content_block_delta"
        and e["event"].get("delta", {}).get("type") == "text_delta", 120, guard)
mark_b = len(s.log)
t = time.monotonic()
rid = s.control("interrupt")
resp = s.control_response(rid, 30)
out["B_interrupt_response"] = resp["response"]
r = s.until_result(60, guard)
out["B_result_ms_after_interrupt"] = int((time.monotonic() - t) * 1000)
out["B_result"] = {k: r.get(k) for k in ("subtype", "is_error", "result", "terminal_reason", "stop_reason")}
out["B_assistant_events_after_interrupt"] = sum(1 for e in s.log[mark_b:] if e["msg"].get("type") == "assistant")
s.save("interrupt-during-text", [str(b)], start=start)

# C: still usable
s.user("Reply with exactly the word AFTER.")
out["C_next_turn"] = s.until_result(60, guard).get("result")

# D: interrupt while idle
rid = s.control("interrupt")
out["D_idle_interrupt_response"] = s.control_response(rid, 30)["response"]
time.sleep(2)
out["D_events_after_idle_interrupt"] = [lib.brief(e["msg"]) for e in s.log if e["dir"] == "out"][-3:]
out["stream_event_types"] = sorted({e["msg"].get("event", {}).get("type") for e in s.log if e["msg"].get("type") == "stream_event"})
out["exit"] = s.close(30)
out["denied_unexpected_calls"] = s.denials
lib.write_result("item03", out)
print(json.dumps(lib.Scrubber([str(b)]).scrub(out), indent=1)[:4000])
