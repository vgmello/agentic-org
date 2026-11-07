#!/usr/bin/env bash
# Run one cheap headless test session in the throwaway workspace with the spike mods.
# Usage: run-p.sh <label> <prompt> [extra claude args...]
# Env: ORDER=presence-first|goals-first (default presence-first), NO_GOALS=1, AC_AGENT override.
set -u
S="$(cd "$(dirname "$0")/.." && pwd)"
WS=/tmp/ac-spike-s1/ws
# Scrub the parent Claude session's env (child-session marker, job dir, agent name, messaging
# socket): otherwise the test session hides from `claude agents`, skips its transcript and
# shows the parent's agent name.
for v in $(env | grep -E -o '^(CLAUDE[A-Z_]*|AI_AGENT)='| tr -d =); do unset "$v"; done
label="$1"; prompt="$2"; shift 2
uuid="$(uuidgen | tr 'A-Z' 'a-z')"
echo "$(date +%s) $uuid $label" >> /tmp/ac-spike-s1/sessions.txt
if [ "${NO_GOALS:-}" = 1 ]; then plugins=(--plugin-dir "$S/presence-mod")
elif [ "${ORDER:-presence-first}" = goals-first ]; then plugins=(--plugin-dir "$S/goals-mod-copy" --plugin-dir "$S/presence-mod")
else plugins=(--plugin-dir "$S/presence-mod" --plugin-dir "$S/goals-mod-copy"); fi
echo "session $uuid ($label)" >&2
cd "$WS" && AC_AGENT="${AC_AGENT-zz-spike-tester-sam}" AC_PRESENCE_SOCKET="${SOCK:-/tmp/ac-spike-s1/run/events.sock}" \
  claude -p "$prompt" --session-id "$uuid" --agent zz-spike-tester-sam --name zz-spike-tester-sam \
  --model "${MODEL:-haiku}" "${plugins[@]}" "$@" < /dev/null
echo "exit=$? session=$uuid" >&2
