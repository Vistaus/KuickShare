import QtQuick
import QtQuick.Controls as QQC2
import QtQuick.Layouts
import org.kde.kirigami as Kirigami

Kirigami.ScrollablePage {
    id: page

    required property var core

    title: i18n("Transfers")

    // Same padding on every page: cards float with rounded corners instead of touching the window edges.
    topPadding: Kirigami.Units.largeSpacing
    leftPadding: Kirigami.Units.largeSpacing
    rightPadding: Kirigami.Units.largeSpacing
    bottomPadding: Kirigami.Units.largeSpacing

    Kirigami.PromptDialog {
        id: clearDialog
        title: i18n("Clear history?")
        subtitle: i18n("Files you received are not touched")
        standardButtons: Kirigami.Dialog.Ok | Kirigami.Dialog.Cancel
        onAccepted: page.core.clearHistory()
    }

    required property var toasts

    // Size of the saved history above which a warning offers to clear it.
    readonly property real bigHistoryBytes: 5 * 1024 * 1024

    // What the details dialog shows; filled by showDetails().
    property string detailResult: ""
    property string detailSize: ""
    property var detailFiles: []
    property string detailText: ""
    property bool detailIsLink: false
    readonly property int maxDetailRows: 5

    function showDetails(item) {
        detailResult = stateText(item.transferState, item.kind, item.ackBytes, item.viaBluetooth);
        detailSize = item.totalBytes > 0 ? core.formatSize(item.totalBytes) : "";
        detailFiles = item.files.map(path => {
            const name = path.substring(path.lastIndexOf("/") + 1);
            return {name: name, icon: core.iconForName(name)};
        });
        detailText = item.shownText;
        detailIsLink = item.files.length === 0 && /^https?:\/\/\S+$/i.test(item.shownText.trim());
        detailsDialog.open();
    }

    Kirigami.Dialog {
        id: detailsDialog
        title: i18n("Transfer details")
        standardButtons: Kirigami.Dialog.NoButton
        footer: null
        preferredWidth: Kirigami.Units.gridUnit * 22

        // The dialog only sizes itself from an Item's implicitHeight; a layout alone gives a collapsed dialog.
        Item {
            implicitHeight: detailsColumn.implicitHeight

            ColumnLayout {
                id: detailsColumn
                width: parent.width
                spacing: 0

                RowLayout {
                    spacing: Kirigami.Units.gridUnit * 2
                    Layout.fillWidth: true
                    Layout.leftMargin: Kirigami.Units.largeSpacing
                    Layout.rightMargin: Kirigami.Units.largeSpacing
                    Layout.bottomMargin: Kirigami.Units.largeSpacing

                    Repeater {
                        model: [[i18n("Result"), page.detailResult], [i18n("Size"), page.detailSize]]
                        delegate: ColumnLayout {
                            id: detail

                            required property var modelData

                            visible: modelData[1].length > 0
                            spacing: 0

                            QQC2.Label {
                                text: detail.modelData[0]
                                opacity: 0.7
                            }
                            Kirigami.Heading {
                                level: 4
                                text: detail.modelData[1]
                            }
                        }
                    }
                }

                Kirigami.Separator {
                    visible: page.detailFiles.length > 0 || page.detailText.length > 0
                    Layout.fillWidth: true
                }

                QQC2.ScrollView {
                    id: filesScroll

                    visible: page.detailFiles.length > 0
                    implicitHeight: Math.min(filesColumn.implicitHeight, page.maxDetailRows * filesColumn.rowHeight)
                    contentWidth: availableWidth
                    leftPadding: Kirigami.Units.largeSpacing
                    rightPadding: QQC2.ScrollBar.vertical ? QQC2.ScrollBar.vertical.implicitWidth : 0
                    Layout.fillWidth: true

                    ColumnLayout {
                        id: filesColumn

                        // One row: the text line plus the space around it.
                        readonly property real rowHeight: Math.max(fileMetrics.height, Kirigami.Units.iconSizes.smallMedium) + Kirigami.Units.largeSpacing * 2 + spacing

                        width: filesScroll.availableWidth
                        spacing: 0

                        FontMetrics {
                            id: fileMetrics
                        }

                        Repeater {
                            model: page.detailFiles
                            delegate: RowLayout {
                                id: fileItem

                                required property var modelData

                                spacing: Kirigami.Units.largeSpacing
                                Layout.fillWidth: true
                                // Even space around the row: this is what rowHeight adds up.
                                Layout.topMargin: Kirigami.Units.largeSpacing
                                Layout.bottomMargin: Kirigami.Units.largeSpacing

                                Kirigami.Icon {
                                    source: fileItem.modelData.icon
                                    Layout.alignment: Qt.AlignVCenter
                                    Layout.preferredWidth: Kirigami.Units.iconSizes.smallMedium
                                    Layout.preferredHeight: Kirigami.Units.iconSizes.smallMedium
                                }
                                QQC2.Label {
                                    text: fileItem.modelData.name
                                    elide: Text.ElideMiddle
                                    verticalAlignment: Text.AlignVCenter
                                    Layout.alignment: Qt.AlignVCenter
                                    Layout.fillWidth: true
                                }
                            }
                        }
                    }
                }

                QQC2.ScrollView {
                    id: textScroll

                    visible: page.detailFiles.length === 0 && page.detailText.length > 0
                    implicitHeight: Math.min(textLabel.implicitHeight, page.maxDetailRows * textMetrics.lineSpacing + Kirigami.Units.largeSpacing * 2)
                    contentWidth: availableWidth
                    leftPadding: Kirigami.Units.largeSpacing
                    rightPadding: QQC2.ScrollBar.vertical ? QQC2.ScrollBar.vertical.implicitWidth : 0
                    Layout.fillWidth: true

                    FontMetrics {
                        id: textMetrics
                    }

                    QQC2.Label {
                        id: textLabel

                        width: textScroll.availableWidth
                        text: page.detailText
                        wrapMode: Text.Wrap
                        textFormat: Text.PlainText
                        topPadding: Kirigami.Units.largeSpacing
                        bottomPadding: Kirigami.Units.largeSpacing
                        color: page.detailIsLink ? Kirigami.Theme.linkColor : Kirigami.Theme.textColor
                        font.underline: page.detailIsLink && detailHover.hovered

                        HoverHandler {
                            id: detailHover
                            enabled: page.detailIsLink
                            cursorShape: Qt.PointingHandCursor
                        }
                        TapHandler {
                            enabled: page.detailIsLink
                            onTapped: page.core.openLink(page.detailText.trim())
                        }
                    }
                }
            }
        }
    }

    function stateText(state, kind, ackBytes, viaBluetooth) {
        switch (state) {
        case "Starting":
            // The send was requested but the core has not reported anything yet.
            return viaBluetooth ? i18n("Looking for the device…") : i18n("Connecting…");
        case "WaitingForUserConsent":
            return i18n("Waiting for your approval");
        case "ReceivingFiles":
            return i18n("Receiving…");
        case "SentIntroduction":
            return kind === "outbound" ? i18n("Waiting for the other device to accept") : i18n("Connecting…");
        case "SendingFiles":
            // Over Bluetooth nothing moves until the Wi-Fi link the phone offers is up.
            return kind === "outbound" && viaBluetooth && ackBytes === 0 ? i18n("Switching to Wi-Fi…") : i18n("Sending…");
        case "Finished":
            return kind === "outbound" ? i18n("Sent") : i18n("Received");
        case "Cancelled":
            return i18n("Cancelled");
        case "Rejected":
            return kind === "outbound" ? i18n("Declined") : i18n("You declined");
        case "Disconnected":
            // The library reports a refusal by the other device as a disconnection.
            return kind === "outbound" && ackBytes === 0 ? i18n("Not accepted") : i18n("Interrupted");
        default:
            return i18n("Connecting…");
        }
    }

    function describe(files, text, preview) {
        if (files.length > 0) {
            return i18np("%1 file", "%1 files", files.length);
        }
        return text.length > 0 ? text : preview;
    }

    function when(seconds) {
        return seconds > 0 ? Qt.locale().toString(new Date(seconds * 1000), Locale.ShortFormat) : "";
    }

    ListView {
        id: list
        model: page.core.transfers
        spacing: Kirigami.Units.largeSpacing
        bottomMargin: Kirigami.Units.largeSpacing

        // No ListView transitions: a card that changes height while one runs leaves its neighbours overlapping,
        // or half transparent. Entries animate themselves instead (see the delegate).

        header: ColumnLayout {
            x: Kirigami.Units.largeSpacing
            width: list.width - Kirigami.Units.largeSpacing * 2
            height: implicitHeight + Kirigami.Units.largeSpacing
            spacing: Kirigami.Units.largeSpacing

            // The saved history grows without limit: past this size, offer to clear it.
            Kirigami.InlineMessage {
                visible: page.core.historyBytes > page.bigHistoryBytes
                type: Kirigami.MessageType.Warning
                text: i18n("The history is large (%1) and may slow KuickShare down.", page.core.formatSize(page.core.historyBytes))
                actions: [
                    Kirigami.Action {
                        text: i18n("Clear history")
                        icon.name: "edit-clear-history"
                        onTriggered: clearDialog.open()
                    }
                ]
                Layout.fillWidth: true
            }

            Kirigami.InlineMessage {
                visible: !page.core.historyEnabled
                type: Kirigami.MessageType.Information
                text: i18n("History is turned off: new transfers are not saved. You can turn it on in Preferences.")
                Layout.fillWidth: true
            }

            RowLayout {
                visible: list.count > 0
                Layout.fillWidth: true

                Item {
                    Layout.fillWidth: true
                }
                QQC2.ToolButton {
                    text: i18n("Clear history")
                    icon.name: "edit-clear-history"
                    onClicked: clearDialog.open()
                }
            }
        }

        Kirigami.PlaceholderMessage {
            anchors.centerIn: parent
            width: parent.width - Kirigami.Units.gridUnit * 4
            visible: list.count === 0
            icon.name: page.core.connected ? "folder-sync" : "dialog-warning"
            text: page.core.connected ? i18n("No transfers yet") : i18n("Starting the sharing service…")
            explanation: !page.core.connected ? "" : page.core.historyEnabled ? i18n("Sent and received transfers are listed here. Make sure this computer is visible in Preferences to receive.") : i18n("Transfers are only listed while the app is open, because the history is turned off.")
        }

        delegate: Item {
            id: cell

            required property string transferId
            required property string kind
            required property string transferState
            required property string device
            required property string pin
            required property string preview
            required property real totalBytes
            required property real ackBytes
            required property var files
            required property string bodyText
            required property string payloadKind
            required property real time
            required property string trusted
            required property string alwaysAccepted

            // A vertical ListView positions its items itself and ignores side margins, so the card
            // is inset inside this full-width cell.
            width: ListView.view.width
            implicitHeight: item.implicitHeight
            height: item.height

            // How far the card is drawn from its cell while it slides into place; the cell itself is always
            // where the list put it. A new entry starts a little above, and when the cell moves (an entry was
            // added or removed above it, or a card above changed height) the card starts where it was.
            property real shift: 0
            property real lastY: NaN

            onYChanged: {
                if (!isNaN(lastY) && lastY !== y) {
                    shift += lastY - y;
                    slide.restart();
                }
                lastY = y;
            }
            ListView.onAdd: {
                shift = -Kirigami.Units.gridUnit;
                slide.restart();
                fadeIn.restart();
            }
            // The entry stays in the list while it fades out.
            ListView.onRemove: removeAnimation.start()

            SequentialAnimation {
                id: removeAnimation

                PropertyAction {
                    target: cell
                    property: "ListView.delayRemove"
                    value: true
                }
                NumberAnimation {
                    target: item
                    property: "opacity"
                    to: 0
                    duration: Kirigami.Units.shortDuration
                }
                PropertyAction {
                    target: cell
                    property: "ListView.delayRemove"
                    value: false
                }
            }

            NumberAnimation {
                id: fadeIn

                target: item
                property: "opacity"
                from: 0
                to: 1
                duration: Kirigami.Units.longDuration
            }

            NumberAnimation {
                id: slide

                target: cell
                property: "shift"
                to: 0
                duration: Kirigami.Units.longDuration
                easing.type: Easing.OutCubic
            }

            Kirigami.AbstractCard {
                id: item

                x: Kirigami.Units.largeSpacing
                y: cell.shift
                width: parent.width - Kirigami.Units.largeSpacing * 2

                readonly property string transferId: cell.transferId
                readonly property bool viaBluetooth: cell.transferId.startsWith("ble:")
                readonly property string kind: cell.kind
                readonly property string transferState: cell.transferState
                readonly property string device: cell.device
                readonly property string pin: cell.pin
                readonly property string preview: cell.preview
                readonly property real totalBytes: cell.totalBytes
                readonly property real ackBytes: cell.ackBytes
                readonly property var files: cell.files
                readonly property string bodyText: cell.bodyText
                readonly property string payloadKind: cell.payloadKind
                readonly property real time: cell.time
                readonly property string trusted: cell.trusted
                readonly property string alwaysAccepted: cell.alwaysAccepted

                readonly property bool awaitingConsent: kind === "inbound" && transferState === "WaitingForUserConsent"
                readonly property bool active: transferState !== "Finished" && transferState !== "Cancelled" && transferState !== "Rejected" && transferState !== "Disconnected"
                readonly property bool inProgress: active && !awaitingConsent && totalBytes > 0
                // An outbound send can be cancelled while it looks for the device or connects.
                readonly property bool cancellable: inProgress || (kind === "outbound" && active)
                readonly property bool receivedFiles: kind === "inbound" && transferState === "Finished" && files.length > 0
                readonly property bool hasText: transferState === "Finished" && files.length === 0 && (bodyText.length > 0 || preview.length > 0)
                readonly property string content: page.describe(files, bodyText, preview)
                readonly property string shownText: bodyText.length > 0 ? bodyText : preview
                readonly property bool isLink: /^https?:\/\/\S+$/i.test(shownText.trim())

                padding: Kirigami.Units.largeSpacing
                highlighted: awaitingConsent

                contentItem: ColumnLayout {
                    spacing: Kirigami.Units.smallSpacing

                    RowLayout {
                        spacing: Kirigami.Units.largeSpacing
                        Layout.fillWidth: true

                        Kirigami.Icon {
                            source: item.files.length === 0 && item.content.length > 0 ? (item.isLink ? "internet-services" : "format-justify-left") : (item.kind === "outbound" ? "document-send" : "folder-download-symbolic")
                            Layout.preferredWidth: Kirigami.Units.iconSizes.medium
                            Layout.preferredHeight: Kirigami.Units.iconSizes.medium
                            Layout.alignment: Qt.AlignTop
                        }

                        ColumnLayout {
                            spacing: Kirigami.Units.smallSpacing
                            Layout.fillWidth: true

                            RowLayout {
                                spacing: Kirigami.Units.largeSpacing
                                Layout.fillWidth: true

                                RowLayout {
                                    spacing: Kirigami.Units.largeSpacing
                                    Layout.fillWidth: true
                                    // Keeps a long name (or the shield) away from the buttons on the right.
                                    Layout.rightMargin: Kirigami.Units.largeSpacing * 2

                                    Kirigami.Heading {
                                        id: deviceName

                                        level: 4
                                        text: item.device.length > 0 ? item.device : i18n("Unknown device")
                                        elide: Text.ElideRight
                                        // As wide as the name, but it gives way (and is elided) when the row is short.
                                        Layout.fillWidth: true
                                        Layout.minimumWidth: 0
                                        // Whole pixels: a layout rounds a width like 84.14 down to 84, and the text would be elided.
                                        Layout.maximumWidth: Math.ceil(implicitWidth)

                                        HoverHandler {
                                            id: nameHover
                                        }
                                        QQC2.ToolTip.visible: nameHover.hovered && deviceName.truncated
                                        QQC2.ToolTip.text: deviceName.text
                                        QQC2.ToolTip.delay: Kirigami.Units.toolTipDelay
                                    }
                                    Kirigami.Icon {
                                        visible: item.kind === "inbound" && (item.alwaysAccepted.length > 0 || item.trusted.length > 0)
                                        source: "security-high"
                                        Layout.preferredWidth: Kirigami.Units.iconSizes.smallMedium
                                        Layout.preferredHeight: Kirigami.Units.iconSizes.smallMedium

                                        HoverHandler {
                                            id: shieldHover
                                        }
                                        QQC2.ToolTip.visible: shieldHover.hovered
                                        QQC2.ToolTip.text: i18n("Trusted")
                                        QQC2.ToolTip.delay: Kirigami.Units.toolTipDelay
                                    }
                                    Item {
                                        Layout.fillWidth: true
                                    }
                                }
                                QQC2.Label {
                                    visible: item.inProgress
                                    text: item.totalBytes > 0 ? i18n("%1%", Math.round(100 * item.ackBytes / item.totalBytes)) : ""
                                    font.weight: Font.DemiBold
                                }
                                QQC2.ToolButton {
                                    visible: item.cancellable
                                    icon.name: "process-stop"
                                    display: QQC2.AbstractButton.IconOnly
                                    text: i18n("Cancel")
                                    onClicked: page.core.cancel(item.transferId)
                                }
                                QQC2.ToolButton {
                                    visible: !item.active
                                    icon.name: "edit-delete-remove"
                                    display: QQC2.AbstractButton.IconOnly
                                    text: i18n("Remove from list")
                                    onClicked: page.core.removeTransfer(item.transferId)
                                }
                            }

                            RowLayout {
                                spacing: Kirigami.Units.largeSpacing
                                Layout.fillWidth: true

                                QQC2.Label {
                                    readonly property string status: item.awaitingConsent && item.pin.length > 0 ? i18n("Wants to share with you · PIN %1", item.pin) : item.inProgress ? page.stateText(item.transferState, item.kind, item.ackBytes, item.viaBluetooth).replace("…", "") + " · " + page.core.formatSize(item.totalBytes) : page.stateText(item.transferState, item.kind, item.ackBytes, item.viaBluetooth)
                                    readonly property string stamp: page.when(item.time)
                                    text: stamp.length > 0 && !item.active ? status + " · " + stamp : status
                                    opacity: 0.7
                                    elide: Text.ElideRight
                                    Layout.fillWidth: true
                                }
                                QQC2.Button {
                                    visible: !item.active
                                    text: i18n("More details")
                                    icon.name: "help-about-symbolic"
                                    onClicked: page.showDetails(item)
                                }
                            }

                            RowLayout {
                                visible: item.content.length > 0 || item.awaitingConsent
                                spacing: Kirigami.Units.largeSpacing
                                Layout.fillWidth: true

                                QQC2.Label {
                                    // The size is already in the status line while a transfer runs.
                                    // One line: the first one, with "…" when the text goes on (otherwise it is only elided if it does not fit).
                                    readonly property var lines: item.content.split("\n").map(l => l.trim()).filter(l => l.length > 0)
                                    text: (lines.length > 0 ? lines[0] : "") + (lines.length > 1 ? "…" : "") + (item.totalBytes > 0 && item.files.length > 0 && !item.inProgress ? " · " + page.core.formatSize(item.totalBytes) : "")
                                    textFormat: Text.PlainText
                                    wrapMode: Text.NoWrap
                                    elide: item.files.length > 0 ? Text.ElideMiddle : Text.ElideRight
                                    // A link is clickable: it opens in the browser.
                                    color: linkTarget ? Kirigami.Theme.linkColor : Kirigami.Theme.textColor
                                    font.underline: linkTarget && linkHover.hovered
                                    Layout.fillWidth: true

                                    readonly property bool linkTarget: item.isLink && item.files.length === 0

                                    HoverHandler {
                                        id: linkHover
                                        enabled: parent.linkTarget
                                        cursorShape: Qt.PointingHandCursor
                                    }
                                    TapHandler {
                                        enabled: parent.linkTarget
                                        onTapped: page.core.openLink(item.shownText.trim())
                                    }
                                }
                                QQC2.Button {
                                    visible: item.awaitingConsent
                                    text: i18n("Accept")
                                    icon.name: "dialog-ok-apply"
                                    onClicked: page.core.accept(item.transferId)
                                }
                                QQC2.Button {
                                    visible: item.awaitingConsent
                                    text: i18n("Decline")
                                    icon.name: "dialog-cancel"
                                    onClicked: page.core.decline(item.transferId)
                                }
                                QQC2.Button {
                                    visible: item.receivedFiles
                                    text: i18n("Open folder")
                                    icon.name: "document-open-folder"
                                    onClicked: page.core.openDownloadFolder()
                                }
                                QQC2.Button {
                                    visible: item.hasText
                                    text: i18n("Copy")
                                    icon.name: "edit-copy"
                                    onClicked: {
                                        page.core.copyToClipboard(item.shownText);
                                        page.toasts.show(Kirigami.MessageType.Positive, i18n("Copied to clipboard"), 1500);
                                    }
                                }
                            }

                            QQC2.ProgressBar {
                                visible: item.inProgress
                                from: 0
                                to: item.totalBytes
                                value: item.ackBytes
                                // Updates come about twice a second: glide between them.
                                Behavior on value {
                                    NumberAnimation {
                                        duration: 500
                                    }
                                }
                                Layout.fillWidth: true
                            }
                        }
                    }
                }
            }
        }
    }
}
