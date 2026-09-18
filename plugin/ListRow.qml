import QtQuick
import qs.Commons

// Hoverable list row. Put content inside; `clicked` fires on the whole row.
Rectangle {
  id: root
  property var app
  property bool highlighted: false
  default property alias content: holder.data
  signal clicked()

  radius: Style.cornerRadius
  color: mouse.pressed ? Style.pressedFillFor(app.fg, app.accent, app.urgent)
    : (mouse.containsMouse || highlighted) ? Style.hoverFillFor(app.fg, app.accent, app.urgent)
    : "transparent"

  MouseArea {
    id: mouse
    anchors.fill: parent
    hoverEnabled: true
    cursorShape: Qt.PointingHandCursor
    onClicked: root.clicked()
  }

  Item {
    id: holder
    anchors.fill: parent
    anchors.leftMargin: Style.space(8)
    anchors.rightMargin: Style.space(8)
  }
}
