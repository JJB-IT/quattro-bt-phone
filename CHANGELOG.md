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
- `quattro-bt-phone` CLI. `dial` asks for confirmation unless `--yes` is given.
- Project documentation, research notes and repository workflow.
- Contribution guide aligned with Omarchy's style, theming and commit conventions.
