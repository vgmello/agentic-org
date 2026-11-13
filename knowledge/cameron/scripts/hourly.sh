#!/bin/bash
# usage: hourly.sh <worktree> <tag>; Linux full gate: fmt, strict clippy debug+release, workspace tests
. /home/ubuntu/gate-env.sh
export CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_RUNNER="timeout 120"
export CARGO_TARGET_DIR=/home/ubuntu/gate/target-full
cd $1; L=/home/ubuntu/gate/logs; S=$L/hourly_$2.txt; : > $S
git -c safe.directory='*' rev-parse HEAD >> $S
step() { name=$1; shift; s=$(date +%s); "$@" > $L/hourly_${2}_${name}.log 2>&1; rc=$?; e=$(date +%s); echo "$name | rc=$rc | wall=$((e-s))s" >> $S; return $rc; }
step fmt cargo fmt --all --check
step clippy_debug cargo clippy --locked --workspace --all-targets -- -D warnings
step clippy_release cargo clippy --locked --release --workspace --all-targets -- -D warnings
step test cargo test --locked --workspace --no-fail-fast
echo DONE >> $S
