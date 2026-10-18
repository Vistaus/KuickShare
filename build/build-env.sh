#!/bin/sh
# Usage: build/build-env.sh [command...] (default: interactive shell)
set -eu
root=$(cd "$(dirname "$0")/.." && pwd)
image=localhost/kuickshare-build:fedora44

podman image exists "$image" || podman build -t "$image" -f "$root/build/Containerfile" "$root/build"

tty=; [ -t 0 ] && tty=-t

exec podman run --rm -i $tty --userns=keep-id --network=host --security-opt label=disable \
    -e HOME="$HOME" -e PATH="$HOME/.cargo/bin:/usr/local/bin:/usr/bin" \
    -v "$HOME/.cargo:$HOME/.cargo" -v "$HOME/.rustup:$HOME/.rustup" \
    -v "$root:$root" -w "$root" \
    "$image" "${@:-bash}"
