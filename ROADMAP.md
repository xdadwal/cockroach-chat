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

### M6 — Hardening
Four-tier battery duty cycling (target ≤15% per 12 h idle), an OEM survival matrix
(Samsung/Xiaomi/Pixel) with in-app whitelisting guidance, all fuzz targets clean for ≥8 h, panic
wipe v2 (duress mode, notification scrubbing), and F-Droid metadata with a direct-APK page as a
store-takedown hedge.

## Before anyone should rely on this

- **External security audit.** Non-negotiable before we promote this for real protest use.
- **Sustained fuzzing.** Targets exist for the three parsers and run in CI, but M6's bar is ≥8 h
  clean per target, and the Noise handshake and store aren't covered yet.
- **Reproducible builds**, so a release binary can be verified against source rather than trusted.

See [`docs/threat-model.md`](docs/threat-model.md) for what is and isn't defended today.

## Deferred

**iOS (was M5) — not being worked on, and not accepting contributions for it right now.**

This is a real limitation, not a technicality: iPhones cannot join the mesh at all, so in a mixed
crowd a large fraction of people are unreachable. It's stated plainly in the README's honest limits
rather than buried here.

The reason to defer is focus. The Android app is unaudited, its parsers have only hours of fuzzing,
and battery duty-cycling (M6) isn't built — a second platform would double the surface needing
review while the first one still isn't trustworthy. Apple's background rules also mean a
backgrounded iPhone barely relays, so an iOS port buys less mesh capacity than its cost suggests.

The design work survives in [`docs/IMPLEMENTATION_PLAN.md`](docs/IMPLEMENTATION_PLAN.md) § M5
(`xcframework` build, CoreBluetooth dual role with state restoration, the overflow-area scan
filter) so nothing is lost if this is picked back up. The Rust core stays platform-agnostic and
sans-IO, which keeps the door open by construction.

## Explicitly not planned

Group DMs, global/internet-bridged rooms, media in public channels, message editing, and anything
that assumes city-scale reach. The target is local clusters of ~50–500 people in physical
proximity — BLE physics doesn't support more, and pretending otherwise is how predecessors failed.
