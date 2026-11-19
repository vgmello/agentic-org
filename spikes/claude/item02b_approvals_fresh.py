#!/usr/bin/env python3
"""Item 2, part b: the deny variants that need a fresh conversation each.

In one conversation, after a first deny the model refuses later writes on its own (no
can_use_tool is even sent), so deny-interrupt, deny-invalid and error-response each get
their own process here. Fixtures: approval-<scenario>.jsonl. Model turns: 3 (+ follow-ups).
"""
import json

import lib

PROMPT = ("Use the Bash tool to run exactly this command and nothing else: {cmd}\n"
          "If the tool call is denied or fails, do not retry and do not try anything else: "
          "reply with the word DENIED followed by the reason you were given.")
SCENARIOS = {
    "deny-interrupt": ("ok", {"behavior": "deny", "message": "Stopped by the owner", "interrupt": True}),
    "deny-invalid": ("ok", {"behavior": "deny"}),
    "error-response": ("err", "office: approval store unavailable"),
}
b = lib.make_tmp("s2b")
ws = b / "ws"
out = {}
argv = lib.stream_args("--permission-prompt-tool", "stdio", "--permission-mode", "manual")
for name, (kind, payload) in SCENARIOS.items():
    s = lib.Session(argv, lib.clean_env(b / "cfg", b / "tmp"), ws, f"item02b-{name}")
    s.user(PROMPT.format(cmd=f"echo x > {name}.txt"))
    reqs = []

    def answer(ev, s=s, kind=kind, payload=payload):
        reqs.append(ev["request_id"])
        (s.respond if kind == "ok" else s.respond_error)(ev["request_id"], payload)

    approve = lib.approver(s, [("Bash", f"echo x > {name}.txt")], on_allowed=answer)  # anything else: denied

    def on(ev, approve=approve):
        print(name, lib.brief(ev))
        approve(ev)

    r = s.until_result(180, on)
    tool_results = [c for e in s.events("user") for c in (e["message"]["content"] if isinstance(e["message"]["content"], list) else [])
                    if c.get("type") == "tool_result"]
    s.user("Reply with exactly the word ALIVE.")
    deny_all = lib.approver(s, [])  # no tool expected: deny (and log) any request
    r2 = s.until_result(120, lambda e: (print(name, "after", lib.brief(e)), deny_all(e)))
    out[name] = {"answer": payload, "requests": len(reqs),
                 "tool_result": tool_results,
                 "result": {k: r.get(k) for k in ("subtype", "is_error", "result", "terminal_reason", "permission_denials")},
                 "next_turn": r2.get("result"), "file_written": (ws / f"{name}.txt").exists()}
    out[name]["exit"] = s.close(30)
    out[name]["denied_unexpected_calls"] = s.denials
    s.save(f"approval-{name}", [str(b)])
lib.write_result("item02b", out)
print(json.dumps(lib.Scrubber([str(b)]).scrub(out), indent=1)[:5000])
