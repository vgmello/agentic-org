#!/bin/sh
# A stand-in for `systemctl --user` in the setup tests: one unit, its state in files next
# to this script. It reads the unit file the adapter wrote and runs its ExecStart= with its
# Environment=, as systemd's user manager would for a Type=exec unit. The tests' values hold
# nothing a shell would expand, so the unit's double-quoted words are read with `eval`.
here=$(dirname "$0")
printf '%s\n' "systemctl $*" >> "$here/calls"
if [ -e "$here/no-bus" ]; then
    echo "Failed to connect to bus: No medium found" >&2
    exit 1
fi
if [ "$1" != "--user" ] || [ -z "$XDG_RUNTIME_DIR" ]; then
    echo "not asked for the user manager" >&2
    exit 2
fi
shift
verb=$1
shift
units=$(cat "$here/unit-directory")

running() {
    [ -s "$here/pid" ] && kill -0 "$(cat "$here/pid")" 2>/dev/null
}

start() {
    running && return 0
    file="$units/$1"
    [ -f "$file" ] || { echo "Unit $1 not found." >&2; return 5; }
    line=$(sed -n 's/^ExecStart=//p' "$file")
    sed -n 's/^Environment=//p' "$file" > "$here/environment"
    (
        while IFS= read -r assignment; do
            eval "export $assignment"
        done < "$here/environment"
        eval "set -- $line"
        umask 077
        cd /
        "$@" >> "$here/service.log" 2>&1 < /dev/null &
        echo $! > "$here/pid"
    )
}

stop() {
    running || return 0
    pid=$(cat "$here/pid")
    kill -TERM "$pid"
    tries=0
    while kill -0 "$pid" 2>/dev/null; do
        tries=$((tries + 1))
        if [ "$tries" -gt 300 ]; then
            kill -KILL "$pid"
            echo "the unit did not stop" >&2
            return 1
        fi
        sleep 0.05
    done
    : > "$here/pid"
}

case "$verb" in
show)
    if [ "$1" = "--property=Version" ]; then
        echo "Version=255"
        exit 0
    fi
    file="$units/$1"
    if [ -f "$file" ]; then
        load=loaded
        fragment=$file
        if [ -e "$here/enabled" ]; then enabled=enabled; else enabled=disabled; fi
    else
        load=not-found
        fragment=
        enabled=
    fi
    if running; then
        active=active
        pid=$(cat "$here/pid")
    else
        active=inactive
        pid=0
    fi
    printf 'LoadState=%s\nActiveState=%s\nMainPID=%s\nFragmentPath=%s\nUnitFileState=%s\nNeedDaemonReload=no\n' \
        "$load" "$active" "$pid" "$fragment" "$enabled"
    ;;
daemon-reload) ;;
enable)
    [ "$1" = "--now" ] || exit 2
    [ -f "$units/$2" ] || { echo "Unit $2 does not exist." >&2; exit 1; }
    : > "$here/enabled"
    start "$2"
    ;;
start) start "$1" ;;
stop) stop ;;
disable)
    [ "$1" = "--now" ] || exit 2
    rm -f "$here/enabled"
    stop
    ;;
reset-failed) exit 1 ;;
*)
    echo "Unknown command verb $verb." >&2
    exit 1
    ;;
esac
