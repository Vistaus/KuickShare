#!/bin/sh
# Builds dist/KuickShare-<version>-x86_64.AppImage (inside the build container, started automatically).
# The runtime is a fixed release (not "continuous", which changes under the pinned hash).
# The AppImage is an installer: running it copies the program into ~/.local and starts it, so the file itself can be deleted. It uses the Qt 6 / KDE Frameworks 6 libraries of the system
set -eu
root=$(cd "$(dirname "$0")/.." && pwd)
cd "$root"
# Run inside the build container (podman creates /run/.containerenv).
[ -e /run/.containerenv ] || exec "$root/build/build-env.sh" "$root/build/build-appimage.sh"
version=$(sed -n 's/^project(kuickshare VERSION \([0-9.]*\).*/\1/p' src/shell/CMakeLists.txt)

mkdir -p dist/tmp
cargo build --release --locked
cmake -S src/shell -B dist/tmp/cmake-release -G Ninja -DCMAKE_BUILD_TYPE=Release
cmake --build dist/tmp/cmake-release

appdir=dist/tmp/appdir
rm -rf "$appdir"
share=$appdir/usr/share/kuickshare
mkdir -p "$appdir/usr/bin" "$share/icons"
install -m755 target/release/kuickshared "$appdir/usr/bin/kuickshared"
install -m755 dist/tmp/cmake-release/kuickshare "$appdir/usr/bin/kuickshare"
strip --strip-unneeded "$appdir/usr/bin/kuickshared" "$appdir/usr/bin/kuickshare"
install -m755 build/appimage/install.sh "$share/install.sh"
install -m755 build/appimage/AppRun "$appdir/AppRun"
echo "$version" > "$share/VERSION"
cp data/io.github.the_beper.KuickShare.desktop data/io.github.the_beper.KuickShare.metainfo.xml data/kuickshare-send.desktop data/kuickshare.notifyrc "$share/"
cp data/icons/hicolor/scalable/apps/io.github.the_beper.KuickShare.svg \
    data/icons/hicolor/symbolic/apps/io.github.the_beper.KuickShare-symbolic.svg "$share/icons/"

# Translations: compile the catalogs to the layout KDE looks for under ~/.local/share/locale.
for lang in $(cat po/LINGUAS); do
    mkdir -p "$share/locale/$lang/LC_MESSAGES"
    msgfmt --check-format -o "$share/locale/$lang/LC_MESSAGES/kuickshare.mo" "po/$lang.po"
done

cp data/icons/hicolor/scalable/apps/io.github.the_beper.KuickShare.svg "$appdir/"
cp "$appdir/io.github.the_beper.KuickShare.svg" "$appdir/.DirIcon"
sed 's|^Exec=.*|Exec=AppRun|' data/io.github.the_beper.KuickShare.desktop > "$appdir/io.github.the_beper.KuickShare.desktop"

runtime=dist/tmp/runtime-x86_64
pin=build/appimage/runtime-x86_64.sha256
if [ ! -f "$runtime" ]; then
    curl -fsSL -o "$runtime" https://github.com/AppImage/type2-runtime/releases/download/20251108/runtime-x86_64
fi
if [ -f "$pin" ]; then
    echo "$(cut -d' ' -f1 "$pin")  $runtime" | sha256sum -c -
else
    echo "$(sha256sum "$runtime" | cut -d' ' -f1)  runtime-x86_64" > "$pin"
    echo "Pinned the checksum of the downloaded runtime in $pin: review it." >&2
fi

mkdir -p dist
out=dist/KuickShare-$version-x86_64.AppImage
mksquashfs "$appdir" dist/tmp/appimage.squashfs -root-owned -noappend -comp zstd -quiet
rm -f "$out" # a running AppImage can't be overwritten ("Text file busy"), but it can be replaced
cat "$runtime" dist/tmp/appimage.squashfs > "$out"
chmod +x "$out"
echo "Built $out ($(du -h "$out" | cut -f1))"
