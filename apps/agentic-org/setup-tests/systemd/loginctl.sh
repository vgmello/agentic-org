#!/bin/sh
# A stand-in for `loginctl` in the setup tests: answers `show-user <uid> --property=Linger`
# with the content of the `linger` file next to this script, and fails without one.
here=$(dirname "$0")
printf '%s\n' "loginctl $*" >> "$here/calls"
if [ "$1" != "show-user" ] || [ "$3" != "--property=Linger" ]; then
    echo "only show-user is ever asked" >&2
    exit 2
fi
if [ ! -s "$here/linger" ]; then
    echo "Failed to get user: User ID $2 is not logged in or lingering" >&2
    exit 1
fi
printf 'Linger=%s\n' "$(cat "$here/linger")"
