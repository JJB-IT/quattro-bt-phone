# Socket protocol

`quattro-bt-phoned` listens on a Unix stream socket at
`$XDG_RUNTIME_DIR/quattro-bt-phone.sock` (mode `0600`). Messages are **newline-delimited
JSON**: one object per line, in both directions. The Rust types live in
[`crates/proto`](../crates/proto/src/lib.rs), which is the source of truth.

```sh
# Try it by hand
socat - UNIX-CONNECT:$XDG_RUNTIME_DIR/quattro-bt-phone.sock
{"cmd":"subscribe"}
```

## Requests (client → daemon)

Every request has a `cmd`, plus an optional numeric `id` that is echoed back in its reply.

| `cmd` | Fields | Effect |
|---|---|---|
| `subscribe` | | Send `state` now and after every change |
| `get_state` | | Send `state` once |
| `select_phone` | `address` | Use this paired device as the phone (saved to config) |
| `connect` | | Connect the phone over Bluetooth |
| `request_calls` | | Connect hands-free; the phone asks the user to allow calls |
| `request_contacts` | | Open PBAP; the phone asks the user to allow contacts |
| `dial` | `number` | Place a call |
| `answer` / `decline` / `hangup` | `call`? | Act on the given call, or the obvious one |
| `hangup_all` | | End every call |
| `tones` | `digits` | Send DTMF (`0-9 * # A-D`) |
| `hold` / `swap` | | Hold/resume; swap active and held |
| `set_muted` | `muted` | Mute your microphone (during a call; cleared when the calls end) |
| `set_route` | `route`: `laptop`\|`phone` | Where call audio plays |
| `get_audio_devices` | | → `audio_devices` |
| `set_audio_device` | `direction`: `output`\|`input`, `name`? | Speakers/microphone for calls by `node.name`; no `name` = system default. Saved to config, applies during a call |
| `sync` | | Re-pull contacts and history |
| `get_contacts` | `query`? | → `contacts` |
| `get_recents` | `missed_only`? | → `recents` |
| `get_recordings` | | → `recordings` |
| `set_auto_record` | `enabled` | Saved to config |
| `set_keypad_sounds` | `enabled` | Saved to config |
| `set_ringback` | `style`: `europe`\|`uk`\|`north_america`\|`chime`\|`custom`\|`off`, `file`? | Ringing tone while an outgoing call rings (the phone only hands over audio once connected). `custom` needs a `file` from `get_ringtones`. Saved to config |
| `get_ringtones` | | → `ringtones` |
| `choose_ringtone` | | Opens the desktop file chooser; the picked file is copied to the ringtones folder and used |
| `play_key_sound` | `key`, `soft`? | A key's DTMF tone, or a soft tick for a typed key (`soft`); nothing while keypad sounds are off |
| `start_recording` / `stop_recording` | `discard`? | Manual recording control |
| `delete_recording` | `id` | Delete a recording file |
| `simulate` | `event`, `number`? | `--mock` only: `ring`, `remote_answer`, `remote_hangup`, `disconnect`, `reset_setup` |

## Messages (daemon → client)

Every message has a `type`.

- **`ringtones`**: `{"type":"ringtones","dir":"/home/me/.config/quattro-bt-phone/ringtones","files":["my tone.mp3"]}`.
- **`audio_devices`**: `{"type":"audio_devices","outputs":[{"name":"alsa_output.pci-…","description":"Built-in Audio Analog Stereo"}],"inputs":[…]}`.
  The phone's own call nodes are left out.
- **`reply`**: `{"type":"reply","id":1,"ok":true}` or `{"type":"reply","ok":false,"error":"…"}`.
  Exactly one reply per request, sent after any data message the request produced.
- **`state`**: the full state, flattened into the message:

  ```json
  {
    "type": "state",
    "phone": {
      "name": "Galaxy S25 FE", "address": "AA:BB:CC:DD:EE:FF",
      "paired": true, "connected": true,
      "calls": "granted", "contacts": "requesting",
      "battery": null, "signal": null, "operator": null
    },
    "devices": [{ "address": "…", "name": "…", "paired": true, "connected": true }],
    "calls": [{
      "id": "/org/pipewire/Telephony/ag1/call1", "number": "+31…", "name": "Mum", "label": "mobile",
      "state": "active", "direction": "incoming", "started_at": 1789735200, "multiparty": false
    }],
    "audio": { "route": "laptop", "muted": false },
    "recording": { "call": "…", "path": "…", "started_at": 1789735201 },
    "sync": { "status": "idle", "error": null, "last_synced": 1789735000, "contacts": 1098, "history": 300 },
    "settings": { "auto_record": false, "audio_output": null, "audio_input": "alsa_input.pci-…", "keypad_sounds": true, "ringback": "custom", "ringback_file": "my tone.mp3" }
  }
  ```

  `sync.status` goes `awaiting_approval` (the phone may be asking "Allow access to contacts?")
  → `syncing` → `idle`, or `error` with `sync.error` set. The daemon syncs again by itself
  whenever the phone connects, but only after a first sync that the user started has worked.

  `settings.audio_output` / `audio_input` are PipeWire `node.name`s, or `null` for the system
  default.

  Permissions (`calls`, `contacts`) are `unknown`, `requesting`, `granted` or `denied`.
  Call states follow oFono: `incoming`, `waiting`, `dialing`, `alerting`, `active`, `held`,
  `disconnected`. Timestamps are Unix seconds, so clients compute call timers themselves.
- **`contacts`**, **`recents`**, **`recordings`**: data for the matching `get_*` request.

## Setup stage

Clients show a setup screen until the phone is ready. The stage comes from `phone`:

| Condition | Stage | UI |
|---|---|---|
| `address` empty | `no_phone` | Pick from `devices`, or open Bluetooth settings to pair |
| `!paired` | `not_paired` | Open Bluetooth settings |
| `!connected` | `not_connected` | **Connect** → `connect` |
| `calls != granted` | `needs_calls` | **Allow calls** → `request_calls` |
| otherwise | `ready` | Dialer. If `contacts != granted`, offer **Allow contacts** → `request_contacts` |
