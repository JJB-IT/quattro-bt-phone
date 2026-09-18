import QtQuick
import QtQuick.Effects
import qs.Commons
import qs.Ui
import "Format.js" as Format

// The contact's photo when the phone shared one, otherwise their initials.
BorderSurface {
  id: root
  property var app
  property string name: ""
  property string photo: ""
  property real size: Style.space(36)

  readonly property bool hasPhoto: root.photo !== "" && image.status === Image.Ready

  width: size
  height: size
  radius: size / 2
  color: Util.alpha(app.accent, 0.22)
  borderSpec: hasPhoto ? Border.none() : Border.controlSpec("normal", app.fg, app.accent)

  Text {
    anchors.centerIn: parent
    visible: !root.hasPhoto
    text: root.name ? Format.initials(root.name) : app.icons.phone
    color: app.fg
    font.family: app.font
    font.pixelSize: root.size * 0.38
    font.bold: true
  }

  Image {
    id: image
    anchors.fill: parent
    visible: false
    source: root.photo
    sourceSize.width: root.size * 2
    sourceSize.height: root.size * 2
    fillMode: Image.PreserveAspectCrop
    asynchronous: true
  }
  Rectangle {
    id: mask
    anchors.fill: parent
    radius: width / 2
    visible: false
    layer.enabled: true
  }
  MultiEffect {
    anchors.fill: parent
    visible: root.hasPhoto
    source: image
    maskEnabled: true
    maskSource: mask
    maskThresholdMin: 0.5
    maskSpreadAtMin: 1.0
  }
}
