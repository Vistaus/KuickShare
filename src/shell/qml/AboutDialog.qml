import QtQuick
import QtQuick.Controls as QQC2
import QtQuick.Layouts
import org.kde.kirigami as Kirigami

// About KuickShare: the name and version, the license, the projects it builds on, the libraries in use.
// Everything sits directly on the dialog, without boxes.
Kirigami.Dialog {
    id: dialog

    required property var core

    // A row with a title and a line under it; with an address it is a link that opens in the browser.
    component Entry: Item {
        id: entry

        property string title
        property string description
        property string url

        implicitHeight: content.implicitHeight
        Layout.fillWidth: true

        RowLayout {
            id: content

            width: parent.width
            spacing: Kirigami.Units.largeSpacing

            ColumnLayout {
                spacing: 0
                Layout.fillWidth: true

                QQC2.Label {
                    text: entry.title
                    color: entry.url.length > 0 ? Kirigami.Theme.linkColor : Kirigami.Theme.textColor
                    font.underline: entry.url.length > 0 && hover.hovered
                    wrapMode: Text.Wrap
                    Layout.fillWidth: true
                }
                QQC2.Label {
                    visible: entry.description.length > 0
                    text: entry.description
                    opacity: 0.7
                    wrapMode: Text.Wrap
                    Layout.fillWidth: true
                }
            }
            Kirigami.Icon {
                visible: entry.url.length > 0
                source: "external-link-symbolic"
                color: Kirigami.Theme.linkColor
                Layout.preferredWidth: Kirigami.Units.iconSizes.small
                Layout.preferredHeight: Kirigami.Units.iconSizes.small
                Layout.alignment: Qt.AlignTop
            }
        }
        HoverHandler {
            id: hover
            enabled: entry.url.length > 0
            cursorShape: Qt.PointingHandCursor
        }
        TapHandler {
            enabled: entry.url.length > 0
            onTapped: Qt.openUrlExternally(entry.url)
        }
    }

    // The room around the header icon and the close button, the same on every side, and the room around the content.
    readonly property real margin: Kirigami.Units.largeSpacing
    readonly property real contentMargin: Kirigami.Units.largeSpacing * 2
    readonly property real headerSize: Kirigami.Units.iconSizes.medium

    title: i18n("About")
    standardButtons: Kirigami.Dialog.NoButton
    footer: null
    preferredWidth: Kirigami.Units.gridUnit * 24
    // Room above and below for the opening animation, which moves the dialog: a dialog as tall as the window
    // would be clamped to it and stretch while it moves. A taller content scrolls.
    maximumHeight: absoluteMaximumHeight - Kirigami.Units.gridUnit * 3

    header: QQC2.Control {
        // Every side set on its own: the desktop style ignores `padding` for the top and bottom.
        // The header spans the dialog border (1 px at the top and the sides); the content sits inside it.
        topPadding: dialog.margin + 1
        bottomPadding: dialog.margin
        leftPadding: dialog.margin + 1
        rightPadding: dialog.margin + 1
        // The icon and the close button are the same size, so the header has the same room above, below and at the
        // sides of what you see: the icon, and the box of the button (it shows when focused).
        contentItem: RowLayout {
            spacing: Kirigami.Units.largeSpacing

            Kirigami.Icon {
                source: "help-about-symbolic"
                Layout.preferredWidth: dialog.headerSize
                Layout.preferredHeight: dialog.headerSize
            }
            Kirigami.Heading {
                level: 3
                text: dialog.title
                Layout.fillWidth: true
            }
            QQC2.ToolButton {
                icon.name: "window-close"
                icon.width: Kirigami.Units.iconSizes.smallMedium
                icon.height: Kirigami.Units.iconSizes.smallMedium
                display: QQC2.AbstractButton.IconOnly
                text: i18n("Close")
                Layout.preferredWidth: dialog.headerSize
                Layout.preferredHeight: dialog.headerSize
                onClicked: dialog.close()
            }
        }
    }

    // The dialog only sizes itself from an Item's implicitHeight, and it scrolls the content when the window is too
    // short. A ScrollView of our own that fills the dialog makes the height depend on the position, and the
    // opening animation (which moves the dialog) then stretches the dialog.
    // The content has its own margin on every side.
    Item {
        implicitHeight: column.implicitHeight + dialog.contentMargin * 2

        ColumnLayout {
            id: column

            anchors {
                left: parent.left
                right: parent.right
                top: parent.top
                leftMargin: dialog.contentMargin
                rightMargin: dialog.contentMargin
                topMargin: dialog.contentMargin
            }
            spacing: Kirigami.Units.largeSpacing

            RowLayout {
                spacing: Kirigami.Units.largeSpacing * 2
                Layout.fillWidth: true

                Kirigami.Icon {
                    source: "io.github.the_beper.KuickShare"
                    fallback: "emblem-shared"
                    Layout.preferredWidth: Kirigami.Units.iconSizes.huge
                    Layout.preferredHeight: Kirigami.Units.iconSizes.huge
                }
                ColumnLayout {
                    spacing: 0
                    Layout.fillWidth: true

                    Kirigami.Heading {
                        level: 2
                        text: i18n("KuickShare %1", dialog.core.version)
                    }
                    QQC2.Label {
                        text: i18n("Quick Share client for KDE")
                        opacity: 0.7
                        wrapMode: Text.Wrap
                        Layout.fillWidth: true
                    }
                    QQC2.Label {
                        text: "© 2026 the-beper"
                        opacity: 0.7
                    }
                }
            }

            Kirigami.Separator {
                Layout.fillWidth: true
            }
            Entry {
                title: i18n("Homepage")
                url: "https://github.com/the-beper/KuickShare"
            }
            Entry {
                title: i18n("Report a Bug")
                url: "https://github.com/the-beper/KuickShare/issues"
            }

            SectionTitle {
                text: i18n("License")
                Layout.leftMargin: 0
            }
            Entry {
                title: i18n("GPL-3.0-or-later")
                url: "https://www.gnu.org/licenses/gpl-3.0.html"
            }

            SectionTitle {
                text: i18n("Built on")
                Layout.leftMargin: 0
            }
            Entry {
                title: i18n("Packet")
                description: i18n("The Quick Share client this started from, by nozwock")
                url: "https://github.com/nozwock/packet"
            }
            Entry {
                title: i18n("rquickshare")
                description: i18n("The Quick Share protocol library (rqs_lib), by nozwock and Martichou")
                url: "https://github.com/nozwock/rquickshare"
            }

            SectionTitle {
                text: i18n("Libraries in use")
                Layout.leftMargin: 0
            }
            Entry {
                title: i18n("Qt %1", dialog.core.qtVersion)
                description: i18n("Cross-platform application framework")
            }
            Entry {
                title: i18n("KDE Frameworks %1", dialog.core.frameworksVersion)
                description: i18n("Kirigami, notifications, jobs and the other KDE libraries")
            }
        }
    }
}
