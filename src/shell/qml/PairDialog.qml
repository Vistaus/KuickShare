import QtQuick
import QtQuick.Controls as QQC2
import QtQuick.Layouts
import org.kde.kirigami as Kirigami

// Pairs this computer with another one running KuickShare. Both computers open this dialog:
// one picks the other from the list, then both users check that the same code is shown.
Kirigami.Dialog {
    id: dialog

    required property var core

    // list, waiting, code, done or failed
    property string stage: "list"
    property string code: ""
    property string peer: ""
    property string message: ""

    title: i18n("Pair a device")
    preferredWidth: Kirigami.Units.gridUnit * 22
    padding: Kirigami.Units.largeSpacing
    standardButtons: Kirigami.Dialog.NoButton

    onOpened: {
        stage = "list";
        core.setPairing(true);
        core.startDiscovery();
    }
    onClosed: {
        if (stage === "code") {
            core.confirmPairing(false);
        }
        core.setPairing(false);
        core.stopDiscovery();
    }

    Connections {
        target: dialog.core
        function onPairingCode(code, peer) {
            dialog.code = code;
            dialog.peer = peer;
            dialog.stage = "code";
        }
        function onPairingFinished(ok, peer, message) {
            dialog.peer = peer;
            dialog.message = message;
            dialog.stage = ok ? "done" : "failed";
        }
    }

    customFooterActions: [
        Kirigami.Action {
            visible: dialog.stage === "code"
            text: i18n("The codes match")
            icon.name: "dialog-ok-apply"
            onTriggered: {
                dialog.core.confirmPairing(true);
                dialog.stage = "waiting";
            }
        },
        Kirigami.Action {
            visible: dialog.stage === "code"
            text: i18n("They differ")
            icon.name: "dialog-cancel"
            onTriggered: dialog.core.confirmPairing(false)
        },
        Kirigami.Action {
            visible: dialog.stage === "done" || dialog.stage === "failed"
            text: i18n("Close")
            icon.name: "window-close"
            onTriggered: dialog.close()
        }
    ]

    ColumnLayout {
        spacing: Kirigami.Units.largeSpacing

        Kirigami.InlineMessage {
            visible: dialog.stage === "list"
            type: Kirigami.MessageType.Information
            text: i18n("Open Preferences → Pair a device on the other computer as well, then choose it here.")
            Layout.fillWidth: true
        }
        Repeater {
            model: dialog.core.devices
            delegate: QQC2.ItemDelegate {
                id: candidate

                required property string deviceId
                required property string name
                required property string trust
                required property string address

                visible: dialog.stage === "list" && trust.length > 0
                enabled: trust !== "trusted"
                icon.name: trust === "trusted" ? "security-high" : "computer-symbolic"
                text: name.length > 0 ? name : address
                // The delegate must keep its size when hidden stages show other items.
                Layout.fillWidth: true
                onClicked: {
                    dialog.core.pairWith(deviceId);
                    dialog.peer = text;
                    dialog.stage = "waiting";
                }

                QQC2.Label {
                    visible: candidate.trust === "trusted"
                    text: i18n("Already paired")
                    opacity: 0.7
                    anchors.right: parent.right
                    anchors.rightMargin: Kirigami.Units.largeSpacing
                    anchors.verticalCenter: parent.verticalCenter
                }
            }
        }
        RowLayout {
            visible: dialog.stage === "list"
            spacing: Kirigami.Units.largeSpacing
            Layout.alignment: Qt.AlignHCenter
            Layout.topMargin: Kirigami.Units.smallSpacing
            Layout.bottomMargin: Kirigami.Units.smallSpacing

            QQC2.BusyIndicator {
                running: dialog.stage === "list"
                Layout.preferredWidth: Kirigami.Units.iconSizes.smallMedium
                Layout.preferredHeight: Kirigami.Units.iconSizes.smallMedium
            }
            QQC2.Label {
                text: i18n("Looking for computers running KuickShare…")
                opacity: 0.7
            }
        }

        ColumnLayout {
            visible: dialog.stage === "waiting"
            spacing: Kirigami.Units.largeSpacing
            Layout.fillWidth: true
            Layout.topMargin: Kirigami.Units.largeSpacing
            Layout.bottomMargin: Kirigami.Units.largeSpacing

            Kirigami.Heading {
                level: 3
                text: dialog.peer.length > 0 ? i18n("Waiting for %1…", dialog.peer) : i18n("Waiting…")
                opacity: 0.7
                horizontalAlignment: Text.AlignHCenter
                wrapMode: Text.Wrap
                Layout.fillWidth: true
            }
            QQC2.ProgressBar {
                indeterminate: true
                Layout.fillWidth: true
            }
        }

        QQC2.Label {
            visible: dialog.stage === "code"
            text: i18n("Check that %1 shows exactly this code:", dialog.peer)
            wrapMode: Text.Wrap
            Layout.fillWidth: true
        }
        Kirigami.Heading {
            visible: dialog.stage === "code"
            level: 1
            text: dialog.code
            font.family: "monospace"
            horizontalAlignment: Text.AlignHCenter
            Layout.fillWidth: true
            Layout.topMargin: Kirigami.Units.largeSpacing
            Layout.bottomMargin: Kirigami.Units.largeSpacing
        }

        Kirigami.PlaceholderMessage {
            visible: dialog.stage === "done"
            icon.name: "security-high"
            text: i18n("Paired with %1", dialog.peer)
            explanation: i18n("Transfers between these computers are accepted without asking.")
            Layout.fillWidth: true
        }
        Kirigami.PlaceholderMessage {
            visible: dialog.stage === "failed"
            icon.name: "dialog-warning"
            text: i18n("Pairing did not complete")
            explanation: dialog.message
            Layout.fillWidth: true
        }
    }
}
