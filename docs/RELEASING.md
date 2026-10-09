# Releasing

The **git tag is the single source of truth for the app version.** There is no version number
to bump in a file — you tag a commit `vX.Y.Z`, and the build derives everything from it.

## Cut a release

```bash
git tag -a v0.2.0 -m "v0.2.0"
git push origin v0.2.0
```

Pushing a `v*` tag triggers [`.github/workflows/release.yml`](../.github/workflows/release.yml),
which cross-compiles the core, builds a signed release APK, and creates a **draft** release. Use
[semantic versioning](https://semver.org/): `MAJOR.MINOR.PATCH`.

Review the draft's APK, checksums, notices, and links before publishing it. Keep the educational
purpose and non-endorsement notice in the release description. Publish only from the intended
source commit; a local uncommitted change is not part of a tag-triggered build.

## Notices before publication

The root `LICENSE` remains MIT. `DISCLAIMER.md` states the maintainers' educational purpose;
it is not an educational-only license restriction or a guarantee against legal claims.

Keep the APK copies identical to the repository notices:

```bash
cp LICENSE android/app/src/main/assets/licenses/MIT-CockroachChat.txt
cp DISCLAIMER.md android/app/src/main/assets/licenses/DISCLAIMER.md
```

When dependencies change, refresh `assets/licenses/THIRD-PARTY.txt` from the locked Cargo graph
and Android `releaseRuntimeClasspath`. Record exact versions and source locations, preserve
copyright and attribution texts, and include the vendored SQLCipher/OpenSSL notices and the
MPL-2.0 license and source availability for UniFFI. The current snapshot includes the full Cargo
build graph, even packages not shipped on Android. Its header records the dependency-file hashes.
Cargo license-policy checks alone do not verify binary notice packaging.

Before publishing, run `cargo deny check advisories licenses bans sources`, build the APK,
and inspect `assets/licenses/` inside it. Verify that the project notices, both font licenses,
and the dependency inventory are present. Recheck English and Hindi app notice text when it
changes. See [the review scope and limitations](legal-review.md).

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
