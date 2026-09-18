import QtQuick
import qs.Commons
import qs.Ui
import "Format.js" as Format

// Incoming call + in-call screen.
Item {
  id: root
  property var app
  readonly property bool ringing: app.phase === "ringing"
  readonly property var who: app.call || ({})
  readonly property bool twoCalls: app.calls.length > 1

  // Status strip: recording + audio route (in call only)
  Item {
    id: strip
    visible: !root.ringing
    anchors.left: parent.left
    anchors.right: parent.right
    anchors.top: parent.top
    height: Style.space(30)

    Row {
      anchors.left: parent.left
      anchors.verticalCenter: parent.verticalCenter
      spacing: Style.space(6)
      Rectangle {
        anchors.verticalCenter: parent.verticalCenter
        width: Style.space(9); height: width; radius: width / 2
        color: app.recording ? app.urgent : Util.alpha(app.fg, 0.3)
        SequentialAnimation on opacity {
          running: app.recording
          loops: Animation.Infinite
          alwaysRunToEnd: true
          NumberAnimation { from: 1; to: 0.25; duration: 800 }
          NumberAnimation { from: 0.25; to: 1; duration: 800 }
        }
      }
      Text {
        anchors.verticalCenter: parent.verticalCenter
        text: app.route === "phone" ? "NOT RECORDING · AUDIO ON PHONE"
            : app.recording ? "REC " + Format.clock(app.recordingSeconds) : "NOT RECORDING"
        color: app.recording ? app.urgent : app.fg
        opacity: app.recording ? 1 : 0.55
        font.family: app.font; font.pixelSize: Style.font.caption; font.bold: true; font.letterSpacing: 1.2
      }
    }

    ButtonGroup {
      anchors.right: parent.right
      anchors.verticalCenter: parent.verticalCenter
      spacing: Style.space(2)
      foreground: app.fg
      fontFamily: app.font
      fontSize: Style.font.caption
      focusable: false
      value: app.route
      options: [
        { value: "laptop", label: "Laptop", icon: app.icons.laptop },
        { value: "phone", label: "Phone", icon: app.icons.cellphone }
      ]
      onChanged: function(v) { app.daemon.act("set_route", { route: v }) }
    }
  }

  // Caller identity
  Column {
    id: identity
    anchors.horizontalCenter: parent.horizontalCenter
    anchors.top: parent.top
    anchors.topMargin: root.ringing ? Style.space(50) : Style.space(46)
    spacing: Style.space(6)

    Item {
      anchors.horizontalCenter: parent.horizontalCenter
      width: Style.space(84); height: width

      // Ripple while ringing
      BorderSurface {
        anchors.centerIn: parent
        width: parent.width; height: width; radius: width / 2
        color: "transparent"
        borderSpec: Border.flat(app.accent, Style.space(2))
        visible: root.ringing
        SequentialAnimation on scale {
          running: root.ringing; loops: Animation.Infinite
          NumberAnimation { from: 1; to: 1.5; duration: 1200; easing.type: Easing.OutCubic }
        }
        SequentialAnimation on opacity {
          running: root.ringing; loops: Animation.Infinite
          NumberAnimation { from: 0.9; to: 0; duration: 1200; easing.type: Easing.OutCubic }
        }
      }
      Avatar { anchors.centerIn: parent; app: root.app; name: root.who.name || ""; photo: app.photoFor(root.who.number); size: parent.width }
    }

    Text {
      anchors.horizontalCenter: parent.horizontalCenter
      text: root.who.name || root.who.number || ""
      color: app.fg; font.family: app.font; font.pixelSize: Style.font.display; font.bold: true
    }
    Text {
      anchors.horizontalCenter: parent.horizontalCenter
      text: (root.who.name ? (root.who.label ? root.who.label + " · " : "") + root.who.number : "Unknown number")
      color: app.fg; opacity: 0.6; font.family: app.font; font.pixelSize: Style.font.bodySmall
    }
    Text {
      anchors.horizontalCenter: parent.horizontalCenter
      text: root.ringing ? (root.who.state === "waiting" ? "Call waiting…" : "Incoming call…")
          : root.who.state === "dialing" ? "Calling…"
          : root.who.state === "alerting" ? "Ringing…"
          : app.held ? "On hold · " + Format.clock(app.callSeconds) : Format.clock(app.callSeconds)
      color: root.ringing ? app.accent : app.fg
      opacity: root.ringing ? 1 : 0.85
      font.family: app.font
      font.pixelSize: root.ringing ? Style.font.body : Style.font.heading
      topPadding: Style.space(4)
    }
  }

  // ---- ringing: decline / answer ----
  Row {
    visible: root.ringing
    anchors.horizontalCenter: parent.horizontalCenter
    anchors.bottom: parent.bottom
    anchors.bottomMargin: Style.space(40)
    spacing: Style.space(70)

    CallButton {
      app: root.app
      glyph: app.icons.hangup
      caption: "Decline"
      tint: app.urgent
      filled: true
      size: Style.space(62)
      onClicked: app.daemon.act("decline", { call: root.who.id })
    }
    CallButton {
      app: root.app
      glyph: app.icons.phone
      caption: "Answer"
      tint: app.accent
      filled: true
      size: Style.space(62)
      onClicked: app.daemon.act("answer", { call: root.who.id })
    }
  }

  // ---- in call: controls ----
  Column {
    visible: !root.ringing && !app.keypadOpen
    anchors.horizontalCenter: parent.horizontalCenter
    anchors.bottom: parent.bottom
    anchors.bottomMargin: Style.space(16)
    spacing: Style.space(18)

    Grid {
      anchors.horizontalCenter: parent.horizontalCenter
      columns: 3
      columnSpacing: Style.space(30)
      rowSpacing: Style.space(14)

      CallButton {
        app: root.app
        glyph: app.muted ? app.icons.micOff : app.icons.mic
        caption: app.muted ? "Unmute" : "Mute"
        checked: app.muted
        onClicked: app.daemon.act("set_muted", { muted: !app.muted })
      }
      CallButton {
        app: root.app
        glyph: app.icons.dialpad
        caption: "Keypad"
        onClicked: app.keypadOpen = true
      }
      CallButton {
        app: root.app
        glyph: app.icons.pause
        caption: app.held ? "Resume" : "Hold"
        checked: app.held
        onClicked: app.daemon.act("hold")
      }
      CallButton {
        app: root.app
        glyph: app.icons.swap
        caption: "Swap"
        enabled: root.twoCalls
        onClicked: app.daemon.act("swap")
      }
      CallButton {
        app: root.app
        glyph: app.recording ? app.icons.stop : app.icons.record
        caption: app.recording ? "Stop rec" : "Record"
        tint: app.urgent
        checked: app.recording
        enabled: app.route === "laptop"
        onClicked: app.daemon.act(app.recording ? "stop_recording" : "start_recording")
      }
      CallButton {
        app: root.app
        glyph: app.icons.hangup
        caption: "End all"
        enabled: root.twoCalls
        onClicked: app.daemon.act("hangup_all")
      }
    }

    CallButton {
      anchors.horizontalCenter: parent.horizontalCenter
      app: root.app
      glyph: app.icons.hangup
      tint: app.urgent
      filled: true
      size: Style.space(60)
      onClicked: app.daemon.act("hangup", { call: root.who.id })
    }
  }

  // ---- in call: DTMF keypad ----
  Column {
    visible: !root.ringing && app.keypadOpen
    anchors.horizontalCenter: parent.horizontalCenter
    anchors.bottom: parent.bottom
    anchors.bottomMargin: Style.space(10)
    spacing: Style.space(8)

    Text {
      anchors.horizontalCenter: parent.horizontalCenter
      text: app.dtmf.length ? app.dtmf : " "
      color: app.fg; font.family: app.font; font.pixelSize: Style.font.heading
    }
    Grid {
      anchors.horizontalCenter: parent.horizontalCenter
      columns: 3
      columnSpacing: Style.space(22)
      rowSpacing: Style.space(6)
      Repeater {
        model: ["1", "2", "3", "4", "5", "6", "7", "8", "9", "*", "0", "#"]
        CallButton {
          required property string modelData
          app: root.app
          glyph: modelData
          glyphIsText: true
          size: Style.space(44)
          onClicked: app.sendTone(modelData)
        }
      }
    }
    Row {
      anchors.horizontalCenter: parent.horizontalCenter
      spacing: Style.space(40)
      CallButton {
        app: root.app
        glyph: app.icons.close
        size: Style.space(44)
        onClicked: app.keypadOpen = false
      }
      CallButton {
        app: root.app
        glyph: app.icons.hangup
        tint: app.urgent
        filled: true
        size: Style.space(44)
        onClicked: app.daemon.act("hangup", { call: root.who.id })
      }
    }
  }
}
