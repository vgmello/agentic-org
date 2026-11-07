#!/usr/bin/env python3
"""A receiver that accepts and never answers (spike S1): proves the mod never holds the chain."""
import os, socket, sys, time, threading
path = sys.argv[1]
try: os.unlink(path)
except FileNotFoundError: pass
s = socket.socket(socket.AF_UNIX); s.bind(path); os.chmod(path, 0o600); s.listen(64)
held = []
deadline = time.time() + float(sys.argv[2] if len(sys.argv) > 2 else 120)
s.settimeout(1)
while time.time() < deadline:
    try: c, _ = s.accept()
    except socket.timeout: continue
    held.append(c)
    c.settimeout(0.05)
    try: head = c.recv(4096).decode(errors='replace')
    except Exception: head = ''
    hook = head.split('"hook":"')[1].split('"')[0] if '"hook":"' in head else '?'
    print(f'{time.time()*1000:.1f} accept #{len(held)} hook={hook} (never answering)', flush=True)
print(f'done; {len(held)} connections held open', flush=True)
