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

Options:
  --archive <path>       Local .tar.gz release archive to install
  --sha256 <file|hex>    Expected archive SHA-256 (file or 64-char hex digest)
  --pubkey <path>        Trusted publisher Ed25519 public key PEM or trust-registry JSON
  --version <tag>        Release tag to download (defaults to latest GitHub Release)
  --repo <owner/repo>    GitHub repository (default: vgmello/agentic-org)
  --base-url <url>       Custom base URL for archive downloads (requires --version)
  --bin-dir <dir>        Directory for CLI symlinks (default: ~/.local/bin)
  --store <dir>          Explicit release store directory
  --allow-unsigned       Allow installing an unsigned local archive
  -h, --help             Show this help
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
BASE_URL=""
BIN_DIR="${HOME}/.local/bin"
STORE_DIR=""
ALLOW_UNSIGNED=0

while [[ $# -gt 0 ]]; do
  case "$1" in
    --archive) require_arg "$@"; ARCHIVE="$2"; shift 2 ;;
    --sha256) require_arg "$@"; SHA256_INPUT="$2"; shift 2 ;;
    --pubkey) require_arg "$@"; PUBKEY_FILE="$2"; shift 2 ;;
    --version) require_arg "$@"; VERSION="$2"; shift 2 ;;
    --repo) require_arg "$@"; REPO="$2"; shift 2 ;;
    --base-url) require_arg "$@"; BASE_URL="$2"; shift 2 ;;
    --bin-dir) require_arg "$@"; BIN_DIR="$2"; shift 2 ;;
    --store) require_arg "$@"; STORE_DIR="$2"; shift 2 ;;
    --allow-unsigned) ALLOW_UNSIGNED=1; shift ;;
    -h|--help)
      usage
      exit 0
      ;;
    *) echo "install.sh: unknown argument: $1" >&2; exit 2 ;;
  esac
done

require_cmd tar
require_cmd awk
require_cmd sed
if ! command -v sha256sum >/dev/null 2>&1 && ! command -v shasum >/dev/null 2>&1; then
  echo "install.sh: required command not found: sha256sum or shasum" >&2
  exit 1
fi

download_file() {
  local url="$1"
  local dest="$2"
  if command -v curl >/dev/null 2>&1; then
    curl -fsSL "$url" -o "$dest"
  elif command -v wget >/dev/null 2>&1; then
    wget -qO "$dest" "$url"
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
    effective_url="$(curl -fsSL -o /dev/null -w '%{url_effective}' "$latest_url")" || {
      echo "install.sh: failed to query latest release for ${repo}" >&2
      exit 1
    }
  elif command -v wget >/dev/null 2>&1; then
    effective_url="$(wget --max-redirect=5 --spider "$latest_url" 2>&1 | awk '/^Location: / { loc = $2 } END { gsub(/\r/, "", loc); print loc }')" || true
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

UNAME_S="$(uname -s)"
UNAME_M="$(uname -m)"
case "$UNAME_S" in
  Linux) PLATFORM="linux" ;;
  Darwin) PLATFORM="macos" ;;
  *) echo "install.sh: unsupported OS: $UNAME_S" >&2; exit 1 ;;
esac
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
trap 'rm -rf "$TMP_DIR"' EXIT

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
  require_cmd xxd
  local spki_der="$TMP_DIR/publisher.pub.der"
  printf '302a300506032b6570032100%s' "$pub_hex" | xxd -r -p > "$spki_der"
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
mkdir -p "$BIN_DIR"

if grep -qxF "release.v1.json" "$TAR_NAMES_FILE" \
   && grep -qxF "release.v1.sig" "$TAR_NAMES_FILE"; then
  echo "Installing signed release bundle into protected release store ..." >&2
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
  TRUSTED_CLI=""
  if TRUSTED_CLI="$(find_trusted_installed_cli)"; then
    echo "Using existing trusted installer: $TRUSTED_CLI" >&2
    INSTALL_JSON="$("$TRUSTED_CLI" "${INSTALL_ARGS[@]}")"
  else
    # Cold bootstrap requires explicit out-of-band trust (--sha256 or --pubkey).
    if [[ "$OUT_OF_BAND_VERIFIED" -ne 1 && -z "$PUBKEY_FILE" ]]; then
      echo "install.sh: cold install requires explicit out-of-band verification: pass --sha256 <file|hex> or --pubkey <publisher.pub.pem|trust-registry.json>" >&2
      exit 1
    fi
    BOOTSTRAP_DIR="$TMP_DIR/bootstrap"
    mkdir -p "$BOOTSTRAP_DIR"
    chmod 0700 "$BOOTSTRAP_DIR"
    if [[ -n "$PUBKEY_FILE" ]]; then
      tar -xzf "$STAGED_ARCHIVE" -C "$BOOTSTRAP_DIR" release.v1.json release.v1.sig
      verify_pubkey_signature "$BOOTSTRAP_DIR/release.v1.json" "$BOOTSTRAP_DIR/release.v1.sig" "$PUBKEY_FILE"
      echo "Verified Ed25519 manifest signature." >&2
    fi
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
    INSTALL_JSON="$("$BOOTSTRAP_DIR/agentic-org" "${INSTALL_ARGS[@]}")"
  fi

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
else
  if [[ "$ALLOW_UNSIGNED" -ne 1 ]]; then
    echo "install.sh: refusing unsigned archive (pass --allow-unsigned only for unsigned local archives)" >&2
    exit 1
  fi
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
