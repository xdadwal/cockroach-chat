# Roadmap

A one-minute view. [`docs/PROGRESS.md`](docs/PROGRESS.md) is the detailed ledger and stays the
source of truth; [`docs/IMPLEMENTATION_PLAN.md`](docs/IMPLEMENTATION_PLAN.md) has the full
architecture and the v2 deferral list.

## Done

| | |
|---|---|
| **M0** | Rust core + deterministic simulator — wire codec, fragmentation, identity + PoW, relay (dedup/jitter/suppression/rate limits), channels, SQLCipher store. 57 unit tests, 10 simulator scenarios. |
| **M1** | Android BLE glue — dual-role GATT, two phones chatting offline in airplane mode. |
| **M2** | Multi-hop relay + store-and-forward, in an always-on foreground service. |
| **M3** | Noise XX encrypted DMs with MITM binding, QR fingerprint verification, safety numbers, petnames. |
| **Polish** | Public channels, panic wipe, `FLAG_SECURE`, English + हिन्दी, design-system UI, app icon. |

Validated on real hardware: Galaxy S23 ↔ OnePlus.

## Now — going public

Making the project safe to promote and possible to contribute to: threat model, security policy,
licensing compliance, CI beyond Rust, and signed releases. Mostly landed; see the open PRs.

**Wanted:** security review, hardware reports from device combinations we don't own.

## Next

### M4 — Media
Voice notes (Opus, 16 kHz mono, 60 s cap) and images (downscaled, ≤256 KiB) over an offer-and-fetch
protocol: 4 KiB chunks at low priority, resuming after a link drop, with per-object blob keys
carried in DMs.

### M5 — iOS  ·  *biggest open contribution*
Not started. The Rust core is built to drop in — the work is an `xcframework` build, CoreBluetooth
dual role with state restoration and pending-connect reconnects (the one durable iOS background
primitive), the Android-side overflow-area scan filter so Android can find backgrounded iPhones, and
SwiftUI screens. **If you know CoreBluetooth, this is the highest-impact thing you could pick up.**

### M6 — Hardening
Four-tier battery duty cycling (target ≤15% per 12 h idle), an OEM survival matrix
(Samsung/Xiaomi/Pixel) with in-app whitelisting guidance, all fuzz targets clean for ≥8 h, panic
wipe v2 (duress mode, notification scrubbing), and F-Droid metadata with a direct-APK page as a
store-takedown hedge.

## Before anyone should rely on this

- **External security audit.** Non-negotiable before we promote this for real protest use.
- **Fuzzed parsers.** The two exploits that broke comparable projects were both parser bugs.
- **Reproducible builds**, so a release binary can be verified against source rather than trusted.

See [`docs/threat-model.md`](docs/threat-model.md) for what is and isn't defended today.

## Explicitly not planned

Group DMs, global/internet-bridged rooms, media in public channels, message editing, and anything
that assumes city-scale reach. The target is local clusters of ~50–500 people in physical
proximity — BLE physics doesn't support more, and pretending otherwise is how predecessors failed.
