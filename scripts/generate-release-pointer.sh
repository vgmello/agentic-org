#!/usr/bin/env bash
# Generate and Ed25519-sign release-pointer.v1.json + release-pointer.v1.sig from signed archives.
set -euo pipefail

usage() {
  cat <<'EOF'
Agentic Org signed release pointer generator

Usage:
  scripts/generate-release-pointer.sh \
    --artifacts-dir artifacts \
    --release-id v0.1.0 \
    --sequence 1 \
    --base-url https://github.com/vgmello/agentic-org/releases/download/v0.1.0 \
    --signing-key /path/to/publisher.key.pem \
    [--trust-registry /path/to/release-trust-registry.json] \
    [--channel stable] \
    [--out-dir artifacts] \
    [--published-at 2026-11-27T23:00:00Z] \
    [--allow-local-url]
EOF
}

require_arg() {
  if [[ $# -lt 2 || -z "${2:-}" ]]; then
    echo "generate-release-pointer.sh: option $1 requires a value" >&2
    exit 2
  fi
}

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
ARTIFACTS_DIR=""
OUT_DIR=""
RELEASE_ID=""
SEQUENCE=""
CHANNEL="stable"
BASE_URL=""
PUBLISHED_AT=""
KEY_FILE=""
TRUST_REGISTRY=""
ALLOW_LOCAL_URL=0

while [[ $# -gt 0 ]]; do
  case "$1" in
    --artifacts-dir) require_arg "$@"; ARTIFACTS_DIR="$2"; shift 2 ;;
    --out-dir) require_arg "$@"; OUT_DIR="$2"; shift 2 ;;
    --release-id) require_arg "$@"; RELEASE_ID="$2"; shift 2 ;;
    --sequence) require_arg "$@"; SEQUENCE="$2"; shift 2 ;;
    --channel) require_arg "$@"; CHANNEL="$2"; shift 2 ;;
    --base-url) require_arg "$@"; BASE_URL="$2"; shift 2 ;;
    --signing-key) require_arg "$@"; KEY_FILE="$2"; shift 2 ;;
    --trust-registry) require_arg "$@"; TRUST_REGISTRY="$2"; shift 2 ;;
    --published-at) require_arg "$@"; PUBLISHED_AT="$2"; shift 2 ;;
    --allow-local-url) ALLOW_LOCAL_URL=1; shift ;;
    -h|--help)
      usage
      exit 0
      ;;
    *) echo "generate-release-pointer.sh: unknown argument: $1" >&2; exit 2 ;;
  esac
done

if [[ -z "$ARTIFACTS_DIR" || -z "$RELEASE_ID" || -z "$SEQUENCE" || -z "$BASE_URL" || -z "$KEY_FILE" ]]; then
  echo "generate-release-pointer.sh: --artifacts-dir, --release-id, --sequence, --base-url, and --signing-key are required" >&2
  usage >&2
  exit 2
fi

EXTRA_ARGS=()
if [[ -n "$OUT_DIR" ]]; then
  EXTRA_ARGS+=(--out-dir "$OUT_DIR")
fi
if [[ -n "$TRUST_REGISTRY" ]]; then
  EXTRA_ARGS+=(--trust-registry "$TRUST_REGISTRY")
fi
if [[ -n "$PUBLISHED_AT" ]]; then
  EXTRA_ARGS+=(--published-at "$PUBLISHED_AT")
fi
if [[ "$ALLOW_LOCAL_URL" -eq 1 ]]; then
  EXTRA_ARGS+=(--allow-local-url)
fi

PYTHONDONTWRITEBYTECODE=1 python3 "$ROOT_DIR/scripts/release_tool.py" pointer \
  --artifacts-dir "$ARTIFACTS_DIR" \
  --release-id "$RELEASE_ID" \
  --sequence "$SEQUENCE" \
  --channel "$CHANNEL" \
  --base-url "$BASE_URL" \
  --signing-key "$KEY_FILE" \
  ${EXTRA_ARGS[@]+"${EXTRA_ARGS[@]}"}
