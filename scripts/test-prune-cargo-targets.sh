#!/usr/bin/env bash
# Self-test for prune-cargo-targets.sh on throwaway fixtures in a temporary folder: stale,
# fresh, locked and busy Cargo target directories, look-alikes that are not Cargo's, a
# symlink to a target outside the root, the --max-depth boundary, changes and locks deep in a
# target, future dates, and symlinks inside a target. Prints "ok" and exits 0 when every check
# passes.

set -euo pipefail

here=$(cd "$(dirname "$0")" && pwd -P)
prune="$here/prune-cargo-targets.sh"
root=$(mktemp -d "${TMPDIR:-/tmp}/prune-cargo-targets-test.XXXXXX")
root=$(cd "$root" && pwd -P)
holders=()

cleanup() {
  local pid
  for pid in ${holders[@]+"${holders[@]}"}; do
    kill "$pid" 2>/dev/null || true
  done
  rm -rf "$root"
}
trap cleanup EXIT

fail() {
  printf 'FAIL: %s\n' "$1" >&2
  if [ -n "${out:-}" ]; then
    printf -- '--- last output ---\n%s\n' "$out" >&2
  fi
  exit 1
}

# A workspace with a Cargo target directory holding some build output, `old` or not.
workspace() {
  local ws="$root/$1"
  mkdir -p "$ws/target/debug/deps" "$ws/target/debug/incremental/lib-1"
  printf '[package]\nname = "fixture"\n' >"$ws/Cargo.toml"
  printf 'Signature: 8a477f597d28d172789f06886806bc55\n# This file is a cache directory tag created by cargo.\n' \
    >"$ws/target/CACHEDIR.TAG"
  : >"$ws/target/debug/.cargo-lock"
  head -c 65536 /dev/zero >"$ws/target/debug/deps/libfixture.rlib"
  head -c 4096 /dev/zero >"$ws/target/debug/incremental/lib-1/query-cache.bin"
  if [ "$2" = old ]; then
    find "$ws" -exec touch -h -t 202001010000 {} +
  fi
}

# Starts a process that holds `$1` until killed: a Cargo lock when `$2` is `lock`, or a
# process that names `$1` on its command line when `$2` is `args`. Returns once it holds it.
hold() {
  local ready="$root/ready.$RANDOM$RANDOM"
  mkfifo "$ready"
  if [ "$2" = lock ]; then
    # The lock path is passed in the environment, so only the lock can show the target in use.
    HOLD_PATH="$1" python3 -c '
import fcntl, os, sys, time
fd = os.open(os.environ["HOLD_PATH"], os.O_RDWR | os.O_CREAT)
fcntl.flock(fd, fcntl.LOCK_EX)
open(sys.argv[1], "w").write("ready\n")
time.sleep(600)
' "$ready" &
  else
    python3 -c '
import sys, time
open(sys.argv[1], "w").write("ready\n")
time.sleep(600)
' "$ready" "$1" &
  fi
  holders+=("$!")
  read -r _ <"$ready"
  rm -f "$ready"
}

run() {
  local status=0
  out=$("$prune" "$@" 2>&1) || status=$?
  return "$status"
}

exists() { [ -e "$root/$1" ] || fail "$1 was removed: it must be kept"; }
gone() { [ ! -e "$root/$1" ] || fail "$1 is still there: it must be removed"; }
says() {
  case "$out" in *"$1"*) ;; *) fail "the output does not say \`$1\`" ;; esac
}

workspace scan/ws-stale old
workspace scan/ws-fresh new
workspace scan/ws-locked old
workspace scan/ws-busy old
workspace outside/ws-elsewhere old
# A `target` that is not Cargo's (no tag), and one with someone else's cache tag.
mkdir -p "$root/scan/ws-plain/target/debug"
touch -t 202001010000 "$root/scan/ws-plain/target/debug" "$root/scan/ws-plain/target"
mkdir -p "$root/scan/ws-foreign/target"
printf 'Signature: 8a477f597d28d172789f06886806bc55\n# Created by another tool.\n' \
  >"$root/scan/ws-foreign/target/CACHEDIR.TAG"
find "$root/scan/ws-foreign" -exec touch -t 202001010000 {} +
# A link from inside the root to a stale target outside it is never followed.
mkdir -p "$root/scan/ws-link"
ln -s "$root/outside/ws-elsewhere/target" "$root/scan/ws-link/target"
# A cache tag nested in a target directory is part of it, not a second one.
mkdir -p "$root/scan/ws-stale/target/nested"
cp "$root/scan/ws-stale/target/CACHEDIR.TAG" "$root/scan/ws-stale/target/nested/"
find "$root/scan/ws-stale" -exec touch -h -t 202001010000 {} +

hold "$root/scan/ws-locked/target/debug/.cargo-lock" lock
hold "$root/scan/ws-busy/target/debug/deps/fixture-test" args

# Usage errors change nothing and exit 2.
for bad in "--max-age-days x" "--max-age-days" "--max-depth -1" "--root $root/missing" "--root /" "--bogus"; do
  status=0
  # shellcheck disable=SC2086 # each case is several words on purpose
  run $bad || status=$?
  [ "$status" -eq 2 ] || fail "\`$bad\` exited $status, not 2"
done

# A dry run removes nothing and says what it would reclaim.
run --dry-run --root "$root/scan" || fail "the dry run failed"
says "would remove"
says "$root/scan/ws-stale/target"
says "Would reclaim"
says "from 1 of 4 Cargo target directories"
exists scan/ws-stale/target

# The real run removes only the stale, idle Cargo target.
run --root "$root/scan" || fail "the run failed"
says "removed"
says "Reclaimed"
says "kept (used within 7 days)  $root/scan/ws-fresh/target"
says "kept (in use: a Cargo build lock is held)  $root/scan/ws-locked/target"
says "kept (in use: a running process uses it)  $root/scan/ws-busy/target"
gone scan/ws-stale/target
exists scan/ws-stale/Cargo.toml
exists scan/ws-fresh/target
exists scan/ws-locked/target/debug/deps/libfixture.rlib
exists scan/ws-busy/target/debug/deps/libfixture.rlib
exists scan/ws-plain/target
exists scan/ws-foreign/target
exists scan/ws-link/target
exists outside/ws-elsewhere/target/debug/deps/libfixture.rlib

# Idempotent: a second run finds nothing more to remove.
run --root "$root/scan" || fail "the second run failed"
says "Reclaimed 0 KiB from 0 of 3 Cargo target directories (3 kept)"

# Age 0 removes every idle target, still never one in use.
run --max-age-days 0 --root "$root/scan" || fail "the age 0 run failed"
gone scan/ws-fresh/target
exists scan/ws-locked/target
exists scan/ws-busy/target

# Once the lock and the process are gone, their targets go too.
for pid in "${holders[@]}"; do
  kill "$pid"
  wait "$pid" 2>/dev/null || true
done
holders=()
run --max-age-days 0 --root "$root/scan" || fail "the last run failed"
gone scan/ws-locked/target
gone scan/ws-busy/target
exists outside/ws-elsewhere/target
exists scan/ws-plain/target

# --max-depth N reaches a target directory N levels below the root (its tag is N + 1 down),
# and not one a level deeper.
workspace depth old    # depth/target: 1 level below the root depth/
workspace depth/ws old # depth/ws/target: 2 levels below
run --max-depth 1 --root "$root/depth" || fail "the depth 1 run failed"
says "from 1 of 1 Cargo target directories (0 kept)"
gone depth/target
exists depth/Cargo.toml
exists depth/ws/target
run --max-depth 2 --root "$root/depth" || fail "the depth 2 run failed"
gone depth/ws/target

# The default depth (12) reaches a target 12 levels below the root, and not one at 13.
deep=deep/1/2/3/4/5/6/7/8/9/10
workspace "$deep/ws12" old      # 10 levels, ws12 at 11, its target at 12
workspace "$deep/more/ws13" old # more at 11, ws13 at 12, its target at 13
run --root "$root/deep" || fail "the default depth run failed"
says "from 1 of 1 Cargo target directories"
gone "$deep/ws12/target"
exists "$deep/more/ws13/target"

# A change anywhere in a target is a use: a fingerprint under a target triple sits deeper than
# the top three levels.
workspace deep-use/ws old
fingerprint="$root/deep-use/ws/target/x86_64-unknown-linux-gnu/debug/.fingerprint/fixture-1"
mkdir -p "$fingerprint"
: >"$fingerprint/lib-fixture"
find "$root/deep-use" -exec touch -h -t 202001010000 {} +
touch "$fingerprint/lib-fixture"
run --root "$root/deep-use" || fail "the deep use run failed"
says "kept (used within 7 days)  $root/deep-use/ws/target"
exists deep-use/ws/target/debug/deps/libfixture.rlib

# A file dated in the future keeps a target at any age but 0, which does not look at age.
workspace future/ws old
touch -t 209901010000 "$root/future/ws/target/debug/deps/libfixture.rlib"
run --max-age-days 7 --root "$root/future" || fail "the future date run failed"
says "kept (used within 7 days)  $root/future/ws/target"
exists future/ws/target
run --max-age-days 0 --root "$root/future" || fail "the future date age 0 run failed"
gone future/ws/target

# A symlink inside a target is never followed: a fresh file behind it does not keep the
# target, and the removal deletes the link, never what it points to.
workspace links/ws old
mkdir -p "$root/links/elsewhere"
: >"$root/links/elsewhere/fresh"
ln -s "$root/links/elsewhere" "$root/links/ws/target/debug/build-link"
touch -h -t 202001010000 "$root/links/ws/target/debug/build-link" "$root/links/ws/target/debug"
run --root "$root/links" || fail "the inner symlink run failed"
gone links/ws/target
exists links/elsewhere/fresh

# A Cargo lock held anywhere in a target keeps it, whatever its name and however deep (a nested
# package build puts one five levels in); nothing is renamed or removed meanwhile.
workspace locks/ws old
workspace locks/ws-artifact old
nested="$root/locks/ws/target/package/fixture-0.1.0/target/debug"
mkdir -p "$nested"
: >"$nested/.cargo-lock"
: >"$root/locks/ws-artifact/target/debug/.cargo-artifact-lock"
find "$root/locks" -exec touch -h -t 202001010000 {} +
hold "$nested/.cargo-lock" lock
hold "$root/locks/ws-artifact/target/debug/.cargo-artifact-lock" lock
for mode in --dry-run --max-age-days=0; do
  run "$mode" --root "$root/locks" || fail "the locked run ($mode) failed"
  says "kept (in use: a Cargo build lock is held)  $root/locks/ws/target"
  says "kept (in use: a Cargo build lock is held)  $root/locks/ws-artifact/target"
  says "from 0 of 2 Cargo target directories (2 kept)"
done
exists locks/ws/target/debug/deps/libfixture.rlib
exists locks/ws-artifact/target/debug/deps/libfixture.rlib
[ -z "$(find "$root/locks" -name '.cargo-prune-*' -print)" ] || fail "a locked target was renamed"

# Once the locks are released, the targets are renamed aside and removed, leaving no staging
# directory and the rest of the workspace as it was.
for pid in "${holders[@]}"; do
  kill "$pid"
  wait "$pid" 2>/dev/null || true
done
holders=()
run --max-age-days 0 --root "$root/locks" || fail "the unlocked run failed"
says "from 2 of 2 Cargo target directories (0 kept)"
gone locks/ws/target
gone locks/ws-artifact/target
exists locks/ws/Cargo.toml
exists locks/ws-artifact/Cargo.toml
leftover=$(find "$root/locks" -name '.cargo-prune-*' -print)
[ -z "$leftover" ] || fail "a staging directory was left behind: $leftover"

echo ok
