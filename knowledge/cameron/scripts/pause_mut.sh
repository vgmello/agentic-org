#!/bin/bash
# usage: pause_mut.sh STOP|CONT  — signal cargo-mutants and all its descendants
SIG=$1
root=$(pgrep -x cargo-mutants | head -1); [ -z "$root" ] && { echo "no cargo-mutants running"; exit 0; }
pids="$root"; frontier="$root"
while [ -n "$frontier" ]; do
  next=""
  for p in $frontier; do next="$next $(pgrep -P $p | tr '\n' ' ')"; done
  frontier=$(echo $next); pids="$pids $frontier"
done
kill -$SIG $pids 2>/dev/null; echo "$SIG sent to: $pids" | cut -c1-200
