#!/bin/sh
# S2 spike marker: one line per hook event, with only the fields and env values the findings need.
IN=$(cat)
python3 -I -c '
import json,sys,os,time
ev=sys.argv[1]
try: d=json.loads(sys.argv[2])
except Exception: d={}
keep={k:d.get(k) for k in ("session_id","hook_event_name","tool_name","notification_type","reason","source","agent_type")}
keep.update(ev=ev,t=round(time.time(),3),AC_AGENT=os.environ.get("AC_AGENT"),CLAUDE_CODE_AGENT=os.environ.get("CLAUDE_CODE_AGENT"),pwd=os.getcwd().replace(os.path.expanduser("~"),"~"),spike_PATH_dirs=[d for d in os.environ.get("PATH","").split(":") if d.startswith("/tmp/ac-spike-s2/")])
# caller_PATH_reached_hook: did a directory the starting/waking client prepended (kit/tools/bgstart.sh adds /tmp/ac-spike-s2/pathA-bg-start) reach the hook?
keep["caller_PATH_reached_hook"]=bool(keep["spike_PATH_dirs"])
open("/tmp/ac-spike-s2/marker2.log","a").write(json.dumps(keep)+"\n")
' "$1" "$IN"
exit 0
