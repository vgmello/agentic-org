#!/usr/bin/env python3
"""Re-apply lib.Scrubber to every existing fixture and result. Offline: no claude, no model call.

Use it after tightening the scrubber, so recorded files match what a re-run would write:
rate-limit and overage values blanked (keys kept), account-synced plugins and their commands and
skills dropped, plus the usual paths, uuids, emails and tokens. The synced names are collected
from all files together, because some files list a synced command without its plugin.
It also crops TUI notice lines from fixtures/takeover-pty.txt (plan, upsell and admin hints).
"""
import json

import lib

jsonl = sorted(lib.FIXTURES.glob("*.jsonl"))
results = sorted(lib.RESULTS.glob("*.json"))
records = {p: [json.loads(line) for line in p.read_text().splitlines() if line.strip()] for p in jsonl}
docs = {p: json.loads(p.read_text()) for p in results}
corpus = [r for rs in records.values() for r in rs] + list(docs.values())
ns, plain = lib.synced_names(corpus)
print(f"synced namespaces: {len(ns)}, synced plain names: {len(plain)}")

for p, rs in records.items():
    sc = lib.Scrubber([], corpus=corpus)
    p.write_text("".join(json.dumps(sc.scrub(r), ensure_ascii=False) + "\n" for r in rs))
for p, d in docs.items():
    sc = lib.Scrubber([], corpus=corpus)
    p.write_text(json.dumps(sc.scrub(d), indent=2, ensure_ascii=False) + "\n")

pty = lib.FIXTURES / "takeover-pty.txt"
if pty.exists():
    pty.write_text(lib.crop_tui_notices(pty.read_text()))
print(f"re-scrubbed {len(jsonl)} fixtures, {len(results)} results, and takeover-pty.txt")
