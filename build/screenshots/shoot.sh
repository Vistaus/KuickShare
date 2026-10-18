#!/bin/sh
# Captures the window of the demo (see demo.sh) as <name>.png, at the size the app opens with.
# Needs spectacle, dbus-daemon, gdbus and python3 with PyGObject on the host (not in the container).
# Usage: build/screenshots/shoot.sh <name> [kuickshare options and files...]
# Output goes to docs/screenshots unless OUT says otherwise.
here=$(cd "$(dirname "$0")" && pwd)
root=$(cd "$here/../.." && pwd)
out=${OUT:-$root/docs/screenshots}
name=$1
shift
mkdir -p "$out"
"$here/demo.sh" "$@" > "${TMPDIR:-/tmp}/kuickshare-demo.log" 2>&1 &
demo=$!
sleep "${WAIT:-8}"
spectacle -b -a -n -o "$out/$name.png" > /dev/null 2>&1
kill -TERM $demo
wait $demo
