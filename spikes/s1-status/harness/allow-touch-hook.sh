#!/bin/sh
# A settings PermissionRequest hook that answers allow (stands for "another hook answered").
cat > /dev/null
echo '{"hookSpecificOutput":{"hookEventName":"PermissionRequest","decision":{"behavior":"allow"}}}'
