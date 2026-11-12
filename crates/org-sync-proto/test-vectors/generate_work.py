"""Independent standard-library reference for Rowan's Work v1 preimages.

Run from the worktree root: python3 crates/org-sync-proto/test-vectors/generate_work.py
"""
from pathlib import Path
import hashlib
import json
import struct

u32 = lambda value: struct.pack(">I", value)
u64 = lambda value: struct.pack(">Q", value)
field = lambda value: u32(len(value)) + value
cases = [
    dict(office="OFF1", incarnation="INC1", seq=1, record="TASK1", generation=1,
         version=1, codec="org.native.changes", codec_version=1,
         payload=b'{"event":"created"}', epoch=1, org_seq=4, created=100),
    dict(office="HQ1", incarnation="01ARZ3NDEKTSV4RRFFQ69G5FAV", seq=2**64-1,
         record="TASK2", generation=2, version=1, codec="org.native.changes",
         codec_version=1, payload=b"BINARY\0\xff", epoch=2, org_seq=0, created=2**64-1),
]
rows = []
for case in cases:
    key = field(case["office"].encode()) + field(case["incarnation"].encode()) + u64(case["seq"])
    record = (b"agentic-org/work-record/v1\0" + field(case["record"].encode())
              + u64(case["generation"]) + u64(case["version"])
              + field(case["codec"].encode()) + u32(case["codec_version"])
              + u64(len(case["payload"])) + case["payload"])
    record_digest = hashlib.sha256(record).digest()
    source = (b"agentic-org/work-change/v1\0" + key + record_digest
              + u64(case["epoch"]) + u64(case["org_seq"]) + u64(case["created"]))
    rows.append({**{k:v for k,v in case.items() if k != "payload"},
                 "payload_hex":case["payload"].hex(), "source_key_hex":key.hex(),
                 "record_preimage_hex":record.hex(), "record_digest_hex":record_digest.hex(),
                 "source_preimage_hex":source.hex(),
                 "source_digest_hex":hashlib.sha256(source).hexdigest()})
Path("crates/org-sync-proto/test-vectors/work-digest-v1.json").write_text(
    json.dumps(rows, indent=2) + "\n")
