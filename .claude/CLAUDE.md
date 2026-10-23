# KuickShare

Quick Share client for KDE Plasma 6: Rust daemon `kuickshared` (`src/core`, protocol from the vendored `third_party/rqs_lib`) plus a Qt 6 / Kirigami shell `kuickshare` (`src/shell`, C++ and QML). Layout, build and checks are in [CONTRIBUTING.md](../CONTRIBUTING.md); do not copy them here. Architecture: [docs/architecture.md](../docs/architecture.md). D-Bus API: [docs/dbus-api.md](../docs/dbus-api.md).

## Commands
- Everything that needs Qt, KF6, ECM or `dbus-daemon` runs through `build/build-env.sh <command>` (podman). A bare `cmake` on the host fails with a missing ECM.
- Full check: the `/check` skill (rustfmt in the root and in `third_party/rqs_lib`, Clippy `-D warnings`, `cargo test --all-features`, `cargo test -p rqs_lib`, CMake build + `ctest`, `qmllint-qt6`, catalogs). `cargo test -p rqs_lib` leaves an untracked `third_party/rqs_lib/Cargo.lock`: delete it.
- Dev build: `build/build-env.sh cargo build` and `build/build-env.sh cmake --build dist/tmp/cmake-dev`. `dist/`, `target/` and `dist/test-wifi.sh` are not in git.

## Conventions that are not obvious
- `third_party/rqs_lib` is a modified copy of rquickshare, not a workspace member. List every change to it in the table in [THIRD_PARTY.md](../THIRD_PARTY.md).
- Docs mirror the code and are updated in the same change. [docs/wifi-direct.md](../docs/wifi-direct.md) marks causes and claims taken from logs or code, not reproduced, with *(inferred)*; keep that marker until a run or a test shows the cause. Never say a hardware path works because the tests pass: BLE, Wi-Fi and NetworkManager have no automated test with a real device.
- Translations: strings are `i18n()` calls in the files listed in `po/POTFILES.in` (add new files there). `po/kuickshare.pot` is generated and a hook blocks editing it. After changing strings run `/update-translations`: it regenerates with `po/extract.sh` and the `translator` agent (haiku) fills `it`, `nl`, `tr`; `en` repeats the msgid. `nl` and `tr` are not natively reviewed.
- `Kirigami.InlineMessage` is hidden by default: set `visible`. Warning boxes use `Layout.fillWidth` and `Layout.topMargin: Kirigami.Units.largeSpacing`, as in `SettingsPage.qml`.
- The hotspot subnet `192.168.49.0/24` is written in two places: `GATEWAY` in `src/core/link.rs` and the mDNS exclusion in `third_party/rqs_lib/src/hdl/mdns.rs`. Change both.
- Outgoing transfers are keyed by the device id (`ble:XXXX` or `ip:port`), which repeats: `TransferModel` treats a non-final event on a finished id as a new row. A send adds a "Starting" row at once (`TransferModel::beginPending`).
- BLE timings are tied together: scan window 12 s for a send, GATT dial 22 s, watchdog `BLE_START_TIMEOUT` 40 s in `src/core/app.rs`, shell timer 45 s in `corebridge.cpp`. Change one, check the others.

## Hand tests with the phones
See the `/phone-test` skill. Time zones differ between log files (daemon UTC, system and btmon local). Only a Pixel 7 and a Galaxy A55 were ever tested.

## Working with the user
- Run `code-reviewer` before suggesting a commit (plus `rqs-reviewer` when `third_party/rqs_lib`, `link.rs` or `app.rs` changed); `/code-review` only when asked.
- Project agents in `.claude/agents/`: `translator` (haiku, catalogs), `phone-log-analyzer` (a `dist/logs` session), `rqs-reviewer` (transfer code), `changelog-writer` (release notes, haiku). `commit-message` is global.
- Use parallel agents for independent files. When an agent edits code, tell it not to build; build and test once at the end.
