# Third-party notices

Cockroach Chat is licensed under the MIT License (see [`LICENSE`](LICENSE)). Its educational
purpose and non-endorsement notice is in [`DISCLAIMER.md`](DISCLAIMER.md). Attribution below
does not imply affiliation, sponsorship, or endorsement by any listed person or organization.
Third-party components retain their own licenses; the educational purpose does not waive them.

It bundles and links the third-party work listed below.

This file is the human-readable index. The notices in
`android/app/src/main/assets/licenses/` are packaged **inside the APK**:

- `MIT-CockroachChat.txt`: the project's copyright and MIT license, verbatim.
- `DISCLAIMER.md`: the educational-purpose and non-endorsement notice.
- `OFL-Archivo.txt` and `OFL-JetBrainsMono.txt`: the font licenses.
- `THIRD-PARTY.txt`: versioned Rust and Android dependency inventories, source locations,
  and license and attribution texts, including SQLCipher, OpenSSL, libffi, and UniFFI.

For example: `unzip -p cockroach-chat-<version>.apk assets/licenses/THIRD-PARTY.txt`.
The app's **Credits** page, reached from **Me**, provides selected acknowledgements and a short
project notice. It does not replace the license texts or certify compliance with every legal
requirement. Refresh the bundled inventory whenever dependencies change; see
[the release instructions](docs/RELEASING.md#notices-before-publication).

## Bundled fonts

Both fonts are used unmodified under the **SIL Open Font License 1.1**, whose terms require the
license to travel with the font. Full texts:
[`android/app/src/main/assets/licenses/`](android/app/src/main/assets/licenses/).

| Font | Version | Copyright | License |
|---|---|---|---|
| Archivo (`res/font/archivo.ttf`) | 2.001 | Copyright 2020 The Archivo Project Authors ([Omnibus-Type/Archivo](https://github.com/Omnibus-Type/Archivo)) | OFL 1.1 |
| JetBrains Mono (`res/font/jetbrainsmono.ttf`) | 2.211 | Copyright 2020 The JetBrains Mono Project Authors ([JetBrains/JetBrainsMono](https://github.com/JetBrains/JetBrainsMono)) | OFL 1.1 |

Neither font declares a Reserved Font Name, so no renaming obligation applies to these files as
shipped. If you modify a font binary, re-read OFL §3 before redistributing.

## Rust dependencies

145 transitive crates, overwhelmingly `MIT OR Apache-2.0`. The ones worth calling out:

| Crate | License | Why it matters |
|---|---|---|
| `uniffi` (+ 7 related crates) | **MPL-2.0** | Used unmodified. Covered source remains available under MPL-2.0; the bundled inventory gives a source download for each exact version. Modifications to covered files must also comply with MPL-2.0. |
| `ed25519-dalek`, `x25519-dalek` | BSD-3-Clause | Signing and key agreement. |
| `snow` | Apache-2.0 OR MIT | Noise XX protocol implementation. |
| `chacha20poly1305`, `sha2`, `zeroize` | Apache-2.0 OR MIT | RustCrypto primitives. |
| `rusqlite`, `libsqlite3-sys` | MIT | SQLCipher binding. |
| `openssl-src` | Apache-2.0 (OpenSSL 3.x) | Vendored and statically linked into the APK to back SQLCipher on Android. |
| `lz4_flex` | MIT | Payload compression, with decompression caps. |

`libsqlite3-sys` is built with the `bundled-sqlcipher-vendored-openssl` feature, which vendors
**SQLCipher** (Zetetic LLC, BSD-style license) and **OpenSSL 3.x** (Apache-2.0) into the shipped
binary. Their license texts are copied from the vendored source into `THIRD-PARTY.txt`.

The inventory covers all 145 external packages in the locked Cargo graph, including build-only
and platform-specific packages. It is not a claim that every listed package ships in the APK.
Unmodified UniFFI 0.28.3 source is available from
[the upstream release](https://github.com/mozilla/uniffi-rs/tree/v0.28.3) and from the individual
crate source URLs in the inventory. Recipients retain their MPL-2.0 rights to that covered source.

Regenerate the full inventory with:

```bash
cargo metadata --format-version 1 | jq -r '.packages[] | "\(.name)\t\(.version)\t\(.license)"' | sort -u
```

## Android dependencies

Jetpack Compose, AndroidX, and `kotlinx-coroutines` are Apache-2.0. `zxing-android-embedded`
(QR generation and scanning) is Apache-2.0. JNA is dual Apache-2.0 / LGPL-2.1+; we use it under
**Apache-2.0**.

`THIRD-PARTY.txt` lists the 62 distinct dependency coordinates resolved for the Android release
runtime classpath, with their published Maven license metadata and any embedded license/notice
files. An Apache-2.0 license copy is included. Names and versions reflect this release snapshot.

## Export note

This project implements and distributes cryptography. Redistributing it may carry notification or
compliance obligations depending on your jurisdiction. See [`SECURITY.md`](SECURITY.md).
