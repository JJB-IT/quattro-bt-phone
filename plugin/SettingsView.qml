import QtQuick
import Quickshell
import qs.Commons
import qs.Ui

// Speakers and microphone for calls (changes apply at once, also during a call), the ringing
// tone for outgoing calls, and keypad sounds.
Item {
  id: root
  property var app

  readonly property var devices: app.daemon.audioDevices
  readonly property var ringtones: app.daemon.ringtones
  readonly property string ringbackValue: app.ringback === "custom" ? "custom:" + app.ringbackFile : app.ringback

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

  // Dropdown assigns its own value on a pick; bind it back to the daemon's state so a refused
  // or cancelled choice doesn't stick.
  function rebind(dropdown, current) { dropdown.value = Qt.binding(current) }

  function chooseRingback(v) {
    if (v === "pick") app.daemon.act("choose_ringtone")
    else if (v.indexOf("custom:") === 0) app.daemon.act("set_ringback", { style: "custom", file: v.substring(7) })
    else app.daemon.act("set_ringback", { style: v })
  }

  onVisibleChanged: if (visible) {
    app.daemon.send("get_audio_devices")
    app.daemon.send("get_ringtones")
  }

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
      id: outputPick
      value: app.audioOutput
      options: root.options(root.devices.outputs, app.audioOutput)
      onChanged: function(v) {
        root.choose("output", v)
        root.rebind(outputPick, function() { return app.audioOutput })
      }
      onPopupOpenChanged: app.editing = popupOpen
    }

    Dropdown {
      width: parent.width
      label: "Microphone"
      fontFamily: app.font
      id: inputPick
      value: app.audioInput
      options: root.options(root.devices.inputs, app.audioInput)
      onChanged: function(v) {
        root.choose("input", v)
        root.rebind(inputPick, function() { return app.audioInput })
      }
      onPopupOpenChanged: app.editing = popupOpen
    }

    Text {
      width: parent.width
      wrapMode: Text.WordWrap
      text: "Used while a call's audio is on the laptop. A Bluetooth headset switches to its low-quality headset mode when its microphone is in use."
      color: app.fg; opacity: 0.55; font.family: app.font; font.pixelSize: Style.font.caption
    }

    PanelSeparator { foreground: app.fg }

    // The phone keeps the network's ringing tone until the call connects, so the daemon plays
    // one in the local style.
    Dropdown {
      width: parent.width
      label: "Ringing tone"
      fontFamily: app.font
      id: ringbackPick
      value: root.ringbackValue
      options: {
        var out = [
          { value: "europe", label: "Europe (single ring)" },
          { value: "uk", label: "UK, South Africa, Australia (double ring)" },
          { value: "north_america", label: "North America" },
          { value: "chime", label: "Soft chime" }
        ]
        for (var i = 0; i < root.ringtones.files.length; i++)
          out.push({ value: "custom:" + root.ringtones.files[i], label: root.ringtones.files[i] })
        out.push({ value: "pick", label: "Custom file…" })
        out.push({ value: "off", label: "Off" })
        return out
      }
      onChanged: function(v) {
        root.chooseRingback(v)
        root.rebind(ringbackPick, function() { return root.ringbackValue })
      }
      onPopupOpenChanged: app.editing = popupOpen
    }

    Text {
      visible: root.ringtones.dir !== ""
      width: parent.width
      elide: Text.ElideRight
      text: app.icons.folder + "  Open the ringtones folder"
      color: app.fg
      opacity: folderArea.containsMouse ? 1 : 0.55
      font.family: app.font
      font.pixelSize: Style.font.caption
      MouseArea {
        id: folderArea
        anchors.fill: parent
        hoverEnabled: true
        cursorShape: Qt.PointingHandCursor
        onClicked: Quickshell.execDetached(["xdg-open", root.ringtones.dir])
      }
    }

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
