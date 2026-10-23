pragma ComponentBehavior: Bound
import QtQuick
import QtQuick.Controls as QQC2
import org.kde.kirigami as Kirigami

// Short coloured messages over the window: each one slides up from the bottom edge, older ones are pushed
// up, and every message disappears by itself after its time (or when closed).
Item {
    id: stack

    readonly property int maxVisible: 4
    property int lastId: 0

    // `type` is a Kirigami.MessageType; a message that stays 3 s or less has no close button.
    function show(type, text, timeout) {
        if (model.count >= maxVisible) {
            model.remove(model.count - 1);
        }
        model.insert(0, {
            uid: ++lastId,
            kind: type,
            body: text,
            timeout: timeout
        });
    }

    function dismiss(uid) {
        for (let i = 0; i < model.count; ++i) {
            if (model.get(i).uid === uid) {
                model.remove(i);
                return;
            }
        }
    }

    parent: QQC2.Overlay.overlay
    z: 100
    width: Math.round(parent.width * 0.7)
    height: list.contentHeight
    anchors.bottom: parent.bottom
    anchors.bottomMargin: Kirigami.Units.gridUnit * 2
    anchors.horizontalCenter: parent.horizontalCenter

    ListView {
        id: list

        anchors.fill: parent
        interactive: false
        spacing: Kirigami.Units.smallSpacing
        // The newest message (index 0) sits at the bottom.
        verticalLayoutDirection: ListView.BottomToTop

        model: ListModel {
            id: model
        }

        add: Transition {
            ParallelAnimation {
                NumberAnimation {
                    property: "y"
                    from: ViewTransition.destination.y + Kirigami.Units.gridUnit * 4
                    duration: Kirigami.Units.longDuration
                    easing.type: Easing.OutCubic
                }
                NumberAnimation {
                    property: "opacity"
                    from: 0
                    to: 1
                    duration: Kirigami.Units.longDuration
                }
            }
        }
        displaced: Transition {
            NumberAnimation {
                property: "y"
                duration: Kirigami.Units.longDuration
                easing.type: Easing.OutCubic
            }
        }
        remove: Transition {
            NumberAnimation {
                property: "opacity"
                to: 0
                duration: Kirigami.Units.shortDuration
            }
        }

        delegate: Kirigami.InlineMessage {
            id: toast

            required property int uid
            required property int kind
            required property string body
            required property int timeout

            width: ListView.view.width
            // Hidden by default; the close button hides it again.
            visible: true
            type: kind
            text: body
            showCloseButton: timeout > 3000
            onVisibleChanged: {
                if (!visible) {
                    Qt.callLater(stack.dismiss, uid);
                }
            }

            Timer {
                running: toast.timeout > 0
                interval: toast.timeout
                onTriggered: stack.dismiss(toast.uid)
            }
        }
    }
}
