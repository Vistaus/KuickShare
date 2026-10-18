## What and why

<!-- A short description of the change, and the issue it relates to. -->

## Checks

- [ ] `cargo fmt --check`, `cargo clippy --all-targets --all-features -- -D warnings` and `cargo test --all-features` pass
- [ ] The shell builds and `ctest` passes; `qmllint-qt6` reports no errors on `src/shell/qml`
- [ ] New texts are marked with `i18n()` and `po/extract.sh` was run
- [ ] I tried it in the running app (what I tried: …)
