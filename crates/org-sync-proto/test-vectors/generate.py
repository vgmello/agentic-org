# Independent reference: Python standard-library SHA-256 and explicit byte assembly.
# Run from the worktree root: python3 crates/org-sync-proto/test-vectors/generate.py
from pathlib import Path
import hashlib,json,struct
rows=[]
def add(name,org,epoch,seq,previous,codec,version,payload):
 field=lambda b:struct.pack('>I',len(b))+b
 preimage=b'agentic-org/org-event/v1\x00'+field(org.encode())+struct.pack('>QQ',epoch,seq)+previous+field(codec.encode())+struct.pack('>I',version)+field(payload)
 digest=hashlib.sha256(preimage).digest()
 rows.append(dict(name=name,org_id=org,epoch=epoch,seq=seq,previous_hex=previous.hex(),codec_id=codec,codec_version=version,payload_hex=payload.hex(),preimage_hex=preimage.hex(),digest_hex=digest.hex()))
 return digest
add('known-answer','ORG7',2,3,bytes(range(32)),'org.test',7,b'\x00\xffA\x00B')
first=add('first-event','ORG1',1,1,bytes(32),'org',1,b'{"created":true}')
add('epoch-change','ORG1',2,1,first,'org',1,b'{"restored":true}')
Path('crates/org-sync-proto/test-vectors/event-digest-v1.json').write_text(json.dumps(rows,indent=2)+'\n')
for row in rows: print(row['name'],row['digest_hex'])
