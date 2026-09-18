# Contributing

Thanks for helping. This project follows [Omarchy](https://github.com/omacom/omarchy)'s own
conventions (`AGENTS.md`, `agents/skills/`, `docs/theming.md` and `.editorconfig` in the
Omarchy repo). The aim is code that could live in Omarchy itself.

## Workflow

1. **Open or pick an issue** first for anything bigger than a typo.
2. **Branch from `main`**: `feat/…`, `fix/…`, `docs/…` or `chore/…`, e.g. `feat/pbap-sync`.
3. **Commit atomically.** Each commit holds one coherent change and nothing unrelated.
4. **Write succinct commit messages** that describe the change in the imperative, Omarchy-style:

   ```
   Resolve the audio gateway by address instead of object path

   The agN index changes when the phone reconnects, so a cached path
   points at nothing after a Bluetooth drop.

   Closes #12
   ```

5. **Open a pull request** to `main` using the template. Keep it focused on one concern.
   The PR title follows the same style as a commit subject, because PRs are
   **squash-merged** and the title becomes the commit on `main`.
6. **CI must pass**: rustfmt, clippy, tests, plugin checks and the Nix build. `main` is
   protected, so there are no direct pushes.

## Style

- **Everything except Rust:** two spaces, no tabs, LF, final newline
  ([`.editorconfig`](.editorconfig), copied from Omarchy).
- **Rust:** `cargo fmt` (4 spaces) and `cargo clippy -- -D warnings`. No `unsafe`.
- **Bash** (Omarchy rules): `#!/bin/bash` shebangs; `[[ ]]` for string/file tests and `(( ))` for
  numeric ones; inside `[[ ]]` don't quote variables, but do quote string literals;
  quote paths with spaces rather than escaping them.
- **Comments** explain *why*, not what.

## The Omarchy plugin (`plugin/`)

Follow `agents/skills/shell-dev.md` and `docs/theming.md` from Omarchy:

- **Theme everything from the shell.** Colours come from `Color.*` or `bar.foreground`,
  structure from `Style.*` (spacing, fonts, radius, control fills), and fonts from
  `bar.fontFamily`. **No literal colours, font families or pixel sizes.** CI rejects them.
  Switching themes with `omarchy theme set …` must restyle the plugin live.
- **Borders:** use `BorderSurface` with `Border.controlSpec(…)` for controls and
  `Border.surfaceSpec(…)` for surfaces, so themes can set gradients and per-side widths.
- **Reuse `qs.Ui` controls** (`Button`, `ButtonGroup`, `TextField`, `ToggleSwitch`,
  `KeyboardPanel`, `PanelKeyCatcher`…) before writing new ones.
- Entry points are `Item`s. Panels expose `open(payloadJson)` / `close()`. The IPC target
  is the plugin id.
- Use `Quickshell.env("OMARCHY_PATH")` when you need Omarchy files. Never derive fallback
  paths.
- Glyphs: when editing files that contain Nerd Font glyphs, make targeted edits, or insert them
  with `String.fromCodePoint(0x…)` in QML or `chr(0x…)` in Python.
- **Verify visual changes in the running UI**, not just in tests. Attach before/after
  screenshots of the panel to the PR. Crop them to the panel, and use `--mock` data so no
  real contacts appear.
- `omarchy plugin validate plugin` must pass. No symlinks inside `plugin/`.

## Local checks

```sh
nix develop
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
nix flake check
omarchy plugin validate plugin
```

## Testing without a phone

`quattro-bt-phoned --mock` simulates a phone with contacts, history and calls. Drive it with
`quattro-bt-phone simulate ring|remote-answer|remote-hangup|disconnect|reset-setup`.

Anything that touches a real phone, such as placing calls or pulling contacts, affects real
people and data. Keep PBAP strictly read-only, and **never commit personal data**: MAC
addresses, phone numbers, vCards, recordings or screenshots with real contacts.

## Releases

Versions follow [SemVer](https://semver.org/). User-visible changes go under
`## [Unreleased]` in [CHANGELOG.md](CHANGELOG.md) in the same PR. A release is a tag `vX.Y.Z`
plus a GitHub release.
