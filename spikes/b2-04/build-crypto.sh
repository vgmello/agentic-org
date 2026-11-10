#!/bin/sh
set -eu
spike_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
bin_dir=${B204_BIN_DIR:-"$spike_dir/linux-bin"}
mkdir -p "$bin_dir"
cd "$spike_dir/crypto"
export CARGO_TARGET_DIR=${CARGO_TARGET_DIR:-"$spike_dir/target"}
export CARGO_BUILD_JOBS=2
cargo build --locked --release
cp "$CARGO_TARGET_DIR/release/sage-b204-sqlcipher" "$bin_dir/sqlcipher-system"
ldd "$bin_dir/sqlcipher-system"
probe_dir=$(mktemp -d /tmp/sage-b204-sqlcipher-XXXXXX)
trap 'rm -rf "$probe_dir"' EXIT
"$bin_dir/sqlcipher-system" "$probe_dir/fixture.db"
cargo build --locked --release --features vendored
cp "$CARGO_TARGET_DIR/release/sage-b204-sqlcipher" "$bin_dir/sqlcipher-vendored"
ldd "$bin_dir/sqlcipher-vendored"
readelf -d "$bin_dir/sqlcipher-vendored" > "$probe_dir/dynamic-dependencies.txt"
if grep -E 'NEEDED.*(libcrypto|libssl|libsqlite|libsqlcipher)' "$probe_dir/dynamic-dependencies.txt"; then
    echo 'FAIL unwanted dynamic crypto or database dependency'
    exit 1
fi
"$bin_dir/sqlcipher-vendored" "$probe_dir/fixture.db"
echo 'PASS system-created SQLCipher fixture reopened by vendored crypto build'
rm "$probe_dir/fixture.db"
"$bin_dir/sqlcipher-vendored" "$probe_dir/fixture.db"
"$bin_dir/sqlcipher-system" "$probe_dir/fixture.db"
echo 'PASS vendored-created fixture reopened by system crypto build'
ls -l "$bin_dir/sqlcipher-system" "$bin_dir/sqlcipher-vendored"
