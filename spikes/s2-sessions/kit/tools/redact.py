# Redact personal data in the S2 evidence folder (in place).
import os, re, sys
sys.path.insert(0, '/tmp/ac-spike-s2/tools'); import ptydrive
E = sys.argv[1]
HOME = os.path.expanduser('~').encode()
# Agent-view rows are redacted by shape, not by keyword: inside the agent view
# (from its banner line "New sessions: …" on), a line survives only if it is a
# known piece of the UI or belongs to a spike session (name starting "zz-spik").
UI_LINE = re.compile(r'^\s*$|^\s*(Needs input|Working|Completed)\s*$|─{5,}|^❯|enter to return|^\s*[✻∙✶✳✢·✽]\s*$|^\s*\d+\s*$|[▐▛▜▝]')
def redact_agent_view(lines):
    out, inside = [], False
    for l in lines:
        if 'New sessions:' in l: inside = True
        if inside and not UI_LINE.search(l) and 'zz-spik' not in l and 'New sessions:' not in l and 'awaiting input' not in l:
            out.append('<redacted: an agent-view session row (name and task summary)>')
        else:
            out.append(l)
    return out
# 1. agent-view captures: keep a redacted text + the OSC titles, drop the raw bytes
titles = []
for f in ('q4-key-left.raw', 'q4-key-ctrlc2.raw'):
    p = os.path.join(E, f)
    if not os.path.exists(p): continue
    b = open(p, 'rb').read()
    titles.append(f + ': ' + repr([m.group(2).decode('utf-8', 'replace') for m in re.finditer(rb'\x1b\](0|2);([^\x07\x1b]*)(\x07|\x1b\\)', b)]))
    s = ptydrive.strip(b).decode('utf-8', 'replace')
    lines = redact_agent_view(re.split(r'[\r\n]+', s))
    open(p + '.txt', 'w').write('# Redacted: rows of real agents\' sessions replaced; raw bytes removed. Titles (OSC 0/2) are in q4-agent-view-titles.txt.\n' + '\n'.join(lines))
    os.remove(p)
if titles:
    open(os.path.join(E, 'q4-agent-view-titles.txt'), 'w').write('OSC 0/2 window titles in the agent-view captures (raw bytes removed for privacy):\n' + '\n'.join(titles) + '\n')
# 2. everything: home path, plan name, plan usage, inherited real agent name in the trust capture
ESC = rb'(?:\x1b\[[0-9;?]*[A-Za-z]|[ \xa0]|\xc2\xa0)*'
subs = [
    (re.escape(HOME), b'~'),
    (rb'Claude' + ESC + rb'(Max|Pro|Team|Enterprise)', b'Claude <plan>'),
    # Plan usage, also when truncated ("7d 14…") or cut off after the digits.
    # Needs a separator after 5h/7d and no letter/digit before it, so ids
    # like 7d15a09c and modes like ?25h are left alone.
    (rb'(?<![0-9A-Za-z?])(5h|7d)((?:\x1b\[[0-9;?]*[A-Za-z]|[ \xa0]|\xc2\xa0)+)\d{1,3}(?![0-9A-Za-z])', rb'\1\2NN'),
    (rb'\xe2\x86\xbb(' + ESC + rb')[0-9dhm]+', b'\xe2\x86\xbb\\1XX'),
    (rb'ac-software-engineer-[a-z]+', b'<inherited-agent-name>'),
    # Session/weekly limit banner, its reset time and timezone, and per-session cost.
    # ESC also matches PTY cursor moves between words; the timezone may itself be
    # split by escapes, so everything up to ')' or end of line goes.
    (rb'used' + ESC + rb'\d{1,3}%' + ESC + rb'of' + ESC + rb'your' + ESC + rb'(session|weekly)' + ESC + rb'limit', b'used NN% of your limit'),
    (rb'resets' + ESC + rb'\d{1,2}(:\d{2})?' + ESC + rb'[ap]m(' + ESC + rb'\([^)\r\n]*\)?)?', b'resets <time> (<tz>)'),
    (rb'\$\d+(\.\d+)?', b'$<cost>'),
]
for root, _, files in os.walk(E):
    for f in files:
        p = os.path.join(root, f); b = open(p, 'rb').read(); o = b
        for pat, rep in subs: b = re.sub(pat, rep, b)
        if b != o: open(p, 'wb').write(b)
