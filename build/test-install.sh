#!/bin/sh
# Smoke test of the AppImage installer in a throw-away HOME (needs a graphical KDE session): install, start, check the core answers on D-Bus, update while running, uninstall with --purge:
#   build/test-install.sh dist/KuickShare-1.0.0-x86_64.AppImage
set -eu
appimage=$(readlink -f "$1")
home=$(mktemp -d)
trap 'pkill -f "^$home/" 2>/dev/null || true; rm -rf "$home"' EXIT
run() {
    env -i PATH="$PATH" HOME="$home" XDG_RUNTIME_DIR="${XDG_RUNTIME_DIR:-}" \
        WAYLAND_DISPLAY="${WAYLAND_DISPLAY:-}" DISPLAY="${DISPLAY:-}" \
        DBUS_SESSION_BUS_ADDRESS="${DBUS_SESSION_BUS_ADDRESS:-}" "$@"
}
app=$home/.local/share/kuickshare
fail() { echo "FAIL: $*" >&2; exit 1; }
owner() {
    gdbus call --session --dest io.github.the_beper.KuickShare --object-path /io/github/the_beper/KuickShare \
        --method org.freedesktop.DBus.Properties.Get io.github.the_beper.KuickShare1 DeviceName >/dev/null 2>&1
}

pgrep -x kuickshare >/dev/null && fail "another KuickShare is running: quit it first"

run "$appimage" --no-launch >/dev/null
[ -x "$app/bin/kuickshare" ] && [ -x "$app/bin/kuickshared" ] || fail "binaries not installed"
[ -x "$home/.local/share/kio/servicemenus/kuickshare-send.desktop" ] || fail "service menu not executable"

run "$appimage" >/dev/null
for _ in 1 2 3 4 5 6 7 8 9 10 11 12; do owner && break; sleep 1; done
owner || fail "the installed app did not start (no D-Bus answer)"
echo "ok: started"

run "$appimage" --no-launch >/dev/null
sleep 4
owner || fail "the app was not restarted after the update"
echo "ok: updated while running"

run "$app/uninstall.sh" --purge >/dev/null
sleep 2
owner && fail "the app is still running after the uninstall"
left=$(find "$home" -type f -not -path '*/.cache/*' | head -3)
[ -z "$left" ] || fail "files left behind: $left"
echo "ok: uninstalled, nothing left"
