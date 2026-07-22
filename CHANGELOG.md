# Changelog

Notable changes to Cockroach Chat. Format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/);
versioning is [Semantic Versioning](https://semver.org/), with the caveat that anything below 1.0
may change the wire format between releases.

Wire-format changes always bump `PROTOCOL_VERSION` in [`docs/protocol.md`](docs/protocol.md) and are
called out here under **Protocol**, because two phones on different protocol versions cannot talk.

This log starts at **0.1**, the state of the project when it was opened up for contributors.
Everything before that is in the git history and in [`docs/PROGRESS.md`](docs/PROGRESS.md), which
remains the detailed build ledger.

## [Unreleased]

### Added
- App icon: adaptive launcher icon with a monochrome layer for themed icons, a matching
  notification icon, and the mark rendered in-app.
- `docs/threat-model.md` — what is defended, and plainly what is not.
- `SECURITY.md`, `CONTRIBUTING.md`, `CODE_OF_CONDUCT.md`, `NOTICE.md`, issue and PR templates.
- SIL Open Font License texts for the bundled Archivo and JetBrains Mono fonts, shipped inside the
  APK under `assets/licenses/` as the OFL requires.
- CI jobs for Android (build, lint, unit tests), dependency audit (`cargo-deny`), MSRV, and a
  60 s-per-target fuzz smoke run. Dependabot for cargo, gradle, and github-actions.
- Fuzz targets for the three parsers (`wire_decode`, `frag_reassemble`, `decompress`), closing a
  claim `docs/IMPLEMENTATION_PLAN.md` had been making without them existing.
- Tag-triggered release workflow producing a signed APK with `SHA256SUMS`.
- First Android unit test: en/hi `String.format` specifier parity.
- In-app **Credits** page (Me → footer) naming every bundled font, library and tool with its
  author and licence.

### Changed
- `scripts/build-android-lib.sh` now runs on Linux as well as macOS, validates the NDK path, and
  accepts `ABIS=` to build a subset.
- Release builds take their signing config from a gitignored `keystore.properties`, and stay
  unsigned without it rather than falling back to the debug key.

### Fixed
- `Cargo.toml` declared `rust-version = "1.75"`, but the dependency tree requires 1.85. The
  declaration now matches what actually builds, and CI enforces it.

## [0.1] — 2026-07-22

The baseline this log starts from: a working Android BLE mesh messenger, validated on real
hardware (Galaxy S23 ↔ OnePlus, airplane mode). **Never published as a binary** — there is no
signed APK for 0.1, and the release pipeline that would produce one arrived afterwards. It is a
source milestone, recorded here so later entries have something to be relative to.

At this point the project had:

- **`meshcore`** — the sans-IO Rust core: wire codec with golden vectors, fragmentation, Ed25519 +
  X25519 identity with hashcash proof-of-work, flooding relay (TTL, jitter, suppression, dedup,
  rate limiting, greylisting), channels with set reconciliation, Noise XX DMs, and
  store-and-forward. 57 unit tests.
- **Deterministic simulator** — 10 scenarios including 200-node broadcast, partition heal,
  duplicate storm, malicious flooder, and multi-hop relay.
- **Encrypted persistence** — SQLCipher via `meshcore-store`, keyed by an Android Keystore
  hardware-wrapped key.
- **Android app** — dual-role BLE GATT, an always-on foreground-service relay, public channels
  with rate limits, end-to-end encrypted DMs with MITM binding, in-person QR verification with
  4-word safety numbers and petnames, panic wipe, `FLAG_SECURE`, and a bilingual
  English / हिन्दी UI.
- **CI** — a single Rust job: fmt, clippy, tests, golden vectors, and a simulator smoke run.

## Cutting a release

1. Move the `Unreleased` entries under a new version heading with today's date.
2. Bump `versionCode` (monotonic integer) and `versionName` in `android/app/build.gradle.kts`.
3. Tag and push: `git tag v0.2.0 && git push origin v0.2.0`.
4. The release workflow builds and signs the APK and opens a **draft** release. Check the printed
   certificate fingerprint against `SECURITY.md`, then publish.

The first signed release must also fill in the fingerprint placeholder in `SECURITY.md` — until
that is published, users have no way to verify what they downloaded.

**Optional:** there is no `v0.1` tag. If you want a git anchor for the baseline above, tag the
commit it describes:

```bash
git tag -a v0.1 0c4d23d -m "Baseline: Android BLE mesh, E2E DMs, verification, i18n"
git push origin v0.1
```

That won't trigger the release workflow — GitHub runs the workflow file as it exists *in the
tagged commit*, and `0c4d23d` predates `release.yml`. Every tag pushed after this branch merges
will trigger it, so from then on use throwaway names like `v0.0.1-test` for experiments.
