# Releasing

The **git tag is the single source of truth for the app version.** There is no version number
to bump in a file — you tag a commit `vX.Y.Z`, and the build derives everything from it.

## Cut a release

```bash
git tag -a v0.2.0 -m "v0.2.0"
git push origin v0.2.0
```

Pushing a `v*` tag triggers [`.github/workflows/release.yml`](../.github/workflows/release.yml),
which cross-compiles the core, builds a signed release APK, and publishes it. Use
[semantic versioning](https://semver.org/): `MAJOR.MINOR.PATCH`.

## How the version reaches the build

[`android/app/build.gradle.kts`](../android/app/build.gradle.kts) derives both fields from git at
configure time:

- **`versionName`**
  - On an exact release tag: the tag without the `v` — `v0.2.0` → `0.2.0`.
  - Between tags (dev build): `git describe` — e.g. `0.2.0-4-g1a2b3c` (nearest tag, commits since,
    short SHA), with `-dirty` appended when the tree has uncommitted changes.
  - No tags / no git (e.g. a source tarball): `0.0.0-dev+<sha>`, or `0.0.0-dev`.
- **`versionCode`** — `MAJOR*10000 + MINOR*100 + PATCH` from the nearest tag (`v0.2.0` → `200`),
  so it increases monotonically across releases. Defaults to `1` when untagged. Missing or
  malformed components read as `0`.

This is why CI checks out with `fetch-depth: 0` — `git describe` needs the tags and full history.

The version is shown in-app on the **Me** screen, under Credits.

## Notes

- **Never move or delete a published tag.** `versionCode` must never decrease, and users verify
  release APKs against the tag. Ship a new patch tag instead.
- A local `./gradlew assembleDebug` with no tags in the clone builds fine — it just reports a
  `0.0.0-dev+<sha>` version.
