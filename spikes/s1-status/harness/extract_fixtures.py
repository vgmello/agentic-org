#!/usr/bin/env python3
"""Cut receiver test fixtures from recorded raw events, by socket peer pid (spike S1).
Usage: extract_fixtures.py <raw-events.jsonl> <out-dir> name=pid [name=pid ...]"""
import json, sys, os
raw, out = sys.argv[1], sys.argv[2]
events = [json.loads(l) for l in open(raw) if l.strip()]
for spec in sys.argv[3:]:
    name, pid = spec.split('=')
    rows = [e for e in events if e.get('_peer_pid') == int(pid)]
    rows.sort(key=lambda e: e['_t_recv'])
    with open(os.path.join(out, f'{name}.jsonl'), 'w') as f:
        for e in rows:
            f.write(json.dumps(e, sort_keys=True) + '\n')
    print(name, len(rows), 'events')
