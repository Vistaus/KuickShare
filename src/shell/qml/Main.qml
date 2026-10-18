pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Window
import QtQuick.Controls as QQC2
import QtQuick.Templates as T
import QtQuick.Layouts
import org.kde.kirigami as Kirigami

Kirigami.ApplicationWindow {
    id: root

    required property var core
    property string startPage: "send"

    property string currentPage: ""
    // Files queued for sending; filled from the Send page or by dropping onto the window.
    property var outgoingFiles: []

    title: i18n("KuickShare")
    // Compact, fixed size (so it cannot be maximized or resized): the pages scroll when needed.
    readonly property real fixedWidth: Kirigami.Units.gridUnit * 26
    readonly property real fixedHeight: Math.min(Kirigami.Units.gridUnit * 37, Screen.desktopAvailableHeight * 0.9)
    width: fixedWidth
    height: fixedHeight
    minimumWidth: fixedWidth
    maximumWidth: fixedWidth
    minimumHeight: fixedHeight
    maximumHeight: fixedHeight
    flags: Qt.Window | Qt.WindowTitleHint | Qt.WindowSystemMenuHint | Qt.WindowMinimizeButtonHint | Qt.WindowCloseButtonHint
    // The window is shown from C++ unless the app was started with --background.
    visible: false

    function show(name) {
        if (name === currentPage) {
            return;
        }
        currentPage = name;
        pageStack.clear();
        const page = pageStack.push(name === "help" ? helpPage : name === "settings" ? settingsPage : name === "transfers" ? transfersPage : sendPage);
        if (page) {
            pageFade.target = page;
            pageFade.restart();
        }
    }

    // The new page fades in, on top of the stack's own slide.
    NumberAnimation {
        id: pageFade
        property: "opacity"
        from: 0
        to: 1
        duration: Kirigami.Units.longDuration
    }

    function openFiles() {
        show("send");
        Qt.callLater(() => root.pageStack.currentItem?.openFileDialog?.());
    }
    function pasteClipboard() {
        show("send");
        Qt.callLater(() => root.pageStack.currentItem?.pasteFromClipboard?.());
    }
    function showSettings() {
        show("settings");
    }

    // Quitting stops the sharing service: ask first if something is being sent or received.
    function requestQuit() {
        if (core.transferActive()) {
            quitDialog.open();
        } else {
            Qt.quit();
        }
    }

    Kirigami.PromptDialog {
        id: quitDialog
        title: i18n("Quit KuickShare?")
        subtitle: i18n("A transfer is in progress. Quitting stops it.")
        standardButtons: Kirigami.Dialog.Ok | Kirigami.Dialog.Cancel
        onAccepted: Qt.quit()
    }

    function queueFiles(paths) {
        const files = outgoingFiles.slice();
        for (const path of paths) {
            if (files.indexOf(path) < 0) {
                files.push(path);
            }
        }
        outgoingFiles = files;
        show("send");
    }

    function addFiles(urls) {
        const added = core.filesFromUrls(urls);
        const files = outgoingFiles.slice();
        for (const path of added) {
            if (files.indexOf(path) < 0) {
                files.push(path);
            }
        }
        outgoingFiles = files;
    }

    // Closing the window keeps the app running in the tray.
    // Without the tray icon there is no way back to a hidden window, so closing it quits the app.
    onClosing: close => {
        close.accepted = false;
        if (core.trayIcon) {
            root.hide();
        } else {
            root.requestQuit();
        }
    }

    Connections {
        target: root.core
        function onErrorOccurred(message) {
            root.showPassiveNotification(message, "long");
        }
    }

    pageStack.globalToolBar.style: Kirigami.ApplicationHeaderStyle.None

    header: Item {
        implicitHeight: tabs.implicitHeight

        // The bar spans the whole header, so its own line runs under "More" as well; the room for
        // the button is taken from its padding.
        Kirigami.NavigationTabBar {
            id: tabs

            anchors.fill: parent
            // The stock bar centres its tabs with extra side padding; here they start at the edge, and the
            // room at the right is the button plus a small gap.
            leftPadding: 0
            rightPadding: more.width + Kirigami.Units.largeSpacing

            delegate: Kirigami.NavigationTabButton {
                id: tab

                required property T.Action modelData

                parent: tabs.contentItem
                action: modelData
                // The same workaround as the stock delegate: without it the tabs are not checkable.
                onActionChanged: action.checkable = true
                display: T.AbstractButton.TextBesideIcon

                Layout.minimumWidth: tabs.buttonWidth
                Layout.maximumWidth: tabs.buttonWidth
                Layout.fillHeight: true

                contentItem: RowLayout {
                    spacing: Kirigami.Units.largeSpacing

                    Item {
                        Layout.fillWidth: true
                    }
                    Kirigami.Icon {
                        source: tab.icon.name
                        color: tab.icon.color
                        Layout.preferredWidth: Kirigami.Units.iconSizes.smallMedium
                        Layout.preferredHeight: Kirigami.Units.iconSizes.smallMedium
                    }
                    QQC2.Label {
                        text: tab.text
                        color: Kirigami.Theme.textColor
                    }
                    Item {
                        Layout.fillWidth: true
                    }
                }
            }

            actions: [
                Kirigami.Action {
                    text: i18n("Send")
                    icon.name: "document-send"
                    checked: root.currentPage === "send"
                    shortcut: "Ctrl+1"
                    onTriggered: root.show("send")
                },
                Kirigami.Action {
                    text: i18n("Transfers")
                    icon.name: "folder-sync"
                    checked: root.currentPage === "transfers"
                    shortcut: "Ctrl+2"
                    onTriggered: root.show("transfers")
                }
            ]
        }
        // A short vertical line midway between the visible edge of the last tab and the button; it stays
        // clear of the line under the header.
        Kirigami.Separator {
            x: Math.round(((tabs.width - tabs.rightPadding - Kirigami.Units.smallSpacing) + more.x) / 2)
            height: Math.round(parent.height * 0.55)
            anchors.verticalCenter: parent.verticalCenter
        }
        QQC2.ToolButton {
            id: more

            text: i18n("More")
            icon.name: "open-menu-symbolic"
            // Only the icon; the text is for screen readers.
            display: QQC2.AbstractButton.IconOnly
            down: root.currentPage === "settings" || root.currentPage === "help"
            anchors {
                right: parent.right
                rightMargin: Kirigami.Units.smallSpacing + tabs.leftPadding
                verticalCenter: parent.verticalCenter
            }
            onClicked: moreMenu.open()
        }
    }

    Kirigami.Action {
        id: settingsAction
        text: i18n("Preferences")
        icon.name: "configure"
        shortcut: "Ctrl+,"
        onTriggered: root.show("settings")
    }
    Kirigami.Action {
        id: shortcutsAction
        text: i18n("Shortcuts")
        icon.name: "input-keyboard-symbolic"
        shortcut: "Ctrl+?"
        onTriggered: shortcutsDialog.open()
    }
    Kirigami.Action {
        id: quitAction
        text: i18n("Quit")
        icon.name: "application-exit"
        shortcut: StandardKey.Quit
        onTriggered: root.requestQuit()
    }
    Kirigami.Action {
        id: helpAction
        text: i18n("Help")
        icon.name: "help-contents"
        shortcut: "Ctrl+H"
        onTriggered: root.show("help")
    }
    Kirigami.Action {
        id: aboutAction
        text: i18n("About")
        icon.name: "help-about-symbolic"
        onTriggered: aboutDialog.open()
    }
    QQC2.Menu {
        id: moreMenu

        parent: root.contentItem
        x: parent.width - width - Kirigami.Units.largeSpacing
        y: Kirigami.Units.smallSpacing

        QQC2.MenuItem {
            action: settingsAction
        }
        QQC2.MenuItem {
            action: shortcutsAction
        }
        QQC2.MenuItem {
            action: helpAction
        }
        QQC2.MenuItem {
            action: aboutAction
        }
        QQC2.MenuSeparator {
        }
        QQC2.MenuItem {
            action: quitAction
        }
    }

    Component {
        id: sendPage
        SendPage {
            core: root.core
            files: root.outgoingFiles
            dragging: dropArea.containsDrag
            onFilesAdded: urls => root.addFiles(urls)
            onFileRemoved: path => root.outgoingFiles = root.outgoingFiles.filter(p => p !== path)
            onFilesCleared: root.outgoingFiles = []
            onSent: root.show("transfers")
        }
    }
    Component {
        id: transfersPage
        TransfersPage {
            core: root.core
        }
    }
    Component {
        id: settingsPage
        SettingsPage {
            core: root.core
        }
    }
    ShortcutsDialog {
        id: shortcutsDialog
    }
    AboutDialog {
        id: aboutDialog
        core: root.core
    }

    Component {
        id: helpPage
        HelpPage {
            core: root.core
        }
    }

    // Keyboard shortcuts (the tabs have theirs on the actions above). They are listed in Help.
    Shortcut {
        sequence: "Ctrl+Shift+/"
        onActivated: shortcutsDialog.open()
    }
    Shortcut {
        sequence: "F1"
        onActivated: root.show("help")
    }
    Shortcut {
        sequence: StandardKey.Open
        onActivated: {
            root.show("send");
            // The page may have just been created: ask it once it exists.
            Qt.callLater(() => root.pageStack.currentItem?.openFileDialog?.());
        }
    }
    // On the Send page, pasting anywhere (outside the text box, which has its own) adds what is on the clipboard.
    Shortcut {
        sequences: [StandardKey.Paste]
        enabled: root.currentPage === "send"
        onActivated: root.pasteClipboard()
    }
    Shortcut {
        sequence: "Ctrl+L"
        onActivated: {
            root.outgoingFiles = [];
            root.pageStack.currentItem?.clearText?.();
        }
    }
    Shortcut {
        sequences: [StandardKey.Close]
        onActivated: root.close()
    }

    // The sharing service is not running (it stopped, or never started): offer to start it.
    Kirigami.InlineMessage {
        parent: root.contentItem
        z: 10
        anchors {
            top: parent.top
            left: parent.left
            right: parent.right
            margins: Kirigami.Units.largeSpacing
        }
        visible: !root.core.connected && !root.core.serviceStarting
        type: Kirigami.MessageType.Error
        text: i18n("The sharing service is not running. Nothing can be sent or received until it is started.")
        actions: [
            Kirigami.Action {
                text: i18n("Start service")
                icon.name: "media-playback-start"
                onTriggered: root.core.startService()
            }
        ]
    }

    DropArea {
        id: dropArea
        anchors.fill: parent
        onDropped: drop => {
            if (drop.hasUrls) {
                root.addFiles(drop.urls);
                root.show("send");
            }
        }
    }

    Component.onCompleted: Qt.callLater(show, startPage)
}
