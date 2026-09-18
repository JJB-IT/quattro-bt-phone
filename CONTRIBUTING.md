# Contributing

Thanks for helping. This file covers how work flows through the repo.

## Workflow

1. **Open or pick an issue** first for anything bigger than a typo, so the change has context.
2. **Branch from `main`**, named `<type>/<short-description>`, e.g. `feat/pbap-sync`,
   `fix/ring-notification`, `docs/readme-install`.
3. **Commit** using [Conventional Commits](https://www.conventionalcommits.org/):

   ```
   <type>(<scope>): <summary in the imperative, ≤ 72 chars>

   Why the change is needed and anything non-obvious about how.

   Closes #12
   ```

   Types: `feat`, `fix`, `docs`, `refactor`, `perf`, `test`, `build`, `ci`, `chore`.
   Scopes: `daemon`, `cli`, `plugin`, `proto`, `audio`, `pbap`, `nix`, `repo`.
4. **Open a pull request** to `main` using the template. Keep PRs focused on one concern.
   Draft PRs are welcome for early feedback.
5. **CI must pass**: formatting, clippy, tests, plugin validation and the Nix build.
6. PRs are **squash-merged**. The PR title becomes the commit on `main`, so it must be a valid
   Conventional Commit. The branch is deleted automatically after merge.

`main` is protected: no direct pushes, and CI must be green before merging.

## Local checks

```sh
nix develop
cargo fmt --all
cargo clippy --all-targets -- -D warnings
cargo test
nix flake check
```

## Testing without a phone

The daemon has a `--mock` mode that simulates a phone, contacts and calls. Use it for UI and
protocol work. Anything that touches a real phone, such as placing calls or pulling contacts,
affects real people and data. Keep PBAP strictly read-only and never commit phone data
(`*.vcf`, recordings, MAC addresses).

## Releases

Versions follow [SemVer](https://semver.org/). User-visible changes go under
`## [Unreleased]` in [CHANGELOG.md](CHANGELOG.md) as part of the PR. A release is a tagged
commit (`vX.Y.Z`) with a GitHub release.
