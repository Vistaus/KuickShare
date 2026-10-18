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

    title: i18n("About")
    standardButtons: Kirigami.Dialog.NoButton
    footer: null
    preferredWidth: Kirigami.Units.gridUnit * 24

    header: QQC2.Control {
        padding: Kirigami.Units.largeSpacing * 2
        contentItem: RowLayout {
            spacing: Kirigami.Units.largeSpacing

            Kirigami.Icon {
                source: "help-about-symbolic"
                Layout.preferredWidth: Kirigami.Units.iconSizes.smallMedium
                Layout.preferredHeight: Kirigami.Units.iconSizes.smallMedium
            }
            Kirigami.Heading {
                level: 3
                text: dialog.title
                Layout.fillWidth: true
            }
            QQC2.ToolButton {
                icon.name: "window-close"
                display: QQC2.AbstractButton.IconOnly
                text: i18n("Close")
                onClicked: dialog.close()
            }
        }
    }

    QQC2.ScrollView {
        id: scroll

        // The whole content: top margin, then the same room under it as at the sides (the dialog scrolls only
        // if the window is too short for it).
        implicitHeight: column.implicitHeight + Kirigami.Units.largeSpacing * 3
        contentWidth: availableWidth
        Layout.fillWidth: true
        Layout.fillHeight: true

        ColumnLayout {
            id: column

            x: Kirigami.Units.largeSpacing * 2
            y: Kirigami.Units.largeSpacing
            width: scroll.availableWidth - Kirigami.Units.largeSpacing * 4
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
                }
            }

            Kirigami.Separator {
                Layout.fillWidth: true
            }
            ColumnLayout {
                spacing: 0

                QQC2.Label {
                    text: i18n("Copyright")
                }
                QQC2.Label {
                    text: "© 2026 the-beper"
                    opacity: 0.7
                }
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
