# Changelog

All notable changes to this project are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the project uses
[Semantic Versioning](https://semver.org/).

## [Unreleased]

### Added
- `quattro-bt-phoned` daemon: tracks the phone over BlueZ and calls over PipeWire Telephony,
  with ring and missed-call notifications and a JSON-lines socket API (`docs/protocol.md`).
- Setup and permission flow in the protocol: select, connect, allow calls, allow contacts.
- `--mock` mode with a simulated phone for UI development.
- Contact and call-history cache in SQLite, one database per phone, with a vCard 2.1/3.0
  parser for PBAP data. Callers are named from the phonebook, and recents merge the phone's
  history with calls the daemon saw itself. Contact search ignores accents.
- Read-only PBAP sync of contacts (with photos) and call history through obexd, started with
  `request_contacts`/`sync` and repeated whenever the phone reconnects after a first
  approved sync.
- Omarchy plugin `jjb.bt-phone`: a bar button and panel with dialer (contact suggestions as
  you type), contacts, recents, recordings, an incoming-call screen and an in-call screen (mute,
  DTMF keypad, hold/swap, audio route, recording). A setup screen walks through choosing,
  connecting and allowing the phone. The panel opens by itself when the phone rings, and
  reconnects when the daemon restarts. It uses only the shell's theme tokens.
- `quattro-bt-phone` CLI. `dial` asks for confirmation unless `--yes` is given.
- Project documentation, research notes and repository workflow.
- Contribution guide aligned with Omarchy's style, theming and commit conventions.
