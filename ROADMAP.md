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

## If you take this into the field

This is built to be used — in a protest, a blackout, a disaster — and we'd rather it be in your
hands than withheld until some future perfect version. But use it with your eyes open.

**Cockroach Chat is under active development and may not work as expected.** It is most likely to
disappoint you exactly where you need it most: dense crowds, heavy radio interference or jamming,
low battery, unfamiliar Android builds, or anything else we could not test. Delivery is
best-effort — a message that appears sent may never arrive, and the app cannot always tell the
difference. **Always have a fallback that does not depend on this app.**

Two limits a reliability warning doesn't cover, and no disclaimer removes:

- **It cannot hide that you are transmitting.** Encryption protects what you say, not the fact that
  a radio near you is speaking. An adversary with a receiver can detect transmissions, and with
  sustained coverage can correlate timing and signal strength to track a device. **This app does
  not make you anonymous, and cannot.**
- **The cryptography is unaudited.** The libraries are vetted and the design follows established
  practice, but no independent expert has reviewed how we assembled them.

Read [`docs/threat-model.md`](docs/threat-model.md) before deciding this is right for your
situation. It is specific about what is defended and what is not.

## What we still owe

- **External security audit.** The single biggest gap. Committed, unscheduled, and we'll say so
  loudly when it happens — and equally loudly if it finds something.
- **Sustained fuzzing.** Targets exist for the three parsers, but CI only runs them 60 s each on
  PRs — that's regression detection, not a search for new bugs. Nothing runs them for long, and no
  corpus is kept between runs. M6's bar is ≥8 h clean per target; the Noise handshake and store
  have no targets at all.
- **Reproducible builds**, so a release binary can be verified against source rather than trusted.

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
