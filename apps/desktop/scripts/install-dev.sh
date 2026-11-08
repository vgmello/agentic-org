#!/usr/bin/env bash
# Build a development copy of the app (with the office host and server inside), sign it ad hoc and put it in
# ~/Applications, so the host's login service can be registered and approved like a real install.
# Ad-hoc signing has no stable identity: macOS asks for approval again after each rebuild, and Keychain items
# the host stores belong to that build. A release is signed with the Developer ID instead (see README).
set -euo pipefail

here="$(cd "$(dirname "$0")" && pwd)"
app_dir="$here/.."
workspace="$(cd "$app_dir/../.." && pwd)"
name="Agentic Org.app"
dest="$HOME/Applications"

"$here/stage-office.sh"
(cd "$app_dir" && bun run tauri build --bundles app)

bundle="$workspace/target/release/bundle/macos/$name"
[ -d "$bundle" ] || { echo "install-dev: $bundle wasn't built" >&2; exit 1; }

# Sign the inside first (the two binaries), then the app that contains them.
for binary in org-office-host org-office-server; do
  codesign --force --sign - "$bundle/Contents/MacOS/$binary"
done
codesign --force --sign - "$bundle"
codesign --verify --strict "$bundle"

mkdir -p "$dest"
rm -rf "${dest:?}/$name"
cp -R "$bundle" "$dest/$name"
echo "install-dev: installed $dest/$name"
echo "Open it, create an organization, then allow it in System Settings → General → Login Items."
