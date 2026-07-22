# Changelog

Notable changes to Cockroach Chat, written for the people who install it.

**Entries are added when a release is tagged, not as changes land.** Each is a short summary of what
that release means in the field — what now works, what changed, what to watch out for. Per-commit
detail lives in the git history and the pull requests; the development ledger is
[`docs/PROGRESS.md`](docs/PROGRESS.md).

Versioning is [Semantic Versioning](https://semver.org/), with the caveat that anything below 1.0
may change the wire format between releases.

**Protocol changes get their own line, always.** Two phones on different `PROTOCOL_VERSION`s cannot
talk to each other, and someone in a crowd has no way to diagnose that — so any change to
[`docs/protocol.md`](docs/protocol.md) is called out explicitly, not folded into a general
"improvements" note.

## Unreleased

No release has been cut yet. The first tagged release will be the first entry here.

Version 0.1 is the pre-changelog baseline — the Android BLE mesh app as it stood when the project
opened up for contributors. It was never published as a binary; see the git history and
[`docs/PROGRESS.md`](docs/PROGRESS.md) for what it contained.

## Cutting a release

1. Add a short summary entry above, under a new version heading with today's date. Summarise —
   don't transcribe the commit log. Call out any protocol change on its own line.
2. Bump `versionCode` (monotonic integer) and `versionName` in `android/app/build.gradle.kts`.
3. Tag and push: `git tag v0.2.0 && git push origin v0.2.0`.
4. The release workflow builds and signs the APK and opens a **draft** release. Check the printed
   certificate fingerprint against [`SECURITY.md`](SECURITY.md), then publish.

The first signed release must also fill in the fingerprint placeholder in `SECURITY.md` — until
that is published, users have no way to verify what they downloaded.

Every tag matching `v*` triggers the release workflow, so use throwaway names like `v0.0.1-test`
for experiments.
