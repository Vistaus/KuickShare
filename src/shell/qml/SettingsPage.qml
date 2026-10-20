import QtQuick
import QtQuick.Controls as QQC2
import QtQuick.Dialogs
import QtQuick.Layouts
import org.kde.kirigami as Kirigami
import org.kde.kirigamiaddons.formcard as FormCard

FormCard.FormCardPage {
    id: page

    required property var core

    title: i18n("Preferences")

    // Same padding on every page: cards float with rounded corners instead of touching the window edges.
    topPadding: Kirigami.Units.largeSpacing
    leftPadding: Kirigami.Units.largeSpacing
    rightPadding: Kirigami.Units.largeSpacing
    bottomPadding: Kirigami.Units.largeSpacing

    PairDialog {
        id: pairDialog
        core: page.core
    }

    FolderDialog {
        id: folderDialog
        title: i18n("Choose the download folder")
        onAccepted: page.core.downloadDir = page.core.toLocalFile(selectedFolder)
    }

    // Shows the port in use; while the port is random the box keeps the last fixed value.
    Binding {
        target: portBox
        property: "value"
        value: page.core.staticPort
        when: page.core.staticPort >= 1024
    }

    // Changing the port restarts the sharing service: wait until the user stops adjusting it.
    Timer {
        id: portTimer
        interval: 1000
        onTriggered: page.core.staticPort = portBox.value
    }

    SectionTitle {
        first: true
        text: i18n("Visibility")
    }
    // FormCard rounds its corners only when it is wider than its maximumWidth (30 grid units by
    // default, wider than this window): cap every card just below its own width.
    FormCard.FormCard {
        maximumWidth: width - 1
        FormCard.FormTextFieldDelegate {
            label: i18n("Device name")
            text: page.core.deviceName
            enabled: page.core.connected
            onEditingFinished: page.core.deviceName = text
        }
        FormCard.FormSwitchDelegate {
            icon.name: "visibility"
            text: i18n("Visible to nearby devices")
            description: i18n("Others can find this computer and send to it")
            checked: page.core.visible
            enabled: page.core.connected
            onToggled: page.core.visible = checked
        }
        FormCard.FormSwitchDelegate {
            icon.name: "notifications-disabled"
            text: i18n("Hide during Do Not Disturb")
            description: i18n("Not visible while Do Not Disturb is on")
            checked: page.core.hideWhenDnd
            enabled: page.core.connected
            onToggled: page.core.hideWhenDnd = checked
        }
    }

    SectionTitle {
        text: i18n("Receiving")
    }
    FormCard.FormCard {
        maximumWidth: width - 1
        FormCard.FormButtonDelegate {
            icon.name: "folder-download-symbolic"
            text: i18n("Download folder")
            description: page.core.downloadDir.length > 0 ? page.core.downloadDir : i18n("Default (Downloads)")
            enabled: page.core.connected
            onClicked: folderDialog.open()
        }
        FormCard.FormButtonDelegate {
            visible: page.core.downloadDir.length > 0
            icon.name: "edit-undo"
            text: i18n("Use the default folder")
            onClicked: page.core.downloadDir = ""
        }
        FormCard.FormSwitchDelegate {
            id: randomPort
            icon.name: "network-wired-symbolic"
            text: i18n("Random port")
            description: i18n("A different port at every start")
            checked: page.core.staticPort === 0
            enabled: page.core.connected
            onToggled: page.core.staticPort = checked ? 0 : portBox.value
        }
        FormCard.AbstractFormDelegate {
            enabled: page.core.connected && !randomPort.checked
            background: null

            contentItem: RowLayout {
                spacing: Kirigami.Units.largeSpacing

                QQC2.Label {
                    text: i18n("Fixed port")
                    Layout.fillWidth: true
                }
                QQC2.SpinBox {
                    id: portBox

                    Layout.preferredWidth: Kirigami.Units.gridUnit * 9
                    editable: true
                    from: 1024
                    to: 65535
                    onValueModified: portTimer.restart()
                }
            }
        }
    }
    FormCard.FormSectionText {
        Layout.bottomMargin: 0
        text: i18n("A fixed port (1024–65535) can be opened in the firewall. Changing the name or the port restarts sharing.")
    }

    SectionTitle {
        text: i18n("Wi-Fi hotspot (experimental)")
    }
    FormCard.FormCard {
        maximumWidth: width - 1
        FormCard.FormSwitchDelegate {
            icon.name: "network-wireless"
            text: i18n("Receive without a shared network")
            description: i18n("Not usable yet without extra privileges. Be found over Bluetooth and move the transfer to a temporary Wi-Fi hotspot")
            checked: page.core.wifiUpgrade
            enabled: page.core.connected
            onToggled: page.core.wifiUpgrade = checked
        }
    }
    FormCard.FormSectionText {
        Layout.bottomMargin: 0
        text: i18n("Experimental. Needs NetworkManager and Bluetooth. While on, this computer is visible over Bluetooth to devices in range, and its Wi-Fi connection may drop while the hotspot is active. See the documentation.")
    }

    SectionTitle {
        text: i18n("Paired devices")
    }
    FormCard.FormCard {
        maximumWidth: width - 1
        Repeater {
            model: page.core.trustedDevices
            delegate: FormCard.FormTextDelegate {
                id: paired

                required property var modelData

                icon.name: "security-high"
                text: modelData.name
                description: i18n("Fingerprint %1", modelData.fingerprint.substring(0, 12))
                trailing: QQC2.ToolButton {
                    icon.name: "edit-delete-remove"
                    display: QQC2.AbstractButton.IconOnly
                    text: i18n("Remove")
                    onClicked: page.core.unpair(paired.modelData.fingerprint)
                }
            }
        }
        FormCard.FormTextDelegate {
            visible: page.core.trustedDevices.length === 0
            text: i18n("No paired computers yet")
            description: i18n("Paired computers can send to each other without asking")
        }
        FormCard.FormButtonDelegate {
            icon.name: "list-add"
            text: i18n("Pair a device…")
            enabled: page.core.connected
            onClicked: pairDialog.open()
        }
    }
    FormCard.FormSectionText {
        Layout.bottomMargin: 0
        text: i18n("A paired computer can send you files without asking.")
    }

    SectionTitle {
        text: i18n("Trusted devices")
    }
    FormCard.FormCard {
        maximumWidth: width - 1
        FormCard.FormSwitchDelegate {
            icon.name: "security-high"
            text: i18n("Trust devices")
            description: i18n("Accept transfers from trusted devices without asking")
            checked: page.core.trustedDevicesEnabled
            enabled: page.core.connected
            onToggled: page.core.trustedDevicesEnabled = checked
        }
        Repeater {
            model: page.core.trustedDevicesEnabled ? page.core.allowedDevices : []
            delegate: FormCard.FormTextDelegate {
                id: allowed

                required property var modelData

                icon.name: "smartphone-symbolic"
                text: modelData.title
                description: i18n("MAC address %1", modelData.mac)
                trailing: QQC2.ToolButton {
                    icon.name: "edit-delete-remove"
                    display: QQC2.AbstractButton.IconOnly
                    text: i18n("Remove")
                    onClicked: page.core.forgetAllowed(allowed.modelData.name, allowed.modelData.mac)
                }
            }
        }
        FormCard.FormTextDelegate {
            visible: page.core.trustedDevicesEnabled && page.core.allowedDevices.length === 0
            text: i18n("No devices yet")
            description: i18n("Use “Accept and trust” on an incoming transfer notification to add one")
        }
    }
    // A warning, not a hint, and only while trusting is on.
    Kirigami.InlineMessage {
        visible: page.core.trustedDevicesEnabled
        type: Kirigami.MessageType.Warning
        text: i18n("Matched by name and MAC address. Another device that copies both would be accepted too. Use only if you trust all devices in your LAN.")
        Layout.fillWidth: true
        Layout.topMargin: Kirigami.Units.largeSpacing
    }

    SectionTitle {
        text: i18n("History")
    }
    FormCard.FormCard {
        maximumWidth: width - 1
        FormCard.FormSwitchDelegate {
            icon.name: "view-history"
            text: i18n("Save transfer history")
            description: i18n("Keep a list of sent and received transfers")
            checked: page.core.historyEnabled
            enabled: page.core.connected
            onToggled: page.core.historyEnabled = checked
        }
        FormCard.AbstractFormDelegate {
            id: keepRow

            readonly property var days: [0, 1, 7, 30, 90]

            enabled: page.core.connected && page.core.historyEnabled
            background: null

            contentItem: RowLayout {
                spacing: Kirigami.Units.largeSpacing

                Kirigami.Icon {
                    source: "edit-clear-history"
                    Layout.preferredWidth: Kirigami.Units.iconSizes.smallMedium
                    Layout.preferredHeight: Kirigami.Units.iconSizes.smallMedium
                }
                QQC2.Label {
                    text: i18n("Delete entries after")
                    Layout.fillWidth: true
                }
                QQC2.ComboBox {
                    id: keepBox

                    Layout.preferredWidth: Kirigami.Units.gridUnit * 9
                    popup.width: width
                    model: [i18n("Never"), i18n("1 day"), i18n("7 days"), i18n("30 days"), i18n("90 days")]
                    currentIndex: Math.max(0, keepRow.days.indexOf(page.core.historyKeepDays))
                    onActivated: index => page.core.historyKeepDays = keepRow.days[index]
                }
            }
        }
    }
    FormCard.FormSectionText {
        Layout.bottomMargin: 0
        text: i18n("With the history turned off nothing is saved. Entries already saved stay until they expire or you clear them.")
    }

    SectionTitle {
        text: i18n("Startup")
    }
    FormCard.FormCard {
        maximumWidth: width - 1
        FormCard.FormSwitchDelegate {
            icon.name: "system-run-symbolic"
            text: i18n("Start at login")
            description: i18n("Starts hidden, in the system tray if it is shown")
            checked: page.core.autostart
            onToggled: page.core.autostart = checked
        }
    }
    SectionTitle {
        text: i18n("Integration")
    }
    FormCard.FormCard {
        maximumWidth: width - 1
        FormCard.FormSwitchDelegate {
            icon.name: "notifications-symbolic"
            text: i18n("Show tray icon")
            description: i18n("Keep KuickShare in the system tray")
            checked: page.core.trayIcon
            enabled: page.core.connected
            onToggled: page.core.trayIcon = checked
        }
        FormCard.FormSwitchDelegate {
            icon.name: "system-file-manager-symbolic"
            text: i18n("Dolphin context menu")
            description: i18n("Add “Send with KuickShare” to the menu of files")
            checked: page.core.dolphinIntegration
            enabled: page.core.connected
            onToggled: page.core.dolphinIntegration = checked
        }
    }
    Kirigami.InlineMessage {
        visible: page.core.connected && !page.core.trayIcon
        type: Kirigami.MessageType.Information
        text: i18n("Without the tray icon, closing the window quits KuickShare, and nothing can be received until you open it again. At login it starts hidden: open KuickShare again to show the window.")
        Layout.fillWidth: true
        Layout.topMargin: Kirigami.Units.largeSpacing
    }
    Kirigami.PromptDialog {
        id: resetDialog
        title: i18n("Reset preferences?")
        subtitle: i18n("Every setting goes back to its default. The history and the paired and trusted devices are kept.")
        standardButtons: Kirigami.Dialog.Ok | Kirigami.Dialog.Cancel
        onAccepted: page.core.resetSettings()
    }

    SectionTitle {
        text: i18n("Reset")
    }
    FormCard.FormCard {
        maximumWidth: width - 1
        FormCard.FormButtonDelegate {
            icon.name: "edit-undo"
            text: i18n("Reset preferences…")
            description: i18n("Go back to the default preferences")
            enabled: page.core.connected
            onClicked: resetDialog.open()
        }
    }
}
