#!/bin/bash
. /home/ubuntu/gate-env.sh
export CARGO_TARGET_DIR=/home/ubuntu/gate/target-fix CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_RUNNER="timeout 120"
cd /home/ubuntu/gate/wtB1
nice -n 19 cargo mutants --in-place --no-shuffle --timeout 120 --build-timeout 600 --in-diff /home/ubuntu/gate/sliceB.diff --output /home/ubuntu/gate/logs/mutB -p org-office -- --lib 2>&1 | tail -25
