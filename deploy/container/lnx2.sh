#!/usr/bin/env bash
# ADR-0017 LNX-2: the whole instance in a container built from this repository's container
# file, with a plain `docker run`, and again after the container is replaced.
#
# Usage, from anywhere, with Docker running, cargo on the PATH and the UI built:
#
#   (cd apps/agentic-org-ui && bun install --frozen-lockfile && bun run build)
#   deploy/container/lnx2.sh
#
# It builds the four executables and its own office program itself, immediately before
# the image, so that the image never holds what an earlier `cargo test` left in the build
# folder (a build with the test adapters), and it checks the packed `agentic-org` for that.
#
# Environment: LNX2_BIN (where the executables are, default target/debug), LNX2_IMAGE (the
# tag to build, default agentic-org:lnx2-<pid>; kept when given, removed otherwise),
# LNX2_PORT (the host port to publish 8443 on, default 18443).
#
# It builds the image, then checks, in order:
#   1. a plain `docker run` on two named volumes sets the instance up and serves;
#   2. the first registration link comes from `docker exec -u 0 ... admin reset --user`,
#      and only from root at a terminal;
#   3. the browser address answers from the Docker host over plain HTTP from the first
#      start, before any office exists: the version, the UI's index, the page of the
#      registration link, and a refusal for any other Host; then an office worker starts
#      and `agent-os` sends and reads inside the container (the office is stored by the
#      test's own program: no command creates one yet);
#   4. LNX-4: everything under /secrets is the service user's, files 600 and folders 700,
#      and no key is under /data; nothing was written outside the two volumes;
#   5. `docker stop` ends the service inside the runtime's grace, with exit code 0;
#   6. a new container on the same volumes is the same instance with the same keys and
#      reads a message sent before the replacement;
#   7. a new container with the data volume and a new, empty secrets volume does not serve;
#   8. a plain `docker run` with no volume named (anonymous volumes) works too;
#   9. what the credential store refuses: a path that is not a mount, a volume root that
#      is not the service user's, and a secrets directory inside the data directory.
# Every container and volume it makes is removed at the end, pass or fail.
#
# Known limits of this test: two rules have no case here because they need a client
# program inside the image: that only this instance's own link is printed when a process of
# the service user answers on the socket in the host's place, and that the instance's
# first link is refused to the service user. Both are tested in `org-host`
# (`only_this_instances_own_link_is_ever_printed`,
# `a_foreground_instance_never_gives_the_initial_link`).
set -euo pipefail

here=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
repo=$(cd "$here/../.." && pwd)
bin=${LNX2_BIN:-target/debug}
image=${LNX2_IMAGE:-agentic-org:lnx2-$$}
port=${LNX2_PORT:-18443}
tag=lnx2-$$
user=agentic
office=01OFFICE00000000000000HQ1
key_hex=$(printf 'b3%.0s' $(seq 1 32))
record=/data/.local/share/agentic-org/setup/default/setup.v1.json
scratch=$(mktemp -d "${TMPDIR:-/var/tmp}/lnx2.XXXXXX")
containers=()
volumes=()
built=
run_image=

say() { printf '\n== %s\n' "$*"; }
fail() {
    printf '\nLNX-2 FAILED: %s\n' "$*" >&2
    exit 1
}
cleanup() {
    status=$?
    for c in "${containers[@]:-}"; do [ -n "$c" ] && docker rm -f -v "$c" >/dev/null 2>&1 || true; done
    for v in "${volumes[@]:-}"; do [ -n "$v" ] && docker volume rm "$v" >/dev/null 2>&1 || true; done
    # The bind-mount cases leave folders owned by the service user's number.
    if [ -n "$built" ] && [ -d "$scratch/binds" ]; then
        docker run --rm -u 0 -v "$scratch":/scratch --entrypoint /bin/rm "$image" \
            -rf /scratch/binds >/dev/null 2>&1 || true
    fi
    docker rmi "$image-overlay" >/dev/null 2>&1 || true
    [ -n "${LNX2_IMAGE:-}" ] || docker rmi "$image" >/dev/null 2>&1 || true
    if [ -n "$scratch" ] && [ -d "$scratch" ]; then rm -rf "$scratch" 2>/dev/null || true; fi
    [ "$status" -eq 0 ] && printf '\nLNX-2 passed.\n'
    exit "$status"
}
trap cleanup EXIT

# has <extended pattern> <text>: the text was read completely first, so no writer is cut
# off by a reader that has seen enough (which `pipefail` would report as a failure).
has() { grep -Eq -- "$1" <<<"$2"; }
logs() { docker logs "$1" 2>&1; }
# The last line a container wrote to its standard error: the refusal that ended it. (Its
# standard output is another stream, which `docker logs` does not order against this one.)
last_error() { docker logs "$1" 2>&1 >/dev/null | tail -n 1; }
now() { date +%s.%N; }
since() { awk -v a="$1" -v b="$(now)" 'BEGIN { printf "%.2f", b - a }'; }

# run <name> <docker run options...>: the documented command plus the options given.
run() {
    name=$1
    shift
    containers+=("$name")
    docker run -d --name "$name" "$@" "${run_image:-$image}" >/dev/null
}
# The instance answers on its own socket.
serving() { has '^running in the foreground' "$(docker exec "$1" agentic-org service status 2>/dev/null || true)"; }
wait_until() {
    what=$1
    shift
    for _ in $(seq 1 150); do
        if "$@"; then return 0; fi
        sleep 0.2
    done
    fail "timed out waiting for $what"
}
running() { [ "$(docker inspect -f '{{.State.Running}}' "$1" 2>/dev/null)" = true ]; }
exited() { [ "$(docker inspect -f '{{.State.Status}}' "$1" 2>/dev/null)" = exited ]; }
instance_id() { docker exec "$1" cat "$record" | sed -n 's/.*"instance_id":"\([^"]*\)".*/\1/p'; }
# The worker's socket in the instance's runtime folder.
worker_socket() {
    docker exec "$1" sh -c 'find /data/.cache/agentic-org/tmp -type s ! -name host.sock 2>/dev/null' | sed -n 1p
}
# A worker process runs and its socket exists. (The runtime folder is in the data volume,
# so a socket file alone may be a stopped worker's.)
worker_runs() {
    has "agentic-org-office --office $office" \
        "$(docker exec "$1" sh -c 'for p in /proc/[0-9]*; do tr "\0" " " < "$p/cmdline" 2>/dev/null; echo; done' || true)"
}
worker_serves() { worker_runs "$1" && [ -n "$(worker_socket "$1")" ]; }
# `agent-os` as an agent of the office. A registered instance keeps its worker's socket in
# its runtime folder, where `agent-os` cannot look yet on any system (the LNX-1 test has the
# same bridge): the agent is given an office home whose `run` folder holds a second name of
# that socket. Everything from the socket on is the product's own route.
agent() {
    container=$1
    agent=$2
    shift 2
    docker exec -e ORG_AGENT="$agent" -e ORG_OFFICE="$office" \
        -e ORG_OFFICE_HOME=/data/.cache/lnx2-view -e ORG_HOME=/data/.cache/lnx2-agent \
        "$container" agent-os "$@"
}
bridge() {
    docker exec "$1" sh -c '
        set -e
        view=/data/.cache/lnx2-view
        rm -rf "$view"
        mkdir -m 700 -p "$view/run" /data/.cache/lnx2-agent
        ln -s /data/instance/office-state/offices "$view/offices"
        socket=$(find /data/.cache/agentic-org/tmp -type s ! -name host.sock | head -n 1)
        ln "$socket" "$view/run/$(basename "$socket")"'
}
# The bridge, made again until the worker answers through it: a worker that has just
# started replaces the socket file of the one before it.
answers() { bridge "$1" 2>/dev/null && agent "$1" nova msg inbox --json >/dev/null 2>&1; }
# A listing of every file in the secrets volume with its owner, mode and digest.
secrets_listing() {
    docker exec "$1" sh -c 'cd /secrets && find . -type f ! -name .lock | sort | while read -r f; do
        printf "%s %s %s\n" "$(stat -c "%u:%g %a" "$f")" "$(sha256sum < "$f" | cut -d" " -f1)" "$f"; done'
}

say "build the executables (never what a test run left in $bin)"
profile=()
case "$bin" in
target/debug) ;;
target/release) profile=(--release) ;;
*) fail "LNX2_BIN is target/debug or target/release" ;;
esac
(cd "$repo" &&
    cargo build --locked "${profile[@]}" -p agentic-org -p agentic-org-office -p agentic-org-web -p agent-os &&
    cargo build --locked "${profile[@]}" -p agentic-org --example lnx2-office) >"$scratch/cargo.log" 2>&1 ||
    fail "cargo build failed: $(tail -n 5 "$scratch/cargo.log")"
for program in agentic-org agentic-org-office agentic-org-web agent-os examples/lnx2-office; do
    [ -x "$repo/$bin/$program" ] || fail "$bin/$program is not built"
done
[ -f "$repo/apps/agentic-org-ui/build/index.html" ] || fail "the UI is not built (see the head of this script)"
command -v curl >/dev/null || fail "curl is needed on the Docker host"
docker version >/dev/null 2>&1 || fail "no Docker daemon answers"

say "build the image from deploy/container/Dockerfile"
started=$(now)
docker build -q -f "$here/Dockerfile" --build-arg BIN="$bin" -t "$image" "$repo" >/dev/null
echo "built $image in $(since "$started") s; size $(docker image inspect -f '{{.Size}}' "$image" | awk '{ printf "%.1f MB", $1 / 1000000 }')"
built=yes
# The packed `agentic-org` is not a build with the test adapters: such a build takes the
# development service adapter this variable names; any other refuses it by name.
seams=$(docker run --rm -e ORG_DEV_SERVICE_DIR=/nonexistent -e ORG_DEV_ACCOUNT_BASE=not-absolute \
    --entrypoint agentic-org "$image" service status 2>&1 || true)
has 'ORG_DEV_SERVICE_DIR names a development test service adapter, which this build does not contain' "$seams" ||
    fail "the image's agentic-org is a test-support build: $seams"
echo "the packed agentic-org has no test adapters"

say "1. a plain docker run on two named volumes"
volumes+=("$tag-data" "$tag-secrets")
docker volume create "$tag-data" >/dev/null
docker volume create "$tag-secrets" >/dev/null
started=$(now)
run "$tag-a" -p "127.0.0.1:$port:8443" -v "$tag-data:/data" -v "$tag-secrets:/secrets"
wait_until "the instance to serve" serving "$tag-a"
echo "serving $(since "$started") s after docker run"
id=$(instance_id "$tag-a")
[ -n "$id" ] || fail "the setup record names no instance"
echo "instance $id"
log=$(logs "$tag-a")
grep -E '^(Setting up|Data:|Address:|Secrets:|Disk encryption|File permissions|Service:|Browser access)' <<<"$log" || true
has '^Secrets: /secrets$' "$log" || fail "setup did not record /secrets"
if has 'no publisher check' "$log"; then
    echo "(a development image: the host logs that the worker has no publisher check)"
fi
grep -E 'browser|web process' <<<"$log" || true
[ "$(docker exec "$tag-a" id -u)" = 61000 ] || fail "the service does not run as 61000"
echo "filesystems: /data is $(docker exec "$tag-a" stat -f -c %T /data), /secrets is $(docker exec "$tag-a" stat -f -c %T /secrets), / is $(docker exec "$tag-a" stat -f -c %T /)"

say "1b. the browser address, from the Docker host, over plain HTTP, before any office exists"
# The configured origin is http://127.0.0.1:8443, which the documented run command gives a
# browser on the Docker host. This test may publish another host port, so it names the
# origin's authority itself; a browser sends it by opening http://127.0.0.1:8443.
browser() { curl -sS -m 10 -o "$scratch/body" -D "$scratch/head" -w '%{http_code}' -H "Host: ${2:-127.0.0.1:8443}" "http://127.0.0.1:$port$1" 2>/dev/null || true; }
answers_http() { [ "$(browser /api/v1/version)" = 200 ]; }
started=$(now)
wait_until "the web process to serve" answers_http
echo "GET /api/v1/version: 200 $(tr -d '\n' < "$scratch/body" | head -c 200)"
[ "$(browser /)" = 200 ] || fail "the UI's index: $(head -n 1 "$scratch/head")"
has '<html' "$(cat "$scratch/body")" || fail "the index is not the UI"
has '^content-security-policy:' "$(tr 'A-Z' 'a-z' < "$scratch/head")" || fail "the index has no policy"
echo "GET /: 200, $(wc -c < "$scratch/body") bytes of HTML with its content security policy"
if has '^(strict-transport-security|set-cookie):' "$(tr 'A-Z' 'a-z' < "$scratch/head")"; then
    fail "plain HTTP answered with HSTS or a cookie: $(cat "$scratch/head")"
fi
register=$(browser /register)
echo "GET /register (the page of the registration link): $register"
for other in "localhost:8443" "127.0.0.1:$port" "$(hostname):8443" "192.0.2.7:8443"; do
    [ "$other" = "127.0.0.1:8443" ] && continue
    code=$(browser /api/v1/version "$other")
    [ "$code" = 400 ] || fail "Host: $other was answered with $code"
done
echo "any other Host (localhost:8443, another port, this machine's name, another address): 400"
log=$(logs "$tag-a")
grep -E 'web process started' <<<"$log" | sed 's/^/  /' || fail "the host did not log the web process's start"
grep -E 'serving plain HTTP on a published port' <<<"$log" | sort -u | sed 's/^/  /'
has 'serving plain HTTP on a published port' "$log" || fail "the plain-HTTP notice is not in the log"
has 'web process started, confined by the seccomp filter' "$log" || fail "the web process is not confined by the filter"

say "2. the first registration link: root, naming the service user, at a terminal"
link=$(docker exec -t -u 0 "$tag-a" agentic-org admin reset --user "$user" | tr -d '\r')
has 'http://127.0.0.1:8443/register#' "$link" || fail "no registration link: $link"
echo "a link was printed for http://127.0.0.1:8443/register#<token>"
if has 'register#' "$(logs "$tag-a")"; then fail "the link reached the container log"; fi
if out=$(docker exec -u 0 "$tag-a" agentic-org admin reset --user "$user" 2>&1); then
    fail "a link was printed without a terminal: $out"
fi
has 'Run this in a terminal' "$out" || fail "without a terminal: $out"
if out=$(docker exec -t "$tag-a" agentic-org admin reset 2>&1); then
    fail "the service user reset the administrator: $out"
fi
has 'needs root. Run it with sudo' "$out" || fail "as the service user: $out"
if out=$(docker exec -t -u 65534 "$tag-a" agentic-org admin reset --user "$user" 2>&1); then
    fail "a third user reset the administrator: $out"
fi
echo "refused without a terminal, as the service user and as a third user"
[ -z "$(docker exec -u 0 "$tag-a" find /data /secrets ! -user "$user")" ] ||
    fail "root left a file of its own in the volumes"

say "3. an office worker, and agent-os sends and reads"
started=$(now)
docker stop "$tag-a" >/dev/null
echo "docker stop (no worker yet): $(since "$started") s, exit code $(docker inspect -f '{{.State.ExitCode}}' "$tag-a")"
docker run --rm --volumes-from "$tag-a" -v "$repo/$bin/examples/lnx2-office:/lnx2-office:ro" \
    --entrypoint /lnx2-office "$image" /data/instance/office-state "/secrets/dev-keys/$id" >/dev/null
docker start "$tag-a" >/dev/null
wait_until "the instance to serve again" serving "$tag-a"
wait_until "the office worker's socket" worker_serves "$tag-a"
wait_until "the worker to answer agent-os" answers "$tag-a"
first=$(agent "$tag-a" pixel msg send --to nova --subject first --json "hello nova")
echo "pixel sent: $first"
inbox=$(agent "$tag-a" nova msg inbox --json)
has '"subject":"first"' "$inbox" || fail "nova's inbox does not show the message: $inbox"
read_back=$(agent "$tag-a" nova msg read --next --json)
has '"body":"hello nova"' "$read_back" || fail "nova read: $read_back"
echo "nova read it back"
agent "$tag-a" pixel msg send --to nova --subject kept --json "read me after the replacement" >/dev/null

# The web process serves on after the restart, with the office there.
wait_until "the web process to serve again" answers_http

say "4. LNX-4 in the container, and nothing outside the volumes"
secrets_listing "$tag-a" | sed 's/^/  /'
[ -z "$(docker exec "$tag-a" find /secrets ! -user "$user")" ] || fail "a secret is not the service user's"
[ -z "$(docker exec "$tag-a" find /secrets -type f ! -perm 600)" ] || fail "a secret file is not 600"
[ -z "$(docker exec "$tag-a" find /secrets -type d ! -perm 700)" ] || fail "a secrets folder is not 700"
docker exec "$tag-a" grep -rlq "$key_hex" /secrets || fail "the office's key is not in /secrets"
if docker exec "$tag-a" grep -rlaqs "$key_hex" /data; then fail "the office's key is under /data"; fi
if docker exec "$tag-a" grep -rlaqsP '\xb3{32}' /data; then fail "the office's key bytes are under /data"; fi
[ -z "$(docker exec "$tag-a" find /data \( -name '*.key' -o -name 'host-secrets' -o -name 'dev-keys' \))" ] ||
    fail "a key store is under /data"
echo "everything under /secrets is $user's, files 600 and folders 700; no key under /data"
changed=$(docker diff "$tag-a")
[ -z "$changed" ] || fail "written outside the volumes: $changed"
echo "docker diff is empty: nothing was written outside /data and /secrets"
before=$(secrets_listing "$tag-a")

say "5. docker stop with a worker running"
started=$(now)
docker stop "$tag-a" >/dev/null
took=$(since "$started")
code=$(docker inspect -f '{{.State.ExitCode}}' "$tag-a")
echo "docker stop: $took s, exit code $code"
[ "$code" = 0 ] || fail "the service ended with $code (137 is the runtime's kill after its grace)"
awk -v t="$took" 'BEGIN { exit !(t < 8) }' || fail "the service took $took s to stop"

say "5b. docker stop with a worker that does not act on the stop (X5 review V2)"
docker start "$tag-a" >/dev/null
wait_until "the instance to serve again" serving "$tag-a"
wait_until "the office worker" worker_runs "$tag-a"
# The worker's own process: its command line begins with the worker's program (the shell
# that asks carries the same words later in its own).
worker=$(docker exec "$tag-a" sh -c 'for p in /proc/[0-9]*; do case "$(tr "\0" " " < "$p/cmdline" 2>/dev/null)" in /opt/agentic-org/bin/agentic-org-office\ --office*) basename "$p" ;; esac; done' || true)
worker=${worker%%$'\n'*}
[ -n "$worker" ] || fail "no worker process to stop"
docker exec "$tag-a" sh -c "kill -STOP $worker"
started=$(now)
docker stop "$tag-a" >/dev/null
took=$(since "$started")
code=$(docker inspect -f '{{.State.ExitCode}}' "$tag-a")
echo "docker stop with the worker stopped (SIGSTOP): $took s, exit code $code"
[ "$code" = 0 ] || fail "the service ended with $code: the runtime had to kill it"
awk -v t="$took" 'BEGIN { exit !(t < 8) }' || fail "the service took $took s to stop"

say "6. a new container on the same volumes"
docker rm "$tag-a" >/dev/null
run "$tag-b" -p "127.0.0.1:$port:8443" -v "$tag-data:/data" -v "$tag-secrets:/secrets"
wait_until "the replaced instance to serve" serving "$tag-b"
wait_until "the office worker's socket" worker_serves "$tag-b"
[ "$(instance_id "$tag-b")" = "$id" ] || fail "another instance: $(instance_id "$tag-b")"
[ "$(secrets_listing "$tag-b")" = "$before" ] || fail "the keys changed"
wait_until "the worker to answer agent-os" answers "$tag-b"
kept=$(agent "$tag-b" nova msg read --next --json)
has '"body":"read me after the replacement"' "$kept" || fail "the earlier message: $kept"
echo "the same instance $id, the same keys, and nova read the message sent before"
docker rm -f "$tag-b" >/dev/null

say "7. the data volume with a new, empty secrets volume"
# The office's key is gone with the old volume. Nothing may make a new one in its place:
# the office's private keys are wrapped with the lost key. A release build's store refuses
# the whole start ("credentials missing"); this development image keeps its keys in the
# development store, which starts. The host then starts the office's worker without a key.
volumes+=("$tag-empty")
docker volume create "$tag-empty" >/dev/null
run "$tag-c" -v "$tag-data:/data" -v "$tag-empty:/secrets"
settled_c() { serving "$tag-c" || exited "$tag-c"; }
wait_until "that container to serve or to end" settled_c
sleep 3
if exited "$tag-c"; then
    echo "the container ended with exit code $(docker inspect -f '{{.State.ExitCode}}' "$tag-c"):"
    last_error "$tag-c" | sed 's/^/  /'
    [ "$(docker inspect -f '{{.State.ExitCode}}' "$tag-c")" != 0 ] || fail "it ended as if nothing was wrong"
else
    log=$(logs "$tag-c")
    grep -E "$office" <<<"$log" | cut -c1-240 | sed 's/^/  /' || true
    has "office $office has no usable key in the host.s key store; it starts without one" "$log" ||
        fail "the host did not say that the office's key is gone"
    echo "  worker process: $(if worker_runs "$tag-c"; then echo running, without a key; else echo none; fi)"
    [ -z "$(docker exec "$tag-c" find /secrets -name '*.key')" ] || fail "a new key was made for the office"
    if answers "$tag-c"; then
        echo "  agent-os reaches that worker: $(agent "$tag-c" nova msg inbox --json 2>&1 | cut -c1-160)"
    else
        echo "  agent-os gets no answer from that worker"
    fi
    echo "no key was made in place of the lost one (the host itself keeps serving)"
fi
docker rm -f -v "$tag-c" >/dev/null

say "8. a plain docker run, no volume named"
started=$(now)
run "$tag-d" -p "127.0.0.1:$port:8443"
wait_until "the instance on anonymous volumes to serve" serving "$tag-d"
echo "serving $(since "$started") s after docker run -d -p 127.0.0.1:$port:8443 <image>"
docker inspect -f '{{range .Mounts}}{{.Type}} {{.Destination}}{{"\n"}}{{end}}' "$tag-d" | sort | sed 's/^/  /'
has 'register#' "$(docker exec -t -u 0 "$tag-d" agentic-org admin reset --user "$user")" ||
    fail "no registration link on anonymous volumes"
started=$(now)
docker stop "$tag-d" >/dev/null
echo "docker stop: $(since "$started") s, exit code $(docker inspect -f '{{.State.ExitCode}}' "$tag-d")"
docker rm -f -v "$tag-d" >/dev/null

say "8b. a refused first start leaves nothing, and the corrected file then works (X5 review V1)"
volumes+=("$tag-v1")
docker volume create "$tag-v1" >/dev/null
sed 's|"/secrets"|"/secret"|' "$here/setup.json" > "$scratch/typo.json"
chmod 644 "$scratch/typo.json"
run "$tag-v1a" -v "$tag-v1:/data" -v "$scratch/typo.json:/etc/agentic-org/setup.json:ro"
wait_until "the mistyped container to end" exited "$tag-v1a"
last_error "$tag-v1a" | cut -c1-200 | sed 's/^/  /'
[ "$(docker inspect -f '{{.State.ExitCode}}' "$tag-v1a")" != 0 ] || fail "a mistyped credential directory started"
left=$(docker run --rm -v "$tag-v1:/data" --entrypoint /bin/sh "$image" -c 'find /data -mindepth 1 \( -name instance -o -name "setup.v1.json" -o -name "instance.json" \)')
[ -z "$left" ] || fail "the refused start left: $left"
docker rm -f -v "$tag-v1a" >/dev/null
run "$tag-v1b" -v "$tag-v1:/data"
wait_until "the corrected container to serve" serving "$tag-v1b"
echo "  no record and no data folder were left; the image's own file then serves on the same data volume"

say "8c. the secrets volume is judged at every start (X5 review V3)"
docker rm -f -v "$tag-v1b" >/dev/null
run "$tag-v3" -v "$tag-v1:/data" --tmpfs /secrets:uid=61000,gid=61000,mode=0700
wait_until "the container on a memory filesystem to end" exited "$tag-v3"
log=$(last_error "$tag-v3")
echo "  ${log:0:260}"
[ "$(docker inspect -f '{{.State.ExitCode}}' "$tag-v3")" != 0 ] || fail "a memory filesystem as /secrets started"
has 'credential directory' "$log" || fail "the refusal does not name the credential directory"
[ -z "$(docker run --rm -v "$tag-v1:/data" --entrypoint /bin/sh "$image" -c 'find /data -name "*.key"')" ] || fail "a key was written"
docker rm -f -v "$tag-v3" >/dev/null

say "9. what the credential store refuses"
# refused <name> <words expected in the log> <docker run options...>
refused() {
    name=$1
    expected=$2
    shift 2
    run "$name" "$@"
    wait_until "$name to end" exited "$name"
    log=$(last_error "$name")
    echo "  $log"
    [ "$(docker inspect -f '{{.State.ExitCode}}' "$name")" != 0 ] || fail "$name started"
    # The rule itself, without the foreground mode's general advice after it.
    has "$expected" "${log%%; in the foreground mode*}" || return 1
    docker rm -f -v "$name" >/dev/null
}
mkdir -p "$scratch/binds" "$scratch/overlay"
echo "a credential directory in the image's own filesystem (overlay), not a mounted volume:"
sed 's|"/secrets"|"/var/lib/agentic-secrets"|' "$here/setup.json" > "$scratch/overlay/setup.json"
printf '%s\n' "FROM $image" "USER 0" \
    "RUN install -d -o 61000 -g 61000 -m 0700 /var/lib/agentic-secrets" \
    "COPY --chmod=0644 setup.json /etc/agentic-org/setup.json" \
    "USER 61000:61000" > "$scratch/overlay/Dockerfile"
docker build -q -t "$image-overlay" "$scratch/overlay" >/dev/null
run_image=$image-overlay
refused "$tag-e" 'mount|filesystem' || fail "the refusal names neither the mount nor the filesystem"
run_image=
echo "a plain directory inside a volume (the right filesystem, but not a mount):"
volumes+=("$tag-plain")
docker volume create "$tag-plain" >/dev/null
docker run --rm -v "$tag-plain:/data" --entrypoint /bin/sh "$image" -c 'mkdir -m 700 /data/plain' >/dev/null
sed 's|"/secrets"|"/data/plain"|' "$here/setup.json" > "$scratch/plain.json"
chmod 644 "$scratch/plain.json"
refused "$tag-p" 'mount' -v "$tag-plain:/data" -v "$scratch/plain.json:/etc/agentic-org/setup.json:ro" ||
    fail "the refusal does not name the mount"
echo "a secrets volume whose root is root's (a fresh bind mount of a root-owned folder):"
docker run --rm -u 0 -v "$scratch/binds:/b" --entrypoint /bin/sh "$image" -c \
    'mkdir -m 755 /b/rootowned && chown 0:0 /b/rootowned' >/dev/null
refused "$tag-f" 'own' -v "$scratch/binds/rootowned:/secrets" ||
    fail "the refusal does not name the owner"
echo "a secrets volume whose root is the service user's, but open to its group:"
docker run --rm -u 0 -v "$scratch/binds:/b" --entrypoint /bin/sh "$image" -c \
    'mkdir -m 750 /b/open && chown 61000:61000 /b/open' >/dev/null
refused "$tag-f2" 'mode' -v "$scratch/binds/open:/secrets" ||
    fail "the refusal does not name the mode"
echo "a lost+found in the volume's root is tolerated:"
volumes+=("$tag-lf")
docker volume create "$tag-lf" >/dev/null
docker run --rm -u 0 -v "$tag-lf:/secrets" --entrypoint /bin/sh "$image" -c 'mkdir -m 700 /secrets/lost+found' >/dev/null
run "$tag-g" -v "$tag-lf:/secrets"
wait_until "the instance beside lost+found to serve" serving "$tag-g"
echo "  serving"
docker rm -f -v "$tag-g" >/dev/null
echo "the secrets directory inside the directory that is the data volume:"
docker run --rm -u 0 -v "$scratch/binds:/b" --entrypoint /bin/sh "$image" -c \
    'mkdir -p /b/agentic/secrets && chown -R 61000:61000 /b/agentic && chmod 700 /b/agentic /b/agentic/secrets' >/dev/null
refused "$tag-h" 'is inside the data volume' -v "$scratch/binds/agentic:/data" -v "$scratch/binds/agentic/secrets:/secrets" ||
    fail "the refusal does not name the data volume"
