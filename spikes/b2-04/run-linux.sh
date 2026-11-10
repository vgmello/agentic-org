#!/usr/bin/env bash
set -euo pipefail
if [[ ${B204_DISPOSABLE_VM:-} != 1 ]]; then
    echo 'Refusing persistent service/provider changes: set B204_DISPOSABLE_VM=1 only in a disposable VM' >&2
    exit 2
fi
spike_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
repo_dir=$(git -C "$spike_dir" rev-parse --show-toplevel)
cd "$repo_dir"
export CARGO_BUILD_JOBS=2
export CARGO_TARGET_DIR=${CARGO_TARGET_DIR:-"$repo_dir/target"}
mkdir -p "$spike_dir/linux-bin" "$spike_dir/evidence"
{ uname -a; systemd-creds --version; getconf GNU_LIBC_VERSION; } | tee "$spike_dir/evidence/environment.log"
cargo build --locked -p org-office-host -p org-office-server 2>&1 | tee "$spike_dir/evidence/build.log"
cp "$CARGO_TARGET_DIR/debug/org-office-host" "$CARGO_TARGET_DIR/debug/org-office-server" "$spike_dir/linux-bin/"
cc -Wall -Wextra -Werror "$spike_dir/exec-fixture.c" -o "$spike_dir/linux-bin/exec-fixture"
cargo test --locked -p org-office-server 2>&1 | tee "$spike_dir/evidence/worker-tests.log"
timeout 20s python3 -I "$spike_dir/linux-exec.py" | tee "$spike_dir/evidence/exec.log"
timeout 80s python3 -I "$spike_dir/linux-worker.py" | tee "$spike_dir/evidence/worker.log"
timeout 60s python3 -I "$spike_dir/linux-host-baseline.py" | tee "$spike_dir/evidence/host-baseline.log"
# Only in a disposable VM with sudo. Enables the prerequisites being evaluated.
sudo systemctl start systemd-creds.socket
sudo systemd-creds setup
probe_uid=$(id -u)
sudo loginctl enable-linger "$(id -un)"
sudo systemctl start "user@$probe_uid.service"
env XDG_RUNTIME_DIR="/run/user/$probe_uid" DBUS_SESSION_BUS_ADDRESS="unix:path=/run/user/$probe_uid/bus" python3 -I "$spike_dir/linux-credentials.py" | tee "$spike_dir/evidence/credentials.log"
sh "$spike_dir/build-crypto.sh" 2>&1 | tee "$spike_dir/evidence/crypto.log"
