# 🪳 Cockroach Chat

**You cannot squash a signal.**

A decentralized, serverless, peer-to-peer messenger that works entirely over **Bluetooth LE —
no internet, no cell, no servers, no accounts.** Built for protests, disasters, and network
blackouts. Named for the thing that survives when the lights go out.

Every phone is both a client and a relay. Messages hop phone-to-phone across a dense crowd, so
the network exists only as long as people's radios are on — and it belongs to no one.

---

## Field principles

The whole app is built to these:

1. **Never fake trust. Never fake connectivity.** If we're unsure, the UI says unsure.
2. **Glanceable in sun or dark, one hand, under duress.**
3. **A field radio, not a social app.**
4. **Screen on = you carry the network.**

---

## What works today

Proven on real hardware (Galaxy S23 ↔ OnePlus, airplane mode):

- **Two phones chat offline over BLE** — discover, connect (dual central + peripheral GATT),
  relay, and exchange messages with **no internet**, each one Ed25519 signature-verified.
- **Always-on relay** — the mesh runs in a foreground service, so it keeps carrying messages
  when the app is backgrounded or the screen is locked. Battery-aware: drops to low-power BLE
  scanning/advertising when idle.
- **Public channels** — ownerless, join-by-name (`#general`, `#alerts`, `#medics`, `#supplies`,
  `#lost+found`, `#exits`), treated honestly as **public squares** (anyone in range can read,
  including police). The Announcement broadcast is rate-limited to 1/min; channels to 2/10s.
- **End-to-end encrypted DMs** — Noise XX with **MITM-binding** (the encrypted session must match
  the peer's announced key). Handshakes retry until they land, so DMs deliver reliably.
- **In-person verification** — scan a peer's QR fingerprint; a 4-word **safety number** to compare
  out loud; assign a private petname. Verifying establishes the session so **both phones flip to
  verified immediately**. A stranger can't DM you without a scan.
- **Panic wipe** — press-and-hold to cryptographically erase keys + the encrypted database
  (hardware-key erasure via Android Keystore). No backup exists anywhere.
- **Forensics-resistant at rest** — messages and identity live in a **SQLCipher** database keyed
  by a hardware-wrapped key; `FLAG_SECURE` blocks screenshots and the recents thumbnail.
- **Bilingual UI** — English and **हिन्दी** (Hindi), switchable in-app, chosen at first run.

**Core:** the Rust `meshcore` is green — 57 unit tests + 10 deterministic simulator scenarios
(broadcast to 200 nodes, partition heal, duplicate-storm suppression, malicious-flooder
containment, multi-hop relay, store-and-forward, DM handshake retry, and more).

**iOS:** not started yet — the shared core is built to drop in (planned M5).

---

## How it works

One protocol, one implementation, no cross-platform drift:

```
┌──────────────────────────────┐        ┌──────────────────────────────┐
│  Android (Kotlin / Compose)  │        │        iOS (planned)         │
│  BLE GATT radio · UI only    │        │  CoreBluetooth · UI only     │
└───────────────┬──────────────┘        └───────────────┬──────────────┘
                │            UniFFI  (meshcore-ffi)      │
                └──────────────────┬────────────────────┘
                                   ▼
        ┌───────────────────────────────────────────────┐
        │   meshcore  — pure sans-IO Rust core           │
        │   wire codec · fragmentation · identity + PoW  │
        │   flooding relay (TTL/jitter/suppression/dedup │
        │   /rate-limit) · channels + GCS sync · Noise   │
        │   XX DMs · store-and-forward · SQLCipher store │
        └───────────────────────────────────────────────┘
```

The core is **sans-IO**: no threads, no sockets, no clock syscalls. Time, transport, and storage
are injected, which is what makes hundreds of virtual nodes replayable in a deterministic
simulator. The native shells own only the BLE radio and the screen.

### Repo layout

| Path | What |
|---|---|
| `crates/meshcore` | The protocol/crypto/mesh core (sans-IO, no platform deps). |
| `crates/meshcore-store` | SQLCipher-backed encrypted persistence (`Store` trait). |
| `crates/meshcore-ffi` | UniFFI wrapper → Kotlin/Swift bindings. |
| `crates/sim` | Desktop simulator: deterministic scenarios over virtual radios. |
| `android/` | Android app (Jetpack Compose, JNA, ZXing). |
| `docs/` | Plan, protocol, progress ledger, ADRs, research brief. |

---

## Honest limits

- **Local clusters, ~50–500 people in physical proximity** — not city-scale realtime chat. BLE
  physics (5–8 reliable links/phone, limited airtime) doesn't allow it, and we don't pretend it does.
- **The network needs radios on.** Backgrounded iPhones barely relay (Apple's rules); the UI is
  honest that screen-on carries the mesh.
- **Public channels are public.** Anyone in radio range reads them — there is no lock, ever.
- **Not yet audited.** The crypto is vetted (Noise via `snow`, ed25519-dalek, SQLCipher), but this
  has **not** had an external security audit. Don't bet a life on it yet.

See `docs/research-brief.md` for the constraints and prior-art lessons everything is built on.

---

## Build & run

**Rust core (any desktop):**

```bash
cargo test --workspace                                   # 57 unit tests + scenarios
cargo run -p sim -- --nodes 200 --scenario broadcast     # deterministic mesh sim
```

**Android app** (needs Rust stable + `aarch64-linux-android` / `armv7-linux-androideabi` /
`x86_64-linux-android` targets, Android SDK + NDK, and a JDK 17+):

```bash
# 1. Cross-compile the Rust core to the 3 ABIs and (re)generate Kotlin bindings
ANDROID_NDK_HOME=~/Library/Android/sdk/ndk/<version> ./scripts/build-android-lib.sh

# 2. Build + install (JAVA_HOME must point at a JDK 17+, e.g. Android Studio's JBR)
cd android && ./gradlew installDebug
```

The `.so` libraries, JNA, and generated UniFFI bindings are packaged into the APK automatically.
Real BLE needs a **physical phone** (emulators have no Bluetooth radio).

---

## Docs

| File | What |
|---|---|
| `docs/IMPLEMENTATION_PLAN.md` | Full plan: architecture, milestones M0–M6, protocol, security. |
| `docs/PROGRESS.md` | Live build ledger — what's done, what's next. |
| `docs/protocol.md` | Normative wire format. |
| `docs/PERFORMANCE.md` | Performance backlog and tuning notes. |
| `docs/research-brief.md` | The constraints and prior-art lessons everything is built on. |
| `docs/decisions/` | Architecture decision records. |

---

## License

See [`LICENSE`](LICENSE).

<br>

> *Together we survive. Radios on, mesh alive. Verify in person.*
