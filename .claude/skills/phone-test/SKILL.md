---
name: phone-test
description: Use when the user has run (or is about to run) a hand test of Wi-Fi Direct / Bluetooth transfers with the phones, or asks to analyse the logs in dist/logs.
---

# Hand test with the phones

The user runs `dist/test-wifi.sh` (local, untracked: `dist/` is ignored) as their normal user. It starts the daemon and the GUI from the build tree and writes everything to `dist/logs/<timestamp>/` (`latest` links to the newest). You cannot run it: it needs the phones, `sudo` for `btmon`, and a session. Build first with `build/build-env.sh cargo build` and `build/build-env.sh cmake --build dist/tmp/cmake-dev`.

Phones: a Pixel 7 (advertises an L2CAP PSM, quick) and a Galaxy A55 with Samsung Quick Share (no PSM: GATT, and it stops advertising in bursts, which is its own behaviour). Both must show Quick Share's Receive screen, visibility Everyone, to be sent to. Other phones, chips and firewalls were never tried.

| File in the log folder | Content | Time zone |
|---|---|---|
| `daemon.log` | The daemon, `RUST_LOG` at info plus debug for ble_*, upgrade, link | UTC |
| `system.log`, `kernel.log`, `nm-monitor.log`, `btmon.txt` | NetworkManager, bluetooth, firewalld, wpa; kernel; `nmcli monitor`; HCI | local (UTC+2 in summer) |
| `logcat-<serial>.log` | The phone's Nearby logs | phone clock, local |
| `nat.log` | nft/iptables/firewalld, only when it changes | UTC (`date -u`) |
| `jobview.log` | Plasma job calls on the session bus | |

Analysis: delegate to the `phone-log-analyzer` agent (it knows the files and the clocks): give it the folder if it is not the newest, the scenario in a paragraph and what the user saw (seconds waited, what the phone showed). Check its claims against the code before changing anything.

Doc rule: `docs/wifi-direct.md` marks behaviour that never ran on a phone with *(not yet run on a phone)*; remove a marker only after a run showed it, and say what the run showed.
