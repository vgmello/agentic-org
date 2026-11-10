#!/usr/bin/env bash
# Build the office host and the office server and put them where `tauri build` picks them up
# (src-tauri/office-bin/, named in tauri.conf.json under bundle.macOS.files). Run from anywhere.
set -euo pipefail

here="$(cd "$(dirname "$0")" && pwd)"
workspace="$(cd "$here/../../.." && pwd)"
out="$here/../src-tauri/office-bin"

cd "$workspace"
for package in org-office-host org-server; do
  if ! cargo metadata --no-deps --format-version 1 | grep -q "\"name\":\"$package\""; then
    echo "stage-office: $package isn't in this workspace (apps/$package on v1). Update your branch from origin/v1." >&2
    exit 1
  fi
done

# NOTE: until a signed build exists (Developer ID, the keychain access group and entitlement the host needs for its key custody),
# the bundled release host refuses to start. Development runs the host by hand instead: see docs/runbooks/local-hq.md.
cargo build --release -p org-office-host -p org-server

rm -rf "$out"
mkdir -p "$out"
cp "$workspace/target/release/org-office-host" "$workspace/target/release/org-server" "$out/"
echo "stage-office: staged in $out"
