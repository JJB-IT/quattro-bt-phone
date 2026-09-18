# Research notes

Findings from the prototype phase (September 2026), gathered on the reference machine:
NixOS 26.05, kernel 7.1, PipeWire 1.6.6, WirePlumber 0.5.14, BlueZ 5.86, an Intel AX201
adapter (BT 5.2), a Samsung Galaxy S25 FE (Android 16) and Omarchy 4.0 (Quickshell 0.3).

Anything marked **unverified** hadn't been observed on a live call when this was written.

---

## 1. Call control: `org.pipewire.Telephony` (PipeWire 1.4+, session bus)

PipeWire's bluez5 plugin plays the HFP **Hands-Free** role and exports this API. No oFono
is needed. Interfaces (from the `libspa-bluez5.so` introspection XML):

```
/org/pipewire/Telephony                      org.freedesktop.DBus.ObjectManager
                                             org.ofono.Manager (GetModems)          # compat shim
/org/pipewire/Telephony/agN                  one per connected phone
  org.pipewire.Telephony.AudioGateway1       Dial(s) SwapCalls() ReleaseAndAnswer() ReleaseAndSwap()
                                             HoldAndAnswer() HangupAll() CreateMultiparty() SendTones(s)
                                             props: Address s (const), SpeakerVolume y rw, MicrophoneVolume y rw
  org.pipewire.Telephony.AudioGatewayTransport1
                                             Activate()   # pull SCO audio to the computer
                                             props: State s, Codec y, RejectSCO b rw
  org.ofono.VoiceCallManager                 same methods + GetCalls(), signals CallAdded/CallRemoved
/org/pipewire/Telephony/agN/callM            one per call
  org.pipewire.Telephony.Call1               Answer() Hangup()
                                             props: LineIdentification s, IncomingLine s, Name s, Multiparty b, State s
  org.ofono.VoiceCall                        same + GetProperties / PropertyChanged
```

- The `agN` index is **not stable**. Always resolve the gateway by its `Address` via
  `GetManagedObjects`.
- Call `State` values follow oFono: `incoming`, `waiting`, `dialing`, `alerting`, `active`,
  `held`, `disconnected` (**unverified**).
- New calls arrive as `InterfacesAdded` on the root ObjectManager. State changes arrive as
  `PropertiesChanged` on the call object.
- Related WirePlumber/PipeWire settings: `bluez5.hfphsp-backend`,
  `bluez5.telephony-dbus-service`, `bluez5.telephony.provide-ofono`,
  `bluez5.telephony.use-system-bus`, `bluez5.telephony.default-reject-sco`,
  `bluez5.hfp-hf.default-{mic,speaker}-volume`, `bluez5.hfp-hf.disable-nrec`.

## 2. Contacts and call history: PBAP via `obexd`

- It works. A read-only count on the reference phone returned about 1,100 entries in `int/pb` (entry 0
  is the owner's own card) and 300 in `int/cch` (combined history). Also available: `ich`
  (incoming), `och` (outgoing), `mch` (missed). `spb`/`sim1` exist on some phones.
- API: `org.bluez.obex.Client1.CreateSession(address, {Target: "PBAP"})` returns a session path.
  On that path, `org.bluez.obex.PhonebookAccess1` provides `Select(location, book)`, `GetSize()`,
  `PullAll(targetfile, filters)` (returns a Transfer1 object; wait for `Status=complete`),
  `List`, `Pull` and `Search`.
- **obexd destroys a session when the D-Bus connection that created it disconnects.**
  Separate `busctl` invocations can't do CreateSession → Select → GetSize. Use one
  long-lived connection. See [`notes/pbap_count.py`](../notes/pbap_count.py).
- **The first access pops an "Allow access to contacts?" prompt on the phone.** The request
  times out (~25 s) if nobody taps it. If "Contacts sharing" isn't switched on in the phone's
  Bluetooth device settings, it may ask every time. The UI shows an "approve on your phone"
  state for this.
- The data is vCard 2.1/3.0: `N`, `FN`, `TEL;TYPE=…`, `PHOTO` (base64). Call-history entries
  carry `X-IRMC-CALL-DATETIME;MISSED|RECEIVED|DIALED`.
- The project is **read-only** towards the phone. It never writes to the phonebook.

## 3. Audio routing

When SCO opens, PipeWire creates a bluez source (the caller's voice) and a sink (the audio
sent to the phone as your mic) on the phone's card (profile `audio-gateway`).

Open questions for the live-call spike:

1. Which nodes appear, and does WirePlumber auto-link them to the default devices?
2. If the default mic is a Bluetooth headset, a call opens a second SCO link on the same
   adapter, and audio becomes choppy. The default policy should prefer the **built-in mic and speakers**.
3. Some phones keep the audio on the handset when you answer on the phone.
   `AudioGatewayTransport1.Activate()` should pull it over.
4. Don't fight virtual sources such as EasyEffects on the mic chain.

## 4. Existing GUIs (why this project exists)

- **No mainstream GUI speaks `org.pipewire.Telephony` yet.** See the API author's post:
  <https://gkiagia.gr/2025-02-20-pipewire-telephony/>
- **[omarchy-dialer](https://github.com/karem505/omarchy-dialer)** (Python + standalone
  Quickshell window) uses the same PipeWire API. It's a good reference. This project differs:
  it's a native Omarchy shell plugin with a Rust daemon, syncs contacts and history from the
  phone over PBAP, records calls, and is packaged with Nix.
- **[handsfree-linux](https://github.com/PavelTarlev1/handsfree-linux)** (PyQt6) runs its own
  HFP profile through BlueZ, which conflicts with PipeWire's.
- **GNOME Calls** expects `org.ofono.Modem`, which PipeWire's shim doesn't provide.
  **Plasma Dialer** uses ModemManager. **KDE Connect** does notifications and SMS only.

## 5. Omarchy plugin contract

- A plugin is a directory with `manifest.json` (`schemaVersion: 1`, `id`, `name`, `version`,
  `kinds`, `entryPoints`) plus QML. Full docs are in `$OMARCHY_PATH/manual/32-shell-plugins.md`
  and `$OMARCHY_PATH/shell/README.md`.
- Validate with `omarchy plugin validate <dir>`. **No symlinks inside the plugin folder**, though the
  folder itself may be a symlink. With home-manager, link the whole store directory as a single
  link (not `recursive = true`).
- A third-party plugin is enabled when its id appears in `~/.config/omarchy/shell.json`.
  After adding one, run `omarchy-shell shell rescanPlugins`. Saving files under
  `~/.config/omarchy/plugins/` hot-reloads the plugin.
- Quickshell 0.3 has no generic D-Bus client, so the QML stays a thin view over a socket
  (`Quickshell.Io.Socket`) and all logic lives in the daemon.
