# Contributing

KuickShare targets KDE Plasma 6: the AppImage and the Flatpak any distribution, the tarball Fedora 44 and distributions with the same libraries. Issues and pull requests are welcome; for anything larger than a small fix, open an issue first to agree on the approach.

## Repository layout

| Path | Content |
|---|---|
| `Cargo.toml`, `Cargo.lock` | Rust project (root) |
| `src/core/` | Daemon `kuickshared`: Quick Share protocol (uses [`third_party/rqs_lib`](THIRD_PARTY.md#changes-to-rqs_lib), a modified copy of rquickshare), settings, D-Bus API. Its unit tests sit next to the code |
| `src/shell/` | C++ and QML app `kuickshare` (Qt 6, KF6, Kirigami), with its CMake project |
| `tests/core/` | Rust integration tests; they start the daemon on a private D-Bus |
| `tests/shell/` | Qt tests: transfer model, Dolphin menu entry, `Exec` quoting, Flatpak and AppImage launcher paths, first-launch integration |
| `data/` | Files installed on the host: `.desktop`, metainfo, icons, notification definitions, Dolphin service menu |
| `build/` | Build container (`build-env.sh`, `Containerfile`), AppImage (`build-appimage.sh`, `appimage/`: Arch image, hook, test), tarball with its installer (`build-host-payload.sh`, `build-tarball.sh`, `appimage/install.sh`, `test-tarball.sh`) and Flatpak (`flatpak/`: manifest, build, test and repository scripts) |
| `po/` | Translation catalogs |
| `docs/` | User and design documentation |
| `dist/` | Generated, not in git: the AppImage and its `.zsync`, the tarball, the Flatpak bundle and repository, and `tmp/` (CMake builds, staging) |

## Build

Dependencies live in a podman image defined by [`build/Containerfile`](build/Containerfile), so nothing is installed on the host. `build/build-env.sh` runs a command inside it (building the image on first use). Rust comes from the host's rustup (stable, edition 2024).

```sh
build/build-env.sh cargo build
build/build-env.sh sh -c 'cmake -S src/shell -B dist/tmp/cmake-dev -G Ninja && cmake --build dist/tmp/cmake-dev'
build/build-appimage.sh    # dist/KuickShare-<version>-x86_64.AppImage and .zsync (Fedora and Arch images; first run is long)
build/appimage/test-appimage.sh dist/KuickShare-<version>-x86_64.AppImage
build/build-tarball.sh && build/test-tarball.sh dist/KuickShare-<version>-x86_64.tar.gz
build/flatpak/build-flatpak.sh && build/flatpak/test-flatpak.sh dist/KuickShare-<version>.flatpak
```

The Flatpak build runs in its own container image (`build/flatpak/Containerfile`, started with `--privileged` for bwrap). The KDE runtime, about 2 GB, and the build cache are kept in the podman volume `kuickshare-flatpak`; `podman volume rm kuickshare-flatpak` removes them. Nothing is installed on the host.

## Run from the build tree

```sh
dist/tmp/cmake-dev/kuickshare [files...]
```

`--background` starts hidden in the tray and `--page send|transfers|settings|help` opens a page (see `--help`). The binary starts the daemon it finds next to it (`dist/tmp/cmake-dev/kuickshared` links to `target/debug/kuickshared`).

The app also needs a few files from `data/` on the host:

| File | Destination |
|---|---|
| `kuickshare.notifyrc` | `~/.local/share/knotifications6/` |
| icons | `~/.local/share/icons/hicolor/` |
| `io.github.the_beper.KuickShare.desktop` | `~/.local/share/applications/`, with `Exec` pointing at the built binary |
| `kuickshare-send.desktop` (executable) | `~/.local/share/kio/servicemenus/`, with `Exec` pointing at the built binary |

## Checks

CI runs these on every push and pull request, in a `fedora:44` container ([`ci.yml`](.github/workflows/ci.yml)). Run them before opening a pull request.

```sh
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-features                              # unit tests and tests/core; needs dbus-daemon
cargo test -p rqs_lib                                  # the vendored library: not a workspace member, so not in the line above
cmake -S src/shell -B dist/tmp/cmake-dev -G Ninja && cmake --build dist/tmp/cmake-dev
ctest --test-dir dist/tmp/cmake-dev
qmllint-qt6 -I /usr/lib64/qt6/qml src/shell/qml/*.qml
```

Outside the build container, prefix the commands with `build/build-env.sh`; `dbus-daemon` is in the image.

### Hand tests with a phone (Wi-Fi upgrade and Bluetooth)

The feature needs a real adapter and phone and cannot run in CI. Build with `build/build-env.sh cargo build` and the GUI as above, then run the programs on the host:

- Receiving needs the `dnsmasq` package on the host: NetworkManager runs it to give the phone an address on the hotspot. Neither direction needs privileges.
- Write the daemon log to a file: `RUST_LOG=info,rqs_lib::hdl::ble_receiver=debug,rqs_lib::upgrade=debug target/debug/kuickshared > kuickshared.log 2>&1` (it logs to stdout), and capture Bluetooth with `sudo btmon -w capture.btsnoop`.
- The phone's side is in `adb logcat -s NearbyConnections NearbyMediums NearbySharing WifiP2pService` (USB debugging on).
- Individual steps have `#[ignore]` tests: run `cargo test -p rqs_lib --no-run` (inside the container; the printed path is the test binary) and execute it on the host with `--ignored --nocapture <name>` (`probe_nearby_phones`, `send_small_file_to_phone`, `observe_upgrade_offer`, `send_through_rqs_api`).

## Code style

- Rust: `rustfmt` defaults, Clippy clean, `Result` with context instead of `unwrap()` outside tests.
- C++ and QML: match the surrounding code.
- Comments, docs and commit messages in English.
- Keep changes small and focused. Add a test for non-trivial logic.
- Update the docs in the same change as the code.

## Screenshots

The images in `docs/screenshots/` come from the real shell running against a fake core, so they show made-up devices and never the data of the machine that takes them. [`build/screenshots/`](build/screenshots/) has the scripts:

| Script | Purpose |
|---|---|
| `all.sh` | Retakes every screenshot that can be scripted |
| `shoot.sh <name> [options]` | Captures one window as `docs/screenshots/<name>.png` (`OUT` changes the folder, `WAIT` the seconds before the capture) |
| `demo.sh [options]` | Opens the shell on its own private D-Bus bus, with settings in a temporary folder |
| `fake_core.py` | The made-up devices, transfers and history; edit it to change what the screenshots show |

They run on the host, not in the container: they need `spectacle`, `dbus-daemon`, `gdbus` and Python with PyGObject, and the shell built in `dist/tmp/cmake-dev` (see [Build](#build)). The windows are captured as the app opens them, at its fixed size, using the colours and icon theme of the desktop: switch to the light theme and close other windows first. `pair.png` needs a click and is taken by hand (Preferences, Pair a device).

## Translations

Texts in the shell are marked with `i18n()`. After adding or changing any, regenerate the catalogs:

```sh
build/build-env.sh po/extract.sh
```

This rebuilds `po/kuickshare.pot` from the files in `po/POTFILES.in` and merges it into `po/<language>.po` for each code in `po/LINGUAS`. A new language is a new `po/<language>.po` plus its code in `po/LINGUAS`.

## Releases

1. Set the new version in `Cargo.toml`, `src/shell/CMakeLists.txt` and the `<releases>` block of `data/io.github.the_beper.KuickShare.metainfo.xml`, and describe it in `CHANGELOG.md`.
2. Tag it `vX.Y.Z`, matching the version, and push the tag.

The tag triggers [`release.yml`](.github/workflows/release.yml): it builds the AppImage and the tarball, checks that their version matches the tag, tests them and publishes them with the `.zsync` file and a `SHA256SUMS` file. It then builds the Flatpak, attaches the bundle and updates the repository on GitHub Pages.

One-time setup for the Flatpak repository:

1. Create a signing key without a passphrase and store its private part as the repository secret `FLATPAK_GPG_PRIVATE_KEY`: generate it in a scratch keyring (`export GNUPGHOME=$(mktemp -d)`) with `gpg --batch --pinentry-mode loopback --passphrase '' --quick-gen-key "KuickShare Flatpak" default default never`, then `gpg --armor --export-secret-keys "KuickShare Flatpak" | gh secret set FLATPAK_GPG_PRIVATE_KEY`. Keep a copy: a new key means every user must re-add the remote.
2. After the first release, set Settings, Pages to deploy from the `gh-pages` branch (root).

## Proposing changes

1. Fork, create a branch, make the change.
2. Make sure all checks pass.
3. Open a pull request describing what and why.

By contributing you agree that your work is licensed under [GPL-3.0-or-later](LICENSE).
