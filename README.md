# quattro-bt-phone

**Make and take real phone calls from your Omarchy desktop, using your Bluetooth-paired phone.**

`quattro-bt-phone` turns your Linux laptop into a hands-free unit for your phone, like a car
kit. Calls go over your normal mobile number and plan. You talk through the laptop's mic and
speakers and control everything from a native panel in the Omarchy bar. There's no VoIP, no
cloud, and no app to install on the phone.

> **Status: in early development.** The UI has been prototyped and the daemon is being built.
> See the [roadmap](#roadmap). Not ready for daily use yet.

## Features

- **Native Omarchy panel.** It's a shell plugin, not a separate window, so it follows your
  Omarchy theme, fonts and bar.
- **Dialer** with contact suggestions as you type.
- **Contact book** synced read-only from the phone over Bluetooth PBAP, with search.
- **Recents** synced from the phone and merged with calls the daemon sees live. Missed calls are highlighted.
- **In-call screen** with a timer, mute, keypad (DTMF), hold/swap, and a laptop ⇄ phone audio switch.
- **Call recording.** Records both sides into a single file, with an optional auto-record for
  every call. A red REC dot in the bar shows when a recording is running.
- **Ring and missed-call notifications.** Click one to open the panel, and the panel also opens by itself when the phone rings.
- **CLI** for scripts and Hyprland keybinds: `quattro-bt-phone answer`, `hangup`, `dial …`.
- **Nix flake** with a home-manager module.

## How it works

```
 Omarchy bar/panel (QML plugin)      quattro-bt-phone CLI
              │  JSON lines over $XDG_RUNTIME_DIR/quattro-bt-phone.sock
              ▼
      quattro-bt-phoned  (Rust, user systemd service)
        ├─ org.pipewire.Telephony  → call control (HFP, via PipeWire)
        ├─ org.bluez.obex          → contacts + call history (PBAP, read-only)
        ├─ org.bluez               → is the phone connected?
        ├─ PipeWire                → call audio routing + recording
        └─ SQLite cache            → ~/.local/share/quattro-bt-phone/
```

PipeWire 1.4+ implements the Bluetooth Hands-Free Profile and exposes call control on D-Bus
as `org.pipewire.Telephony`. Quickshell has no generic D-Bus client, so the daemon owns all
bus traffic and the QML plugin is a thin view over a Unix socket. The full background is in
[`docs/research.md`](docs/research.md).

## Requirements

| Component | Version | Notes |
|---|---|---|
| Omarchy | 4.0+ | Quickshell-based shell with plugin support |
| PipeWire / WirePlumber | 1.4+ / 0.5+ | Provides `org.pipewire.Telephony` |
| BlueZ | 5.8x | `obexd` is needed for contacts |
| Phone | Any phone that acts as a Bluetooth hands-free audio gateway | Tested with a Galaxy S25 FE (Android 16) |

On the phone, allow **Calls** and **Contacts sharing** for your computer in its Bluetooth
device settings.

## Install

> Not published yet. These instructions describe the planned setup.

### NixOS / home-manager (flake)

```nix
# flake.nix
inputs.quattro-bt-phone.url = "github:JJB-IT/quattro-bt-phone";

# home-manager configuration
imports = [ inputs.quattro-bt-phone.homeManagerModules.default ];
services.quattro-bt-phone = {
  enable = true;
  phoneAddress = "AA:BB:CC:DD:EE:FF";   # your phone's Bluetooth MAC
};
```

### Other distributions

Build with `cargo build --release`, install the two binaries, then
`cp -r plugin ~/.config/omarchy/plugins/jjb.bt-phone` and
`omarchy plugin enable jjb.bt-phone`. More detailed instructions will come with the first release.

## Usage

Click the phone icon in the bar. From the CLI:

```sh
quattro-bt-phone status
quattro-bt-phone dial "+31 6 1234 5678"   # asks first; --yes for keybinds/scripts
quattro-bt-phone answer | decline | hangup
quattro-bt-phone tones 1234#
quattro-bt-phone sync            # refresh contacts and call history from the phone
```

## Call recording and the law

Recording laws differ by country and state. Some places require **everyone on the call** to
consent. Auto-record is off by default, and no announcement tone is played. You're responsible
for recording legally where you are.

## Roadmap

Progress is tracked in [milestones](https://github.com/JJB-IT/quattro-bt-phone/milestones).

1. Live-call spike: confirm call states and audio node behaviour on real hardware.
2. Daemon core: call tracking, socket API, CLI, notifications.
3. PBAP sync: contacts and call history cache, caller names.
4. Omarchy plugin: bar widget, panel, in-call view.
5. Call recording.
6. Packaging: Nix flake and home-manager module, first release.

## Development

```sh
nix develop          # Rust toolchain + system libraries
cargo test
cargo clippy --all-targets -- -D warnings
omarchy plugin validate plugin
```

For UI work without a phone, run the daemon with `--mock` and drive it with
`quattro-bt-phone simulate ring|remote-answer|remote-hangup|disconnect|reset-setup`.
See [CONTRIBUTING.md](CONTRIBUTING.md) for the branch, commit and PR workflow.

The code follows Omarchy's own conventions (`AGENTS.md`, `docs/theming.md`, `.editorconfig`).
The plugin uses only the shell's theme tokens, so it restyles itself whenever you switch Omarchy
themes. It's meant to be easy to adopt upstream.

## Credits

- [PipeWire Telephony](https://gkiagia.gr/2025-02-20-pipewire-telephony/) by George Kiagiadakis.
- [omarchy-dialer](https://github.com/karem505/omarchy-dialer), which proved the same PipeWire
  API can drive a Quickshell UI.
- [Omarchy](https://omarchy.org) and [Quickshell](https://quickshell.org).

## License

[MIT](LICENSE)
