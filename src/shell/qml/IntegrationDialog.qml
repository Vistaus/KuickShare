import QtQuick
import QtQuick.Controls as QQC2
import QtQuick.Layouts
import org.kde.kirigami as Kirigami

Kirigami.PromptDialog {
    id: dialog

    required property var core

    title: i18n("Add KuickShare to the menus?")
    iconName: "qrc:/data/icons/hicolor/scalable/apps/io.github.the_beper.KuickShare.svg"
    standardButtons: Kirigami.Dialog.Ok | Kirigami.Dialog.Cancel
    closePolicy: QQC2.Popup.NoAutoClose
    onAccepted: core.integrateAppImage()
    onRejected: core.declineIntegration()

    QQC2.Label {
        text: i18n("It will appear in the application menu and in Dolphin's context menu.")
        wrapMode: Text.Wrap
        Layout.fillWidth: true
        Layout.topMargin: Kirigami.Units.smallSpacing / 2
    }
}
