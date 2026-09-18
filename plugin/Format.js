.pragma library

// Pure helpers shared by the views. The daemon's state shape is described in docs/protocol.md.

// Mirrors State::setup_stage() in crates/proto.
function setupStage(state) {
  if (!state) return "offline"
  var p = state.phone
  if (!p.address) return "no_phone"
  if (!p.paired) return "not_paired"
  if (!p.connected) return "not_connected"
  if (p.calls !== "granted") return "needs_calls"
  return "ready"
}

// Mirrors State::focused_call(): ringing first, then active, then anything.
function focusedCall(state) {
  if (!state || !state.calls.length) return null
  var calls = state.calls
  var pick = function(states) {
    for (var i = 0; i < calls.length; i++) if (states.indexOf(calls[i].state) >= 0) return calls[i]
    return null
  }
  return pick(["incoming", "waiting"]) || pick(["active", "dialing", "alerting"]) || calls[0]
}

function isRinging(call) { return !!call && (call.state === "incoming" || call.state === "waiting") }

function initials(name) {
  var parts = String(name || "").split(" ").filter(function(p) { return p.length > 0 })
  if (parts.length === 0) return ""
  if (parts.length === 1) return parts[0].charAt(0).toUpperCase()
  return (parts[0].charAt(0) + parts[parts.length - 1].charAt(0)).toUpperCase()
}

function digits(s) { return String(s || "").replace(/[^0-9+*#]/g, "") }

// Mirrors match_key() in the daemon's store: the last nine digits, so "+31 6 1234 5678" matches
// "06-12345678". Short numbers and service codes must match exactly.
function matchKey(number) {
  var n = digits(number)
  var d = n.replace(/[^0-9]/g, "")
  if (/[*#]/.test(n) || d.length < 9) return n.replace(/^\+/, "")
  return d.slice(-9)
}

// Lower case without accents, like the daemon's search.
function fold(s) { return String(s || "").normalize("NFD").replace(/[̀-ͯ]/g, "").toLowerCase() }

function clock(seconds) {
  seconds = Math.max(0, Math.floor(seconds))
  var m = Math.floor(seconds / 60), s = seconds % 60
  if (m >= 60) return Math.floor(m / 60) + ":" + String(m % 60).padStart(2, "0") + ":" + String(s).padStart(2, "0")
  return m + ":" + String(s).padStart(2, "0")
}

function contactsWithHeaders(contacts, query) {
  var q = fold(query).trim()
  var qd = digits(q)
  return contacts.filter(function(c) {
    if (!q) return true
    if (fold(c.name).indexOf(q) >= 0) return true
    if (qd.length >= 2) {
      for (var i = 0; i < c.numbers.length; i++)
        if (digits(c.numbers[i].number).indexOf(qd) >= 0) return true
    }
    return false
  }).map(function(c, i, arr) {
    var letter = fold(c.name).charAt(0).toUpperCase()
    var prev = i > 0 ? fold(arr[i - 1].name).charAt(0).toUpperCase() : ""
    return { name: c.name, numbers: c.numbers, photo: c.photo || "", letter: letter, first: letter !== prev }
  })
}

// Contact numbers containing the dialled digits, for the dialer suggestion. A leading trunk "0"
// is optional, so "0612" finds "+31 6 12…".
function matchNumber(contacts, dialled) {
  var d = digits(dialled).replace(/^\+/, "")
  if (d.length < 3) return []
  var national = d.charAt(0) === "0" ? d.replace(/^0+/, "") : ""
  var out = []
  for (var i = 0; i < contacts.length && out.length < 2; i++) {
    var c = contacts[i]
    for (var j = 0; j < c.numbers.length; j++) {
      var n = digits(c.numbers[j].number)
      if (n.indexOf(d) >= 0 || (national.length >= 3 && n.indexOf(national) >= 0))
        out.push({ name: c.name, label: c.numbers[j].label, number: c.numbers[j].number, photo: c.photo || "" })
    }
  }
  return out.slice(0, 2)
}

function startOfDay(date) { return new Date(date.getFullYear(), date.getMonth(), date.getDate()).getTime() }

function dayLabel(at, now) {
  var days = Math.round((startOfDay(now) - startOfDay(new Date(at * 1000))) / 86400000)
  if (days === 0) return "Today"
  if (days === 1) return "Yesterday"
  var d = new Date(at * 1000)
  if (days < 7) return Qt.locale().dayName(d.getDay(), 0)
  return d.toLocaleDateString(Qt.locale(), 1)
}

function timeOfDay(at) {
  var d = new Date(at * 1000)
  return String(d.getHours()).padStart(2, "0") + ":" + String(d.getMinutes()).padStart(2, "0")
}

// Recents with a day header on the first entry of each day.
function recentsWithHeaders(entries, now) {
  return entries.map(function(r, i, arr) {
    var o = Object.assign({}, r)
    o.dayLabel = dayLabel(r.at, now)
    o.first = i === 0 || dayLabel(arr[i - 1].at, now) !== o.dayLabel
    return o
  })
}

function ago(at, now) {
  var s = Math.max(0, Math.floor(now.getTime() / 1000) - at)
  if (s < 60) return "just now"
  if (s < 3600) return Math.floor(s / 60) + " min ago"
  if (s < 86400) return Math.floor(s / 3600) + " h ago"
  return Math.floor(s / 86400) + " d ago"
}

function size(bytes) {
  if (bytes < 1024 * 1024) return Math.max(1, Math.round(bytes / 1024)) + " kB"
  return (bytes / 1024 / 1024).toFixed(1) + " MB"
}
