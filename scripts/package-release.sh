#!/usr/bin/env bash
# Reproducible Agentic Org release packager (TASK-121, ADR-0009).
#
# Usage:
#   scripts/package-release.sh \
#     --release-id v0.1.0 \
#     --sequence 1 \
#     --out-dir dist \
#     [--profile release|dev] \
#     [--signing-key /path/to/publisher.key.pem] \
#     [--trust-registry /path/to/release-trust-registry.json] \
#     [--ui-dir /path/to/built/ui] \
#     [--skip-build] \
#     [--deb]
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
RELEASE_ID=""
SEQUENCE="1"
OUT_DIR="$ROOT_DIR/dist"
PROFILE="release"
SIGNING_KEY=""
TRUST_REGISTRY=""
UI_DIR="$ROOT_DIR/apps/agentic-org-ui/build"
SKIP_BUILD=0
BUILD_DEB=0

while [[ $# -gt 0 ]]; do
  case "$1" in
    --release-id) RELEASE_ID="$2"; shift 2 ;;
    --sequence) SEQUENCE="$2"; shift 2 ;;
    --out-dir) OUT_DIR="$2"; shift 2 ;;
    --profile) PROFILE="$2"; shift 2 ;;
    --signing-key) SIGNING_KEY="$2"; shift 2 ;;
    --trust-registry) TRUST_REGISTRY="$2"; shift 2 ;;
    --ui-dir) UI_DIR="$2"; shift 2 ;;
    --skip-build) SKIP_BUILD=1; shift ;;
    --deb) BUILD_DEB=1; shift ;;
    -h|--help)
      sed -n '2,15p' "$0"
      exit 0
      ;;
    *) echo "package-release.sh: unknown argument: $1" >&2; exit 2 ;;
  esac
done

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
  echo "Building static UI into $UI_DIR ..." >&2
  (cd "$ROOT_DIR/apps/agentic-org-ui" && bun install --frozen-lockfile && bun run build)
fi

if [[ "$PROFILE" == "release" ]]; then
  if [[ -z "$SIGNING_KEY" || -z "$TRUST_REGISTRY" ]]; then
    echo "package-release.sh: --profile release requires both --signing-key and --trust-registry" >&2
    exit 2
  fi
  BIN_DIR="$ROOT_DIR/target/release"
  if [[ "$SKIP_BUILD" -eq 0 ]]; then
    echo "Building Rust release binaries with embedded trust registry ..." >&2
    ORG_RELEASE_TRUST_REGISTRY="$(cd "$(dirname "$TRUST_REGISTRY")" && pwd)/$(basename "$TRUST_REGISTRY")" \
    ORG_RELEASE_MIN_SEQUENCE="$SEQUENCE" \
      cargo build --manifest-path "$ROOT_DIR/Cargo.toml" --release \
      -p agentic-org -p agentic-org-office -p agentic-org-web -p agent-os
  fi
else
  BIN_DIR="$ROOT_DIR/target/debug"
  if [[ "$SKIP_BUILD" -eq 0 ]]; then
    echo "Building Rust dev binaries ..." >&2
    cargo build --manifest-path "$ROOT_DIR/Cargo.toml" \
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
