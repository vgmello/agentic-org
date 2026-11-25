#!/usr/bin/env bash
# Agentic Org prebuilt release installer.
# Verifies release bundles before installation without compiling from source.
set -euo pipefail

usage() {
  cat <<'EOF'
Agentic Org Installer

Usage:
  # Install or upgrade from a local release archive:
  install.sh --archive <path.tar.gz> [--sha256 <file|hex>] [--pubkey <file>] [--bin-dir ~/.local/bin] [--store <dir>]

  # Install or upgrade from a GitHub Release tag (or custom --base-url):
  install.sh --version <tag> --sha256 <file|hex> [--repo vgmello/agentic-org] [--base-url <url>] [--bin-dir ~/.local/bin] [--store <dir>]
  install.sh --version <tag> --pubkey <publisher.pub.pem|trust-registry.json> [--bin-dir ~/.local/bin]

  # Install an unsigned development archive (explicit opt-in only):
  install.sh --archive <path-dev.tar.gz> --allow-unsigned [--bin-dir ~/.local/bin]

Options:
  --archive <path>       Local .tar.gz release archive to install
  --sha256 <file|hex>    Expected archive SHA-256 (file or 64-char hex digest)
  --pubkey <path>        Trusted publisher Ed25519 public key PEM or trust-registry JSON
  --version <tag>        Release tag to download (e.g. v0.1.0)
  --repo <owner/repo>    GitHub repository (default: vgmello/agentic-org)
  --base-url <url>       Custom base URL for archive downloads
  --bin-dir <dir>        Directory for CLI symlinks (default: ~/.local/bin)
  --store <dir>          Explicit release store directory
  --allow-unsigned       Allow installing an unsigned development archive
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
  local verbose_listing names_listing bad_type bad_path
  verbose_listing="$(tar -tvzf "$archive")"
  names_listing="$(tar -tzf "$archive")"
  if [[ -z "$verbose_listing" || -z "$names_listing" ]]; then
    echo "install.sh: archive is empty" >&2
    exit 1
  fi
  # 1. Reject symlinks, hardlinks, devices, FIFOs, and directory entries (regular files only).
  bad_type="$(printf '%s\n' "$verbose_listing" | awk 'substr($1, 1, 1) != "-" { print; exit }')"
  if [[ -n "$bad_type" ]]; then
    echo "install.sh: refusing non-regular archive entry: $bad_type" >&2
    exit 1
  fi
  # 2. Validate full unsplit entry paths from `tar -tzf` (immune to whitespace/$NF tricks):
  #    reject leading `/`, `..` path segments, whitespace/control chars, backslashes, and duplicates.
  bad_path="$(printf '%s\n' "$names_listing" | awk '
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
  ')"
  if [[ -n "$bad_path" ]]; then
    echo "install.sh: refusing unsafe archive path: $bad_path" >&2
    exit 1
  fi
}

extract_json_string() {
  local json_text="$1"
  local key="$2"
  printf '%s\n' "$json_text" | sed -n "s/.*\"${key}\"[[:space:]]*:[[:space:]]*\"\([^\"]*\)\".*/\1/p" | head -n 1
}

extract_json_uint() {
  local json_text="$1"
  local key="$2"
  printf '%s\n' "$json_text" | sed -n "s/.*\"${key}\"[[:space:]]*:[[:space:]]*\([0-9][0-9]*\).*/\1/p" | head -n 1
}

verify_pubkey_signature() {
  local manifest_file="$1"
  local sig_file="$2"
  local pubkey_input="$3"
  require_cmd openssl
  if ! openssl pkeyutl -help 2>&1 | grep -q -- '-rawin'; then
    echo "install.sh: installed openssl does not support Ed25519 '-rawin' verification (e.g. stock macOS LibreSSL); install OpenSSL 3+ or pass --sha256 <file|hex>" >&2
    exit 1
  fi
  if [[ ! -f "$pubkey_input" || -L "$pubkey_input" ]]; then
    echo "install.sh: public key file not found or is a symlink: $pubkey_input" >&2
    exit 1
  fi
  local pem_file="$pubkey_input"
  if ! grep -q "BEGIN PUBLIC KEY" "$pubkey_input" 2>/dev/null; then
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
    # Exact string match (no regex interpolation) on key_id; extract public_key_hex and sequence bounds.
    reg_line="$(awk -v kid="$key_id" '
      /"key_id"[[:space:]]*:/ {
        v = $0
        sub(/.*"key_id"[[:space:]]*:[[:space:]]*"/, "", v)
        sub(/".*/, "", v)
        matched = (v == kid)
        if (matched) { pub = ""; min_s = ""; max_s = "" }
      }
      matched && /"public_key_hex"[[:space:]]*:/ {
        v = $0
        sub(/.*"public_key_hex"[[:space:]]*:[[:space:]]*"/, "", v)
        sub(/".*/, "", v)
        pub = v
      }
      matched && /"min_sequence"[[:space:]]*:/ {
        v = $0
        sub(/.*"min_sequence"[[:space:]]*:[[:space:]]*/, "", v)
        sub(/[^0-9].*/, "", v)
        min_s = v
      }
      matched && /"max_sequence"[[:space:]]*:/ {
        v = $0
        sub(/.*"max_sequence"[[:space:]]*:[[:space:]]*/, "", v)
        sub(/[^0-9].*/, "", v)
        max_s = v
      }
      matched && /\}/ {
        print pub, min_s, max_s
        exit
      }
    ' "$pubkey_input")"
    read -r pub_hex min_seq max_seq <<< "$reg_line"
    if [[ ! "${pub_hex:-}" =~ ^[0-9a-fA-F]{64}$ ]]; then
      echo "install.sh: signing key_id not found in trust registry: $pubkey_input" >&2
      exit 1
    fi
    if [[ -n "${min_seq:-}" && "$seq" -lt "$min_seq" ]]; then
      echo "install.sh: manifest sequence ($seq) below key min_sequence ($min_seq)" >&2
      exit 1
    fi
    if [[ -n "${max_seq:-}" && "$seq" -gt "$max_seq" ]]; then
      echo "install.sh: manifest sequence ($seq) exceeds key max_sequence ($max_seq)" >&2
      exit 1
    fi
    require_cmd xxd
    pem_file="$TMP_DIR/publisher.pub.pem"
    local spki_der="$TMP_DIR/publisher.pub.der"
    printf '302a300506032b6570032100%s' "$pub_hex" | xxd -r -p > "$spki_der"
    openssl pkey -pubin -inform DER -in "$spki_der" -out "$pem_file" >/dev/null 2>&1
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
    echo "install.sh: pass --archive <path.tar.gz> or --version <tag>" >&2
    exit 2
  fi
  require_cmd curl
  NAME="agentic-org-${VERSION}-${PLATFORM}-${ARCH}.tar.gz"
  if [[ -z "$BASE_URL" ]]; then
    BASE_URL="https://github.com/${REPO}/releases/download/${VERSION}"
  fi
  ARCHIVE="$TMP_DIR/$NAME"
  ORIGINAL_BASENAME="$NAME"
  echo "Downloading ${BASE_URL}/${NAME} ..." >&2
  curl -fsSL "${BASE_URL}/${NAME}" -o "$ARCHIVE"
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

validate_tar_entries "$STAGED_ARCHIVE"
mkdir -p "$BIN_DIR"

ARCHIVE_ENTRIES="$(tar -tzf "$STAGED_ARCHIVE")"
if printf '%s\n' "$ARCHIVE_ENTRIES" | grep -qx "release.v1.json" \
   && printf '%s\n' "$ARCHIVE_ENTRIES" | grep -qx "release.v1.sig"; then
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
      EXPECTED_CLI_SHA="$(awk '
        /"path"[[:space:]]*:[[:space:]]*"agentic-org"/ { in_entry=1 }
        in_entry && /"sha256"/ {
          gsub(/.*"sha256"[[:space:]]*:[[:space:]]*"/, "")
          gsub(/".*/, "")
          print
          exit
        }
      ' "$BOOTSTRAP_DIR/release.v1.json")"
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
    echo "install.sh: refusing unsigned archive (pass --allow-unsigned only for local development bundles)" >&2
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
