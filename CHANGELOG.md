# Changelog

## 1.2.0

- **Send to and receive from a phone that shares no network with the computer (experimental, off by default).** The new **Wi-Fi Direct** section of Preferences finds the phone over Bluetooth and moves the transfer to a temporary Wi-Fi link: when receiving, the computer hosts a hotspot; when sending, it joins the group the phone offers. It needs NetworkManager, its `dnsmasq` package and Bluetooth, and was tried with a Pixel 7 and a Galaxy A55 (Samsung Quick Share). The requirements and limits are in [docs/wifi-direct.md](docs/wifi-direct.md); a Help section explains it in the app.
- Phones found over Bluetooth appear in the Send list with a refresh button; a phone seen on both the network and Bluetooth is listed once. Phones that offer no Bluetooth L2CAP channel (Samsung) are sent to over GATT.
- The route is checked when you pick a device. A device that left the network is dropped from the list and the send says "The device is not reachable", or goes over Bluetooth if the phone is seen there; a phone chosen over Bluetooth that is now on the network is sent to over the network.
- Connecting no longer waits for ever: 8 s to connect and 20 s for the first answers, then the transfer fails. A send can be cancelled while it connects, from the Transfers page and from Plasma's job.
- The Transfers page shows the transfer as soon as you pick a device. Short messages (completed, failed, declined, cancelled) slide up from the bottom of the window and stack. The Send page does not start another transfer while one is running.
- Cancelling a send tells the phone before the link is closed, and a cancel from the phone in the middle of a file is noticed at once.
- The AppImage is named `KuickShare-x86_64.AppImage`, without the version, so `releases/latest/download/KuickShare-x86_64.AppImage` is a stable link. Its update information follows the new name; the tarball keeps the version.
- Preferences: new "Application menu entry" switch in the AppImage, to add or remove the menu entry at any time.
- The first-start dialog is skipped when `XDG_CURRENT_DESKTOP` is not set (Xvfb, containers), so it no longer covers the main window there.
- Add the Turkish translation (thanks to dosulu). Dialog texts have no full stop at the end.

## 1.1.2

- Add the Dutch translation (thanks to Vistaus).
- Report crashes through DrKonqi (KCrash) and fail the build early when the Kirigami QML module is missing.

## 1.1.1

- Lower memory use while the window is hidden: closing it now destroys the window and its QML engine, and opening it creates them again. Started with `--background` the shell takes about 60 MB instead of about 150 MB (RSS, measured with the host build). Files queued but not yet sent, the current page and open dialogs are lost when the window closes.
- Quit from the tray menu exits at once, without flashing the window, unless a transfer is running (then the window opens to ask).

## 1.1.0

- The AppImage is now a normal, self-contained AppImage (Anylinux): it brings Qt, KDE Frameworks and its own libc, runs on any distribution including NixOS and musl ones, follows the desktop theme, starts without installing anything and carries zsync update information. The first start asks once whether to add KuickShare to the application menu and Dolphin.
- The installer behaviour of the old AppImage moves to a new tarball with an `install.sh` (system's Qt 6 and KDE Frameworks 6) for people who want neither the AppImage nor the Flatpak.
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
