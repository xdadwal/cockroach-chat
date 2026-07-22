# Changelog

Notable changes to Cockroach Chat. Format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/);
versioning is [Semantic Versioning](https://semver.org/), with the caveat that anything below 1.0
may change the wire format between releases.

Wire-format changes always bump `PROTOCOL_VERSION` in [`docs/protocol.md`](docs/protocol.md) and are
called out here under **Protocol**, because two phones on different protocol versions cannot talk.

## [Unreleased]

### Added
- App icon: adaptive launcher icon with a monochrome layer for themed icons, a matching
  notification icon, and the mark rendered in-app.
- `docs/threat-model.md` — what is defended, and plainly what is not.
- `SECURITY.md`, `CONTRIBUTING.md`, `GOVERNANCE.md`, `CODE_OF_CONDUCT.md`, `ROADMAP.md`,
  `NOTICE.md`, issue and PR templates.
- SIL Open Font License texts for the bundled Archivo and JetBrains Mono fonts, shipped inside the
  APK under `assets/licenses/` as the OFL requires.
- CI jobs for Android (build, lint, unit tests), dependency audit (`cargo-deny`), and MSRV.
  Dependabot for cargo, gradle, and github-actions.
- Tag-triggered release workflow producing a signed APK with `SHA256SUMS`.
- First Android unit test: en/hi `String.format` specifier parity.

### Changed
- `scripts/build-android-lib.sh` now runs on Linux as well as macOS, validates the NDK path, and
  accepts `ABIS=` to build a subset.
- Release builds take their signing config from a gitignored `keystore.properties`, and stay
  unsigned without it rather than falling back to the debug key.

### Fixed
- `Cargo.toml` declared `rust-version = "1.75"`, but the dependency tree requires 1.85. The
  declaration now matches what actually builds, and CI enforces it.

## Cutting a release

1. Move the `Unreleased` entries under a new version heading with today's date.
2. Bump `versionCode` (monotonic integer) and `versionName` in `android/app/build.gradle.kts`.
3. Tag and push: `git tag v0.2.0 && git push origin v0.2.0`.
4. The release workflow builds and signs the APK and opens a **draft** release. Check the printed
   certificate fingerprint against `SECURITY.md`, then publish.

The first signed release must also fill in the fingerprint placeholder in `SECURITY.md` — until
that is published, users have no way to verify what they downloaded.
