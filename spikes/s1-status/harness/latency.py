#!/usr/bin/env python3
"""Latency summary for spike S1 from the receiver log and usernoted's 'Presenting' log lines.
Usage: latency.py <receiver.log> <usernoted-presenting.log>"""
import json, re, sys, statistics, datetime
rlog, ulog = sys.argv[1], sys.argv[2]
# Accepts a receiver log ('RAW {...}' lines) or a raw-events .jsonl (one JSON object per line).
recv, notifies = [], []
for line in open(rlog):
    if line.startswith('RAW ') or line.startswith('{'):
        d = json.loads(line[4:] if line.startswith('RAW ') else line)
        if 't_send' in d:
            # hook->recv: from the hook firing (or, for Stop/PermissionRequest, from when the chain beneath settled)
            start = d.get('t_after_next', d['t_hook'])
            recv.append((d['hook'], d['_t_recv'] - start))
    # Both log formats: run1 '... | ok | hook->notify-call-start A ms, call B ms, ...' and
    # run3+ '... | ok | hook->notify-call-start A ms, call B ms' (or exit->... for crashes).
    m = re.match(r'([\d.]+) NOTIFY\[native\] (.*?) \| ok \| (hook|exit)->notify-call-start ([\d.]+) ms, call ([\d.]+) ms', line)
    if m:
        notifies.append({'text': m.group(2), 'logged': float(m.group(1)), 'origin': m.group(3),
                         'off': float(m.group(4)), 'call': float(m.group(5))})
pres = []
for line in open(ulog):
    m = re.match(r'(\d{4}-\d\d-\d\d \d\d:\d\d:\d\d\.\d+).*Presenting <NotificationRecord app:"com.apple.ScriptEditor2"', line)
    if m:
        pres.append(datetime.datetime.strptime(m.group(1), '%Y-%m-%d %H:%M:%S.%f').timestamp() * 1000)
def stats(xs):
    xs = sorted(xs)
    return f'n={len(xs)} p50={statistics.median(xs):.1f} ms p90={xs[int(round(0.9*(len(xs)-1)))]:.1f} ms max={xs[-1]:.1f} ms'
print('hook -> receiver (all forwarded events):', stats([x for _, x in recv]))
for h in sorted({h for h, _ in recv}):
    print(f'  {h:18s}', stats([x for hh, x in recv if hh == h]))
# Match each native notify call to the first 'Presenting' at or after its start (call start = logged - ~2005 ms wait).
rows = []
for n in notifies:
    call_start = n['logged'] - n['call']  # the logged duration of the notify call (~2 s: mac-notification-sys's delivery-confirm wait)
    t_hook = call_start - n['off']
    cand = [p for p in pres if p >= call_start - 50 and p <= call_start + 1500]
    if cand:
        rows.append((n['origin'], n['text'], cand[0] - t_hook))
for o, t, ms in rows:
    print(f'  {o} -> usernoted Presenting {ms:7.1f} ms  {t}')
hk = [ms for o, _, ms in rows if o == 'hook']
if hk:
    print('hook -> banner presented (usernoted):', stats(hk))
