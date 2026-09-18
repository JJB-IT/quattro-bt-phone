import QtQuick
import qs.Commons
import qs.Ui

// Speakers and microphone for calls (changes apply at once, also during a call) and keypad
// sounds.
Item {
  id: root
  property var app

  readonly property var devices: app.daemon.audioDevices

  // "" stands for the system default. A saved device that isn't there right now stays listed,
  // so the choice doesn't look lost while a headset is off.
  function options(list, current) {
    var out = [{ value: "", label: "System default" }]
    var found = false
    for (var i = 0; i < list.length; i++) {
      out.push({ value: list[i].name, label: list[i].description })
      if (list[i].name === current) found = true
    }
    if (current && !found) out.push({ value: current, label: current + " (not connected)" })
    return out
  }

  function choose(direction, value) {
    app.daemon.act("set_audio_device", value ? { direction: direction, name: value } : { direction: direction })
  }

  onVisibleChanged: if (visible) app.daemon.send("get_audio_devices")

  Column {
    anchors.fill: parent
    anchors.topMargin: Style.space(4)
    spacing: Style.space(14)

    Item {
      width: parent.width
      height: back.implicitHeight
      Text {
        anchors.left: parent.left
        anchors.verticalCenter: parent.verticalCenter
        text: "Settings"
        color: app.fg; font.family: app.font; font.pixelSize: Style.font.body; font.bold: true
      }
      Text {
        id: back
        anchors.right: parent.right
        anchors.verticalCenter: parent.verticalCenter
        text: app.icons.close
        color: app.fg
        opacity: backArea.containsMouse ? 1 : 0.6
        font.family: app.font
        font.pixelSize: Style.font.title
        MouseArea {
          id: backArea
          anchors.fill: parent
          anchors.margins: -Style.space(6)
          hoverEnabled: true
          cursorShape: Qt.PointingHandCursor
          onClicked: app.settingsOpen = false
        }
      }
    }

    Dropdown {
      width: parent.width
      label: "Speakers"
      fontFamily: app.font
      value: app.audioOutput
      options: root.options(root.devices.outputs, app.audioOutput)
      onChanged: function(v) { root.choose("output", v) }
      onPopupOpenChanged: app.editing = popupOpen
    }

    Dropdown {
      width: parent.width
      label: "Microphone"
      fontFamily: app.font
      value: app.audioInput
      options: root.options(root.devices.inputs, app.audioInput)
      onChanged: function(v) { root.choose("input", v) }
      onPopupOpenChanged: app.editing = popupOpen
    }

    Text {
      width: parent.width
      wrapMode: Text.WordWrap
      text: "Used while a call's audio is on the laptop. A Bluetooth headset switches to its low-quality headset mode when its microphone is in use."
      color: app.fg; opacity: 0.55; font.family: app.font; font.pixelSize: Style.font.caption
    }

    PanelSeparator { foreground: app.fg }

    Item {
      width: parent.width
      height: Math.max(soundsToggle.height, soundsLabels.implicitHeight)
      Column {
        id: soundsLabels
        anchors.left: parent.left
        anchors.right: soundsToggle.left
        anchors.rightMargin: Style.space(10)
        anchors.verticalCenter: parent.verticalCenter
        Text {
          text: "Keypad sounds"
          color: app.fg; font.family: app.font; font.pixelSize: Style.font.body; font.bold: true
        }
        Text {
          width: parent.width
          wrapMode: Text.WordWrap
          text: "A dial tone for each key you click, and a soft tick for digits you type."
          color: app.fg; opacity: 0.55; font.family: app.font; font.pixelSize: Style.font.caption
        }
      }
      ToggleSwitch {
        id: soundsToggle
        anchors.right: parent.right
        anchors.verticalCenter: parent.verticalCenter
        checked: app.keypadSounds
        foreground: app.fg
        onToggled: app.daemon.act("set_keypad_sounds", { enabled: !app.keypadSounds })
      }
    }
  }
}
