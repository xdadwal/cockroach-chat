# Research Brief — BLE Mesh Protest Messenger

> Synthesized from a 6-agent research sweep (bitchat, iOS/Android BLE limits, prior-art failures, mesh scaling theory, protest security, stack options). Preserved here because the original ran in an ephemeral workspace. This is source material for the implementation plan; numbers here are treated as hard constraints.

# Planning Brief: Decentralized BLE-Mesh Protest Messenger (iOS+Android, ~100K-user city scale)

## 1. Hard Physical & Platform Limits — Non-Negotiable Numbers

**Radio & link layer**
- BLE hop range in crowds: **10–30 m** (theoretical outdoor 55–78 m) — [bitchat WHITEPAPER](https://github.com/permissionlesstech/bitchat/blob/main/WHITEPAPER.md)
- Concurrent GATT links: **Android ~7 hard cap** (`BTA_GATTC_CONN_MAX`, status-133 beyond — [Google thread](https://support.google.com/android/thread/43071437)); **iOS ~10–12 empirical** (plan for 8). Practical node degree: **5–8 links** regardless of hundreds of phones in range.
- ATT MTU: **iOS 185 B auto-negotiated, no API** ([ble-guides](https://github.com/chrisc11/ble-guides/blob/master/ios-mtu.md)); Android up to 517 via `requestMtu()`. Min connection interval: iOS **15 ms** (often scaled to 30 — [Apple QA1931](https://developer.apple.com/library/archive/qa/qa1931/_index.html)); Android 7.5 ms.
- Realistic per-link throughput: **30–80 KB/s iOS / 50–100 KB/s Android GATT**; ~100 KB/s L2CAP-as-peripheral ([Punch Through](https://punchthrough.com/maximizing-ble-throughput-on-ios-and-android/)). Per-link bandwidth is NOT the bottleneck; shared 2.4 GHz airtime + flooding is.
- Connection setup dominates latency: **~0.5–3 s per connect + service discovery**.
- Advertising payload (interoperable): **legacy 31 B only**; iOS foreground ~28 B usable, **name + service UUIDs only — CoreBluetooth cannot transmit manufacturer/service data or extended adverts** ([Apple docs](https://developer.apple.com/documentation/corebluetooth/cbadvertisementdatamanufacturerdatakey), [forums 690000](https://developer.apple.com/forums/thread/690000)).
- Shared-medium flooding capacity: measured reliable BLE-mesh aggregate is **~1–3 kbps network-wide** (Ericsson large-scale sim, QoS 99.1% dense case — [bluetooth.com](https://www.bluetooth.com/blog/ericsson-presents-large-scale-building-automation-test-case-for-mesh-networking/)); **~5% relay nodes optimal, all-relay hurts**; a radio neighborhood sustains **~1–10 short msgs/sec** before congestion collapse ([Rondón, arXiv 1910.03345](https://arxiv.org/pdf/1910.03345)). Rebroadcast adds at most **41%** new coverage; ~0 after 4 hears (Ni et al. broadcast-storm — [paper](https://people.eecs.berkeley.edu/~culler/cs294-f03/papers/bcast-storm.pdf)).

**iOS platform**
- Backgrounded advertising: local name dropped; service UUIDs move to Apple **"overflow area"** — discoverable only by iOS scanning for that exact UUID, **and only while the scanner's screen is illuminated**; ~1 Hz ([Apple background doc](https://developer.apple.com/library/archive/documentation/NetworkingInternetWeb/Conceptual/CoreBluetooth_concepts/CoreBluetoothBackgroundProcessingForIOSApps/PerformingTasksWhileYourAppIsInTheBackground.html), [davidgyoung reverse engineering](https://github.com/davidgyoung/ios-overflow-area)). Android can only see it via a manufacturer-data bitmask filter hack ([NHS issue](https://github.com/nhsx/COVID-19-app-Android-BETA/issues/10)) or broad-scan+connect+GATT-discover.
- Background scans must specify service UUIDs; duplicates coalesced; **~10 s runtime per wakeup**. State restoration does NOT survive force-quit, BT toggle, or reboot. Pending `connect()` to known peripherals is the one durable primitive (wakes app days later).
- MultipeerConnectivity: **zero background capability** (~3 min grace), ~8 peers/session. AWDL private. **Wi-Fi Aware (iOS 26, iPhone 12+)** requires explicit per-device-pair user pairing dialog — a hard blocker for stranger meshes ([WWDC 2025](https://developer.apple.com/videos/play/wwdc2025/228/)).

**Android platform**
- Foreground service (`connectedDevice` type) effectively mandatory; Android 14/15 block FGS launch from background. OEM battery killers (Xiaomi/Huawei/Samsung) kill even FGS apps without user whitelisting.
- Scan throttles: >5 scan starts/30 s → 30 s block; unfiltered scans killed at 30 min; screen-off scanning suspended on some Android 15 OEM builds ([devsflow](https://www.devsflow.ca/blog/ble-android-lessons.html)).
- Advertise modes 100 ms–1 s; scan duty cycles 10–100%; `BluetoothLeAdvertiser` may be null on cheap devices.

**Both**
- **MAC randomization ~every 15 min**; iOS uses non-resolvable private addresses — **OS-level MAC is useless as peer ID; identity must be app-layer crypto** ([needcode.io](https://needcode.io/ble-privacy/)). Every rotation = reconnection churn.
- Battery: continuous LOW_LATENCY scanning = **10–15%/hr**; duty-cycled ~1–3%/hr ([bleadvertiser](https://bleadvertiserapp.medium.com/why-your-ble-app-is-draining-battery-and-the-scan-strategy-that-fixes-it-2a10d904febf)). Budget: ≤15% per 12 h day only with adaptive duty cycling (bitchat's 4-tier scheme).
- **Bluetooth SIG Mesh is unusable from phone apps** — advertising bearer not exposed; phones can't relay SIG-mesh PDUs ([Argenox](https://argenox.com/blog/10-reasons-why-ble-mesh-has-struggled-to-gain-traction)). All phone meshes are custom app-layer overlays over GATT.

## 2. What Works at 100K City Scale — and What Physically Cannot

**Physically CANNOT work:**
- **City-wide realtime chat via flooding.** Flood cost ≈ N_local transmissions per message; even with bitchat's log₂(degree) fanout + suppression, ~40% of nodes transmit. A dense radio cell carries ~10–50 msg-transmissions/s total → **whole-crowd origination budget <1 flooded msg/sec/cell; per-user budget of a few short messages per hour** if traffic is unscoped. Collapse is a cliff (collision→retransmit→collision), not graceful degradation.
- **Global 100K broadcast at conversational latency.** ~200–1000 overlapping radio cells → one network-wide broadcast per several seconds, best case. Gossip theory needs only ~11–12 rounds for 100K ([Karp et al. FOCS 2000](https://archive.cone.informatik.uni-freiburg.de/pubs/rumor.pdf)) but latency is governed by contact opportunities once the graph partitions.
- **Background phones as relays**, especially iPhones (overflow area + screen-on requirement). Assume screen-on, foregrounded participation for relaying.
- **Sparse-area operation.** 10–30 m hops need crowd density; dispersal kills the mesh. Precedent: Meshtastic's managed flood broke at **2,000+ nodes at DEF CON** and degrades at ~100 nodes/channel ([2bn.de](https://www.2bn.de/en/2025/11/meshtastic-optimization-how-to-actually-connect-with-your-neighbors/)) — and that's LoRa with km links.

**What DOES work:**
- **Dense-crowd local cells** (hundreds of phones/block): local zone broadcast, short text, alerts — the FireChat/bitchat protest niche. Field-proven adoption spikes: Bridgefy Myanmar **600K–1M downloads/48 h** ([SCMP](https://www.scmp.com/news/asia/southeast-asia/article/3120276/myanmar-bridgefy-saw-600000-downloads-offline-message-app)); bitchat Nepal **48K/day** ([Forbes](https://www.forbes.com/sites/digital-assets/2025/09/11/jack-dorseys-bitchat-gains-traction-during-nepals-unrest/)).
- **DTN cross-city delivery**: minutes-to-hours via store-carry-forward on human mobility (spray-and-wait couriers). Binary Spray-and-Wait is delay-optimal; copy budget independent of N ([Spyropoulos SIGCOMM'05](http://conferences.sigcomm.org/sigcomm/2005/paper-SpyPso.pdf)). Epidemic routing has >3000 tx/delivered-msg overhead — forbidden.
- **Geo-scoped channels** (geohash zones) so traffic never needs city-wide flooding; unicast along routed/courier paths; periodic set-reconciliation (GCS/Bloom) instead of re-flooding history.
- **Hybrid internet fallback** (Nostr/Tor-style) for range when internet is partially up — every survivor app (FireChat, Bridgefy, Briar, bitchat) converged on hybrid. Caveat: Global Voices found FireChat HK's celebrated "mesh" traffic was almost all internet chatrooms; verified mesh use was ~60–120 coordinators ([globalvoices.org](https://globalvoices.org/2015/01/13/fact-checking-firechat-mesh-networks-coverage-hong-kong-protests/)). Design honestly for this.
- **Design targets that follow:** messages ≤500 B compressed (1–3 ATT writes); priority classes (alerts preempt chat — missing in bitchat, a known gap); relay election (~5% of nodes) rather than all-relay; hard per-user rate limits.

## 3. Lessons From Prior Apps — What Killed Each One

| App | Fate | Kill factor |
|---|---|---|
| **FireChat** (2014–20) | Dead, silent shutdown Feb 2020 | VC-funded, zero revenue, spiky crisis usage, no retention; no encryption at launch (police monitored openly); protest use "was an accident" ([fromjason.xyz](https://www.fromjason.xyz/p/notebook/firechat-was-a-tool-for-revolution-then-it-disappeared/)) |
| **Bridgefy** (2014–) | Alive but twice broken | No auth → impersonation/MITM; plaintext routing IDs → social-graph mapping; zip-bomb killed entire mesh ([CT-RSA 2021](https://eprint.iacr.org/2021/214.pdf)); libsignal retrofit still broken via TOCTOU ~50% success ([USENIX Sec 2022](https://www.usenix.org/system/files/sec22-albrecht.pdf)). Lesson: **wrapping Signal ≠ secure; mesh breaks live in session mgmt, broadcast keys, metadata** |
| **Briar** (2018–) | Alive, niche | Gold-standard security (Cure53 audit) but Android-only (iOS background kills its model — [issue #445](https://code.briarproject.org/briar/briar/-/issues/445)), QR-only contact add = no stranger broadcast, no viral loop, ~4x battery drain |
| **Serval** (~2010–16) | Abandoned | Built on WiFi ad-hoc mode Android never officially exposed; grant-funded, no productization. **Building on unsupported OS capabilities is fatal** |
| **SSB/Manyverse** | Momentum lost | Append-only full-history gossip wrong for ephemeral traffic; lead maintainer left 2024, ~80% of manpower gone |
| **Meshtastic** | Thriving (hardware) | Proves flooding dies at low hundreds of nodes; PSK crypto weak (CVE-2025-53627 silent downgrade shown-as-encrypted); radios conspicuous at protests |
| **bitchat** (2025–) | Alive, best reference | Launched with zero security review; MITM via unbound identity keys ([Supernetworks](https://www.supernetworks.org/pages/blog/agentic-insecurity-vibes-on-bitchat)); fixed via Noise XX migration; **dual hand-synced Swift/Kotlin implementations = chronic interop bugs** ([#272](https://github.com/permissionlesstech/bitchat/issues/272), [#355](https://github.com/permissionlesstech/bitchat/issues/355), [#673](https://github.com/permissionlesstech/bitchat-android/issues/673)) |

**Cross-cutting:** (1) Adoption is event-driven and app-store-mediated — the app must exist in stores BEFORE the crisis; ship sideload/F-Droid/APK-over-mesh as store-takedown hedge (HKmap.live removed on China pressure — [SCMP](https://www.scmp.com/tech/apps-social/article/3032310/about-face-apple-removes-hong-kong-protest-map-app-following-china); Apple pulled Signal/Telegram/WhatsApp from China 2024, ~98 VPNs from Russia). (2) Availability + UX beat security in adoption (Bridgefy grew through two public breaks). (3) What HK 2019 protesters actually used most: Telegram + AirDrop stranger-broadcast ([arXiv 2105.14869](https://arxiv.org/pdf/2105.14869)) — **anonymous local stranger-broadcast is the killer primitive**. (4) Crisis usage monetizes badly — plan grant/community funding + everyday dual-use features.

## 4. Recommended Architecture Patterns

**Transport layer**
- Primary: **BLE GATT overlay, every node dual-role central+peripheral** (bitchat-proven). Legacy 31 B adverts only (iOS interop). Identity established post-connect via GATT handshake (iOS can't advertise IDs). RSSI-gated link selection, cap ~5 active links, retry logic for status-133/GATT cache staleness.
- iOS-specific: state restoration + **pending-connect() to known peers** for background reconnect; Android scans must include both service-UUID filter AND Apple-manufacturer-data bitmask filter to catch backgrounded iPhones.
- Secondary (opportunistic, not required): Wi-Fi Aware where present (Android 8+ chipset-gated; iOS 26/iPhone 12+ pairing-gated) for bulk transfer between consenting peers; internet fallback via Nostr-style relays over Tor when reachable (mutual-contact DMs + geo channels — bitchat's model works).
- Sneakernet: QR/file export, app-APK-sharing over mesh (Briar model).

**Relay/routing (three regimes, not one)**
1. **Local broadcast (same cell):** managed flood — TTL 7 clamped to 5 when degree ≥6; counter-based suppression (cancel scheduled relay on duplicate heard); relay jitter 10–220 ms; deterministic ~log₂(degree) fanout; split-horizon; LRU dedup (1000 entries/5 min) keyed on digest. Add what bitchat lacks: **priority classes (alert > control > chat), density-adaptive rebroadcast probability, per-cell airtime budget, relay election (~5% relays)**.
2. **Unicast:** source routing when a fresh bidirectional path exists (announces carry ≤10 neighbor IDs, 60 s freshness); fall back to flood on path failure.
3. **Cross-city / disconnected:** **Binary Spray-and-Wait couriers** (copy budget 4, cap 8, halve on handover; envelopes ≤16 KiB, 24 h TTL; per-depositor quotas by trust tier; 1 handover/envelope/10 min) + periodic **GCS set-reconciliation** (~15 s) for public history with hard retention caps (6 h broadcasts, 15 min fragments).

**Message format**
- bitchat-style binary: 14–16 B header (version/type/TTL/timestamp/flags/length) + 8 B sender ID + optional 8 B recipient + payload + 64 B Ed25519 sig; **signature excludes TTL byte** so relays decrement without re-signing; pad to uniform block sizes; broadcast = 0xFF recipient; TLV for announces. Fragmentation ~469 B chunks, 128 concurrent reassemblies, 30 s timeout, 1 MiB cap. **Decompression ratio caps mandatory** (Bridgefy zip-bomb). Version/capability negotiation in announces from day one — bitchat's silent-drop-on-unknown-type is the interop bug factory.

**Crypto/identity**
- Identity: long-term **Curve25519 (agreement) + Ed25519 (signing)** in hardware keystore; stable ID = SHA-256 fingerprint of pubkey; **rotating ephemeral session IDs on the wire, rotated atomically with BLE RPA (~15 min) and all payload fields, after movement** — cross-layer correlation otherwise defeats it ([NDSS 2025](https://www.ndss-symposium.org/wp-content/uploads/2025-703-paper.pdf)).
- Sessions: **Noise XX** (mutual auth, identity-hiding) handshake, `25519_ChaChaPoly_SHA256`, then **Double-Ratchet-style per-message ratchet** (fixes bitchat's session-only FS). X3DH doesn't fit (needs a prekey server); instead **distribute prekey bundles at pairing/gossip so store-and-forward envelopes get FS** (avoid Noise X's FS gap). Never silently downgrade (Meshtastic CVE-2025-53627 anti-pattern). Consider hybrid PQ (PQXDH-style) hard-required, no compat fallback ([Signal PQXDH](https://signal.org/blog/pqxdh/)).
- Verification: **QR fingerprint commitment exchange in person (Briar BQP model** — [spec](https://code.briarproject.org/briar/briar-spec/-/blob/master/protocols/BQP.md)) + vouch/introduction for web-of-trust; **local petnames, never trust wire display names** (Zooko's triangle; the Bridgefy/bitchat-Favorites impersonation hole). UI must distinguish verified/unverified/unauthenticated.
- Metadata: daily-rotating 16 B HMAC recipient tags for couriers; opaque encrypted packets to relays; uniform padding; document honestly that co-located RF traffic analysis is unbeatable.
- Forensics: no plaintext at rest ever; SQLCipher-class DB with hardware-bound key (not key-next-to-DB — the Signal Desktop mistake); disappearing messages as **cryptographic erasure**; **panic/duress wipe** of keys, courier mail, history (GrapheneOS duress-PIN pattern). FBI recovered "disappeared" Signal messages from a seized iPhone ([Forbes](https://www.forbes.com/sites/larsdaniel/2026/04/10/fbi-pulled-deleted-signal-messages-from-an-iphone-without-breaking-encryption/)).
- Sybil: unsolvable absolutely without a CA ([arXiv 1403.5871](https://arxiv.org/pdf/1403.5871)); mitigate via OOB trust tiers gating quotas (verified > unverified) + PoW on identity creation.

## 5. Stack Trade-off Matrix

| Criterion | A. Native Swift+Kotlin | **B. Rust core (UniFFI) + native BLE glue** | C. KMP | D. Flutter/RN |
|---|---|---|---|---|
| Dual-role BLE | Full, mature | Full (glue is native Swift/Kotlin; Rust `blew` too immature/AGPL) | Kable central-only; peripheral = 2-star lib or hand-rolled | Flutter `bluetooth_low_energy` only credible option; RN peripheral libs dead |
| Protocol written once | No — proven interop-bug source (bitchat #272/#355/#673) | **Yes** (framing/crypto/routing/store-fwd in Rust) | Yes | Yes |
| iOS background BLE | Best possible | **Equal to native** (in-process dylib, no VM to wake) | Near-native, unproven restoration path | **Fatal: Dart/JS suspended in background** — relay logic dies exactly when needed ([flutter #116715](https://github.com/flutter/flutter/issues/116715)) |
| Precedent | bitchat (26.1k★) | Berty (Go+native BLE drivers — [berty.tech](https://berty.tech/blog/bluetooth-low-energy/)), Element X (Matrix rust-sdk via [UniFFI](https://github.com/mozilla/uniffi-rs)) | None for BLE mesh | None |
| Crypto risk | Duplicated (2 chances to get Noise wrong) | **One audited impl** (snow/RustCrypto) | One impl, less-audited iOS path | Weak crypto ecosystems |
| Buy options | — | — | — | Bridgefy SDK (online license check breaks offline-purity; 3x broken crypto history + [7ASecurity audit](https://7asecurity.com/reports/pentest-report-bridgefy.pdf)); Ditto (enterprise-priced ~$420k ARR, closed) — both fail auditability/censorship-resistance |

**Recommendation: Option B — Rust core + UniFFI + thin Swift/Kotlin BLE drivers.** Rust owns framing, fragmentation, Noise/ratchet crypto, routing, dedup, store-and-forward, quotas; platform code owns only CoreBluetooth/Android-BLE callbacks exposed as a byte-pipe transport trait. This eliminates the dominant bug class (dual-implementation drift) while keeping native-grade background behavior. `uniffi-bindgen-react-native` future-proofs an RN UI if wanted.
**Criteria it depends on:** (1) team can staff Rust + enough Swift/Kotlin for BLE glue — if the team is pure-mobile with no Rust, fall back to A with a shared conformance test-vector suite and protocol fuzzing; (2) iOS background relaying matters (it does — rules out D beyond a foreground MVP); (3) auditability matters (rules out closed SDKs); (4) if Kotlin-centric and willing to pioneer iOS peripheral via Kotlin/Native, C is defensible but unprecedented.

## 6. Top 10 Risks, Ranked

1. **Congestion collapse at density** — flooding hits a cliff (~1–10 msg/s per radio cell; Meshtastic broke at 2K nodes at DEF CON). Mitigate: geo-scoped channels, priority classes, airtime budgets, relay election, hard rate limits — designed in from day one, not retrofitted. This is the single biggest product-viability risk at 100K.
2. **iOS background = near-dead relays** — overflow area + screen-on discovery requirement means backgrounded iPhones barely participate; mesh capacity may be a small fraction of installed base. Mitigate: pending-connect reconnection, Android-side manufacturer-data filter, UX that encourages screen-on "relay mode"; set expectations honestly.
3. **Authentication/impersonation** — the #1 real-world break (Bridgefy, bitchat MITM, Meshtastic downgrade). Mitigate: Noise XX identity binding, QR verification, petnames, no silent downgrade, external audit before protest marketing.
4. **App-store takedown in target jurisdictions** — HKmap.live, China CAC removals, Russia VPN purge; China app-filing regime bars foreign devs entirely. Mitigate: Android-first, F-Droid + reproducible builds + APK-over-mesh; iOS reach is EU-DMA/jurisdiction-dependent.
5. **Single-message DoS / resource exhaustion** — Bridgefy's zip-bomb killed the whole mesh. Mitigate: decompression caps, strict input validation, fragment/reassembly quotas, fuzzing the binary parser (bitchat's buffer overflow was in signature parsing).
6. **Metadata/social-graph surveillance & RF traffic analysis** — plaintext routing IDs mapped Bridgefy users' relationships; co-located radio adversary can always do RSSI/timing analysis. Mitigate: encrypted routing metadata, rotating tags, padding, flood-origin blur; document residual risk honestly (don't overpromise anonymity).
7. **Battery drain kills all-day participation** — 10–15%/hr continuous scanning; Briar ~4x drain reputation. Mitigate: adaptive 4-tier duty cycling keyed to screen/battery/connectivity; target ≤15%/12 h.
8. **Cross-platform protocol drift** — bitchat's chronic iOS↔Android silent-drop bugs. Mitigate: single Rust implementation + shared test vectors + capability negotiation in announces.
9. **Seized-device forensics** — cached plaintext, notification artifacts, recoverable "deleted" messages endanger users legally. Mitigate: ciphertext-only at rest, hardware-bound keys, cryptographic erasure, duress wipe.
10. **Sustainability collapse** — FireChat (VC), Serval (grants), SSB (maintainer loss) all died of funding/maintenance, not tech. Mitigate: open protocol + community governance, grant funding (OTF model), everyday dual-use features (festivals, disasters) for between-crisis retention.
