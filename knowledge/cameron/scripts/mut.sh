#!/bin/bash
. /home/ubuntu/gate-env.sh
export CARGO_TARGET_DIR=/home/ubuntu/gate/target-mut
export CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_RUNNER="timeout 120"
cd /home/ubuntu/gate/wtMut; L=/home/ubuntu/gate/logs/mut; mkdir -p $L; : > $L/summary.txt
run() { name=$1; shift; s=$(date +%s); cargo mutants --in-place --no-shuffle --timeout 120 --build-timeout 600 --output $L/$name "$@" > $L/$name.log 2>&1; rc=$?; e=$(date +%s); echo "$name | rc=$rc | wall=$((e-s))s | $*" >> $L/summary.txt; }
run orgdocs -p org-docs
run activity_org -p org-office --file crates/org-office/src/store/org/activity.rs -- --lib
run activity_work -p org-office --file crates/org-office/src/store/work/activity.rs -- --lib
run startup -p org-server --file apps/org-server/src/startup.rs -- --lib
echo DONE >> $L/summary.txt
