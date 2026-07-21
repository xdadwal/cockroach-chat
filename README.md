# Cockroach Chat

A decentralized, serverless, peer-to-peer messenger for iOS and Android that works over
Bluetooth LE with **no internet and no servers** — built for protests and network blackouts.
Inspired by bitchat, but full-featured and security-serious.

Named for the thing that survives when the lights go out.

## Status

🚧 Early build. Planning is complete; implementation starts at **M0** (Rust core + simulator).

## How it works

Every phone is both a relay and a client. Messages hop phone-to-phone over BLE (store-and-
forward) to cover a dense crowd of ~50–500 people in physical proximity. A shared **Rust core**
(`meshcore`) owns the protocol, crypto, and mesh logic; thin native shells (Kotlin/Compose,
then Swift/SwiftUI) own only the BLE radio and UI. One implementation → no cross-platform drift.

- Public ownerless broadcast channels (IRC-style), treated honestly as public squares.
- End-to-end encrypted 1:1 DMs (Noise XX + per-message ratchet), QR in-person verification.
- Voice notes + images via offer-and-fetch (never flooded).
- Panic wipe, ciphertext-only at rest, hashcash-gated identities.

It does **not** claim city-scale realtime chat — BLE physics doesn't allow it. See
`docs/research-brief.md` for the honest limits.

## Docs

| File | What |
|---|---|
| `docs/IMPLEMENTATION_PLAN.md` | Full plan: architecture, milestones M0–M6, protocol, security. |
| `docs/PROGRESS.md` | Live build ledger — what's done, what's next. |
| `docs/protocol.md` | Normative wire format (written at M0). |
| `docs/research-brief.md` | The constraints and prior-art lessons everything is built on. |
| `docs/threat-model.md` | What's defended and what isn't (written at M6). |
| `PROMPT.md` | Standing prompt for autonomous (Ralph-loop) iterative builds. |

## Android app

A working Android app runs the Rust core through UniFFI. On the emulator (which has no Bluetooth
radio) it uses an in-app **loopback** transport to demonstrate two real mesh nodes chatting —
signing, relaying, and **signature-verifying** each message through the actual core. The real BLE
GATT dual-role transport (`app/.../ble/`) compiles and is a drop-in replacement for hardware.

```bash
# 1. Cross-compile the Rust core to arm64-v8a and generate Kotlin bindings
ANDROID_NDK_HOME=~/Library/Android/sdk/ndk/<version> ./scripts/build-android-lib.sh
# 2. Build + install the app (JAVA_HOME must point at a JDK 17+; e.g. Android Studio's JBR)
cd android && ./gradlew installDebug
```

Requires: Rust stable + `aarch64-linux-android` target, Android SDK + NDK. The `.so`, JNA, and
generated bindings are packaged into the APK automatically.

## Building with the Ralph loop

This repo is structured for iterative autonomous development. To drive the build:

```bash
/ralph-loop "$(cat PROMPT.md)" --completion-promise "COCKROACH_CHAT_COMPLETE" --max-iterations 60
```

Each iteration reads `docs/PROGRESS.md`, does the next atomic task test-first, verifies, and
commits. Milestones **M1, M2, M4, M5** require physical phones and can't be verified by an
unattended loop — the loop hands those off to you. M0 and the crypto core (M3) are fully
loop-buildable.

## License

See `LICENSE`.
