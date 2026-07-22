# Third-party notices

Cockroach Chat is licensed under the MIT License (see [`LICENSE`](LICENSE)). It bundles and links
the third-party work listed below.

This file is the human-readable index. The license texts themselves live in
`android/app/src/main/assets/licenses/` and are packaged **inside the APK**, so they travel with
the binary as the OFL requires — not only with this repository. To read them from a downloaded
APK: `unzip -p cockroach-chat-<version>.apk assets/licenses/OFL-Archivo.txt`.

There is no in-app licenses viewer yet. Bundling satisfies the license terms; a viewer would be
the friendlier form and is a welcome contribution.

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
| `uniffi` (+ 7 related crates) | **MPL-2.0** | Weak, file-level copyleft. Compatible with shipping this MIT app, but modifications *to UniFFI's own files* must stay MPL-2.0 and be published. We use it unmodified. |
| `ed25519-dalek`, `x25519-dalek` | BSD-3-Clause | Signing and key agreement. |
| `snow` | Apache-2.0 OR MIT | Noise XX protocol implementation. |
| `chacha20poly1305`, `sha2`, `zeroize` | Apache-2.0 OR MIT | RustCrypto primitives. |
| `rusqlite`, `libsqlite3-sys` | MIT | SQLCipher binding. |
| `openssl-src` | Apache-2.0 (OpenSSL 3.x) | Vendored and statically linked into the APK to back SQLCipher on Android. |
| `lz4_flex` | MIT | Payload compression, with decompression caps. |

`libsqlite3-sys` is built with the `bundled-sqlcipher-vendored-openssl` feature, which vendors
**SQLCipher** (Zetetic LLC, BSD-style license) and **OpenSSL 3.x** (Apache-2.0) into the shipped
binary. Their license texts are distributed inside those crates' source.

Regenerate the full inventory with:

```bash
cargo metadata --format-version 1 | jq -r '.packages[] | "\(.name)\t\(.version)\t\(.license)"' | sort -u
```

## Android dependencies

Jetpack Compose, AndroidX, and `kotlinx-coroutines` are Apache-2.0. `zxing-android-embedded`
(QR generation and scanning) is Apache-2.0. JNA is dual Apache-2.0 / LGPL-2.1+; we use it under
**Apache-2.0**.

## Export note

This project implements and distributes cryptography. Redistributing it may carry notification or
compliance obligations depending on your jurisdiction. See [`SECURITY.md`](SECURITY.md).
