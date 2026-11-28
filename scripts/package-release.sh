#!/usr/bin/env bash
# Reproducible Agentic Org release packager.
set -euo pipefail

usage() {
  cat <<'EOF'
Reproducible Agentic Org release packager

Usage:
  scripts/package-release.sh \
    --release-id v0.1.0 \
    --sequence 1 \
    [--min-sequence 1] \
    --out-dir dist \
    [--profile release|dev] \
    [--signing-key /path/to/publisher.key.pem] \
    [--trust-registry /path/to/release-trust-registry.json] \
    [--ui-dir /path/to/built/ui] \
    [--skip-build] \
    [--deb]

After packaging signed per-platform bundles, run `scripts/generate-release-pointer.sh`
to emit and sign `release-pointer.v1.json` + `release-pointer.v1.sig` for pull-verified
auto-updates (`agentic-org release check` / `agentic-org release update`).
EOF
}

require_arg() {
  if [[ $# -lt 2 || -z "${2:-}" ]]; then
    echo "package-release.sh: option $1 requires a value" >&2
    exit 2
  fi
}

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
RELEASE_ID=""
SEQUENCE="1"
MIN_SEQUENCE="${ORG_RELEASE_MIN_SEQUENCE:-1}"
OUT_DIR="$ROOT_DIR/dist"
PROFILE="release"
SIGNING_KEY=""
TRUST_REGISTRY=""
UI_DIR="$ROOT_DIR/apps/agentic-org-ui/build"
UI_DIR_EXPLICIT=0
SKIP_BUILD=0
BUILD_DEB=0

while [[ $# -gt 0 ]]; do
  case "$1" in
    --release-id) require_arg "$@"; RELEASE_ID="$2"; shift 2 ;;
    --sequence) require_arg "$@"; SEQUENCE="$2"; shift 2 ;;
    --min-sequence) require_arg "$@"; MIN_SEQUENCE="$2"; shift 2 ;;
    --out-dir) require_arg "$@"; OUT_DIR="$2"; shift 2 ;;
    --profile) require_arg "$@"; PROFILE="$2"; shift 2 ;;
    --signing-key) require_arg "$@"; SIGNING_KEY="$2"; shift 2 ;;
    --trust-registry) require_arg "$@"; TRUST_REGISTRY="$2"; shift 2 ;;
    --ui-dir) require_arg "$@"; UI_DIR="$2"; UI_DIR_EXPLICIT=1; shift 2 ;;
    --skip-build) SKIP_BUILD=1; shift ;;
    --deb) BUILD_DEB=1; shift ;;
    -h|--help)
      usage
      exit 0
      ;;
    *) echo "package-release.sh: unknown argument: $1" >&2; exit 2 ;;
  esac
done

if [[ "$PROFILE" != "release" && "$PROFILE" != "dev" ]]; then
  echo "package-release.sh: --profile must be 'release' or 'dev' (got '$PROFILE')" >&2
  exit 2
fi

if [[ "$BUILD_DEB" -eq 1 && "$PROFILE" == "release" ]]; then
  echo "package-release.sh: --deb is only supported with --profile dev (signed Linux releases install via agentic-org release install)" >&2
  exit 2
fi

if [[ -z "$RELEASE_ID" ]]; then
  SHORT_SHA="$(git -C "$ROOT_DIR" rev-parse --short HEAD 2>/dev/null || echo dev)"
  RELEASE_ID="0.1.0-${SHORT_SHA}"
fi

UNAME_S="$(uname -s)"
UNAME_M="$(uname -m)"
case "$UNAME_S" in
  Linux) PLATFORM="linux" ;;
  Darwin) PLATFORM="macos" ;;
  *) echo "package-release.sh: unsupported OS: $UNAME_S" >&2; exit 1 ;;
esac
case "$UNAME_M" in
  x86_64|amd64) ARCH="x86_64" ;;
  aarch64|arm64) ARCH="aarch64" ;;
  *) echo "package-release.sh: unsupported arch: $UNAME_M" >&2; exit 1 ;;
esac

if [[ ! -f "$UI_DIR/index.html" ]]; then
  if [[ "$UI_DIR_EXPLICIT" -eq 1 ]]; then
    echo "package-release.sh: --ui-dir does not contain index.html: $UI_DIR" >&2
    exit 1
  fi
  echo "Building static UI into $UI_DIR ..." >&2
  (cd "$ROOT_DIR/apps/agentic-org-ui" && bun install --frozen-lockfile && bun run build)
fi

if [[ ! "$SEQUENCE" =~ ^[1-9][0-9]*$ ]]; then
  echo "package-release.sh: --sequence must be a positive integer (got '$SEQUENCE')" >&2
  exit 2
fi
if [[ ! "$MIN_SEQUENCE" =~ ^[1-9][0-9]*$ ]] || (( MIN_SEQUENCE > SEQUENCE )); then
  echo "package-release.sh: --min-sequence must be an integer in 1..=$SEQUENCE (got '$MIN_SEQUENCE')" >&2
  exit 2
fi

if [[ "$PROFILE" == "release" ]]; then
  if [[ -z "$SIGNING_KEY" || -z "$TRUST_REGISTRY" ]]; then
    echo "package-release.sh: --profile release requires both --signing-key and --trust-registry" >&2
    exit 2
  fi
  BIN_DIR="$ROOT_DIR/target/release"
  if [[ "$SKIP_BUILD" -eq 0 ]]; then
    echo "Building release binaries with embedded trust registry (min_sequence=$MIN_SEQUENCE) ..." >&2
    ORG_RELEASE_TRUST_REGISTRY="$(cd "$(dirname "$TRUST_REGISTRY")" && pwd)/$(basename "$TRUST_REGISTRY")" \
    ORG_RELEASE_MIN_SEQUENCE="$MIN_SEQUENCE" \
      cargo build --locked --manifest-path "$ROOT_DIR/Cargo.toml" --release \
      -p agentic-org -p agentic-org-office -p agentic-org-web -p agent-os
  fi
else
  BIN_DIR="$ROOT_DIR/target/debug"
  if [[ "$SKIP_BUILD" -eq 0 ]]; then
    echo "Building development binaries ..." >&2
    cargo build --locked --manifest-path "$ROOT_DIR/Cargo.toml" \
      -p agentic-org -p agentic-org-office -p agentic-org-web -p agent-os
  fi
fi

EXTRA_ARGS=()
if [[ -n "$SIGNING_KEY" && "$PROFILE" == "release" ]]; then
  EXTRA_ARGS+=(--signing-key "$SIGNING_KEY")
fi
if [[ "$BUILD_DEB" -eq 1 ]]; then
  EXTRA_ARGS+=(--deb)
fi

PYTHONDONTWRITEBYTECODE=1 python3 "$ROOT_DIR/scripts/release_tool.py" package \
  --bin-dir "$BIN_DIR" \
  --ui-dir "$UI_DIR" \
  --release-id "$RELEASE_ID" \
  --sequence "$SEQUENCE" \
  --platform "$PLATFORM" \
  --arch "$ARCH" \
  --out-dir "$OUT_DIR" \
  ${EXTRA_ARGS[@]+"${EXTRA_ARGS[@]}"}
