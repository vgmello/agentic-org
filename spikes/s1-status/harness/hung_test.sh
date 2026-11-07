#!/usr/bin/env bash
# Item 12 proof: one cheap -p session against hung_receiver.py; prints exit timing and the state file.
# Usage: hung_test.sh <label> <transport>
set -u
S="$(cd "$(dirname "$0")/.." && pwd)"
label="$1"; transport="$2"
mkdir -p /tmp/ac-spike-s1/hung && chmod 700 /tmp/ac-spike-s1/hung
pkill -f 'hung_receiver.py /tmp/ac-spike-s1/hung' ; python3 -c 'import time;time.sleep(0.3)'
python3 "$S/harness/hung_receiver.py" /tmp/ac-spike-s1/hung/events.sock 100 > "/tmp/ac-spike-s1/hung-$label.log" 2>&1 &
python3 -c 'import time;time.sleep(1)'
rm -rf /tmp/ac-spike-s1/state
t0=$(python3 -c 'import time;print(int(time.time()*1000))')
AC_PRESENCE_TRANSPORT="$transport" SOCK=/tmp/ac-spike-s1/hung/events.sock "$S/harness/run-p.sh" "$label" \
  "Use the Read tool to read the file .claude/agents/zz-spike-tester-sam.md, then reply with only the value of its name field." 2>&1 | tail -1
t1=$(python3 -c 'import time;print(int(time.time()*1000))')
echo "transport=$transport start=$t0 process_exit=$t1 wall_ms=$((t1-t0))"
cat "/tmp/ac-spike-s1/hung-$label.log"
python3 - "$t1" <<'PY'
import json, re, sys, glob
t1 = int(sys.argv[1])
log = open(glob.glob('/tmp/ac-spike-s1/hung-*.log')[0]).read()  # unused; accepts printed above
for f in glob.glob('/tmp/ac-spike-s1/state/*.json'):
    d = json.load(open(f))
    print(f'state file {f.split("/")[-1]}: hook={d["hook"]} reason={d.get("reason")} seq={d["seq"]} posts_skipped={d["posts_skipped"]} written_at={d["written_at"]} (process exit {t1 - d["t_hook"]} ms after SessionEnd hook)')
PY
pkill -f 'hung_receiver.py /tmp/ac-spike-s1/hung'
