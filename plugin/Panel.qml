import QtQuick
import Quickshell
import Quickshell.Hyprland
import qs.Commons
import qs.Ui
import "Format.js" as Format

// Bar button and panel for quattro-bt-phoned. All phone state comes from the daemon over its
// socket; this file only keeps UI state (tab, typed digits, keypad).
Panel {
  id: root
  moduleName: "jjb.bt-phone"
  ipcTarget: "jjb.bt-phone"

  // ---- daemon -----------------------------------------------------------
  property alias daemon: daemonClient
  Daemon {
    id: daemonClient
    path: root.setting("socket", Quickshell.env("XDG_RUNTIME_DIR") + "/quattro-bt-phone.sock")
    onFailed: function(message) { root.showError(message) }
  }

  readonly property var state: daemonClient.state
  readonly property var phone: state ? state.phone : ({})
  readonly property string stage: Format.setupStage(state)
  readonly property var calls: state ? state.calls : []
  readonly property var call: Format.focusedCall(state)
  // setup | idle | ringing | incall
  readonly property string phase: call ? (Format.isRinging(call) ? "ringing" : "incall")
    : stage === "ready" ? "idle" : "setup"
  readonly property bool inCall: phase === "incall"
  readonly property bool held: !!call && call.state === "held"
  readonly property bool recording: !!state && !!state.recording
  readonly property bool muted: state ? state.audio.muted : false
  readonly property string route: state ? state.audio.route : "laptop"
  readonly property bool autoRecord: state ? state.settings.auto_record : false
  // "" = the system default.
  readonly property string audioOutput: state && state.settings.audio_output ? state.settings.audio_output : ""
  readonly property string audioInput: state && state.settings.audio_input ? state.settings.audio_input : ""
  readonly property bool keypadSounds: state ? !!state.settings.keypad_sounds : false
  readonly property var contacts: daemonClient.contacts
  readonly property var recents: daemonClient.recents
  readonly property var recordings: daemonClient.recordings

  // Call timers are computed from the daemon's timestamps.
  property date now: new Date()
  readonly property int callSeconds: call && call.started_at ? Math.floor(now.getTime() / 1000) - call.started_at : 0
  readonly property int recordingSeconds: recording ? Math.floor(now.getTime() / 1000) - state.recording.started_at : 0
  Timer {
    interval: 1000
    repeat: true
    running: root.calls.length > 0 || root.opened
    triggeredOnStart: true
    onTriggered: root.now = new Date()
  }

  // ---- UI state ---------------------------------------------------------
  property int tab: 0                    // 0 dialer · 1 contacts · 2 recents · 3 recordings
  property string dialled: ""
  property bool keypadOpen: false
  property string dtmf: ""
  property bool editing: false           // a text field has focus → key catcher steps aside
  property bool settingsOpen: false      // call audio settings replace the body
  property string errorText: ""

  // ---- theme shorthands -------------------------------------------------
  readonly property color fg: root.bar ? root.bar.foreground : Color.foreground
  readonly property color accent: Color.accent
  readonly property color urgent: Color.urgent
  readonly property string font: root.bar ? root.bar.fontFamily : Style.font.family

  readonly property var icons: ({
    phone: String.fromCodePoint(0xf03f2), hangup: String.fromCodePoint(0xf03f5), inTalk: String.fromCodePoint(0xf03f6), incoming: String.fromCodePoint(0xf03f7),
    missed: String.fromCodePoint(0xf03fa), outgoing: String.fromCodePoint(0xf03fb), ring: String.fromCodePoint(0xf11ab), phoneOff: String.fromCodePoint(0xf0def),
    cellphone: String.fromCodePoint(0xf011c), cellphoneOff: String.fromCodePoint(0xf0950), dialpad: String.fromCodePoint(0xf061c), contacts: String.fromCodePoint(0xf06cb),
    history: String.fromCodePoint(0xf02da), record: String.fromCodePoint(0xf044a), rec: String.fromCodePoint(0xf044b), mic: String.fromCodePoint(0xf036c), micOff: String.fromCodePoint(0xf036d),
    pause: String.fromCodePoint(0xf03e4), play: String.fromCodePoint(0xf040a), stop: String.fromCodePoint(0xf04db), swap: String.fromCodePoint(0xf04e1), backspace: String.fromCodePoint(0xf0b5c),
    laptop: String.fromCodePoint(0xf0322), search: String.fromCodePoint(0xf0349), close: String.fromCodePoint(0xf0156), folder: String.fromCodePoint(0xf0770), trash: String.fromCodePoint(0xf01b4),
    refresh: String.fromCodePoint(0xf0450), cog: String.fromCodePoint(0xf0493), chevronDown: String.fromCodePoint(0xf0140), chevronRight: String.fromCodePoint(0xf0142)
  })

  readonly property string statusText: {
    switch (phase) {
    case "ringing": return call.state === "waiting" ? "Call waiting" : "Incoming call"
    case "incall": return held ? "On hold" : calls.length > 1 ? calls.length + " calls" : "In call"
    case "idle": return "Connected" + (phone.operator ? " · " + phone.operator : "")
    }
    switch (stage) {
    case "offline": return "Service not running"
    case "no_phone": return "No phone selected"
    case "not_paired": return "Not paired"
    case "not_connected": return "Not connected"
    case "needs_calls": return phone.calls === "requesting" ? "Waiting for your phone" : "Calls not allowed"
    }
    return ""
  }

  // ---- actions ----------------------------------------------------------
  function dial(number) {
    daemonClient.send("dial", { number: number }, function(ok, error) {
      if (ok) root.dialled = ""
      else root.showError(error)
    })
  }
  // The daemon plays it: a DTMF tone for a clicked key, a soft tick for a typed one.
  function keySound(key, soft) {
    if (keypadSounds) daemonClient.send("play_key_sound", { key: key, soft: soft })
  }
  function sendTone(digit) {
    dtmf += digit
    daemonClient.act("tones", { digits: digit })
  }
  function showError(message) {
    errorText = message
    errorTimer.restart()
  }
  Timer {
    id: errorTimer
    interval: 6000
    onTriggered: root.errorText = ""
  }

  // A contact photo for the caller, when the number is in the phonebook.
  function photoFor(number) {
    var key = Format.matchKey(number)
    if (key.length < 3) return ""
    for (var i = 0; i < contacts.length; i++)
      for (var j = 0; j < contacts[i].numbers.length; j++)
        if (Format.matchKey(contacts[i].numbers[j].number) === key) return contacts[i].photo || ""
    return ""
  }

  // ---- keeping lists fresh ----------------------------------------------
  readonly property string syncKey: daemonClient.online && state
    ? [state.sync.last_synced, state.sync.contacts, state.sync.history, state.phone.address].join(":") : ""
  onSyncKeyChanged: if (syncKey) { daemonClient.send("get_contacts"); daemonClient.send("get_recents") }
  // A call ended: it's in recents now, and maybe in recordings.
  readonly property int callCount: calls.length
  onCallCountChanged: if (daemonClient.online) daemonClient.send("get_recents")
  onRecordingChanged: if (daemonClient.online) daemonClient.send("get_recordings")
  onTabChanged: if (tab === 3) daemonClient.send("get_recordings")

  // Open by itself when the phone rings, on the focused monitor only (every monitor has its own
  // bar and widget); back to recents once the calls are over.
  readonly property string ringingId: phase === "ringing" ? call.id : ""
  onRingingIdChanged: {
    if (!ringingId) return
    var focused = Hyprland.focusedMonitor ? String(Hyprland.focusedMonitor.name || "") : ""
    var window = button.QsWindow.window
    var mine = window && window.screen ? String(window.screen.name || "") : ""
    if (!focused || !mine || focused === mine) root.open()
  }
  onPhaseChanged: {
    if (phase !== "incall") { keypadOpen = false; dtmf = "" }
    if (phase === "ringing" || phase === "setup") settingsOpen = false
    if (phase === "idle" && _hadCall) tab = 2
    _hadCall = phase === "ringing" || phase === "incall"
  }
  property bool _hadCall: false
  // Digits sent to one call don't belong to the next.
  readonly property string callId: call ? call.id : ""
  onCallIdChanged: dtmf = ""

  // ---- bar icon ---------------------------------------------------------
  readonly property string barText: {
    if (phase === "ringing") return icons.ring + " " + (call.name || call.number || "")
    if (phase === "incall") return icons.inTalk + " " + Format.clock(callSeconds) + (recording ? " " + icons.record : "")
    if (stage !== "ready") return icons.phoneOff
    return icons.phone
  }

  implicitWidth: button.implicitWidth
  implicitHeight: button.implicitHeight

  BarIconButton {
    id: button
    anchors.fill: parent
    bar: root.bar
    text: root.barText
    active: root.phase === "ringing" || root.recording
    dimmed: root.stage !== "ready"
    slotSize: Style.bar.iconSlot * (button.vertical ? 1 : (root.phase === "ringing" ? 3 : (root.inCall ? (root.recording ? 3.2 : 2.6) : 1)))
    tooltipText: ""
    onPressed: function(b) { root.toggle() }

    SequentialAnimation on opacity {
      running: root.phase === "ringing"
      loops: Animation.Infinite
      alwaysRunToEnd: true
      NumberAnimation { from: 1.0; to: 0.35; duration: 600; easing.type: Easing.InOutSine }
      NumberAnimation { from: 0.35; to: 1.0; duration: 600; easing.type: Easing.InOutSine }
    }
  }

  // ---- panel ------------------------------------------------------------
  KeyboardPanel {
    id: panel
    anchorItem: button
    owner: root
    bar: root.bar
    open: root.opened
    focusTarget: keyCatcher
    contentWidth: panel.fittedContentWidth(Style.space(380))
    contentHeight: panel.fittedContentHeight(column.implicitHeight)

    PanelKeyCatcher {
      id: keyCatcher
      anchors.fill: parent
      blocked: root.editing
      onCloseRequested: root.close()
      onTabRequested: function(direction) { root.switchPanel(direction) }
      onMoveRequested: function(dx, dy) {
        if (root.phase === "idle" && dx !== 0) root.tab = (root.tab + dx + 4) % 4
      }
      onTextKey: function(t) {
        if (!/^[0-9*#+]$/.test(t)) return
        if (root.inCall && root.keypadOpen) root.sendTone(t)
        else if (root.phase === "idle" && root.tab === 0 && !root.settingsOpen) {
          root.dialled += t
          root.keySound(t, true)
        }
      }
      onDeleteRequested: if (root.phase === "idle" && root.tab === 0 && !root.settingsOpen) root.dialled = root.dialled.slice(0, -1)
      // Enter never answers: the panel opens by itself on a ring and takes keyboard focus, so a
      // stray Enter typed into another window would pick up the call.
      onActivateRequested: if (root.phase === "idle" && root.tab === 0 && root.dialled.length > 0) root.dial(root.dialled)

      Column {
        id: column
        anchors.left: parent.left
        anchors.right: parent.right
        anchors.top: parent.top
        spacing: Style.space(12)

        // Header: device + status
        Item {
          width: parent.width
          implicitHeight: Math.max(devIcon.implicitHeight, devLabels.implicitHeight)

          Text {
            id: devIcon
            anchors.left: parent.left
            anchors.verticalCenter: parent.verticalCenter
            text: root.stage === "ready" ? root.icons.cellphone : root.icons.cellphoneOff
            color: root.fg
            opacity: root.stage === "ready" ? 1 : 0.5
            font.family: root.font
            font.pixelSize: Style.font.display
          }
          Column {
            id: devLabels
            anchors.left: devIcon.right
            anchors.leftMargin: Style.space(12)
            anchors.right: gear.left
            anchors.verticalCenter: parent.verticalCenter
            spacing: Style.space(2)
            Text {
              width: parent.width
              elide: Text.ElideRight
              text: root.phone.name || "Phone"
              color: root.fg
              font.family: root.font
              font.pixelSize: Style.font.title
              font.bold: true
            }
            Text {
              text: root.statusText.toUpperCase()
              color: root.fg
              opacity: 0.6
              font.family: root.font
              font.pixelSize: Style.font.caption
              font.bold: true
              font.letterSpacing: 1.2
            }
          }
          Text {
            id: gear
            anchors.right: batt.visible ? batt.left : parent.right
            anchors.rightMargin: batt.visible ? Style.space(10) : 0
            anchors.verticalCenter: parent.verticalCenter
            visible: root.phase === "idle" || root.inCall
            width: visible ? implicitWidth : 0
            text: root.icons.cog
            color: root.settingsOpen ? root.accent : root.fg
            opacity: root.settingsOpen || gearArea.containsMouse ? 1 : 0.6
            font.family: root.font
            font.pixelSize: Style.font.title
            MouseArea {
              id: gearArea
              anchors.fill: parent
              anchors.margins: -Style.space(6)
              hoverEnabled: true
              cursorShape: Qt.PointingHandCursor
              onClicked: root.settingsOpen = !root.settingsOpen
            }
          }
          Text {
            id: batt
            anchors.right: parent.right
            anchors.verticalCenter: parent.verticalCenter
            visible: root.stage === "ready" && root.phone.battery !== null && root.phone.battery !== undefined
            text: root.phone.battery + "%"
            color: root.fg
            opacity: 0.7
            font.family: root.font
            font.pixelSize: Style.font.body
          }
        }

        PanelSeparator { foreground: root.fg }

        // Tabs (hidden during a call)
        ButtonGroup {
          id: tabs
          visible: root.phase === "idle" && !root.settingsOpen
          width: parent.width
          spacing: Style.space(6)
          foreground: root.fg
          fontFamily: root.font
          fontSize: Style.font.bodySmall
          focusable: false
          value: String(root.tab)
          options: [
            { value: "0", label: "Dialer", icon: root.icons.dialpad },
            { value: "1", label: "Contacts", icon: root.icons.contacts },
            { value: "2", label: "Recents", icon: root.icons.history },
            { value: "3", label: "Recs", icon: root.icons.record }
          ]
          onChanged: function(v) { root.tab = parseInt(v) }
        }

        // Body
        Item {
          id: body
          width: parent.width
          height: Style.space(470)

          DialerView {
            anchors.fill: parent
            app: root
            visible: root.phase === "idle" && root.tab === 0 && !root.settingsOpen
          }
          ContactsView {
            anchors.fill: parent
            app: root
            visible: root.phase === "idle" && root.tab === 1 && !root.settingsOpen
          }
          RecentsView {
            anchors.fill: parent
            app: root
            visible: root.phase === "idle" && root.tab === 2 && !root.settingsOpen
          }
          RecordingsView {
            anchors.fill: parent
            app: root
            visible: root.phase === "idle" && root.tab === 3 && !root.settingsOpen
          }
          CallView {
            anchors.fill: parent
            app: root
            visible: root.phase === "ringing" || (root.inCall && !root.settingsOpen)
          }
          SettingsView {
            anchors.fill: parent
            app: root
            visible: root.settingsOpen && (root.phase === "idle" || root.inCall)
          }
          SetupView {
            anchors.fill: parent
            app: root
            visible: root.phase === "setup"
          }
        }

        Text {
          visible: root.errorText !== ""
          width: parent.width
          wrapMode: Text.WordWrap
          text: root.errorText
          color: root.urgent
          font.family: root.font
          font.pixelSize: Style.font.caption
        }
      }
    }
  }
}
