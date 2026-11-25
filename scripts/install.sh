#!/usr/bin/env bash
# One-command Agentic Org prebuilt release installer (TASK-121, ADR-0009).
# Zero Rust, Node, or C compiler prerequisites on target hosts.
#
# Usage:
#   # Install from a local prebuilt archive:
#   scripts/install.sh --archive /path/to/agentic-org-<id>-linux-x86_64.tar.gz [--sha256 <file>] [--bin-dir ~/.local/bin]
#
#   # Install from a GitHub Release tag (or custom --base-url):
#   scripts/install.sh --version v0.1.0 [--repo vgmello/agentic-org] [--bin-dir ~/.local/bin]
set -euo pipefail

ARCHIVE=""
SHA256_FILE=""
VERSION=""
REPO="vgmello/agentic-org"
BASE_URL=""
BIN_DIR="${HOME}/.local/bin"
STORE_DIR=""

while [[ $# -gt 0 ]]; do
  case "$1" in
    --archive) ARCHIVE="$2"; shift 2 ;;
    --sha256) SHA256_FILE="$2"; shift 2 ;;
    --version) VERSION="$2"; shift 2 ;;
    --repo) REPO="$2"; shift 2 ;;
    --base-url) BASE_URL="$2"; shift 2 ;;
    --bin-dir) BIN_DIR="$2"; shift 2 ;;
    --store) STORE_DIR="$2"; shift 2 ;;
    -h|--help)
      sed -n '2,12p' "$0"
      exit 0
      ;;
    *) echo "install.sh: unknown argument: $1" >&2; exit 2 ;;
  esac
done

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

TMP_DIR="$(mktemp -d "${TMPDIR:-/tmp}/agentic-org-install.XXXXXX")"
trap 'rm -rf "$TMP_DIR"' EXIT

verify_sha256() {
  local file="$1"
  local expected="$2"
  local actual=""
  if command -v sha256sum >/dev/null 2>&1; then
    actual="$(sha256sum "$file" | awk '{print $1}')"
  elif command -v shasum >/dev/null 2>&1; then
    actual="$(shasum -a 256 "$file" | awk '{print $1}')"
  else
    echo "install.sh: neither sha256sum nor shasum found" >&2
    exit 1
  fi
  if [[ "$actual" != "$expected" ]]; then
    echo "install.sh: SHA-256 mismatch for $(basename "$file") (expected $expected, got $actual)" >&2
    exit 1
  fi
}

if [[ -z "$ARCHIVE" ]]; then
  if [[ -z "$VERSION" ]]; then
    echo "install.sh: pass --archive <path.tar.gz> or --version <tag>" >&2
    exit 2
  fi
  NAME="agentic-org-${VERSION}-${PLATFORM}-${ARCH}.tar.gz"
  if [[ -z "$BASE_URL" ]]; then
    BASE_URL="https://github.com/${REPO}/releases/download/${VERSION}"
  fi
  ARCHIVE="$TMP_DIR/$NAME"
  SHA256_FILE="$TMP_DIR/${NAME}.sha256"
  echo "Downloading ${BASE_URL}/${NAME} ..." >&2
  curl -fsSL "${BASE_URL}/${NAME}" -o "$ARCHIVE"
  curl -fsSL "${BASE_URL}/${NAME}.sha256" -o "$SHA256_FILE"
fi

if [[ ! -f "$ARCHIVE" ]]; then
  echo "install.sh: archive not found: $ARCHIVE" >&2
  exit 1
fi

if [[ -z "$SHA256_FILE" && -f "${ARCHIVE}.sha256" ]]; then
  SHA256_FILE="${ARCHIVE}.sha256"
fi
if [[ -n "$SHA256_FILE" ]]; then
  EXPECTED_SHA="$(awk '{print $1; exit}' "$SHA256_FILE")"
  verify_sha256 "$ARCHIVE" "$EXPECTED_SHA"
  echo "Verified archive SHA-256: $EXPECTED_SHA" >&2
fi

mkdir -p "$BIN_DIR"

# Detect signed ADR-0009 release bundle vs dev bundle by inspecting archive entries.
if tar -tzf "$ARCHIVE" | grep -qx "release.v1.json"; then
  echo "Detected signed ADR-0009 release bundle; installing into protected release store ..." >&2
  BOOTSTRAP_DIR="$TMP_DIR/bootstrap"
  mkdir -p "$BOOTSTRAP_DIR"
  tar -xzf "$ARCHIVE" -C "$BOOTSTRAP_DIR" agentic-org
  chmod 0700 "$BOOTSTRAP_DIR/agentic-org"
  INSTALL_ARGS=(release install "$ARCHIVE" --json)
  if [[ -n "$STORE_DIR" ]]; then
    INSTALL_ARGS+=(--store "$STORE_DIR")
  fi
  INSTALL_JSON="$("$BOOTSTRAP_DIR/agentic-org" "${INSTALL_ARGS[@]}")"
  echo "$INSTALL_JSON" >&2
  INSTALLED_CLI="$(printf '%s' "$INSTALL_JSON" | python3 -c 'import sys,json; print(json.load(sys.stdin)["agentic_org"])')"
  INSTALLED_AGENT_OS="$(printf '%s' "$INSTALL_JSON" | python3 -c 'import sys,json; print(json.load(sys.stdin)["agent_os"])')"
  ln -sfn "$INSTALLED_CLI" "$BIN_DIR/agentic-org"
  ln -sfn "$INSTALLED_AGENT_OS" "$BIN_DIR/agent-os"
else
  echo "Detected unsigned development bundle; extracting sibling layout ..." >&2
  BUNDLE_NAME="$(basename "$ARCHIVE" .tar.gz)"
  DEST_DIR="${HOME}/.local/lib/agentic-org/${BUNDLE_NAME}"
  rm -rf "$DEST_DIR"
  mkdir -p "$DEST_DIR"
  chmod 0700 "$DEST_DIR"
  tar -xzf "$ARCHIVE" -C "$DEST_DIR"
  for bin in agentic-org agentic-org-office agentic-org-web agent-os; do
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
