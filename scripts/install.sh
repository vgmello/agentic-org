#!/usr/bin/env bash
# Agentic Org prebuilt release installer.
# Verifies release bundles before installation without compiling from source.
set -euo pipefail

usage() {
  cat <<'EOF'
Agentic Org Installer

Usage:
  # Upgrade an existing installation to the latest GitHub Release:
  curl -fsSL https://raw.githubusercontent.com/vgmello/agentic-org/v1/scripts/install.sh | bash

  # Install the latest (or pinned --version) GitHub Release on a new host:
  install.sh [--version <tag>] --sha256 <file|hex> [--repo vgmello/agentic-org] [--base-url <url>] [--bin-dir ~/.local/bin] [--store <dir>]
  install.sh [--version <tag>] --pubkey <publisher.pub.pem|trust-registry.json> [--bin-dir ~/.local/bin] [--store <dir>]

  # Install or upgrade from a local release archive:
  install.sh --archive <path.tar.gz> [--sha256 <file|hex>] [--pubkey <file>] [--bin-dir ~/.local/bin] [--store <dir>]

  # Install an unsigned local archive (explicit opt-in only):
  install.sh --archive <path.tar.gz> --allow-unsigned [--bin-dir ~/.local/bin]

  # Install or print the minimal scoped AppArmor profile for bwrap (Ubuntu 24.04+):
  sudo bash scripts/install.sh --install-bwrap-apparmor
  bash scripts/install.sh --print-bwrap-apparmor

Options:
  --archive <path>           Local .tar.gz release archive to install
  --sha256 <file|hex>        Expected archive SHA-256 (file or 64-char hex digest)
  --pubkey <path>            Trusted publisher Ed25519 public key PEM or trust-registry JSON
  --version <tag>            Release tag to download (defaults to latest GitHub Release)
  --repo <owner/repo>        GitHub repository (default: vgmello/agentic-org)
  --base-url <url>           Custom base URL for archive downloads (requires --version)
  --bin-dir <dir>            Directory for CLI symlinks (default: ~/.local/bin)
  --store <dir>              Explicit release store directory
  --allow-unsigned           Allow installing an unsigned local archive
  --no-restart               Do not start again the services the upgrade stopped
  --install-bwrap-apparmor   Install minimal scoped AppArmor profile for bwrap (Linux)
  --print-bwrap-apparmor     Print the canonical minimal scoped AppArmor profile and exit
  -h, --help                 Show this help

Upgrades move each instance's registered service (launchd/systemd) that setup wrote to the
new release: a running service is stopped for the install and started again on the new
release (unless --no-restart), a stopped one stays stopped, and a service definition changed
by hand is left untouched with the steps to upgrade it.
EOF
}

require_arg() {
  if [[ $# -lt 2 || -z "${2:-}" ]]; then
    echo "install.sh: option $1 requires a value" >&2
    exit 2
  fi
}

require_cmd() {
  if ! command -v "$1" >/dev/null 2>&1; then
    echo "install.sh: required command not found: $1" >&2
    exit 1
  fi
}

ARCHIVE=""
SHA256_INPUT=""
PUBKEY_FILE=""
VERSION=""
REPO="vgmello/agentic-org"
REPO_SET=0
BASE_URL=""
BIN_DIR="${HOME}/.local/bin"
BIN_DIR_SET=0
STORE_DIR=""
ALLOW_UNSIGNED=0
NO_RESTART=0
INSTALL_BWRAP_APPARMOR=0
PRINT_BWRAP_APPARMOR=0
TMP_DIR=""
TMP_APPARMOR_PROFILE=""
BWRAP_APPARMOR_CHANGED=0

cleanup_temp_paths() {
  if [[ -n "${TMP_APPARMOR_PROFILE:-}" ]]; then
    rm -f "$TMP_APPARMOR_PROFILE"
  fi
  if [[ -n "${TMP_DIR:-}" ]]; then
    rm -rf "$TMP_DIR"
  fi
}
trap cleanup_temp_paths EXIT

while [[ $# -gt 0 ]]; do
  case "$1" in
    --archive) require_arg "$@"; ARCHIVE="$2"; shift 2 ;;
    --sha256) require_arg "$@"; SHA256_INPUT="$2"; shift 2 ;;
    --pubkey) require_arg "$@"; PUBKEY_FILE="$2"; shift 2 ;;
    --version) require_arg "$@"; VERSION="$2"; shift 2 ;;
    --repo) require_arg "$@"; REPO="$2"; REPO_SET=1; shift 2 ;;
    --base-url) require_arg "$@"; BASE_URL="$2"; shift 2 ;;
    --bin-dir) require_arg "$@"; BIN_DIR="$2"; BIN_DIR_SET=1; shift 2 ;;
    --store) require_arg "$@"; STORE_DIR="$2"; shift 2 ;;
    --allow-unsigned) ALLOW_UNSIGNED=1; shift ;;
    --no-restart) NO_RESTART=1; shift ;;
    --install-bwrap-apparmor) INSTALL_BWRAP_APPARMOR=1; shift ;;
    --print-bwrap-apparmor) PRINT_BWRAP_APPARMOR=1; shift ;;
    -h|--help)
      usage
      exit 0
      ;;
    *) echo "install.sh: unknown argument: $1" >&2; exit 2 ;;
  esac
done

# Emit the canonical minimal scoped AppArmor 4.0+ profile for bubblewrap (`bwrap`).
# Keep byte-for-byte identical with `packaging/apparmor/agentic-org-bwrap`.
print_bwrap_apparmor_profile() {
  cat <<'EOF'
# Minimal scoped AppArmor 4.0+ profile for bubblewrap (`bwrap`) used by
# Agentic Org managed agent sandboxes on Ubuntu 24.04+ / 26.04+ hosts.
#
# Keep `kernel.apparmor_restrict_unprivileged_userns` enabled (1) host-wide
# rather than disabling it globally. This profile grants `userns` exclusively
# to `/{usr/,}bin/bwrap` so all other unprivileged processes stay restricted.
#
# Install:
#   sudo bash scripts/install.sh --install-bwrap-apparmor

abi <abi/4.0>,
include <tunables/global>

profile agentic-org-bwrap /{usr/,}bin/bwrap flags=(unconfined) {
  userns,

  # Site-specific additions and overrides.
  include if exists <local/agentic-org-bwrap>
}
EOF
}

resolve_apparmor_parser() {
  if command -v apparmor_parser >/dev/null 2>&1; then
    command -v apparmor_parser
    return 0
  fi
  local candidate
  for candidate in /usr/sbin/apparmor_parser /sbin/apparmor_parser; do
    if [[ -x "$candidate" ]]; then
      printf '%s\n' "$candidate"
      return 0
    fi
  done
  return 1
}

normalize_apparmor_dir() {
  local raw="$1"
  while [[ "$raw" == */ && "$raw" != "/" ]]; do
    raw="${raw%/}"
  done
  printf '%s' "$raw"
}

apparmor_profile_is_disabled() {
  local dir_prefix="${1%/}"
  local name="$2"
  [[ -e "${dir_prefix}/disable/${name}" || -L "${dir_prefix}/disable/${name}" ]]
}

apparmor_restrict_profile_is_active() {
  local dir_prefix="${1%/}"
  [[ -f "${dir_prefix}/bwrap-userns-restrict" && ! -L "${dir_prefix}/bwrap-userns-restrict" ]] \
    && ! apparmor_profile_is_disabled "$1" "bwrap-userns-restrict"
}

apparmor_profile_grants_scoped_bwrap() {
  local profile_file="$1"
  [[ -f "$profile_file" && ! -L "$profile_file" && -r "$profile_file" ]] || return 1
  require_cmd awk
  LC_ALL=C awk '
    function targets_bwrap(tok) {
      if (tok ~ /^".*"$/) tok = substr(tok, 2, length(tok) - 2)
      return (substr(tok, 1, 1) == "/" && index(tok, "..") == 0 && tok ~ /\/bwrap$/)
    }
    function has_unconfined_flag(line,    rest, o, c, pfx, inner, n, i, a) {
      rest = line; sub(/[[:space:]]*[{][[:space:]]*$/, "", rest)
      while ((o = index(rest, "(")) > 0) {
        pfx = substr(rest, 1, o - 1); sub(/[[:space:]]+$/, "", pfx)
        rest = substr(rest, o + 1); c = index(rest, ")"); if (c == 0) break
        inner = substr(rest, 1, c - 1); rest = substr(rest, c + 1)
        if (pfx ~ /=$/ && pfx !~ /(^|[[:space:]])flags[[:space:]]*=$/) continue
        n = split(inner, a, /[,[:space:]]+/)
        for (i = 1; i <= n; i++) if (a[i] == "unconfined") return 1
      }
      return 0
    }
    function header_att(line,    n, t, i, c, p) {
      if (line ~ /[,;]$/) return ""
      n = split(line, t, /[[:space:]]+/)
      for (i = 1; i <= n && t[i] != "{" && substr(t[i], 1, 1) != "(" && index(t[i], "=") == 0; i++) {
        sub(/[{]+$/, "", t[i]); p[++c] = t[i]
      }
      if (c > 3 || c == 0) return ""
      if (p[1] == "profile") return (c == 2 ? p[2] : (c == 3 ? p[3] : ""))
      return (c == 1 ? p[1] : "")
    }
    {
      line = $0; sub(/#.*$/, "", line); gsub(/^[[:space:]]+|[[:space:]]+$/, "", line)
      if (line == "") next
      att = header_att(line)
      if (att != "" && targets_bwrap(att)) {
        if (has_unconfined_flag(line)) has_unconf = 1; else has_conf = 1
      }
      if (index(line, "unpriv_bwrap") > 0) restricts_caps = 1
      n = split(line, toks, /[[:space:]]+/)
      ld = 0; lu = 0; lc = 0
      for (i = 1; i <= n; i++) {
        gsub(/[,;]+$/, "", toks[i])
        if (toks[i] == "deny") ld = 1; else if (toks[i] == "userns") lu = 1; else if (toks[i] == "capability") lc = 1
      }
      if (lu) { if (ld) denies_userns = 1; else has_userns = 1 }
      if (ld && lc) restricts_caps = 1
    }
    END { exit !(has_unconf && !has_conf && has_userns && !denies_userns && !restricts_caps) }
  ' "$profile_file"
}

has_active_scoped_bwrap_apparmor_profile() {
  local dir="$1"
  local dir_prefix="${dir%/}"
  if apparmor_restrict_profile_is_active "$dir"; then
    return 1
  fi
  local bwrap_path="${dir_prefix}/bwrap"
  if [[ -e "$bwrap_path" || -L "$bwrap_path" ]] \
     && ! apparmor_profile_is_disabled "$dir" "bwrap"; then
    apparmor_profile_grants_scoped_bwrap "$bwrap_path"
    return $?
  fi
  ! apparmor_profile_is_disabled "$dir" "agentic-org-bwrap" \
    && apparmor_profile_grants_scoped_bwrap "${dir_prefix}/agentic-org-bwrap"
}

can_install_apparmor_dir() {
  local dir="$1"
  [[ "${AGENTIC_ORG_SIMULATE_UNPRIVILEGED:-0}" != "1" ]] || return 1
  [[ -d "$dir" && ! -L "$dir" && -w "$dir" ]] || return 1
  [[ -n "${AGENTIC_ORG_APPARMOR_DIR:-}" || "$(id -u)" -eq 0 ]]
}

detect_bwrap_binary() {
  if [[ -n "${AGENTIC_ORG_BWRAP_BIN:-}" ]]; then
    [[ "${AGENTIC_ORG_BWRAP_BIN}" != "none" && -x "${AGENTIC_ORG_BWRAP_BIN}" ]]
    return
  fi
  [[ -x /usr/bin/bwrap || -x /bin/bwrap ]] || command -v bwrap >/dev/null 2>&1
}

read_userns_sysctl_restricted() {
  local sysctl_path="${AGENTIC_ORG_USERNS_SYSCTL:-/proc/sys/kernel/apparmor_restrict_unprivileged_userns}"
  [[ -f "$sysctl_path" && ! -L "$sysctl_path" && -r "$sysctl_path" ]] || return 1
  local val
  val="$(tr -d '[:space:]' < "$sysctl_path" 2>/dev/null || true)"
  [[ "$val" == "1" ]]
}

print_bwrap_apparmor_manual_steps() {
  local dir="$1"
  local dir_prefix="${dir%/}"
  local q_dir q_target q_disable_dir q_restrict q_restrict_disable
  printf -v q_dir '%q' "$dir"
  printf -v q_target '%q' "${dir_prefix}/agentic-org-bwrap"
  printf -v q_disable_dir '%q' "${dir_prefix}/disable"
  printf -v q_restrict '%q' "${dir_prefix}/bwrap-userns-restrict"
  printf -v q_restrict_disable '%q' "${dir_prefix}/disable/bwrap-userns-restrict"
  echo "NOTE: AppArmor restricts unprivileged user namespaces (apparmor_restrict_unprivileged_userns=1)" >&2
  echo "and no active scoped bwrap AppArmor profile was detected under ${q_dir}." >&2
  echo "Keep kernel.apparmor_restrict_unprivileged_userns enabled (never disable it globally)." >&2
  echo "Install the minimal scoped profile for bwrap only (requires sudo):" >&2
  echo "" >&2
  echo "  # Option 1: install directly via install.sh" >&2
  echo "  sudo bash scripts/install.sh --install-bwrap-apparmor" >&2
  echo "" >&2
  echo "  # Option 2: inspect and install using --print-bwrap-apparmor" >&2
  echo "  bash scripts/install.sh --print-bwrap-apparmor | sudo tee ${q_target} >/dev/null" >&2
  echo "  sudo chmod 0644 ${q_target}" >&2
  if apparmor_restrict_profile_is_active "$dir"; then
    echo "  sudo mkdir -p ${q_disable_dir}" >&2
    echo "  sudo ln -sf ${q_restrict} ${q_restrict_disable}" >&2
    echo "  sudo apparmor_parser -R ${q_restrict} || true" >&2
  fi
  echo "  sudo apparmor_parser -r ${q_target}" >&2
}

apparmor_fail_or_warn() {
  local strict="$1"
  local display_dir="$2"
  local message="$3"
  if [[ "$strict" -eq 1 ]]; then
    echo "install.sh: ${message}" >&2
    exit 1
  fi
  echo "WARNING: ${message}" >&2
  print_bwrap_apparmor_manual_steps "$display_dir"
  return 1
}

is_unsafe_file_target() {
  [[ -L "$1" ]] || { [[ -e "$1" ]] && [[ ! -f "$1" ]]; }
}

is_unsafe_dir_target() {
  [[ -L "$1" ]] || { [[ -e "$1" ]] && [[ ! -d "$1" ]]; }
}

validate_apparmor_dir_path() {
  local apparmor_dir="$1"
  local strict="$2"
  if [[ "$apparmor_dir" != /* ]]; then
    apparmor_fail_or_warn "$strict" "/etc/apparmor.d" \
      "refusing non-absolute AppArmor directory path: ${apparmor_dir}"
    return 1
  fi
  case "${apparmor_dir}/" in
    *"/../"*|*"/./"*)
      apparmor_fail_or_warn "$strict" "/etc/apparmor.d" \
        "refusing unsafe AppArmor directory path: ${apparmor_dir}"
      return 1
      ;;
  esac
  if [[ -L "$apparmor_dir" ]]; then
    apparmor_fail_or_warn "$strict" "/etc/apparmor.d" \
      "refusing symlinked AppArmor directory: ${apparmor_dir}"
    return 1
  fi
  if [[ ! -d "$apparmor_dir" ]]; then
    apparmor_fail_or_warn "$strict" "$apparmor_dir" \
      "AppArmor profile directory not found: ${apparmor_dir}"
    return 1
  fi
  return 0
}

validate_apparmor_target_paths() {
  local apparmor_dir="$1"
  local strict="$2"
  local dir_prefix="${apparmor_dir%/}"
  local profile_path="${dir_prefix}/agentic-org-bwrap"
  local disable_dir="${dir_prefix}/disable"

  if is_unsafe_file_target "$profile_path"; then
    apparmor_fail_or_warn "$strict" "$apparmor_dir" \
      "refusing symlinked or non-regular AppArmor profile target: ${profile_path}"
    return 1
  fi
  if ! can_install_apparmor_dir "$apparmor_dir"; then
    print_bwrap_apparmor_manual_steps "$apparmor_dir"
    if [[ "$strict" -eq 1 ]]; then
      echo "install.sh: insufficient privileges to write ${profile_path}; re-run with sudo." >&2
      exit 1
    fi
    return 1
  fi
  if is_unsafe_dir_target "$disable_dir"; then
    apparmor_fail_or_warn "$strict" "$apparmor_dir" \
      "refusing symlinked or non-directory AppArmor disable path: ${disable_dir}"
    return 1
  fi
  return 0
}

clear_own_apparmor_disable() {
  local apparmor_dir="$1"
  local strict="$2"
  local own_disable="${apparmor_dir%/}/disable/agentic-org-bwrap"
  if [[ -d "$own_disable" && ! -L "$own_disable" ]]; then
    apparmor_fail_or_warn "$strict" "$apparmor_dir" \
      "refusing directory at AppArmor disable entry: ${own_disable}"
    return 1
  fi
  if [[ -L "$own_disable" || -f "$own_disable" ]]; then
    if ! rm -f "$own_disable"; then
      apparmor_fail_or_warn "$strict" "$apparmor_dir" \
        "failed to remove stale disable entry: ${own_disable}"
      return 1
    fi
  fi
  return 0
}

link_disable_restrict_profile() {
  local apparmor_dir="$1"
  local restrict_path="$2"
  local strict="$3"
  local disable_dir="${apparmor_dir%/}/disable"
  local restrict_disable="${disable_dir}/bwrap-userns-restrict"
  if ! mkdir -p "$disable_dir" || ! chmod 0755 "$disable_dir" \
     || ! ln -s "$restrict_path" "$restrict_disable"; then
    apparmor_fail_or_warn "$strict" "$apparmor_dir" \
      "failed to disable conflicting profile: ${restrict_path}"
    return 1
  fi
  echo "Disabled conflicting capability-stripping profile: ${restrict_path}" >&2
  return 0
}

disable_conflicting_restrict_profile() {
  local apparmor_dir="$1"
  local parser="$2"
  local strict="$3"
  local restrict_path="${apparmor_dir%/}/bwrap-userns-restrict"

  [[ -f "$restrict_path" && ! -L "$restrict_path" ]] || return 0
  if ! apparmor_profile_is_disabled "$apparmor_dir" "bwrap-userns-restrict"; then
    link_disable_restrict_profile "$apparmor_dir" "$restrict_path" "$strict" || return 1
  fi
  if [[ -n "$parser" ]]; then
    "$parser" -R "$restrict_path" >/dev/null 2>&1 || true
  fi
  return 0
}

stage_bwrap_apparmor_temp() {
  local apparmor_dir="$1"
  local strict="$2"
  local dir_prefix="${apparmor_dir%/}"
  if ! TMP_APPARMOR_PROFILE="$(mktemp "${dir_prefix}/.agentic-org-bwrap.XXXXXX")"; then
    TMP_APPARMOR_PROFILE=""
    apparmor_fail_or_warn "$strict" "$apparmor_dir" \
      "failed to stage scoped AppArmor profile in ${apparmor_dir}"
    return 1
  fi
  if ! print_bwrap_apparmor_profile > "$TMP_APPARMOR_PROFILE" \
     || ! chmod 0644 "$TMP_APPARMOR_PROFILE"; then
    rm -f "$TMP_APPARMOR_PROFILE"
    TMP_APPARMOR_PROFILE=""
    apparmor_fail_or_warn "$strict" "$apparmor_dir" \
      "failed to write scoped AppArmor profile in ${apparmor_dir}"
    return 1
  fi
  return 0
}

commit_bwrap_apparmor_profile() {
  local apparmor_dir="$1"
  local strict="$2"
  local profile_path="${apparmor_dir%/}/agentic-org-bwrap"
  BWRAP_APPARMOR_CHANGED=1
  if [[ -f "$profile_path" && ! -L "$profile_path" ]] \
     && cmp -s "$TMP_APPARMOR_PROFILE" "$profile_path"; then
    rm -f "$TMP_APPARMOR_PROFILE"
    TMP_APPARMOR_PROFILE=""
    chmod 0644 "$profile_path" || true
    BWRAP_APPARMOR_CHANGED=0
    return 0
  fi
  if ! mv -f "$TMP_APPARMOR_PROFILE" "$profile_path"; then
    rm -f "$TMP_APPARMOR_PROFILE"
    TMP_APPARMOR_PROFILE=""
    apparmor_fail_or_warn "$strict" "$apparmor_dir" \
      "failed to install scoped AppArmor profile at ${profile_path}"
    return 1
  fi
  TMP_APPARMOR_PROFILE=""
  return 0
}

reload_bwrap_apparmor_profile() {
  local apparmor_dir="$1"
  local profile_path="$2"
  local parser="$3"
  local strict="$4"
  if [[ "${AGENTIC_ORG_SKIP_APPARMOR_PARSER:-0}" == "1" ]]; then
    return 0
  fi
  if [[ -z "$parser" ]]; then
    echo "install.sh: wrote ${profile_path}, but apparmor_parser was not found; reload AppArmor to apply." >&2
    return 0
  fi
  if ! "$parser" -r "$profile_path"; then
    apparmor_fail_or_warn "$strict" "$apparmor_dir" \
      "wrote ${profile_path}, but ${parser} -r failed to reload profile"
    return 1
  fi
  return 0
}

apply_bwrap_apparmor_profile() {
  local apparmor_dir="$1"
  local parser="$2"
  local strict="$3"
  local profile_path="${apparmor_dir%/}/agentic-org-bwrap"

  clear_own_apparmor_disable "$apparmor_dir" "$strict" || return 0
  disable_conflicting_restrict_profile "$apparmor_dir" "$parser" "$strict" || return 0
  stage_bwrap_apparmor_temp "$apparmor_dir" "$strict" || return 0
  commit_bwrap_apparmor_profile "$apparmor_dir" "$strict" || return 0
  reload_bwrap_apparmor_profile "$apparmor_dir" "$profile_path" "$parser" "$strict" || return 0

  if [[ "$BWRAP_APPARMOR_CHANGED" -eq 1 ]]; then
    echo "Installed scoped bwrap AppArmor profile: ${profile_path}" >&2
  else
    echo "Scoped bwrap AppArmor profile already up to date: ${profile_path}" >&2
  fi
}

install_bwrap_apparmor_profile() {
  local strict="${1:-1}"
  if [[ "$PLATFORM" != "linux" ]]; then
    if [[ "$strict" -eq 1 ]]; then
      echo "install.sh: --install-bwrap-apparmor is only supported on Linux (detected: $UNAME_S)" >&2
      exit 1
    fi
    return 0
  fi

  local apparmor_dir parser=""
  apparmor_dir="$(normalize_apparmor_dir "${AGENTIC_ORG_APPARMOR_DIR:-/etc/apparmor.d}")"
  validate_apparmor_dir_path "$apparmor_dir" "$strict" || return 0
  validate_apparmor_target_paths "$apparmor_dir" "$strict" || return 0
  if [[ "${AGENTIC_ORG_SKIP_APPARMOR_PARSER:-0}" != "1" ]]; then
    parser="$(resolve_apparmor_parser || true)"
  fi
  apply_bwrap_apparmor_profile "$apparmor_dir" "$parser" "$strict"
}

maybe_handle_linux_bwrap_apparmor() {
  if [[ "$INSTALL_BWRAP_APPARMOR" -eq 1 ]]; then
    install_bwrap_apparmor_profile 1
    return 0
  fi
  [[ "$PLATFORM" == "linux" ]] || return 0
  read_userns_sysctl_restricted || return 0
  detect_bwrap_binary || return 0
  local apparmor_dir
  apparmor_dir="$(normalize_apparmor_dir "${AGENTIC_ORG_APPARMOR_DIR:-/etc/apparmor.d}")"
  if has_active_scoped_bwrap_apparmor_profile "$apparmor_dir"; then
    return 0
  fi
  install_bwrap_apparmor_profile 0
}

has_release_source_flag() {
  [[ -n "$ARCHIVE$SHA256_INPUT$PUBKEY_FILE$VERSION$BASE_URL" || "$ALLOW_UNSIGNED" -eq 1 ]]
}

has_non_source_release_flag() {
  [[ -n "$STORE_DIR" || "$REPO_SET" -eq 1 || "$BIN_DIR_SET" -eq 1 || "$NO_RESTART" -eq 1 ]]
}

if [[ "$PRINT_BWRAP_APPARMOR" -eq 1 ]]; then
  if [[ "$INSTALL_BWRAP_APPARMOR" -eq 1 ]]; then
    echo "install.sh: --print-bwrap-apparmor cannot be combined with --install-bwrap-apparmor" >&2
    exit 2
  fi
  if has_release_source_flag || has_non_source_release_flag; then
    echo "install.sh: --print-bwrap-apparmor cannot be combined with release install options" >&2
    exit 2
  fi
  print_bwrap_apparmor_profile
  exit 0
fi

UNAME_S="$(uname -s)"
case "$UNAME_S" in
  Linux) PLATFORM="linux" ;;
  Darwin) PLATFORM="macos" ;;
  *) echo "install.sh: unsupported OS: $UNAME_S" >&2; exit 1 ;;
esac

if [[ "$INSTALL_BWRAP_APPARMOR" -eq 1 ]] && ! has_release_source_flag; then
  if has_non_source_release_flag; then
    echo "install.sh: standalone --install-bwrap-apparmor cannot be combined with release install options" >&2
    exit 2
  fi
  install_bwrap_apparmor_profile 1
  exit 0
fi

require_cmd tar
require_cmd awk
if ! command -v sha256sum >/dev/null 2>&1 && ! command -v shasum >/dev/null 2>&1; then
  echo "install.sh: required command not found: sha256sum or shasum" >&2
  exit 1
fi

if [[ ! "$REPO" =~ ^[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+$ ]]; then
  echo "install.sh: invalid --repo value (expected owner/repo): $REPO" >&2
  exit 2
fi

if [[ -n "$BASE_URL" ]]; then
  BASE_URL="${BASE_URL%/}"
  if [[ ! "$BASE_URL" =~ ^https?:// ]]; then
    echo "install.sh: --base-url must start with http:// or https://: $BASE_URL" >&2
    exit 2
  fi
fi

if [[ "$ALLOW_UNSIGNED" -eq 1 && -n "$PUBKEY_FILE" ]]; then
  echo "install.sh: --allow-unsigned cannot be combined with --pubkey" >&2
  exit 2
fi

if [[ "$ALLOW_UNSIGNED" -eq 1 && -z "$ARCHIVE" ]]; then
  echo "install.sh: --allow-unsigned is only supported with local --archive installs" >&2
  exit 2
fi

download_file() {
  local url="$1"
  local dest="$2"
  if command -v curl >/dev/null 2>&1; then
    curl -fsSL -o "$dest" -- "$url"
  elif command -v wget >/dev/null 2>&1; then
    LC_ALL=C wget -q -O "$dest" -- "$url"
  else
    echo "install.sh: required command not found: curl or wget" >&2
    exit 1
  fi
}

resolve_latest_version() {
  local repo="$1"
  local latest_url="https://github.com/${repo}/releases/latest"
  local effective_url=""
  if command -v curl >/dev/null 2>&1; then
    effective_url="$(curl -fsSL -o /dev/null -w '%{url_effective}' -- "$latest_url")" || {
      echo "install.sh: failed to query latest release for ${repo}" >&2
      exit 1
    }
  elif command -v wget >/dev/null 2>&1; then
    effective_url="$(LC_ALL=C wget -v -S --max-redirect=5 --spider -- "$latest_url" 2>&1 \
      | awk 'tolower($1) == "location:" { loc = $2 } END { gsub(/\r/, "", loc); print loc }')" || true
    if [[ -z "$effective_url" ]]; then
      echo "install.sh: failed to query latest release for ${repo}" >&2
      exit 1
    fi
  else
    echo "install.sh: required command not found: curl or wget" >&2
    exit 1
  fi
  if [[ "$effective_url" != *"/releases/tag/"* ]]; then
    echo "install.sh: no published release found for ${repo}; pass --version <tag> or --archive <path.tar.gz>" >&2
    exit 1
  fi
  local tag="${effective_url##*/releases/tag/}"
  tag="${tag%%\?*}"
  tag="${tag%%#*}"
  tag="${tag%/}"
  if [[ ! "$tag" =~ ^[A-Za-z0-9][A-Za-z0-9._-]{0,95}$ ]]; then
    echo "install.sh: resolved invalid release tag from ${effective_url}" >&2
    exit 1
  fi
  printf '%s' "$tag"
}

UNAME_M="$(uname -m)"
case "$UNAME_M" in
  x86_64|amd64) ARCH="x86_64" ;;
  aarch64|arm64) ARCH="aarch64" ;;
  *) echo "install.sh: unsupported CPU architecture: $UNAME_M" >&2; exit 1 ;;
esac

# Match org_host::default_release_store_dir() (<trusted-app-base>/release-store).
if [[ -n "$STORE_DIR" ]]; then
  EFFECTIVE_STORE_DIR="$STORE_DIR"
elif [[ "$PLATFORM" == "linux" ]]; then
  EFFECTIVE_STORE_DIR="${HOME}/.local/share/agentic-org/release-store"
else
  EFFECTIVE_STORE_DIR="${HOME}/Library/Application Support/agentic.org/release-store"
fi

RAW_TMP_DIR="$(mktemp -d "${TMPDIR:-/tmp}/agentic-org-install.XXXXXX")"
chmod 0700 "$RAW_TMP_DIR"
# Resolve macOS /tmp -> /private/tmp and /var -> /private/var symlinks canonically.
TMP_DIR="$(cd "$RAW_TMP_DIR" && pwd -P)"

compute_sha256() {
  local file="$1"
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$file" | awk '{print tolower($1)}'
  elif command -v shasum >/dev/null 2>&1; then
    shasum -a 256 "$file" | awk '{print tolower($1)}'
  else
    echo "install.sh: neither sha256sum nor shasum found" >&2
    exit 1
  fi
}

verify_sha256() {
  local file="$1"
  local expected="$2"
  expected="$(printf '%s' "$expected" | tr 'A-F' 'a-f')"
  if [[ ! "$expected" =~ ^[0-9a-f]{64}$ ]]; then
    echo "install.sh: invalid SHA-256 digest: $expected" >&2
    exit 1
  fi
  local actual
  actual="$(compute_sha256 "$file")"
  if [[ "$actual" != "$expected" ]]; then
    echo "install.sh: SHA-256 mismatch for $(basename "$file") (expected $expected, got $actual)" >&2
    exit 1
  fi
}

resolve_sha256_input() {
  local raw="$1"
  if [[ "$raw" =~ ^[0-9a-fA-F]{64}$ ]]; then
    printf '%s' "$raw"
  elif [[ -f "$raw" && ! -L "$raw" ]]; then
    awk '{print $1; exit}' "$raw"
  else
    echo "install.sh: --sha256 must be a regular checksum file or a 64-char hex digest: $raw" >&2
    exit 1
  fi
}

validate_tar_entries() {
  local archive="$1"
  local verbose_file="$2"
  local names_file="$3"
  local bad_type bad_path
  # Write tar listings to private files (never piped into early-exit awk/grep under pipefail).
  tar -tvzf "$archive" > "$verbose_file"
  tar -tzf "$archive" > "$names_file"
  if [[ ! -s "$verbose_file" || ! -s "$names_file" ]]; then
    echo "install.sh: archive is empty" >&2
    exit 1
  fi
  # 1. Reject symlinks, hardlinks, devices, FIFOs, and directory entries (regular files only).
  bad_type="$(awk 'substr($1, 1, 1) != "-" { print; exit }' "$verbose_file")"
  if [[ -n "$bad_type" ]]; then
    echo "install.sh: refusing non-regular archive entry: $bad_type" >&2
    exit 1
  fi
  # 2. Validate full unsplit entry paths from `tar -tzf` (immune to whitespace/$NF tricks):
  #    reject leading `/`, `..` path segments, whitespace/control chars, backslashes, and duplicates.
  bad_path="$(awk '
    {
      p = $0
      if (p == "" || substr(p, 1, 1) == "/" || p ~ /[[:space:][:cntrl:]]/ || index(p, "\\") > 0) {
        print p; exit
      }
      if (p == ".." || substr(p, 1, 3) == "../" || index(p, "/../") > 0 || (length(p) >= 3 && substr(p, length(p) - 2) == "/..")) {
        print p; exit
      }
      if (seen[p]++) {
        print "duplicate:" p; exit
      }
    }
  ' "$names_file")"
  if [[ -n "$bad_path" ]]; then
    echo "install.sh: refusing unsafe archive path: $bad_path" >&2
    exit 1
  fi
}

extract_json_string() {
  local json_text="$1"
  local key="$2"
  awk -v k="$key" '
    { buf = buf $0 "\n" }
    END {
      pat = "\"" k "\"[[:space:]]*:[[:space:]]*\"[^\"]*\""
      if (match(buf, pat)) {
        s = substr(buf, RSTART, RLENGTH)
        sub(/^[^:]*:[[:space:]]*"/, "", s)
        sub(/"$/, "", s)
        print s
      }
    }
  ' <<< "$json_text"
}

extract_json_uint() {
  local json_text="$1"
  local key="$2"
  awk -v k="$key" '
    { buf = buf $0 "\n" }
    END {
      pat = "\"" k "\"[[:space:]]*:[[:space:]]*[0-9]+([[:space:]]*[,}]|[[:space:]]*$)"
      if (match(buf, pat)) {
        s = substr(buf, RSTART, RLENGTH)
        sub(/^[^:]*:[[:space:]]*/, "", s)
        sub(/[[:space:]]*[,}][[:space:]]*$/, "", s)
        sub(/[[:space:]]*$/, "", s)
        print s
      }
    }
  ' <<< "$json_text"
}

# Order-independent, minified- and pretty-JSON-safe lookup of a key entry in
# release-trust-registry.json. Emits `public_key_hex|min_sequence|max_sequence`
# (where `max_sequence` may be `null`) for the exact matching `key_id`.
lookup_registry_key_entry() {
  local registry_file="$1"
  local target_kid="$2"
  awk -v kid="$target_kid" '
    BEGIN { RS = "}" }
    {
      obj = $0
      gsub(/[\r\n]/, " ", obj)
      # Use regex separator `/[{]/` so macOS BSD awk never splits on newlines when RS != "\n".
      n = split(obj, parts, /[{]/)
      seg = parts[n]
      got_kid = ""
      if (match(seg, /"key_id"[[:space:]]*:[[:space:]]*"[^"]*"/)) {
        got_kid = substr(seg, RSTART, RLENGTH)
        sub(/^[^:]*:[[:space:]]*"/, "", got_kid)
        sub(/"$/, "", got_kid)
      }
      if (got_kid != kid) next
      pub = ""; min_s = ""; max_s = ""
      if (match(seg, /"public_key_hex"[[:space:]]*:[[:space:]]*"[^"]*"/)) {
        pub = substr(seg, RSTART, RLENGTH)
        sub(/^[^:]*:[[:space:]]*"/, "", pub)
        sub(/"$/, "", pub)
      }
      if (match(seg, /"min_sequence"[[:space:]]*:[[:space:]]*[0-9]+([[:space:]]*,|[[:space:]]*$)/)) {
        min_s = substr(seg, RSTART, RLENGTH)
        sub(/^[^:]*:[[:space:]]*/, "", min_s)
        sub(/[[:space:]]*,[[:space:]]*$/, "", min_s)
        sub(/[[:space:]]*$/, "", min_s)
      }
      if (match(seg, /"max_sequence"[[:space:]]*:[[:space:]]*([0-9]+|null)([[:space:]]*,|[[:space:]]*$)/)) {
        max_s = substr(seg, RSTART, RLENGTH)
        sub(/^[^:]*:[[:space:]]*/, "", max_s)
        sub(/[[:space:]]*,[[:space:]]*$/, "", max_s)
        sub(/[[:space:]]*$/, "", max_s)
      }
      printf "%s|%s|%s\n", pub, min_s, max_s
      exit
    }
  ' "$registry_file"
}

# Order-independent, minified- and pretty-JSON-safe lookup of a file entry SHA-256
# in release.v1.json.
lookup_manifest_file_sha256() {
  local manifest_file="$1"
  local target_path="$2"
  awk -v wanted="$target_path" '
    BEGIN { RS = "}" }
    {
      obj = $0
      gsub(/[\r\n]/, " ", obj)
      n = split(obj, parts, /[{]/)
      seg = parts[n]
      got_path = ""
      if (match(seg, /"path"[[:space:]]*:[[:space:]]*"[^"]*"/)) {
        got_path = substr(seg, RSTART, RLENGTH)
        sub(/^[^:]*:[[:space:]]*"/, "", got_path)
        sub(/"$/, "", got_path)
      }
      if (got_path != wanted) next
      if (match(seg, /"sha256"[[:space:]]*:[[:space:]]*"[^"]*"/)) {
        sha = substr(seg, RSTART, RLENGTH)
        sub(/^[^:]*:[[:space:]]*"/, "", sha)
        sub(/"$/, "", sha)
        print sha
        exit
      }
    }
  ' "$manifest_file"
}

hex_to_bin() {
  local hex="$1"
  local i byte
  for ((i = 0; i < ${#hex}; i += 2)); do
    byte="${hex:i:2}"
    printf '%b' "\\0$(printf '%03o' "0x${byte}")"
  done
}

resolve_registry_pubkey_pem() {
  local manifest_file="$1"
  local registry_file="$2"
  local out_pem="$3"
  local manifest_text key_id seq reg_line pub_hex min_seq max_seq
  manifest_text="$(cat "$manifest_file")"
  key_id="$(extract_json_string "$manifest_text" "key_id")"
  seq="$(extract_json_uint "$manifest_text" "sequence")"
  if [[ ! "$key_id" =~ ^[0-9a-f]{64}$ ]]; then
    echo "install.sh: manifest contains invalid key_id" >&2
    exit 1
  fi
  if [[ ! "$seq" =~ ^[1-9][0-9]*$ ]]; then
    echo "install.sh: manifest contains invalid sequence" >&2
    exit 1
  fi
  reg_line="$(lookup_registry_key_entry "$registry_file" "$key_id")"
  IFS='|' read -r pub_hex min_seq max_seq <<< "$reg_line"
  if [[ ! "${pub_hex:-}" =~ ^[0-9a-fA-F]{64}$ ]]; then
    echo "install.sh: signing key_id not found in trust registry: $registry_file" >&2
    exit 1
  fi
  verify_registry_sequence_bounds "$seq" "${min_seq:-}" "${max_seq:-}"
  local spki_der="$TMP_DIR/publisher.pub.der"
  hex_to_bin "302a300506032b6570032100${pub_hex}" > "$spki_der"
  openssl pkey -pubin -inform DER -in "$spki_der" -out "$out_pem" >/dev/null 2>&1
}

# Compare two normalized unsigned decimal integers (0 or [1-9][0-9]*) without float/64-bit overflow.
uint_lt() {
  LC_ALL=C awk -v a="$1" -v b="$2" \
    'BEGIN { exit !(length(a) < length(b) || (length(a) == length(b) && ("s" a) < ("s" b))) }'
}

verify_registry_sequence_bounds() {
  local seq="$1"
  local min_seq="$2"
  local max_seq="$3"
  local u64_max="18446744073709551615"
  # Fail closed unless min_sequence is a valid u64 and max_sequence is either
  # `null` (unbounded) or a valid u64 >= min_sequence.
  if uint_lt "$u64_max" "$seq"; then
    echo "install.sh: manifest sequence ($seq) exceeds unsigned 64-bit limit" >&2
    exit 1
  fi
  if [[ ! "$min_seq" =~ ^(0|[1-9][0-9]*)$ ]] || uint_lt "$u64_max" "$min_seq"; then
    echo "install.sh: trust registry key entry missing valid min_sequence bound" >&2
    exit 1
  fi
  if [[ "$max_seq" != "null" ]] && { [[ ! "$max_seq" =~ ^(0|[1-9][0-9]*)$ ]] || uint_lt "$u64_max" "$max_seq"; }; then
    echo "install.sh: trust registry key entry missing valid max_sequence bound" >&2
    exit 1
  fi
  if uint_lt "$seq" "$min_seq"; then
    echo "install.sh: manifest sequence ($seq) below key min_sequence ($min_seq)" >&2
    exit 1
  fi
  if [[ "$max_seq" != "null" ]]; then
    if uint_lt "$max_seq" "$min_seq"; then
      echo "install.sh: trust registry key has invalid bounds (min_sequence=$min_seq > max_sequence=$max_seq)" >&2
      exit 1
    fi
    if uint_lt "$max_seq" "$seq"; then
      echo "install.sh: manifest sequence ($seq) exceeds key max_sequence ($max_seq)" >&2
      exit 1
    fi
  fi
}

verify_pubkey_signature() {
  local manifest_file="$1"
  local sig_file="$2"
  local pubkey_input="$3"
  require_cmd openssl
  local pkeyutl_help
  pkeyutl_help="$(openssl pkeyutl -help 2>&1 || true)"
  if [[ "$pkeyutl_help" != *"-rawin"* ]]; then
    echo "install.sh: installed openssl does not support Ed25519 '-rawin' verification (e.g. stock macOS LibreSSL); install OpenSSL 3+ or pass --sha256 <file|hex>" >&2
    exit 1
  fi
  if [[ ! -f "$pubkey_input" || -L "$pubkey_input" ]]; then
    echo "install.sh: public key file not found or is a symlink: $pubkey_input" >&2
    exit 1
  fi
  local pem_file="$pubkey_input"
  if ! grep -q "BEGIN PUBLIC KEY" "$pubkey_input" 2>/dev/null; then
    pem_file="$TMP_DIR/publisher.pub.pem"
    resolve_registry_pubkey_pem "$manifest_file" "$pubkey_input" "$pem_file"
  fi
  if ! openssl pkeyutl -verify -pubin -inkey "$pem_file" -rawin -in "$manifest_file" -sigfile "$sig_file" >/dev/null 2>&1; then
    echo "install.sh: Ed25519 signature verification failed for release.v1.json" >&2
    exit 1
  fi
}

find_trusted_installed_cli() {
  local candidate=""
  if [[ -x "$BIN_DIR/agentic-org" ]]; then
    candidate="$BIN_DIR/agentic-org"
  elif command -v agentic-org >/dev/null 2>&1; then
    candidate="$(command -v agentic-org)"
  fi
  if [[ -z "$candidate" ]]; then
    return 1
  fi
  local resolved_dir resolved_bin canonical_store
  resolved_dir="$(cd "$(dirname "$candidate")" && pwd -P)"
  if [[ -L "$candidate" ]]; then
    local target
    target="$(readlink "$candidate")"
    if [[ "$target" != /* ]]; then
      target="${resolved_dir}/${target}"
    fi
    if [[ -f "$target" && -x "$target" && ! -L "$target" ]]; then
      resolved_bin="$(cd "$(dirname "$target")" && pwd -P)/$(basename "$target")"
    else
      return 1
    fi
  elif [[ -f "$candidate" && -x "$candidate" ]]; then
    resolved_bin="${resolved_dir}/$(basename "$candidate")"
  else
    return 1
  fi
  if [[ -d "$EFFECTIVE_STORE_DIR/releases" ]]; then
    canonical_store="$(cd "$EFFECTIVE_STORE_DIR/releases" && pwd -P)"
    case "$resolved_bin" in
      "${canonical_store}"/*)
        printf '%s' "$resolved_bin"
        return 0
        ;;
    esac
  fi
  return 1
}

verify_installed_binary_in_store() {
  local bin_path="$1"
  local store_releases_dir="$2"
  if [[ -z "$bin_path" || ! -f "$bin_path" || -L "$bin_path" || ! -x "$bin_path" ]]; then
    echo "install.sh: installed binary missing or not a regular executable: $bin_path" >&2
    exit 1
  fi
  local canon_store canon_bin
  canon_store="$(cd "$store_releases_dir" && pwd -P)"
  canon_bin="$(cd "$(dirname "$bin_path")" && pwd -P)/$(basename "$bin_path")"
  case "$canon_bin" in
    "${canon_store}"/*) ;;
    *)
      echo "install.sh: refusing binary path outside release store: $canon_bin" >&2
      exit 1
      ;;
  esac
}

# Whether an agentic-org CLI moves the registered services to the release it installs
# (its `release install` has `--no-restart`). Help is captured first, never piped into an
# early-exit grep under pipefail.
cli_refreshes_services() {
  local help
  help="$("$1" release install --help 2>/dev/null </dev/null || true)"
  [[ "$help" == *"--no-restart"* ]]
}

# What `release install --json` (compact in a pipe) did to each registered service: one
# line with the instance, the outcome and whether it runs, then its note (the steps an
# operator takes for a refused, failed or stopped service), and the `services_warning` when
# the services could not be read. JSON strings are read with their escapes undone; control
# characters, and any `\uXXXX` escape, are shown as a space or `?`, never printed raw.
report_services() {
  LC_ALL=C awk '
    function clean(text) {
      gsub(/[[:cntrl:]]/, " ", text)
      return text
    }
    { buf = buf $0 "\n" }
    END {
      n = length(buf)
      i = 1
      count = 0
      while (i <= n) {
        c = substr(buf, i, 1)
        if (c == "\"") {
          text = ""
          i++
          while (i <= n) {
            c = substr(buf, i, 1)
            if (c == "\"") {
              break
            }
            if (c == "\\") {
              i++
              c = substr(buf, i, 1)
              if (c == "u") {
                text = text "?"
                i += 4
              } else if (c == "\"" || c == "\\" || c == "/") {
                text = text c
              } else {
                text = text " "
              }
            } else {
              text = text c
            }
            i++
          }
          count++
          kind[count] = "string"
          token[count] = clean(text)
          i++
        } else if (index("{}[]:,", c) > 0) {
          count++
          kind[count] = "mark"
          token[count] = c
          i++
        } else if (c ~ /[[:space:]]/) {
          i++
        } else {
          word = ""
          while (i <= n) {
            c = substr(buf, i, 1)
            if (index("{}[]:,\" \t\r\n", c) > 0) {
              break
            }
            word = word c
            i++
          }
          count++
          kind[count] = "word"
          token[count] = word
        }
      }
      depth = 0
      key = ""
      listing = 0
      services = 0
      warning = ""
      for (t = 1; t <= count; t++) {
        if (kind[t] == "mark") {
          if (token[t] == "{" || token[t] == "[") {
            depth++
            if (token[t] == "[" && depth == 2 && key == "services") {
              listing = 1
            } else if (token[t] == "{" && listing && depth == 3) {
              services++
              instance[services] = ""
              outcome[services] = ""
              running[services] = ""
              note[services] = ""
            }
          } else if (token[t] == "}" || token[t] == "]") {
            if (token[t] == "]" && depth == 2) {
              listing = 0
            }
            depth--
          }
          continue
        }
        if (kind[t] != "string" || t + 1 > count || kind[t + 1] != "mark" || token[t + 1] != ":") {
          continue
        }
        key = token[t]
        t += 2
        if (t > count) {
          break
        }
        if (kind[t] == "mark") {
          # An object or a list: its marks are counted above.
          t--
          continue
        }
        if (depth == 1 && key == "services_warning" && kind[t] == "string") {
          warning = token[t]
        } else if (listing && depth == 3) {
          if (key == "instance") {
            instance[services] = token[t]
          } else if (key == "outcome") {
            outcome[services] = token[t]
          } else if (key == "running") {
            running[services] = token[t]
          } else if (key == "note" && kind[t] == "string") {
            note[services] = token[t]
          }
        }
      }
      for (k = 1; k <= services; k++) {
        state = (running[k] == "true") ? "running" : "not running"
        printf "Service of instance \"%s\": %s (%s).\n", instance[k], outcome[k], state
        if (note[k] != "") {
          printf "  %s\n", note[k]
        }
      }
      if (warning != "") {
        printf "Warning: %s\n", warning
      }
    }
  ' <<< "$1" >&2
}

TRUSTED_CLI=""
if TRUSTED_CLI="$(find_trusted_installed_cli)"; then
  :
else
  TRUSTED_CLI=""
fi

# Fail closed BEFORE downloading or mutating any directories when no trust anchor is available.
if [[ -z "$TRUSTED_CLI" && -z "$SHA256_INPUT" && -z "$PUBKEY_FILE" && "$ALLOW_UNSIGNED" -ne 1 ]]; then
  echo "install.sh: cold install requires explicit out-of-band verification: pass --sha256 <file|hex> or --pubkey <publisher.pub.pem|trust-registry.json>" >&2
  exit 1
fi

OUT_OF_BAND_VERIFIED=0
ORIGINAL_BASENAME=""
if [[ -z "$ARCHIVE" ]]; then
  if [[ -z "$VERSION" ]]; then
    if [[ -n "$BASE_URL" ]]; then
      echo "install.sh: --base-url requires --version <tag>" >&2
      exit 2
    fi
    VERSION="$(resolve_latest_version "$REPO")"
    echo "Resolved latest release: ${VERSION}" >&2
  fi
  if [[ ! "$VERSION" =~ ^[A-Za-z0-9][A-Za-z0-9._-]{0,95}$ ]]; then
    echo "install.sh: invalid --version tag: $VERSION" >&2
    exit 2
  fi
  NAME="agentic-org-${VERSION}-${PLATFORM}-${ARCH}.tar.gz"
  if [[ -z "$BASE_URL" ]]; then
    BASE_URL="https://github.com/${REPO}/releases/download/${VERSION}"
  fi
  ARCHIVE="$TMP_DIR/$NAME"
  ORIGINAL_BASENAME="$NAME"
  echo "Downloading ${BASE_URL}/${NAME} ..." >&2
  download_file "${BASE_URL}/${NAME}" "$ARCHIVE"
else
  ORIGINAL_BASENAME="$(basename "$ARCHIVE")"
fi

if [[ ! -f "$ARCHIVE" || -L "$ARCHIVE" ]]; then
  echo "install.sh: archive not found or is a symlink: $ARCHIVE" >&2
  exit 1
fi

# Stage the input archive into our private 0700 TMP_DIR immediately so all hashing,
# tar inspection, and extraction operate on an immutable private copy (no TOCTOU swap).
STAGED_ARCHIVE="$TMP_DIR/release.tar.gz"
cp "$ARCHIVE" "$STAGED_ARCHIVE"
chmod 0600 "$STAGED_ARCHIVE"

# Only an explicit caller-provided --sha256 establishes out-of-band checksum trust.
if [[ -n "$SHA256_INPUT" ]]; then
  EXPECTED_SHA="$(resolve_sha256_input "$SHA256_INPUT")"
  verify_sha256 "$STAGED_ARCHIVE" "$EXPECTED_SHA"
  OUT_OF_BAND_VERIFIED=1
  echo "Verified archive SHA-256: $EXPECTED_SHA" >&2
fi

TAR_VERBOSE_FILE="$TMP_DIR/tar.verbose"
TAR_NAMES_FILE="$TMP_DIR/tar.names"
validate_tar_entries "$STAGED_ARCHIVE" "$TAR_VERBOSE_FILE" "$TAR_NAMES_FILE"

if grep -qxF "release.v1.json" "$TAR_NAMES_FILE" \
   && grep -qxF "release.v1.sig" "$TAR_NAMES_FILE"; then
  BOOTSTRAP_DIR="$TMP_DIR/bootstrap"
  mkdir -p "$BOOTSTRAP_DIR"
  chmod 0700 "$BOOTSTRAP_DIR"

  # Always verify caller-supplied --pubkey when provided (even during upgrades).
  if [[ -n "$PUBKEY_FILE" ]]; then
    tar -xzf "$STAGED_ARCHIVE" -C "$BOOTSTRAP_DIR" release.v1.json release.v1.sig
    verify_pubkey_signature "$BOOTSTRAP_DIR/release.v1.json" "$BOOTSTRAP_DIR/release.v1.sig" "$PUBKEY_FILE"
    echo "Verified Ed25519 manifest signature." >&2
  fi

  if [[ -z "$TRUSTED_CLI" && "$OUT_OF_BAND_VERIFIED" -ne 1 && -z "$PUBKEY_FILE" ]]; then
    echo "install.sh: cold install requires explicit out-of-band verification: pass --sha256 <file|hex> or --pubkey <publisher.pub.pem|trust-registry.json>" >&2
    exit 1
  fi

  echo "Installing signed release bundle into protected release store ..." >&2
  mkdir -p "$BIN_DIR"
  if [[ -z "$STORE_DIR" ]]; then
    if [[ "$PLATFORM" == "linux" ]]; then
      mkdir -p "${HOME}/.local/share/agentic-org"
      chmod go-w "${HOME}/.local" "${HOME}/.local/share" 2>/dev/null || true
      chmod 0700 "${HOME}/.local/share/agentic-org"
    elif [[ "$PLATFORM" == "macos" ]]; then
      mkdir -p "${HOME}/Library/Application Support/agentic.org"
      chmod 0700 "${HOME}/Library/Application Support/agentic.org"
    fi
  else
    mkdir -p "$(dirname "$STORE_DIR")"
  fi

  INSTALL_ARGS=(release install "$STAGED_ARCHIVE" --json)
  if [[ -n "$STORE_DIR" ]]; then
    INSTALL_ARGS+=(--store "$STORE_DIR")
  fi

  # On upgrades, always use the existing trusted installed binary — never execute the untrusted archive.
  if [[ -n "$TRUSTED_CLI" ]]; then
    echo "Using existing trusted installer: $TRUSTED_CLI" >&2
    INSTALLER_CLI="$TRUSTED_CLI"
  else
    tar -xzf "$STAGED_ARCHIVE" -C "$BOOTSTRAP_DIR" agentic-org
    if [[ ! -f "$BOOTSTRAP_DIR/agentic-org" || -L "$BOOTSTRAP_DIR/agentic-org" ]]; then
      echo "install.sh: extracted agentic-org is not a regular file" >&2
      exit 1
    fi
    if [[ -n "$PUBKEY_FILE" ]]; then
      EXPECTED_CLI_SHA="$(lookup_manifest_file_sha256 "$BOOTSTRAP_DIR/release.v1.json" "agentic-org")"
      verify_sha256 "$BOOTSTRAP_DIR/agentic-org" "$EXPECTED_CLI_SHA"
    fi
    chmod 0700 "$BOOTSTRAP_DIR/agentic-org"
    INSTALLER_CLI="$BOOTSTRAP_DIR/agentic-org"
  fi

  # A CLI that moves the registered services to the new release does it within its install
  # (a running service holds the store, so it is stopped for the install and started again).
  # An older installed CLI does not: it needs those services stopped first, and the newly
  # installed release, verified into the protected store, then moves them below.
  RUN_ARGS=("${INSTALL_ARGS[@]}")
  INSTALLER_REFRESHES=0
  if cli_refreshes_services "$INSTALLER_CLI"; then
    INSTALLER_REFRESHES=1
    if [[ "$NO_RESTART" -eq 1 ]]; then
      RUN_ARGS+=(--no-restart)
    fi
  fi
  INSTALL_JSON="$("$INSTALLER_CLI" "${RUN_ARGS[@]}" </dev/null)"

  RELEASE_ID="$(extract_json_string "$INSTALL_JSON" "release_id")"
  if [[ ! "$RELEASE_ID" =~ ^[A-Za-z0-9][A-Za-z0-9._-]{0,95}$ ]]; then
    echo "install.sh: installer returned invalid release_id" >&2
    exit 1
  fi
  INSTALLED_CLI="${EFFECTIVE_STORE_DIR}/releases/${RELEASE_ID}/agentic-org"
  INSTALLED_AGENT_OS="${EFFECTIVE_STORE_DIR}/releases/${RELEASE_ID}/agent-os"
  verify_installed_binary_in_store "$INSTALLED_CLI" "${EFFECTIVE_STORE_DIR}/releases"
  verify_installed_binary_in_store "$INSTALLED_AGENT_OS" "${EFFECTIVE_STORE_DIR}/releases"
  ln -sfn "$INSTALLED_CLI" "$BIN_DIR/agentic-org"
  ln -sfn "$INSTALLED_AGENT_OS" "$BIN_DIR/agent-os"
  echo "Pinned release ${RELEASE_ID} in ${EFFECTIVE_STORE_DIR}." >&2
  if [[ "$INSTALLER_REFRESHES" -eq 1 ]]; then
    report_services "$INSTALL_JSON"
  elif cli_refreshes_services "$INSTALLED_CLI"; then
    # The release is pinned already (an install of the same release changes nothing); this
    # run moves the stopped services to it and reports them in its own words.
    REFRESH_ARGS=(release install "$STAGED_ARCHIVE")
    if [[ -n "$STORE_DIR" ]]; then
      REFRESH_ARGS+=(--store "$STORE_DIR")
    fi
    if [[ "$NO_RESTART" -eq 1 ]]; then
      REFRESH_ARGS+=(--no-restart)
    fi
    echo "Moving registered services to release ${RELEASE_ID} ..." >&2
    if ! "$INSTALLED_CLI" "${REFRESH_ARGS[@]}" </dev/null >&2; then
      echo "install.sh: release ${RELEASE_ID} is pinned, but its registered services were not moved to it; stop them (agentic-org service stop --yes) and run: agentic-org release install <archive>" >&2
    fi
  fi
else
  if [[ "$ALLOW_UNSIGNED" -ne 1 ]]; then
    echo "install.sh: refusing unsigned archive (pass --allow-unsigned only for unsigned local archives)" >&2
    exit 1
  fi
  mkdir -p "$BIN_DIR"
  BUNDLE_NAME="${ORIGINAL_BASENAME%.tar.gz}"
  if [[ ! "$BUNDLE_NAME" =~ ^[A-Za-z0-9][A-Za-z0-9._-]{0,95}$ ]]; then
    echo "install.sh: unsafe archive filename: $BUNDLE_NAME" >&2
    exit 1
  fi
  DEST_DIR="${HOME}/.local/lib/agentic-org/${BUNDLE_NAME}"
  rm -rf "$DEST_DIR"
  mkdir -p "$DEST_DIR"
  chmod 0700 "$DEST_DIR"
  tar -xzf "$STAGED_ARCHIVE" -C "$DEST_DIR"
  for bin in agentic-org agentic-org-office agentic-org-web agent-os; do
    if [[ ! -f "$DEST_DIR/$bin" || -L "$DEST_DIR/$bin" ]]; then
      echo "install.sh: missing or non-regular binary in archive: $bin" >&2
      exit 1
    fi
    chmod 0700 "$DEST_DIR/$bin"
  done
  ln -sfn "$DEST_DIR/agentic-org" "$BIN_DIR/agentic-org"
  ln -sfn "$DEST_DIR/agent-os" "$BIN_DIR/agent-os"
fi

echo "Installed:" >&2
"$BIN_DIR/agentic-org" --version >&2
"$BIN_DIR/agent-os" --version >&2
case ":${PATH}:" in
  *":${BIN_DIR}:"*) ;;
  *)
    echo "NOTE: Add ${BIN_DIR} to your PATH (e.g., export PATH=\"${BIN_DIR}:\$PATH\")." >&2
    ;;
esac

maybe_handle_linux_bwrap_apparmor
