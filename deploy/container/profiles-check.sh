#!/usr/bin/env bash
# The optional seccomp and AppArmor profiles of this folder, checked on a Docker host that
# enforces AppArmor (README.md, "Optional: profiles for agents' sandboxes"; LNX-2 step 10).
#
# Usage, with the image built (deploy/container/lnx2.sh runs this with LNX2_PROFILES=1):
#
#   sudo apparmor_parser -r -W deploy/container/apparmor-agentic-org
#   deploy/container/profiles-check.sh <image>
#
# It checks that, under the two profiles together:
#   1. the service sets an instance up, serves and stops within 2.5 s with exit code 0;
#   2. a vendor-style sandbox runs in the container as the service user:
#      `bwrap --unshare-all --ro-bind / / --dev /dev --tmpfs /tmp true`, and the same with
#      `--proc /proc` once the container also has `--security-opt systempaths=unconfined`;
#      without it a fresh /proc is refused (Docker's masked /proc paths), as the README says;
#   3. with Docker's default profiles the same sandbox cannot start (the profiles are what
#      lets it).
# bubblewrap is not in the image: a derived image adds it for this check. Every container
# and image it makes is removed at the end, pass or fail.
set -euo pipefail

here=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
image=${1:?usage: profiles-check.sh <image>}
tag=profiles-$$
sandbox=$image-bwrap-$$
scratch=$(mktemp -d "${TMPDIR:-/var/tmp}/profiles.XXXXXX")
containers=()

fail() {
    printf '\nPROFILES FAILED: %s\n' "$*" >&2
    exit 1
}
cleanup() {
    status=$?
    for c in "${containers[@]:-}"; do [ -n "$c" ] && docker rm -f -v "$c" >/dev/null 2>&1 || true; done
    docker rmi "$sandbox" >/dev/null 2>&1 || true
    rm -rf "$scratch"
    [ "$status" -eq 0 ] && printf '\nThe optional profiles passed.\n'
    exit "$status"
}
trap cleanup EXIT
now() { date +%s.%N; }
since() { awk -v a="$1" -v b="$(now)" 'BEGIN { printf "%.2f", b - a }'; }
has() { grep -Eq -- "$1" <<<"$2"; }

profiles=(--security-opt "seccomp=$here/seccomp.json" --security-opt apparmor=agentic-org)
has apparmor "$(docker info -f '{{.SecurityOptions}}')" ||
    fail "this Docker host does not enforce AppArmor"

echo "== 1. the service under both profiles"
containers+=("$tag-a")
docker run -d --name "$tag-a" "${profiles[@]}" "$image" >/dev/null
serving() { has '^running in the foreground' "$(docker exec "$tag-a" agentic-org service status 2>/dev/null || true)"; }
for _ in $(seq 1 150); do serving && break; sleep 0.2; done
serving || fail "the service did not serve under the profiles: $(docker logs "$tag-a" 2>&1 | tail -n 3)"
[ "$(docker inspect -f '{{.AppArmorProfile}}' "$tag-a")" = agentic-org ] ||
    fail "the container is not under the agentic-org profile (is it loaded?)"
confined() { has 'web process started, confined by the seccomp filter' "$(docker logs "$tag-a" 2>&1)"; }
for _ in $(seq 1 150); do confined && break; sleep 0.2; done
confined || fail "the web process is not confined: $(docker logs "$tag-a" 2>&1 | grep -i web | tail -n 2)"
started=$(now)
docker stop "$tag-a" >/dev/null
took=$(since "$started")
code=$(docker inspect -f '{{.State.ExitCode}}' "$tag-a")
echo "serving, the web process confined; docker stop: $took s, exit code $code"
[ "$code" = 0 ] || fail "the service ended with $code"
awk -v t="$took" 'BEGIN { exit !(t < 2.5) }' || fail "the service took $took s to stop"

echo "== 2. a vendor-style sandbox, as the service user"
printf '%s\n' "FROM $image" "USER 0" \
    "RUN apt-get update && apt-get install -y --no-install-recommends bubblewrap && rm -rf /var/lib/apt/lists/*" \
    "USER 61000:61000" >"$scratch/Dockerfile"
docker build -q -t "$sandbox" "$scratch" >/dev/null
# sandbox <docker options...> -- <bwrap options...>: bwrap's output, and its exit code last.
sandbox() {
    options=()
    while [ "$1" != -- ]; do options+=("$1"); shift; done
    shift
    docker run --rm "${options[@]}" --entrypoint /bin/sh "$sandbox" -c \
        'id -u; bwrap --unshare-all --ro-bind / / "$@" --dev /dev --tmpfs /tmp true 2>&1; echo "exit $?"' \
        sh "$@" 2>&1
}
out=$(sandbox "${profiles[@]}" --)
echo "  without a fresh /proc: $(tr '\n' ' ' <<<"$out")"
has '^61000$' "$out" && has '^exit 0$' "$out" || fail "bwrap did not run under the profiles: $out"
out=$(sandbox "${profiles[@]}" --security-opt systempaths=unconfined -- --proc /proc)
echo "  with --proc /proc and systempaths=unconfined: $(tr '\n' ' ' <<<"$out")"
has '^exit 0$' "$out" || fail "bwrap --proc /proc did not run with systempaths=unconfined: $out"
out=$(sandbox "${profiles[@]}" -- --proc /proc)
echo "  with --proc /proc, Docker's masked /proc paths kept: $(tr '\n' ' ' <<<"$out")"
has "Can't mount proc" "$out" || fail "a fresh /proc under masked paths: $out"

echo "== 3. the same sandbox with Docker's default profiles"
out=$(sandbox --)
echo "  $(tr '\n' ' ' <<<"$out")"
if has '^exit 0$' "$out"; then fail "bwrap ran without the profiles: the check proves nothing"; fi
