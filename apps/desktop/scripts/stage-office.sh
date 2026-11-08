#!/usr/bin/env bash
# Build the office host and the office server and put them where `tauri build` picks them up
# (src-tauri/office-bin/, named in tauri.conf.json under bundle.macOS.files). Run from anywhere.
set -euo pipefail

here="$(cd "$(dirname "$0")" && pwd)"
workspace="$(cd "$here/../../.." && pwd)"
out="$here/../src-tauri/office-bin"

cd "$workspace"
for package in org-office-host org-office-server; do
  if ! cargo metadata --no-deps --format-version 1 | grep -q "\"name\":\"$package\""; then
    echo "stage-office: $package isn't in this workspace. The host lives on river/office-host until it is merged." >&2
    exit 1
  fi
done

cargo build --release -p org-office-host -p org-office-server

rm -rf "$out"
mkdir -p "$out"
cp "$workspace/target/release/org-office-host" "$workspace/target/release/org-office-server" "$out/"
echo "stage-office: staged in $out"
