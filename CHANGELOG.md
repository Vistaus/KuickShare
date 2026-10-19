# Changelog

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
