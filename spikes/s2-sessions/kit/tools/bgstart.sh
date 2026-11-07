#!/bin/sh
# usage: bgstart.sh <evidence-file> <label> <claude args...> ; stdin is /dev/null; records timing and output
F=$1; L=$2; shift 2
cd /tmp/ac-spike-s2/ws
T0=$(python3 -c 'import time;print(time.time())')
OUT=$(AC_AGENT=zz-from-caller-env PATH="/tmp/ac-spike-s2/pathA-bg-start:$PATH" /tmp/ac-spike-s2/tools/cc --bg "$@" </dev/null 2>&1); RC=$?
T1=$(python3 -c 'import time;print(time.time())')
{ echo "### $L"; echo "argv: claude --bg $*"; echo "exit=$RC elapsed_s=$(python3 -c "print(round($T1-$T0,3))")"; echo "$OUT"; } | tee -a "/tmp/ac-spike-s2/$F"
