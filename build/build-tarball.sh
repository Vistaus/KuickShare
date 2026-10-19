#!/bin/sh
# Builds dist/KuickShare-<version>-x86_64.tar.gz: the program built against the system's Qt 6 / KDE Frameworks 6 and
# an installer, for people who want neither the AppImage nor the Flatpak:
#   tar xf KuickShare-<version>-x86_64.tar.gz && KuickShare-<version>/install.sh
# The payload is the one staged by build/build-host-payload.sh (run first if dist/tmp/host is missing).
set -eu
root=$(cd "$(dirname "$0")/.." && pwd)
cd "$root"
version=$(sed -n 's/^project(kuickshare VERSION \([0-9.]*\).*/\1/p' src/shell/CMakeLists.txt)
# A payload left from another version would give a tarball with the wrong contents.
[ "$(cat dist/tmp/host/usr/share/kuickshare/VERSION 2>/dev/null)" = "$version" ] || "$root/build/build-host-payload.sh"
name=KuickShare-$version-x86_64

top=dist/tmp/tarball/$name
rm -rf dist/tmp/tarball
mkdir -p "$top"
cp -r dist/tmp/host/usr "$top/usr"
cat > "$top/install.sh" <<'SH'
#!/bin/sh
# Installs KuickShare into ~/.local (see usr/share/kuickshare/install.sh for the options):
#   ./install.sh [--no-launch] [files...]      install or update, then start the app
#   ./install.sh --uninstall [--purge]         remove it
exec "$(dirname "$(readlink -f "$0")")/usr/share/kuickshare/install.sh" "$@"
SH
chmod 755 "$top/install.sh"

mkdir -p dist
out=dist/$name.tar.gz
# Fixed owner, order and mtime: the same sources give the same archive.
tar --sort=name --owner=0 --group=0 --numeric-owner --mtime='2026-01-01 00:00Z' -C dist/tmp/tarball -czf "$out" "$name"
echo "Built $out ($(du -h "$out" | cut -f1))"
