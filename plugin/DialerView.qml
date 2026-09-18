import QtQuick
import qs.Commons
import "Format.js" as Format

Item {
  id: root
  property var app

  readonly property var keys: [
    ["1", ""], ["2", "ABC"], ["3", "DEF"],
    ["4", "GHI"], ["5", "JKL"], ["6", "MNO"],
    ["7", "PQRS"], ["8", "TUV"], ["9", "WXYZ"],
    ["*", ""], ["0", "+"], ["#", ""]
  ]
  readonly property var suggestions: Format.matchNumber(app.contacts, app.dialled)

  Column {
    anchors.fill: parent
    anchors.topMargin: Style.space(8)
    spacing: Style.space(10)

    // Number display
    Item {
      width: parent.width
      height: Style.space(40)

      Text {
        anchors.centerIn: parent
        width: parent.width - Style.space(80)
        horizontalAlignment: Text.AlignHCenter
        text: app.dialled.length ? app.dialled : "Type or click a number"
        color: app.fg
        opacity: app.dialled.length ? 1 : 0.35
        font.family: app.font
        font.pixelSize: app.dialled.length ? Style.font.display : Style.font.body
        elide: Text.ElideLeft
      }
      Text {
        anchors.right: parent.right
        anchors.verticalCenter: parent.verticalCenter
        visible: app.dialled.length > 0
        text: app.icons.backspace
        color: app.fg
        opacity: back.containsMouse ? 1 : 0.6
        font.family: app.font
        font.pixelSize: Style.font.display * 0.8
        MouseArea {
          id: back
          anchors.fill: parent
          anchors.margins: -Style.space(6)
          hoverEnabled: true
          cursorShape: Qt.PointingHandCursor
          onClicked: app.dialled = app.dialled.slice(0, -1)
          onPressAndHold: app.dialled = ""
        }
      }
    }

    // Contact match for the typed digits
    Item {
      width: parent.width
      height: Style.space(40)

      ListRow {
        anchors.fill: parent
        app: root.app
        visible: root.suggestions.length > 0
        onClicked: {
          app.dial(root.suggestions[0].number)
        }
        Row {
          anchors.verticalCenter: parent.verticalCenter
          spacing: Style.space(10)
          Avatar {
            app: root.app
            name: root.suggestions.length ? root.suggestions[0].name : ""
            photo: root.suggestions.length ? root.suggestions[0].photo : ""
            size: Style.space(28)
          }
          Column {
            anchors.verticalCenter: parent.verticalCenter
            Text {
              text: root.suggestions.length ? root.suggestions[0].name : ""
              color: app.fg; font.family: app.font; font.pixelSize: Style.font.body; font.bold: true
            }
            Text {
              text: root.suggestions.length ? root.suggestions[0].label + " · " + root.suggestions[0].number : ""
              color: app.fg; opacity: 0.6; font.family: app.font; font.pixelSize: Style.font.caption
            }
          }
        }
      }
    }

    // Keypad
    Grid {
      id: pad
      anchors.horizontalCenter: parent.horizontalCenter
      columns: 3
      columnSpacing: Style.space(26)
      rowSpacing: Style.space(10)

      Repeater {
        model: root.keys
        CallButton {
          required property var modelData
          app: root.app
          glyph: modelData[0]
          subCaption: modelData[1]
          glyphIsText: true
          size: Style.space(56)
          onClicked: app.dialled += modelData[0]
        }
      }
    }

    Item { width: 1; height: Style.space(2) }

    CallButton {
      anchors.horizontalCenter: parent.horizontalCenter
      app: root.app
      glyph: app.icons.phone
      tint: app.accent
      filled: true
      size: Style.space(60)
      opacity: app.dialled.length ? 1 : 0.4
      onClicked: if (app.dialled.length) app.dial(app.dialled)
    }
  }
}
