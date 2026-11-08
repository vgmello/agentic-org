#!/usr/bin/env python3
"""Item 1 (long-lived stream-json) and item 11 (memory of an idle process).

One process, kept alive across turns:
  initialize control request -> turn 1 (text only) -> turn 2 (Read tool) ->
  two user lines written back to back (queued turn) -> close stdin.
RSS is sampled after initialize (no turn yet), after turn 1 and at the end.
Fixtures: stream-json-multi-turn.jsonl, stream-json-queued.jsonl
Model turns: 4.
"""
import json
import time

import lib

b = lib.make_tmp("s1")
(b / "ws" / "note.txt").write_text("TWO-FROM-FILE\n")
s = lib.Session(lib.stream_args("--permission-mode", "manual"), lib.clean_env(b / "cfg", b / "tmp"), b / "ws", "item01")
res = {"argv": s.argv[1:], "rss_kb": {}}

rid = s.control("initialize")
s.control_response(rid)
time.sleep(5)
res["rss_kb"]["idle_after_initialize"] = lib.rss_kb(s.p.pid)

s.user("Reply with exactly the word ONE and nothing else.")
r1 = s.until_result(on=lambda e: print(lib.brief(e)))
time.sleep(5)
res["rss_kb"]["idle_after_turn_1"] = lib.rss_kb(s.p.pid)
res["alive_after_turn_1"] = s.p.poll() is None

s.user("Use the Read tool to read note.txt in the current directory, then reply with exactly its contents.")
r2 = s.until_result(on=lambda e: print(lib.brief(e)))
mark = len(s.log)

s.user("Reply with exactly the word THREE.")
s.user("Reply with exactly the word FOUR.")
r3 = s.until_result(on=lambda e: print(lib.brief(e)))
r4 = s.until_result(on=lambda e: print(lib.brief(e)))
time.sleep(5)
res["rss_kb"]["idle_after_turn_4"] = lib.rss_kb(s.p.pid)

t = time.monotonic()
res["exit_code_after_stdin_close"] = s.close(60)
res["exit_seconds"] = round(time.monotonic() - t, 2)
res["stderr_tail"] = s.stderr[-10:]
res["results"] = [{k: r.get(k) for k in ("subtype", "is_error", "result", "stop_reason", "terminal_reason", "num_turns",
                                          "result_index", "queued_turn_count", "total_cost_usd")} for r in (r1, r2, r3, r4)]
res["event_types"] = sorted({f"{e['msg'].get('type')}/{e['msg'].get('subtype', '')}" for e in s.log if e["dir"] == "out"})
res["init_count"] = len(s.events("system", "init"))
init = s.events("system", "init")[0]
res["init_keys"] = sorted(init.keys())
lib.write_result("item01", res)
s.save("stream-json-multi-turn", [str(b)], end=mark)
s.save("stream-json-queued", [str(b)], start=mark)
print(json.dumps(res, indent=2)[:3000])
