#!/bin/sh
set -eu
spike_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
probe_dir=$(mktemp -d /tmp/sage-b204-mac-XXXXXX)
trap 'rm -rf "$probe_dir"' EXIT
set -x
security find-identity -v -p codesigning
# The C no-prompt option is deprecated in this SDK; this is a bounded probe only.
clang -Wall -Wextra -Werror -Wno-deprecated-declarations "$spike_dir/mac-keychain.c" -framework Security -framework CoreFoundation -o "$probe_dir/helper"
codesign --force --sign - --options runtime --identifier org.agentic.sage.b204 --entitlements "$spike_dir/mac-entitlements.plist" "$probe_dir/helper"
codesign --verify --strict --verbose=2 "$probe_dir/helper"
status=0
"$probe_dir/helper" org.agentic.sage.b204.20261110 || status=$?
echo "unauthorized-entitlement helper exit=$status"
codesign --force --sign - --options runtime --identifier org.agentic.sage.b204 "$probe_dir/helper"
"$probe_dir/helper" org.agentic.sage.b204.20261110
