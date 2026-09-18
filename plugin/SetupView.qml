import QtQuick
import Quickshell
import qs.Commons
import qs.Ui

// First-run and "not ready" screen: one step at a time until the phone can take calls.
Item {
  id: root
  property var app

  readonly property string stage: app.stage
  readonly property var phone: app.state ? app.state.phone : ({})
  readonly property var devices: app.state ? app.state.devices : []
  readonly property bool waiting: stage === "needs_calls" && phone.calls === "requesting"

  function openBluetooth() {
    Quickshell.execDetached(["omarchy-shell", "omarchy.bluetooth", "open"])
    app.close()
  }

  readonly property string title: {
    switch (stage) {
    case "offline": return "Phone service not running"
    case "no_phone": return devices.length ? "Choose your phone" : "Pair your phone"
    case "not_paired": return "Pair your phone"
    case "not_connected": return phone.name + " is not connected"
    case "needs_calls":
      return waiting ? "Check your phone" : phone.calls === "denied" ? "Calls not allowed" : "Allow calls"
    }
    return ""
  }

  readonly property string detail: {
    switch (stage) {
    case "offline":
      return "Start it with “systemctl --user start quattro-bt-phoned”. This panel connects as soon as it runs."
    case "no_phone":
      return devices.length
        ? "Pick the phone to make and take calls with."
        : "Pair your phone with this computer in Bluetooth settings first."
    case "not_paired": return "Pair " + phone.name + " with this computer in Bluetooth settings."
    case "not_connected":
      return "Turn on Bluetooth on your phone and keep it nearby. Contacts and recents from the last sync stay available."
    case "needs_calls":
      if (waiting) return "Tap “Allow” when your phone asks about calls from this computer."
      if (phone.calls === "denied")
        return "Your phone refused. Turn on “Calls” for this computer in the phone’s Bluetooth device settings, then try again."
      return "Your phone asks for permission the first time this computer controls its calls."
    }
    return ""
  }

  readonly property string glyph: stage === "offline" || stage === "not_connected" ? app.icons.cellphoneOff : app.icons.cellphone

  Column {
    anchors.centerIn: parent
    width: parent.width - Style.space(40)
    spacing: Style.space(12)

    Text {
      anchors.horizontalCenter: parent.horizontalCenter
      text: root.glyph
      color: root.waiting ? app.accent : app.fg
      opacity: root.waiting ? 1 : 0.4
      font.family: app.font; font.pixelSize: Style.font.display * 2.2
      SequentialAnimation on opacity {
        running: root.waiting
        loops: Animation.Infinite
        alwaysRunToEnd: true
        NumberAnimation { from: 1; to: 0.3; duration: 700 }
        NumberAnimation { from: 0.3; to: 1; duration: 700 }
      }
    }
    Text {
      width: parent.width
      horizontalAlignment: Text.AlignHCenter
      wrapMode: Text.WordWrap
      text: root.title
      color: app.fg; font.family: app.font; font.pixelSize: Style.font.title; font.bold: true
    }
    Text {
      width: parent.width
      horizontalAlignment: Text.AlignHCenter
      wrapMode: Text.WordWrap
      text: root.detail
      color: app.fg; opacity: 0.6; font.family: app.font; font.pixelSize: Style.font.bodySmall
    }

    // Paired phones to choose from.
    Column {
      width: parent.width
      visible: root.stage === "no_phone" && root.devices.length > 0
      spacing: Style.space(2)
      Repeater {
        model: root.stage === "no_phone" ? root.devices : []
        ListRow {
          required property var modelData
          width: parent.width
          height: Style.space(44)
          app: root.app
          onClicked: app.daemon.act("select_phone", { address: modelData.address })
          Text {
            id: devGlyph
            anchors.left: parent.left
            anchors.verticalCenter: parent.verticalCenter
            text: app.icons.cellphone
            color: app.fg
            font.family: app.font; font.pixelSize: Style.font.heading
          }
          Column {
            anchors.left: devGlyph.right
            anchors.leftMargin: Style.space(10)
            anchors.verticalCenter: parent.verticalCenter
            Text {
              text: modelData.name
              color: app.fg; font.family: app.font; font.pixelSize: Style.font.body; font.bold: true
            }
            Text {
              text: modelData.connected ? "Connected" : "Paired"
              color: app.fg; opacity: 0.55; font.family: app.font; font.pixelSize: Style.font.caption
            }
          }
        }
      }
    }

    Row {
      anchors.horizontalCenter: parent.horizontalCenter
      spacing: Style.space(8)

      Button {
        visible: root.stage === "not_connected"
        text: "Connect"
        iconText: app.icons.cellphone
        foreground: app.fg
        fontFamily: app.font
        bordered: true
        onClicked: app.daemon.act("connect")
      }
      Button {
        visible: root.stage === "needs_calls" && !root.waiting
        text: root.phone.calls === "denied" ? "Try again" : "Allow calls"
        iconText: app.icons.phone
        foreground: app.fg
        fontFamily: app.font
        bordered: true
        onClicked: app.daemon.act("request_calls")
      }
      Button {
        visible: root.stage === "no_phone" || root.stage === "not_paired" || root.stage === "not_connected"
        text: "Bluetooth settings"
        foreground: app.fg
        fontFamily: app.font
        bordered: root.stage !== "not_connected"
        onClicked: root.openBluetooth()
      }
    }
  }
}
