#!/usr/bin/env bash
# Self-test for install.sh's report_services: what `agentic-org release install --json` did
# to each registered service is shown with its note (escaped quotes and backslashes undone,
# control characters never printed raw, braces and keys inside a note not read as JSON),
# compact or pretty-printed, and with the services_warning. Prints "ok" and exits 0 when
# every check passes.
# The samples hold literal backticks, which must not expand (whole file):
# shellcheck disable=SC2016

set -euo pipefail

here=$(cd "$(dirname "$0")" && pwd -P)
definition=$(sed -n '/^report_services() {$/,/^}$/p' "$here/install.sh")
if [[ -z "$definition" ]]; then
  echo "FAIL: report_services not found in install.sh" >&2
  exit 1
fi
eval "$definition"

fail() {
  printf 'FAIL: %s\n--- got ---\n%s\n--- wanted ---\n%s\n' "$1" "$2" "$3" >&2
  exit 1
}

check() {
  local what=$1 json=$2 wanted=$3 got
  got=$(report_services "$json" 2>&1)
  [[ "$got" == "$wanted" ]] || fail "$what" "$got" "$wanted"
}

# As serde_json writes it in a pipe, with the CLI's control escapes (`\u001b`, `\u2028`).
compact='{"state":"installed","store":"/s","release_id":"r2","sequence":2,"services":[{"instance":"default","outcome":"refused","was_running":true,"running":true,"program":"/s/releases/r1/agentic-org","note":"Instance \"default\": its service definition was left untouched because it is not what setup wrote (/u/a.plist is not a definition this adapter wrote (it was changed by hand); it is left untouched); stop it (`agentic-org service stop --yes`), then run `agentic-org setup`"},{"instance":"lab","outcome":"failed","was_running":true,"running":false,"program":null,"note":"Instance \"lab\": its service now names C:\\x\\y but could not be started again (line\nbreak\ttab \u001b[31mred \u2028sep {\"instance\":\"evil\"} ]): run `agentic-org setup --name lab`."},{"instance":"box","outcome":"refreshed","was_running":false,"running":false,"program":"/s/releases/r2/agentic-org","note":"Instance \"box\": its service now names /s/releases/r2/agentic-org; it stays stopped: it starts at the next login, or now with `agentic-org setup --name box`."}],"services_warning":"The \"x\" services were not checked\u0007"}'

wanted='Service of instance "default": refused (running).
  Instance "default": its service definition was left untouched because it is not what setup wrote (/u/a.plist is not a definition this adapter wrote (it was changed by hand); it is left untouched); stop it (`agentic-org service stop --yes`), then run `agentic-org setup`
Service of instance "lab": failed (not running).
  Instance "lab": its service now names C:\x\y but could not be started again (line break tab ?[31mred ?sep {"instance":"evil"} ]): run `agentic-org setup --name lab`.
Service of instance "box": refreshed (not running).
  Instance "box": its service now names /s/releases/r2/agentic-org; it stays stopped: it starts at the next login, or now with `agentic-org setup --name box`.
Warning: The "x" services were not checked?'

check "compact output" "$compact" "$wanted"

# Pretty-printed (a terminal): the same report.
pretty='{
  "state": "installed",
  "services": [
    {
      "instance": "default",
      "outcome": "unchanged",
      "was_running": true,
      "running": true,
      "program": "/s/releases/r2/agentic-org",
      "note": "Instance \"default\": its service already runs /s/releases/r2/agentic-org."
    }
  ]
}'
check "pretty output" "$pretty" 'Service of instance "default": unchanged (running).
  Instance "default": its service already runs /s/releases/r2/agentic-org.'

# No registered services and no warning: nothing is said.
check "no services" '{"state":"installed","services":[]}' ''

# A note holding raw control bytes (never written by the CLI) is still not printed raw.
esc=$'\033'
bel=$'\a'
raw="{\"services\":[{\"instance\":\"default\",\"outcome\":\"failed\",\"running\":false,\"note\":\"a${esc}[2Jb${bel}c\"}]}"
check "raw controls" "$raw" 'Service of instance "default": failed (not running).
  a [2Jb c'

# A key named like a service field outside the list is not a service.
check "top-level keys" '{"note":"not a service","instance":"x","services":[]}' ''

echo ok
