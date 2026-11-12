"""Independent Work ACK v1 proof preimage reference. Run from repository root."""
from pathlib import Path
import hashlib,json,struct
field=lambda b:struct.pack('>I',len(b))+b
cases=[dict(office='OFF1',incarnation='INC1',from_exclusive=0,through_seq=3,rejected=[dict(seq=2,code='not_owner')]),dict(office='HQ1',incarnation='01ARZ3NDEKTSV4RRFFQ69G5FAV',from_exclusive=90,through_seq=100,rejected=[dict(seq=99,code='stale_version')])]
for row in cases:
 pre=(b'agentic-org/work-ack/v1\0'+field(row['office'].encode())+field(row['incarnation'].encode())+struct.pack('>QQI',row['from_exclusive'],row['through_seq'],len(row['rejected'])))
 for r in row['rejected']:pre+=struct.pack('>Q',r['seq'])+field(r['code'].encode())
 row['preimage_hex']=pre.hex();row['digest_hex']=hashlib.sha256(pre).hexdigest()
Path('crates/org-sync-proto/test-vectors/work-ack-v1.json').write_text(json.dumps(cases,indent=2)+'\n')
