import QtQuick
import qs.Commons
import qs.Ui

// Round action button with an optional caption underneath (call controls, dialpad keys).
Item {
  id: root
  property var app
  property string glyph: ""
  property string caption: ""
  property string subCaption: ""
  property real size: Style.space(52)
  property color tint: app.fg
  property bool filled: false        // solid tint background (answer / end call)
  property bool checked: false       // toggled state (mute, hold)
  property bool glyphIsText: false   // dial keys use the text font, not icons
  signal clicked()

  width: Math.max(size, captionText.implicitWidth)
  height: size + (caption ? captionText.implicitHeight + Style.space(4) : 0)

  BorderSurface {
    id: disc
    anchors.horizontalCenter: parent.horizontalCenter
    width: root.size
    height: root.size
    radius: root.size / 2
    opacity: root.enabled ? 1 : 0.35
    color: root.filled ? root.tint
      : mouse.pressed ? Util.alpha(root.tint, 0.28)
      : root.checked ? Util.alpha(root.tint, 0.22)
      : mouse.containsMouse ? Util.alpha(root.tint, 0.14)
      : Util.alpha(root.tint, 0.06)
    borderSpec: root.filled ? Border.none()
      : Border.controlSpec(root.checked ? "selected" : mouse.containsMouse ? "hover" : "normal", root.tint, app.accent, app.urgent)
    scale: mouse.pressed ? 0.94 : 1
    Behavior on color { ColorAnimation { duration: 120 } }
    Behavior on scale { NumberAnimation { duration: 90 } }

    Column {
      anchors.centerIn: parent
      spacing: 0
      Text {
        anchors.horizontalCenter: parent.horizontalCenter
        text: root.glyph
        color: root.filled ? Color.background : root.tint
        font.family: app.font
        font.pixelSize: root.glyphIsText ? root.size * 0.4 : root.size * 0.42
      }
      Text {
        anchors.horizontalCenter: parent.horizontalCenter
        visible: root.subCaption !== ""
        text: root.subCaption
        color: root.tint
        opacity: 0.5
        font.family: app.font
        font.pixelSize: Style.font.caption * 0.85
        font.letterSpacing: 1
      }
    }

    MouseArea {
      id: mouse
      anchors.fill: parent
      hoverEnabled: true
      cursorShape: Qt.PointingHandCursor
      onClicked: root.clicked()
    }
  }

  Text {
    id: captionText
    anchors.top: disc.bottom
    anchors.topMargin: Style.space(4)
    anchors.horizontalCenter: parent.horizontalCenter
    visible: root.caption !== ""
    text: root.caption
    color: app.fg
    opacity: 0.7
    font.family: app.font
    font.pixelSize: Style.font.caption
  }
}
