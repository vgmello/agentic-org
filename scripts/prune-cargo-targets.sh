#!/usr/bin/env bash
# Removes stale Cargo target directories under one or more roots, and never one in use.
#
# A Cargo target directory is a real directory holding the CACHEDIR.TAG that Cargo writes. It is
# stale when nothing anywhere in it changed for --max-age-days days (with 0, its age is not
# looked at, so even a file dated in the future does not keep it). It is kept, however old,
# while:
#   - any of its Cargo locks (.cargo-lock, .cargo-build-lock, .cargo-artifact-lock, at any depth)
#     is held, that is while a build, check, test or run is in progress;
#   - a cargo, rustc, rustdoc, clippy or rust-analyzer process works in the workspace that owns it;
#   - any process names a path inside it on its command line (a test binary, a rustc --out-dir).
# The removal takes every Cargo lock of the directory, renames the directory aside within its
# parent while it holds them all, and only then deletes the renamed tree. A build that starts
# meanwhile either waits for a lock and then builds from scratch, or finds no directory and
# builds from scratch; the deletion never touches the directory a new build creates.
#
# Only directories strictly inside a --root are considered; symlinks are never followed, and
# .git and node_modules folders are not searched. Running it twice removes nothing the second
# time. Exit status: 0 when every stale directory was removed (or would be, with --dry-run),
# 1 when a check or removal failed, 2 for a usage error.

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
  --max-depth N       Look for target directories at most N levels below a root (default: 12).
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
max_depth=12
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
# Strip leading zeros so the numbers are never read as octal.
max_age_days=$((10#$max_age_days))
max_depth=$((10#$max_depth))
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

# Walks a target directory, never following a symlink, to check its age or to take its Cargo
# locks and remove it. Usage:
#   recent DIR DAYS   0 when nothing in DIR changed within DAYS days, 5 when something did;
#   probe DIR         0 when every Cargo lock in DIR can be taken right now;
#   remove DIR ROOT   takes every Cargo lock in DIR, renames DIR aside within its parent while
#                     holding them, then deletes the renamed tree; 0 when DIR is gone.
# Exit status 3: a Cargo lock is held by someone else, so DIR is in use. 4: a check or the
# removal failed (the reason is on stderr).
read -r -d '' HELPER <<'PY' || true
import fcntl, fnmatch, os, secrets, shutil, stat, sys, time

DONE, HELD, FAILED, RECENT = 0, 3, 4, 5


def walk(top):
    """Every directory in `top` with its entries; symlinks are listed, never followed. A
    directory that vanishes meanwhile is skipped; any other error is raised."""

    def failed(err):
        if not isinstance(err, FileNotFoundError):
            raise err

    return os.walk(top, onerror=failed, followlinks=False)


def used_since(target, cutoff):
    try:
        if os.lstat(target).st_mtime > cutoff:
            return True
    except FileNotFoundError:
        return False
    for path, dirs, files in walk(target):
        for name in dirs + files:
            try:
                if os.lstat(os.path.join(path, name)).st_mtime > cutoff:
                    return True
            except FileNotFoundError:
                continue
    return False


def is_cargo_lock(name):
    return name == ".cargo-lock" or fnmatch.fnmatchcase(name, ".cargo-*-lock")


def take_locks(top, held, owned):
    """Takes, without waiting, every Cargo lock in `top` not taken yet. False when one cannot
    be taken: someone else holds it, or it cannot even be opened."""
    for path, _, files in walk(top):
        for name in files:
            if not is_cargo_lock(name):
                continue
            flags = os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK | os.O_CLOEXEC
            try:
                fd = os.open(os.path.join(path, name), flags)
            except FileNotFoundError:
                continue
            except OSError:
                return False
            info = os.fstat(fd)
            key = (info.st_dev, info.st_ino)
            if key in owned:
                # Already ours (a hard link): a second flock on it would refuse itself.
                os.close(fd)
                continue
            try:
                fcntl.flock(fd, fcntl.LOCK_EX | fcntl.LOCK_NB)
            except OSError:
                os.close(fd)
                return False
            held.append(fd)
            owned.add(key)
    return True


def remove(target, root):
    parent = os.path.dirname(target)
    held, owned = [], set()
    try:
        if not take_locks(target, held, owned):
            return HELD
        try:
            info = os.lstat(target)
        except FileNotFoundError:
            return DONE
        if (
            not stat.S_ISDIR(info.st_mode)
            or os.path.realpath(target) != target
            or target == root
            or os.path.commonpath([root, target]) != root
        ):
            print(f"{target}: no longer a real directory inside {root}", file=sys.stderr)
            return FAILED
        staging = os.path.join(parent, f".cargo-prune-{os.getpid()}-{secrets.token_hex(6)}")
        os.rename(target, staging)
        # A lock created after the walk, before the rename, is a build that started meanwhile:
        # put its directory back for it when nothing has taken the name since.
        if not take_locks(staging, held, owned):
            if not os.path.lexists(target):
                os.rename(staging, target)
                return HELD
            print(f"{target}: a build started during the removal; left at {staging}", file=sys.stderr)
            return FAILED
        shutil.rmtree(staging)
        return DONE
    finally:
        for fd in held:
            os.close(fd)


def main(argv):
    mode, target = argv[1], argv[2]
    try:
        if mode == "recent":
            cutoff = time.time() - int(argv[3]) * 86400
            return RECENT if used_since(target, cutoff) else DONE
        if mode == "probe":
            held = []
            try:
                return DONE if take_locks(target, held, set()) else HELD
            finally:
                for fd in held:
                    os.close(fd)
        if mode == "remove":
            return remove(target, argv[3])
    except OSError as err:
        print(f"{err.filename or target}: {err.strerror or err}", file=sys.stderr)
        return FAILED
    print(f"unknown mode {mode}", file=sys.stderr)
    return FAILED


sys.exit(main(sys.argv))
PY
readonly HELPER

# Runs the helper above in isolated mode, so no module in the current directory shadows its own.
helper() {
  python3 -I -c "$HELPER" "$@"
}

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
# another target directory (in whatever order find meets them) belongs to that one. A target
# directory N levels below a root holds its tag N + 1 levels below it. Each directory's root
# is kept beside it (same index), for the removal to check it is still inside.
found=()
found_roots=()
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
    found_roots+=("$root")
  done < <(find "$root" -maxdepth "$((max_depth + 1))" \( -name .git -o -name node_modules \) \
    -prune -o -type f -name CACHEDIR.TAG -print0 2>/dev/null)
done
candidates=()
candidate_roots=()
i=0
while [ "$i" -lt "${#found[@]}" ]; do
  dir=${found[$i]}
  keep=1
  for other in "${found[@]}"; do
    case "$dir" in "$other"/*) keep=0 ;; esac
  done
  for other in ${candidates[@]+"${candidates[@]}"}; do
    [ "$dir" != "$other" ] || keep=0
  done
  if [ "$keep" -eq 1 ]; then
    candidates+=("$dir")
    candidate_roots+=("${found_roots[$i]}")
  fi
  i=$((i + 1))
done

# The working directory of a process, or nothing when it cannot be read. awk, the last command
# of the pipe, stops at the first name; lsof's failure (a SIGPIPE from that included) is
# ignored, so `pipefail` never turns it into an error.
cwd_of() {
  if [ -d /proc/self ]; then
    readlink "/proc/$1/cwd" 2>/dev/null || true
  elif command -v lsof >/dev/null 2>&1; then
    { lsof -a -p "$1" -d cwd -Fn 2>/dev/null || true; } | awk '/^n/ { print substr($0, 2); exit }'
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
i=0
while [ "$i" -lt "${#candidates[@]}" ]; do
  dir=${candidates[$i]}
  root=${candidate_roots[$i]}
  i=$((i + 1))
  [ -d "$dir" ] || continue
  if [ "$max_age_days" -gt 0 ]; then
    status=0
    helper recent "$dir" "$max_age_days" || status=$?
    case "$status" in
      0) ;;
      5)
        printf 'kept (used within %s days)  %s\n' "$max_age_days" "$dir"
        kept=$((kept + 1))
        continue
        ;;
      *)
        printf 'failed to check  %s\n' "$dir" >&2
        failed=$((failed + 1))
        continue
        ;;
    esac
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
    helper probe "$dir" || status=$?
  else
    helper remove "$dir" "$root" || status=$?
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
