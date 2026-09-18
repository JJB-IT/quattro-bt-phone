import QtQuick
import qs.Commons
import qs.Ui
import "Format.js" as Format

Item {
  id: root
  property var app
  property int expanded: -1
  readonly property var model: Format.contactsWithHeaders(app.contacts, search.text)
  readonly property var sync: app.state ? app.state.sync : ({})
  readonly property string permission: app.state ? app.state.phone.contacts : "unknown"
  // The phone is (or may be) showing "Allow access to contacts?".
  readonly property bool approving: sync.status === "awaiting_approval"
  // Never synced: offer to ask the phone for access.
  readonly property bool needsAccess: !approving && !sync.last_synced && sync.status !== "syncing"

  Binding { target: app; property: "editing"; value: search.activeFocus; when: root.visible }

  Column {
    anchors.fill: parent
    anchors.topMargin: Style.space(4)
    spacing: Style.space(8)

    TextField {
      id: search
      width: parent.width
      enabled: !root.approving && !root.needsAccess
      placeholderText: app.icons.search + "  Search " + app.contacts.length + " contacts…"
      font.family: app.font
      font.pixelSize: Style.font.body
      foreground: app.fg
      onTextChanged: root.expanded = -1
      Keys.onEscapePressed: { if (text) text = ""; else focus = false }
    }

    // Access to contacts: ask, or wait while the phone asks the user.
    BorderSurface {
      id: accessCard
      visible: root.approving || root.needsAccess
      width: parent.width
      height: approveCol.implicitHeight + Style.space(24)
      radius: Style.cornerRadius
      color: Util.alpha(app.accent, 0.1)
      borderSpec: Border.controlSpec("selected", app.fg, app.accent)

      Column {
        id: approveCol
        anchors.centerIn: parent
        width: parent.width - Style.space(24)
        spacing: Style.space(8)
        Text {
          anchors.horizontalCenter: parent.horizontalCenter
          text: app.icons.cellphone
          color: app.accent
          font.family: app.font
          font.pixelSize: Style.font.display * 1.4
          SequentialAnimation on opacity {
            running: root.approving
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
          text: root.approving ? "Check your phone" : "Show your contacts here"
          color: app.fg; font.family: app.font; font.pixelSize: Style.font.title; font.bold: true
        }
        Text {
          width: parent.width
          horizontalAlignment: Text.AlignHCenter
          wrapMode: Text.WordWrap
          text: root.approving
            ? "If your phone asks to allow access to contacts and call history, tap “Allow”."
            : root.permission === "denied"
              ? "Your phone didn’t allow access. Turn on contact sharing for this computer in the phone’s Bluetooth settings, then try again."
              : "Contacts and call history are copied from your phone. They are only read, never changed."
          color: app.fg; opacity: 0.7; font.family: app.font; font.pixelSize: Style.font.bodySmall
        }
        Button {
          anchors.horizontalCenter: parent.horizontalCenter
          visible: !root.approving
          text: root.permission === "denied" ? "Try again" : "Allow contacts"
          iconText: app.icons.contacts
          foreground: app.fg
          fontFamily: app.font
          bordered: true
          onClicked: app.daemon.act("request_contacts")
        }
      }
    }

    ListView {
      id: list
      width: parent.width
      height: parent.height - search.height - footer.height - Style.space(16)
              - (accessCard.visible ? accessCard.height + Style.space(8) : 0)
      clip: true
      visible: !root.needsAccess || app.contacts.length > 0
      opacity: root.approving ? 0.3 : 1
      model: root.model
      boundsBehavior: Flickable.StopAtBounds
      spacing: Style.space(2)

      delegate: Column {
        id: row
        required property var modelData
        required property int index
        readonly property bool open: root.expanded === index
        width: list.width

        Text {
          visible: row.modelData.first
          width: parent.width
          topPadding: Style.space(6)
          bottomPadding: Style.space(2)
          leftPadding: Style.space(8)
          text: row.modelData.letter
          color: app.accent
          font.family: app.font
          font.pixelSize: Style.font.caption
          font.bold: true
        }

        ListRow {
          width: parent.width
          height: Style.space(46)
          app: root.app
          highlighted: row.open
          onClicked: root.expanded = row.open ? -1 : row.index

          Avatar {
            id: av
            app: root.app
            name: row.modelData.name
            photo: row.modelData.photo
            size: Style.space(32)
            anchors.verticalCenter: parent.verticalCenter
          }
          Column {
            anchors.left: av.right
            anchors.leftMargin: Style.space(10)
            anchors.right: callIcon.left
            anchors.verticalCenter: parent.verticalCenter
            Text {
              text: row.modelData.name
              width: parent.width
              elide: Text.ElideRight
              color: app.fg; font.family: app.font; font.pixelSize: Style.font.body; font.bold: true
            }
            Text {
              text: row.modelData.numbers[0].label + " · " + row.modelData.numbers[0].number
                    + (row.modelData.numbers.length > 1 ? "  +" + (row.modelData.numbers.length - 1) + " more" : "")
              width: parent.width
              elide: Text.ElideRight
              color: app.fg; opacity: 0.6; font.family: app.font; font.pixelSize: Style.font.caption
            }
          }
          Text {
            id: callIcon
            anchors.right: parent.right
            anchors.verticalCenter: parent.verticalCenter
            text: app.icons.phone
            color: callMouse.containsMouse ? app.accent : app.fg
            opacity: callMouse.containsMouse ? 1 : 0.6
            font.family: app.font
            font.pixelSize: Style.font.heading
            MouseArea {
              id: callMouse
              anchors.fill: parent
              anchors.margins: -Style.space(8)
              hoverEnabled: true
              cursorShape: Qt.PointingHandCursor
              onClicked: app.dial(row.modelData.numbers[0].number)
            }
          }
        }

        // Expanded: every number, tap to call
        Repeater {
          model: row.open ? row.modelData.numbers : []
          ListRow {
            required property var modelData
            width: row.width
            height: Style.space(32)
            app: root.app
            onClicked: app.dial(modelData.number)
            Text {
              anchors.left: parent.left
              anchors.leftMargin: Style.space(42)
              anchors.verticalCenter: parent.verticalCenter
              text: modelData.label.toUpperCase()
              color: app.fg; opacity: 0.5; font.family: app.font; font.pixelSize: Style.font.caption; font.letterSpacing: 1
            }
            Text {
              anchors.left: parent.left
              anchors.leftMargin: Style.space(110)
              anchors.verticalCenter: parent.verticalCenter
              text: modelData.number
              color: app.fg; font.family: app.font; font.pixelSize: Style.font.bodySmall
            }
            Text {
              anchors.right: parent.right
              anchors.verticalCenter: parent.verticalCenter
              text: app.icons.phone
              color: app.accent; font.family: app.font; font.pixelSize: Style.font.body
            }
          }
        }
      }

      Text {
        anchors.centerIn: parent
        visible: list.count === 0 && !accessCard.visible
        text: search.text ? "No contacts match “" + search.text + "”" : "No contacts yet"
        color: app.fg; opacity: 0.5; font.family: app.font; font.pixelSize: Style.font.bodySmall
      }
    }

    Item {
      id: footer
      width: parent.width
      height: Style.space(18)
      Text {
        anchors.left: parent.left
        anchors.right: refresh.left
        anchors.verticalCenter: parent.verticalCenter
        elide: Text.ElideRight
        text: root.sync.status === "syncing" ? "Syncing…"
            : root.approving ? "Waiting for approval…"
            : root.sync.status === "error" ? "Sync failed: " + (root.sync.error || "")
            : root.sync.last_synced ? "Synced " + Format.ago(root.sync.last_synced, app.now) + " · " + root.sync.contacts + " contacts"
            : ""
        color: root.sync.status === "error" ? app.urgent : app.fg
        opacity: root.sync.status === "error" ? 1 : 0.45
        font.family: app.font; font.pixelSize: Style.font.caption
      }
      Text {
        id: refresh
        anchors.right: parent.right
        anchors.verticalCenter: parent.verticalCenter
        visible: !!root.sync.last_synced && root.sync.status !== "syncing" && !root.approving
        text: app.icons.refresh
        color: app.fg
        opacity: refreshMouse.containsMouse ? 1 : 0.45
        font.family: app.font; font.pixelSize: Style.font.body
        MouseArea {
          id: refreshMouse
          anchors.fill: parent
          anchors.margins: -Style.space(6)
          hoverEnabled: true
          cursorShape: Qt.PointingHandCursor
          onClicked: app.daemon.act("sync")
        }
      }
    }
  }
}
