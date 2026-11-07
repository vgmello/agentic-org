#!/usr/bin/env bash
# Run one interactive test session in a PTY, driven by a JSON step script.
# Usage: run-i.sh <label> <steps.json> [extra claude args...]
set -u
S="$(cd "$(dirname "$0")/.." && pwd)"
WS=/tmp/ac-spike-s1/ws
# Scrub the parent Claude session's env (child-session marker, job dir, agent name, messaging
# socket): otherwise the test session hides from `claude agents`, skips its transcript and
# shows the parent's agent name.
for v in $(env | grep -E -o '^(CLAUDE[A-Z_]*|AI_AGENT)='| tr -d =); do unset "$v"; done
label="$1"; steps="$2"; shift 2
uuid="${UUID:-$(uuidgen | tr 'A-Z' 'a-z')}"
echo "$(date +%s) $uuid $label (interactive)" >> /tmp/ac-spike-s1/sessions.txt
echo "session $uuid ($label)" >&2
cd "$WS" && AC_AGENT=zz-spike-tester-sam AC_PRESENCE_SOCKET=/tmp/ac-spike-s1/run/events.sock \
  python3 "$S/harness/pty_drive.py" "$steps" "/tmp/ac-spike-s1/pty-$label.log" -- \
  claude --session-id "$uuid" --agent zz-spike-tester-sam --name zz-spike-tester-sam --model haiku \
  --plugin-dir "$S/presence-mod" --plugin-dir "$S/goals-mod-copy" "$@"
