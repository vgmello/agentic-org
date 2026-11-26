#!/usr/bin/env bash
# Removes stale Cargo target directories under one or more roots, and never one in use.
#
# A Cargo target directory is a real directory holding the CACHEDIR.TAG that Cargo writes. It is
# stale when nothing in its top three levels changed for --max-age-days days. It is kept, however
# old, while:
#   - any of its Cargo locks (.cargo-lock, .cargo-build-lock, .cargo-artifact-lock) is held, that
#     is while a build, check, test or run is in progress;
#   - a cargo, rustc, rustdoc, clippy or rust-analyzer process works in the workspace that owns it;
#   - any process names a path inside it on its command line (a test binary, a rustc --out-dir).
# The removal holds every Cargo lock of the directory until it is done, so a build that starts
# meanwhile waits for it and then builds from scratch instead of failing halfway.
#
# Only directories strictly inside a --root are considered; symlinks are never followed, and
# .git and node_modules folders are not searched. Running it twice removes nothing the second
# time. Exit status: 0 when every stale directory was removed (or would be, with --dry-run),
# 1 when a removal failed, 2 for a usage error.

set -euo pipefail

readonly CACHEDIR_SIGNATURE="Signature: 8a477f597d28d172789f06886806bc55"
readonly CARGO_TAG_COMMENT="created by cargo"
readonly RUST_TOOLS="cargo rustc rustdoc clippy-driver cargo-clippy rust-analyzer"
PROGRAM=$(basename "$0")
readonly PROGRAM

usage() {
  cat <<EOF
Usage: $PROGRAM [--root DIR]... [--max-age-days N] [--max-depth N] [--dry-run]

Removes Cargo target directories under each DIR (default: the current directory) that have
not been used for N days (default: 7; 0 removes every idle one), skipping any in use.

  --root DIR          Search below DIR. Give it more than once for several roots.
  --max-age-days N    Keep a target directory used within the last N days.
  --max-depth N       Look for target directories at most N levels below a root (default: 5).
  --dry-run           Show what would be removed and how much space it holds; remove nothing.
  -h, --help          Show this help.
EOF
}

usage_error() {
  printf "%s: %s\nRun '%s --help' for usage.\n" "$PROGRAM" "$1" "$PROGRAM" >&2
  exit 2
}

whole_number() {
  case "$2" in
    '' | *[!0-9]*) usage_error "$1 takes a whole number of at most 5 digits, not \`$2\`." ;;
  esac
  if [ "${#2}" -gt 5 ]; then
    usage_error "$1 takes a whole number of at most 5 digits, not \`$2\`."
  fi
}

roots=()
max_age_days=7
max_depth=5
dry_run=0
while [ $# -gt 0 ]; do
  case "$1" in
    --root)
      [ $# -ge 2 ] || usage_error "--root needs a directory."
      roots+=("$2")
      shift 2
      ;;
    --root=*)
      roots+=("${1#--root=}")
      shift
      ;;
    --max-age-days)
      [ $# -ge 2 ] || usage_error "--max-age-days needs a number."
      whole_number --max-age-days "$2"
      max_age_days=$2
      shift 2
      ;;
    --max-age-days=*)
      whole_number --max-age-days "${1#--max-age-days=}"
      max_age_days=${1#--max-age-days=}
      shift
      ;;
    --max-depth)
      [ $# -ge 2 ] || usage_error "--max-depth needs a number."
      whole_number --max-depth "$2"
      max_depth=$2
      shift 2
      ;;
    --max-depth=*)
      whole_number --max-depth "${1#--max-depth=}"
      max_depth=${1#--max-depth=}
      shift
      ;;
    --dry-run)
      dry_run=1
      shift
      ;;
    -h | --help)
      usage
      exit 0
      ;;
    *) usage_error "unknown argument \`$1\`." ;;
  esac
done
if [ "${#roots[@]}" -eq 0 ]; then
  roots=(".")
fi
command -v python3 >/dev/null 2>&1 ||
  usage_error "python3 is required to check and hold Cargo's build locks."

# Each root as its real path, so every path below it is real too (find never follows a link).
real_roots=()
for root in "${roots[@]}"; do
  [ -d "$root" ] || usage_error "--root \`$root\` is not a directory."
  real=$(cd "$root" && pwd -P) || usage_error "--root \`$root\` cannot be entered."
  [ "$real" != "/" ] || usage_error "--root / is too broad; name the folder that holds your workspaces."
  real_roots+=("$real")
done

# KiB as a short human-readable size.
human() {
  awk -v k="$1" 'BEGIN {
    split("KiB MiB GiB TiB", unit, " "); i = 1
    while (k >= 1024 && i < 4) { k /= 1024; i++ }
    if (i == 1) printf "%d %s", k, unit[i]; else printf "%.1f %s", k, unit[i]
  }'
}

is_cargo_tag() {
  grep -qF "$CACHEDIR_SIGNATURE" "$1" 2>/dev/null && grep -qF "$CARGO_TAG_COMMENT" "$1" 2>/dev/null
}

# Every Cargo target directory below the roots, once each, outermost only: a tag found inside
# another target directory (in whatever order find meets them) belongs to that one.
found=()
for root in "${real_roots[@]}"; do
  while IFS= read -r -d '' tag; do
    dir=${tag%/CACHEDIR.TAG}
    [ "$dir" != "$root" ] || continue
    case "$dir" in "$root"/*) ;; *) continue ;; esac
    if [ ! -d "$dir" ] || [ -L "$dir" ]; then
      continue
    fi
    is_cargo_tag "$tag" || continue
    found+=("$dir")
  done < <(find "$root" -maxdepth "$max_depth" \( -name .git -o -name node_modules \) -prune \
    -o -type f -name CACHEDIR.TAG -print0 2>/dev/null)
done
candidates=()
for dir in ${found[@]+"${found[@]}"}; do
  keep=1
  for other in ${found[@]+"${found[@]}"}; do
    case "$dir" in "$other"/*) keep=0 ;; esac
  done
  for other in ${candidates[@]+"${candidates[@]}"}; do
    [ "$dir" != "$other" ] || keep=0
  done
  [ "$keep" -eq 0 ] || candidates+=("$dir")
done

# The working directory of a process, or nothing when it cannot be read.
cwd_of() {
  if [ -d /proc/self ]; then
    readlink "/proc/$1/cwd" 2>/dev/null || true
  elif command -v lsof >/dev/null 2>&1; then
    { lsof -a -p "$1" -d cwd -Fn 2>/dev/null || true; } | sed -n 's/^n//p' | head -n 1
  fi
  return 0
}

# Why `$1` is in use by a process right now, or nothing.
in_use_by_process() {
  local dir=$1 workspace pid comm name cwd
  workspace=$(dirname "$dir")
  case "$(ps -ww -A -o args= 2>/dev/null || true)" in
    *"$dir"*)
      echo "a running process uses it"
      return
      ;;
  esac
  while read -r pid comm; do
    name=${comm##*/}
    case " $RUST_TOOLS " in *" $name "*) ;; *) continue ;; esac
    cwd=$(cwd_of "$pid")
    case "$cwd/" in
      "$workspace"/*)
        echo "$name is running in its workspace"
        return
        ;;
    esac
  done < <(ps -A -o pid= -o comm= 2>/dev/null || true)
  return 0
}

# Takes every Cargo lock of `$2` without waiting and, unless `$1` is `probe`, removes `$2` while
# holding them all. Exit status: 0 done, 3 a lock is held by someone else, 4 the removal failed.
locked() {
  local mode=$1 dir=$2 locks=()
  while IFS= read -r -d '' lock; do
    locks+=("$lock")
  done < <(find "$dir" -maxdepth 4 -type f \( -name .cargo-lock -o -name '.cargo-*-lock' \) \
    -print0 2>/dev/null)
  python3 - "$mode" "$dir" ${locks[@]+"${locks[@]}"} <<'PY'
import fcntl, os, shutil, stat, sys

mode, target, held = sys.argv[1], sys.argv[2], []
for path in sys.argv[3:]:
    try:
        fd = os.open(path, os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK | os.O_CLOEXEC)
    except FileNotFoundError:
        continue
    except OSError:
        sys.exit(3)
    try:
        fcntl.flock(fd, fcntl.LOCK_EX | fcntl.LOCK_NB)
    except OSError:
        sys.exit(3)
    held.append(fd)
if mode == "probe":
    sys.exit(0)
try:
    if not stat.S_ISDIR(os.lstat(target).st_mode):
        sys.exit(4)
    shutil.rmtree(target)
except FileNotFoundError:
    pass
except OSError as err:
    print(f"{target}: {err.strerror}", file=sys.stderr)
    sys.exit(4)
PY
}

mode_word="Reclaimed"
action="removed"
if [ "$dry_run" -eq 1 ]; then
  mode_word="Would reclaim"
  action="would remove"
fi
reclaimed_kib=0
removed=0
kept=0
failed=0
for dir in ${candidates[@]+"${candidates[@]}"}; do
  [ -d "$dir" ] || continue
  if [ -n "$(find "$dir" -maxdepth 3 -mtime "-$max_age_days" -print -quit 2>/dev/null || true)" ]; then
    printf 'kept (used within %s days)  %s\n' "$max_age_days" "$dir"
    kept=$((kept + 1))
    continue
  fi
  reason=$(in_use_by_process "$dir")
  if [ -n "$reason" ]; then
    printf 'kept (in use: %s)  %s\n' "$reason" "$dir"
    kept=$((kept + 1))
    continue
  fi
  kib=$({ du -sk "$dir" 2>/dev/null || true; } | awk 'NR == 1 {print $1}')
  kib=${kib:-0}
  status=0
  if [ "$dry_run" -eq 1 ]; then
    locked probe "$dir" || status=$?
  else
    locked remove "$dir" || status=$?
  fi
  case "$status" in
    0)
      printf '%s %10s  %s\n' "$action" "$(human "$kib")" "$dir"
      reclaimed_kib=$((reclaimed_kib + kib))
      removed=$((removed + 1))
      ;;
    3)
      printf 'kept (in use: a Cargo build lock is held)  %s\n' "$dir"
      kept=$((kept + 1))
      ;;
    *)
      printf 'failed to remove  %s\n' "$dir" >&2
      failed=$((failed + 1))
      ;;
  esac
done

printf '%s %s from %d of %d Cargo target directories (%d kept' "$mode_word" \
  "$(human "$reclaimed_kib")" "$removed" "${#candidates[@]}" "$kept"
if [ "$failed" -gt 0 ]; then
  printf ', %d failed' "$failed"
fi
printf ').\n'
[ "$failed" -eq 0 ] || exit 1
