import QtQuick
import Quickshell.Io

// Client for quattro-bt-phoned: newline-delimited JSON over a Unix socket (docs/protocol.md).
// It subscribes on connect, keeps the latest state, and reconnects when the daemon restarts.
Item {
  id: root
  visible: false

  property string path: ""
  // The daemon's full state, or null while it isn't reachable.
  property var state: null
  property var contacts: []
  property var recents: []
  property var recordings: []
  property var audioDevices: ({ outputs: [], inputs: [] })
  readonly property bool online: state !== null

  // A command the user started failed; the panel shows the message.
  signal failed(string message)

  property int _nextId: 1
  property var _pending: ({})

  // Send a command. `done(ok, error)` runs when its reply arrives.
  function send(cmd, args, done) {
    var sock = link.item
    if (!sock || !sock.connected) {
      if (done) done(false, "The phone service is not running")
      return
    }
    var msg = Object.assign({ id: _nextId++, cmd: cmd }, args || {})
    if (done) _pending[msg.id] = done
    sock.write(JSON.stringify(msg) + "\n")
    sock.flush()
  }

  // Like send(), but a failure is reported through `failed`.
  function act(cmd, args) {
    send(cmd, args, function(ok, error) { if (!ok) root.failed(error) })
  }

  function handle(line) {
    var m
    try { m = JSON.parse(line) } catch (e) { return }
    if (m.type === "state") {
      delete m.type
      state = m
    } else if (m.type === "contacts") {
      contacts = m.contacts
    } else if (m.type === "recents") {
      recents = m.entries
    } else if (m.type === "recordings") {
      recordings = m.recordings
    } else if (m.type === "audio_devices") {
      audioDevices = { outputs: m.outputs, inputs: m.inputs }
    } else if (m.type === "reply" && m.id !== undefined) {
      var done = _pending[m.id]
      delete _pending[m.id]
      if (done) done(m.ok, m.error || "")
    }
  }

  // A fresh Socket per attempt: once an attempt fails, a Socket doesn't reliably retry.
  Loader {
    id: link
    active: root.path !== ""
    sourceComponent: Socket {
      path: root.path
      connected: true
      parser: SplitParser {
        onRead: function(line) { root.handle(line) }
      }
      onConnectionStateChanged: {
        if (connected) {
          // Written directly: the Loader hasn't published `item` yet while this runs.
          write(JSON.stringify({ cmd: "subscribe" }) + "\n")
          flush()
        } else {
          root.state = null
          root._pending = ({})
        }
      }
    }
  }

  // The daemon may start after the shell, or restart; keep trying quietly until a state
  // message arrives.
  Timer {
    interval: 2000
    repeat: true
    running: root.state === null && root.path !== ""
    onTriggered: {
      link.active = false
      link.active = true
    }
  }
}
