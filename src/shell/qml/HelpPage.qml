import QtQuick
import QtQuick.Controls as QQC2
import QtQuick.Layouts
import org.kde.kirigami as Kirigami
import org.kde.kirigamiaddons.formcard as FormCard

FormCard.FormCardPage {
    id: page

    required property var core

    title: i18n("Help")

    // Same padding on every page: cards float with rounded corners instead of touching the window edges.
    topPadding: Kirigami.Units.largeSpacing
    leftPadding: Kirigami.Units.largeSpacing
    rightPadding: Kirigami.Units.largeSpacing
    bottomPadding: Kirigami.Units.largeSpacing

    component Entry: FormCard.FormTextDelegate {
    }

    SectionTitle {
        first: true
        text: i18n("Getting started")
    }
    FormCard.FormCard {
        maximumWidth: width - 1
        Entry {
            icon.name: "document-send"
            text: i18n("Send")
            description: i18n("Choose or drop files, or type some text, on the Send page, then pick a device. On the phone, open Quick Share.")
        }
        Entry {
            icon.name: "visibility"
            text: i18n("Be found")
            description: i18n("Keep “Visible to nearby devices” on in Preferences. The phone must be set to Everyone to be found, and Android sets it back after 10 minutes.")
        }
        Entry {
            icon.name: "folder-download-symbolic"
            text: i18n("Receive")
            description: i18n("An incoming transfer asks for your approval in a notification. Received files go to the download folder chosen in Preferences.")
        }
    }

    SectionTitle {
        text: i18n("Paired and trusted devices")
    }
    FormCard.FormCard {
        maximumWidth: width - 1
        Entry {
            icon.name: "security-high"
            text: i18n("Paired computers")
            description: i18n("Two computers running KuickShare can pair with an 8-digit code, then send to each other without asking. The port 47891 must be reachable.")
        }
        Entry {
            icon.name: "security-high"
            text: i18n("Trusted devices")
            description: i18n("“Accept and trust” on a notification accepts a phone without asking from then on. It is recognised by name and MAC address, so only on the same network.")
        }
    }

    SectionTitle {
        text: i18n("Troubleshooting")
    }
    FormCard.FormCard {
        maximumWidth: width - 1
        Entry {
            icon.name: "help-hint"
            text: i18n("The phone does not see this computer")
            description: i18n("Check that “Visible to nearby devices” is on, that Do Not Disturb is off (it hides this computer) and that both are on the same network.")
        }
        Entry {
            icon.name: "help-hint"
            text: i18n("This computer does not see the phone")
            description: i18n("Set the phone to Everyone and keep its Quick Share screen open.")
        }
        Entry {
            icon.name: "help-hint"
            text: i18n("Firewall")
            description: i18n("Discovery uses mDNS. The transfer port is random unless you fix one in Preferences: open that port in the firewall.")
        }
    }
}
