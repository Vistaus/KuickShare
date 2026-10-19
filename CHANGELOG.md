# Changelog

## Unreleased

- The AppImage is now a normal, self-contained AppImage (Anylinux): it brings Qt, KDE Frameworks and its own libc, runs on any distribution including NixOS and musl ones, follows the desktop theme, starts without installing anything and carries zsync update information. The first start asks once whether to add KuickShare to the application menu and Dolphin.
- The installer behaviour of the old AppImage moves to a new tarball with an `install.sh` (system's Qt 6 and KDE Frameworks 6) for people who want neither the AppImage nor the Flatpak.

## 1.0.2

- Flatpak package for any distribution, from a signed repository on GitHub Pages (`kuickshare.flatpakrepo`) and as a release asset. The Dolphin entry and the autostart entry are written by the app and start it through `flatpak run`; a download folder the sandbox cannot share is refused with the command to allow it.
- Fix transfer cards overlapping or staying half transparent when a transfer changes state while the list is animating.
- The About dialog opens with the same animation as the Shortcuts dialog, and the close button of both lines up with the content below.

## 1.0.1

- Fix the AppImage not starting on Arch-based distributions (Qt 6.12): no Qt private API, no copy relocations.
- The installer dependency check shows the real error and an Arch install command.

## 1.0.0

First release.

- Send files and text or links to nearby devices, and receive them: Quick Share over the local network, with Android phones and other Quick Share clients.
- Breeze-style Qt 6 / Kirigami interface with Send and Transfers pages, drag and drop, clipboard paste (text, files, images) and a Do Not Disturb aware visibility.
- Plasma integration: tray icon with quick actions, notifications with Accept / Decline, job progress, Dolphin "Send with KuickShare" entry.
- Transfer history with details, per-entry removal, automatic cleanup and a size warning.
- Paired computers (8-digit code over an encrypted channel) and trusted devices (name and MAC address) that skip the approval.
- Preferences, Help, keyboard shortcuts and About; English and Italian.
- Safeguards on incoming transfers: unsafe file names refused, consent timeout, rate limit on the pairing port.
- AppImage installer into `~/.local`, with uninstall script.
