---
name: check
description: Use before proposing a commit or a pull request: runs every KuickShare check (rustfmt, Clippy, Rust tests, vendored library tests, shell build, ctest, qmllint, catalogs) the way CI does.
---

# Full check

Run from the repository root; everything that needs Qt, KF6 or `dbus-daemon` goes through `build/build-env.sh` (the host has no ECM: a bare `cmake --build` fails). Report only what you saw pass.

```sh
build/build-env.sh cargo fmt --check
(cd third_party/rqs_lib && ../../build/build-env.sh cargo fmt --check)
build/build-env.sh cargo clippy --all-targets --all-features -- -D warnings
build/build-env.sh cargo test --all-features          # unit tests and tests/core
build/build-env.sh cargo test -p rqs_lib              # vendored library, not a workspace member
build/build-env.sh sh -c 'cmake -S src/shell -B dist/tmp/cmake-dev -G Ninja && cmake --build dist/tmp/cmake-dev'
build/build-env.sh ctest --test-dir dist/tmp/cmake-dev
build/build-env.sh qmllint-qt6 -I /usr/lib64/qt6/qml src/shell/qml/*.qml
for l in $(cat po/LINGUAS); do msgfmt -c --statistics -o /dev/null po/$l.po; done
python3 .claude/scripts/po.py status
```

Afterwards:
- `rm -f third_party/rqs_lib/Cargo.lock` (the library tests create it; it is not tracked).
- `git status --short` must show only intended files; `dist/` and `target/` are ignored.
- Hardware paths (BLE, Wi-Fi, NetworkManager, phones) have no automated test: say which changes are untested on a phone.
- Then the `code-reviewer` agent, as the global instructions say, before suggesting a commit.
