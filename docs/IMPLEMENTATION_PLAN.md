# Cockroach Chat — Implementation Plan

> **Execution model:** this plan is driven by a Ralph loop. The loop reads `docs/PROGRESS.md`
> (mutable ledger) and follows `docs/ai-build-loop.md` (standing instructions). This file is the stable,
> detailed reference — the "what and why." Hard constraints live in `docs/research-brief.md`;
> the normative wire format lives in `docs/protocol.md`.

## Context

A decentralized, serverless, peer-to-peer messenger for iOS + Android that works over
Bluetooth LE with no internet — for protests and network-blackout situations. Inspired by
bitchat, but full-featured and security-serious.

Grounded in a 6-agent research sweep (bitchat architecture, iOS/Android BLE platform limits,
prior-art failures — FireChat/Bridgefy/Briar/Meshtastic, mesh scaling theory, protest-app
security, stack options). See `docs/research-brief.md`.

**Locked decisions:**
- **Scale target: local clusters of 50–500 people** in physical proximity — *not* city-wide
  gossip. Fits BLE physics: 5–8 reliable links/phone, ~1–10 flooded msgs/sec per radio cell
  before congestion collapse.
- **V1 features:** public ownerless broadcast channels (IRC-style), E2E-encrypted 1:1 DMs,
  voice notes + images. **Cut from v1:** private groups, internet/Nostr bridge.
- **Stack: shared Rust core (`meshcore`) via UniFFI** + thin native BLE glue and native UIs
  (Kotlin/Compose, then Swift/SwiftUI). One protocol/crypto implementation = no iOS↔Android
  drift (bitchat's chronic bug source) and half the audit surface.
- **Build order: Android first**, then iOS on the same core.
- **Security bar: real-world deployment ambition** — vetted crypto only, forensics-resistant
  storage, honest threat model, external audit before promoting real-protest use.

## Product shape (v1)

- **Onboarding is nothing:** open app → display name → chatting. Keypair generated silently.
  A fresh install must be useful in 30 seconds (crisis adoption is spiky).
- **Channels:** ownerless, exist by being spoken into (`#medic`, `#north-gate`); anyone in
  range reads/writes. Default `#general`. Treated honestly as *public squares* — anyone in
  radio range (incl. police) can read; "encryption" of an open channel is theater. ~6 h retention.
- **DMs:** tap a peer → Noise XX E2E session. QR in-person verification + user petnames; wire
  display names rendered as untrusted.
- **Media = offer-and-fetch, never flooded:** a tiny offer (hash/size/2 KiB thumb) flows as a
  message; recipients pull the blob over a *direct link* from the nearest holder; holders serve
  onward (BitTorrent-ish). Opus voice ~1 KB/s; images downscaled ≤256 KiB.
- **"Mesh Active" honesty:** foreground = full relay; Android background = foreground-service
  relay at reduced duty; backgrounded iPhone ≈ no relaying (Apple overflow-area rules,
  unfixable). UI: "screen on = you're carrying the network."
- **Spam defense without moderators:** per-identity relay-enforced rate limits + hashcash PoW
  (~2–4 s) to mint an identity.

## Architecture

```
cockroach-chat/
├── Cargo.toml                  # workspace
├── crates/
│   ├── meshcore/               # pure sans-IO protocol core (no FFI, no platform deps)
│   ├── meshcore-ffi/           # UniFFI wrapper; cdylib+staticlib
│   └── sim/                    # desktop simulator: scenario DSL + CLI (hundreds of virtual nodes)
├── fuzz/                       # cargo-fuzz targets
├── testvectors/                # cross-platform protocol vectors (JSON)
├── android/                    # Gradle app (Compose)
├── ios/                        # Xcode + SPM wrapping MeshCore.xcframework (M5)
├── scripts/                    # gen-bindings.sh, build-xcframework.sh, run-sim.sh
├── docs/                       # protocol.md, threat-model.md, research-brief.md, IMPLEMENTATION_PLAN.md, PROGRESS.md, decisions/
└── .github/workflows/ci.yml
```

**`meshcore` modules:** `wire/` (hand-rolled fixed binary codec — unknown *versions* rejected,
unknown *types* relayed-not-parsed), `frag.rs`, `identity.rs` (Ed25519+X25519, rotating 8 B
ephemeral wire IDs synced to BLE MAC rotation, PoW mint), `noise.rs` (snow XX + per-message
HKDF ratchet, skip window 64, no downgrade path), `relay/` (SeenCache LRU-1000/5-min, TTL,
jitter, suppression, priorities P0–P3, rate limiter), `channels.rs` (GCS-filter sync for
partition heal), `store/` (SQLCipher via trait; `panic_wipe()`), `media.rs` (offer/fetch state
machines, chunked resume), `transport.rs` (**the only surface platforms implement**:
`send(link, frame)` out; `LinkUp/LinkDown/FrameReceived/MtuChanged` in), `node.rs`
(single-threaded sans-IO state machine; injected clock; `tick(now) -> next_wake`).

**FFI surface (UniFFI proc-macros):** object `MeshNode` (`start/stop`, `send_channel_message`,
`send_dm`, `join_channel`, `start_verification -> QrPayload`, `complete_verification`,
`set_petname`, `offer_media`, `fetch_media`, `on_transport_event`, `tick`, `panic_wipe`);
callback interfaces implemented natively: `BleTransport`, `KeyVault` (platform wraps/unwraps the
32 B DB master key via Android Keystore / Secure Enclave — never key-next-to-DB),
`EventListener(MeshEvent)`.

**Key dependencies:** `uniffi` 0.29.x (pinned; Element X-proven), `snow`
(Noise_XX_25519_ChaChaPoly_SHA256), `ed25519-dalek`/`x25519-dalek`,
`chacha20poly1305`/`hkdf`/`sha2`/`blake3`/`zeroize`, `rusqlite` +
`bundled-sqlcipher-vendored-openssl`, `lz4_flex`, `opus` crate **in core** (platforms feed PCM),
**no async runtime** (sync sans-IO core), Android: `org.mozilla.rust-android-gradle` 0.9.6 +
JNA, Compose, Accompanist permissions, ML Kit/zxing QR.

## Protocol essentials (pin in `docs/protocol.md` v0.1 at M0; tune only via simulator evidence)

- **Link frames** fit 182 B usable ATT payload (iOS 185 MTU floor): Complete `0x01` or
  Fragment `0x02` (`digest 8B + index 2B + total 2B + chunk`); reassembly 128 concurrent, 30 s
  timeout, 1 MiB cap, 16/link.
- **Packet:** `version(1) | type(1) | flags(1) | TTL(1, excluded from signature so relays
  decrement without re-signing) | timestamp_ms(8) | sender_eph_id(8) | [recipient_eph_id(8)] |
  payload_len(2) | payload | Ed25519 sig(64)`. Dedup key: first 8 B of SHA-256(signature).
- **Types:** Announce, ChannelMessage, DirectMessage, NoiseHandshake, Receipt,
  SyncRequest/Response, MediaOffer/FetchRequest/Chunk/FetchAck. Announce TLVs: static pubkeys,
  capability bitmap, PoW proof, prekey bundle (FS for store-and-forward), neighbor list ≤10.
- **Relay params:** TTL 7 (clamp 5 at degree ≥6); jitter 10–220 ms; cancel after ≥3 copies
  heard; rebroadcast prob 1.0/0.7/0.45 by density; split-horizon.
- **Priorities:** P0 control > P1 DM > P2 channel > P3 media (media only when others idle).
  Rate limits: 10 pkts/10 s burst, 30/min sustained per sender, 60 s greylist; no-PoW peers
  get half quota.
- **Compression:** LZ4 only >128 B; decompressed cap 4 KiB, ratio ≤10:1 else reject (zip-bomb defense).
- **PoW mint:** SHA-256(pubkey ‖ nonce), 22 leading zero bits (~2–4 s midrange phone).
- **Retention:** channel history 6 h / 1000 msgs; envelopes 24 h / 100 per peer; reassembly 30 s.

## Security model

Long-term keys in hardware keystore; **wire IDs rotate atomically with BLE MAC randomization
(~15 min)** to defeat cross-layer tracking; Noise XX (identity-hiding, MITM-binding — the
launch-day bitchat hole); QR fingerprint verification (Briar BQP-style) + petnames;
ciphertext-only at rest (SQLCipher, hardware-wrapped key); panic wipe (keys + DB + keystore
destroy) and disappearing messages as cryptographic erasure; FLAG_SECURE + notification
scrubbing; decompression caps + fuzzed parsers (the two exploits that actually broke
predecessors); `docs/threat-model.md` stating honestly what is **not** defended (co-located RF
traffic analysis, sybil without OOB trust); external audit before real-protest promotion.

## Milestones

### M0 — Rust core + simulator (~2–3 wks; longest; everything leans on it)
Workspace + CI + `protocol.md` v0.1 + ADR-0001 → `wire` codec + golden vectors → `frag`
property tests → `identity` + PoW → `relay` (pure, injected clock) → `SimTransport` (per-link
latency/loss/MTU + ~10 frames/s per-cell airtime budget) → `sim` scenario DSL → `store` →
`node.rs` + channels → FFI crate compiles + **JVM Kotlin smoke test through UniFFI (de-risks
M1's choke point)**.
**Done:** `cargo test --workspace` green incl. all sim scenarios; `cargo run -p sim -- --nodes
200 --scenario broadcast` ≥95% delivery; vectors committed; JVM smoke test passes.

### M1 — Android BLE glue: two phones chat offline (~3 wks) — *hardware-gated*
Gradle + rust-android-gradle wiring (cargoBuild before mergeJniLibs; `uniffi-bindgen generate
--library` task; pin NDK) → runtime permissions (`BLUETOOTH_SCAN` neverForLocation, `_CONNECT`,
`_ADVERTISE`) → `MeshForegroundService` (`connectedDevice` type, started from foreground —
Android 14/15 rule; notification IS the "Mesh Active" surface) → GATT dual role (one service
UUID; RX write-no-response / TX notify; filtered scan, RSSI gate −85 dBm, ≤5 links,
`requestMtu(517)`) → **status-133 discipline** (always `close()`, backoff 1→30 s, 5-strike
blacklist) → announce handshake before `LinkUp` → minimal Compose UI (`#general`, peer list).
**Done:** two physical phones (one cheap OEM), airplane mode: discover, connect, chat both
ways; reconnect after app kill; no 133 loops in logcat.

### M2 — Relay live: multi-hop, store-and-forward (~3 wks) — *hardware-gated*
3-phone A–B–C relay (A,C out of range) → channel history + GCS sync on link-up → offline
envelopes (24 h TTL, 100/peer) → on-device dedup/suppression validation (5 phones: ≤0.5·N
relay tx per message) → debug stats screen.
**Done:** filmed 3-phone relay demo; sim scenarios still green (same code path).

### M3 — Noise XX DMs + verification (~2 wks)
Sessions auto-established on first DM; ratchet + persisted-encrypted state; prekey bundles in
announce (FS for queued envelopes) → QR mutual-scan verification (verified/unverified/
unauthenticated UI) → panic wipe v1. *(Crypto core is loop-testable; verification UX is on-device.)*
**Done:** offline DMs; impersonation test (3rd phone replaying a victim's name) visibly flagged;
post-wipe `run-as` shows no DB files; `testvectors/noise-handshake.json` committed.

### M4 — Media (~3 wks)
AudioRecord PCM → core Opus (16 kHz mono, 60 s cap); image downscale 1280 px ≤256 KiB →
offer/fetch over direct links, 4 KiB chunks, P3 priority, resume on link drop; per-object blob
keys in DMs; `media_chunk` fuzz target.
**Done:** voice note + photo phone→phone offline; chat sent mid-transfer lands <2 s; fetch
resumes after walking away and back.

### M5 — iOS shell (~4 wks) — **DEFERRED**
> **Deferred indefinitely; the project is Android-only.** Kept here as design research so nothing
> is lost if it's picked back up — see the reasoning in [`ROADMAP.md`](../ROADMAP.md) § Deferred.
> The core stays sans-IO and platform-agnostic, so the door remains open by construction.

`build-xcframework.sh` (aarch64-ios + sim, Swift bindings, SPM package; macOS CI job) →
Info.plist background modes (`bluetooth-central`, `bluetooth-peripheral`) → CoreBluetooth dual
role with **state restoration** + pending-`connect()` reconnects (the one durable iOS
background primitive) → **Android-side overflow-area scan filter** (manufacturer-data 0x004C
bitmask — NHS COVID-app technique) so Android finds backgrounded iPhones → SwiftUI screens →
Swift test target consumes `testvectors/*.json`.
**Done:** iPhone↔Android offline chat incl. DMs; Android discovers a backgrounded iPhone; iOS
force-quit limitation documented.

### M6 — Hardening (~3 wks)
4-tier battery duty cycling (target ≤15%/12 h idle mesh; Battery Historian) → OEM survival
matrix (Samsung/Xiaomi/Pixel) + in-app whitelisting guidance → all fuzz targets ≥8 h clean →
panic wipe v2 (duress, FLAG_SECURE, notification scrubbing) → `docs/threat-model.md` → F-Droid
metadata + direct-APK page (store-takedown hedge).

## Testing & CI

- **Sim scenarios (CI, deterministic):** two_nodes_hello; broadcast_200_nodes; partition_heal;
  duplicate_storm; malicious_flooder; zipbomb_rejected; fragment_loss_timeout; dm_ratchet_reorder;
  ttl_clamp_dense; media_fetch_under_chat_load.
- **Fuzz targets:** packet_decode, frame_reassemble, announce_tlv, noise_ingest, decompress, media_chunk.
- **Cross-platform vectors:** generated by Rust, consumed by Rust + Kotlin JVM + (M5) Swift;
  CI fails if vectors drift without a spec version bump.
- **GitHub Actions:** rust (fmt/clippy -D warnings/test/sim smoke), android (NDK-cached
  cargoBuild + assembleDebug), ios from M5 (macos-14), fuzz-smoke (60 s/target on parser PRs + cron).
- **Physical rig:** fixed 2-phone desk rig (one cheap OEM) + scripted manual checklist per
  milestone; sans-IO core keeps ~95% of logic testable off-device.

## Top risks

| Risk | Mitigation |
|---|---|
| UniFFI/Gradle/NDK plumbing stalls M1 | JVM smoke test in M0; pin uniffi+plugin+NDK; copy Element X's known-good config |
| OEM battery killers murder the service | Cheap-OEM phone from M1; whitelisting UX; M6 survival matrix |
| Backgrounded iPhones invisible | Overflow-area bitmask filter on Android; pending-connect; honest screen-on-relay UX |
| SQLCipher per-ABI build pain | bundled-sqlcipher-vendored-openssl first; fallback: SQLite + app-layer AEAD behind same Store trait |
| No BLE in emulators | Sim-first design; desk rig for the native 5% |
| Solo burnout (the FireChat lesson) | Every milestone demoable; enforced v2 deferral list |
| Store rejection/takedown | Honest FGS declaration; F-Droid + direct APK from M6 |

## Deferred to v2+ (enforced)

Private groups; internet/Nostr bridge; geohash-scoped channels; spray-and-wait couriers; Wi-Fi
Aware bulk lane; PQ-hybrid handshake (flag bit reserved); relay election + airtime budgets
beyond static caps; multi-hop media fetch; APK-over-mesh; desktop client; cover traffic;
moderation/blocklists.
