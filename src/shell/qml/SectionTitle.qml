import QtQuick
import QtQuick.Layouts
import org.kde.kirigami as Kirigami

Kirigami.Heading {
    // The first title of a page needs no space above it: the page padding is enough.
    property bool first: false

    level: 4
    font.weight: Font.DemiBold
    Layout.fillWidth: true
    Layout.topMargin: first ? 0 : Kirigami.Units.largeSpacing * 2
    Layout.bottomMargin: Kirigami.Units.smallSpacing * 2
    Layout.leftMargin: Kirigami.Units.largeSpacing
}
