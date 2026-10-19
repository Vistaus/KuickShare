#!/bin/sh
# Finishes a Flatpak repository for hosting on a static web server (GitHub Pages): signs and prunes the OSTree
# repository, and writes the files users add it with.
# Usage: GPG_KEY_ID=<key> build/flatpak/publish-repo.sh <site-dir> <base-url>
#   <site-dir>/repo      the OSTree repository (build it with KUICKSHARE_FLATPAK_REPO=<site-dir>/repo)
#   <base-url>           where <site-dir> is served, e.g. https://the-beper.github.io/KuickShare
# Needs flatpak, gpg and the signing key in the keyring ($GNUPGHOME).
set -eu
site=$(readlink -f "$1")
url=${2%/}
id=io.github.the_beper.KuickShare
: "${GPG_KEY_ID:?set GPG_KEY_ID to the signing key}"

flatpak build-update-repo --gpg-sign="$GPG_KEY_ID" --generate-static-deltas --prune --prune-depth=3 "$site/repo"
key=$(gpg --export "$GPG_KEY_ID" | base64 -w0)

cat > "$site/kuickshare.flatpakrepo" <<REPO
[Flatpak Repo]
Title=KuickShare
Url=$url/repo/
Homepage=https://github.com/the-beper/KuickShare
Comment=Quick Share client for KDE Plasma
Description=Send files, text and links between your Linux desktop and Android phones.
Icon=$url/icon.svg
GPGKey=$key
REPO

cat > "$site/kuickshare.flatpakref" <<REF
[Flatpak Ref]
Title=KuickShare
Name=$id
Branch=stable
Url=$url/repo/
SuggestRemoteName=kuickshare
IsRuntime=false
RuntimeRepo=https://dl.flathub.org/repo/flathub.flatpakrepo
GPGKey=$key
REF

cp "$(dirname "$0")/../../data/icons/hicolor/scalable/apps/$id.svg" "$site/icon.svg"
# GitHub Pages runs Jekyll unless told not to, and Jekyll skips the dot-files OSTree uses.
touch "$site/.nojekyll"
