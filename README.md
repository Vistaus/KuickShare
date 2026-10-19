# KuickShare

[![License: GPL-3.0-or-later](https://img.shields.io/badge/license-GPL--3.0--or--later-blue)](LICENSE)
[![CI](https://github.com/the-beper/KuickShare/actions/workflows/ci.yml/badge.svg)](https://github.com/the-beper/KuickShare/actions/workflows/ci.yml)

> [!IMPORTANT]
> KuickShare began as a project for my own personal use, and is shared as is. Generative AI (Claude Code) assisted me in writing the code (almost all of the C++ and QML and a large part of the Rust core) and drafted the documentation and diagrams.

Send files, text and links between your Linux desktop and Android phones with [Quick Share](https://www.android.com/better-together/quick-share/), from a native KDE Plasma 6 app.

<table>
  <tr>
    <td><img src="docs/screenshots/send-files.png" alt="The Send page with files ready to send"></td>
    <td><img src="docs/screenshots/transfers.png" alt="The Transfers page with the history"></td>
  </tr>
</table>

## Why

[Packet](https://github.com/nozwock/packet) is a good Quick Share client, but it is a GTK4 + libadwaita app built for a GNOME environment. KuickShare started from it and keeps its protocol work, with an interface written for Plasma: Breeze look, system tray, Plasma notifications and job progress, and a Dolphin menu entry.

## Why not KDE Connect?

I would love to use KDE Connect, but unfortunately its data transfer feature has been quite unreliable for me. The problems seem to be related to `wpa_supplicant`, but I was not able to triage the issue and switching to `iwd` would cause my Wi-Fi card to simply stop working.

## Features

- Send and receive files, text and links with phones and other Quick Share devices on the same Wi-Fi network.
- Accept or decline incoming transfers from a notification; progress shows in Plasma's job tracker.
- Drag and drop, clipboard paste and **Send with KuickShare** in Dolphin.
- History of past transfers, with automatic cleanup.
- Trusted devices (matched by name and MAC address) and paired KuickShare computers (8-digit code) can send without asking.
- Tray icon, start at login, Do Not Disturb aware.
- English and Italian interface.

## Install

Requires Fedora 44 with KDE Plasma 6. Download the AppImage from the [releases](https://github.com/the-beper/KuickShare/releases) and run it: it installs KuickShare into `~/.local` and starts it.

```sh
chmod +x KuickShare-*-x86_64.AppImage
./KuickShare-*-x86_64.AppImage
```

Update, uninstall and requirements are in the [usage guide](docs/usage.md).

## Documentation

| Document | Content |
|---|---|
| [Usage](docs/usage.md) | Install, update, uninstall, everyday use, trusted and paired devices |
| [Architecture](docs/architecture.md) | Design, security model, packaging |
| [D-Bus API](docs/dbus-api.md) | The interface of the background service |
| [Contributing](CONTRIBUTING.md) | Build, tests, translations, releases |
| [Security](SECURITY.md) | Reporting a vulnerability, known limits |
| [Changelog](CHANGELOG.md) | Release notes |

## Status

Version 1.0.1. Tested on Fedora 44 with KDE Plasma 6. On Arch (Qt 6.12) the installer and the core start in a container; the interface is not verified on a real Arch desktop. Other distributions and desktops are unverified.

## Credits and license

KuickShare is derived from [Packet](https://github.com/nozwock/packet) and uses [`rqs_lib`](https://github.com/nozwock/rquickshare) for the Quick Share protocol. Dependencies are listed in [THIRD_PARTY.md](THIRD_PARTY.md).

Licensed under [GPL-3.0-or-later](LICENSE).

Quick Share is a Google product. KuickShare is an independent project, not affiliated with or endorsed by Google.
