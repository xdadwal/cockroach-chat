# PROGRESS — Cockroach Chat build ledger

This is the **single source of truth for "what's done / what's next."** The Ralph loop
(and any agent) reads this file first, does the **next unchecked task**, then updates it.

- Do the **smallest next unchecked `[ ]` task**, not a whole milestone.
- When a task is genuinely done **and verified** (tests/build pass — paste the command you
  ran), check it `[x]` and add a one-line note under it.
- If blocked, mark the task `[!]`, write the blocker under **Blockers**, and move to the
  next independent task.
- Keep this file honest. A checked box means verified, not "written."

**Status:** M0 core is **complete and green** — `cargo test --workspace` passes (49 core unit
tests + golden vectors + 3 topology + 6 mesh scenarios), `clippy -D warnings` clean, and the
200-node broadcast sim delivers to 97.5% of nodes. Remaining M0 tail (SQLCipher, UniFFI, JVM
smoke) is toolchain-gated and deferred into M1 where it is actually testable — see notes.
**Current milestone:** M0 done except gated tail → next up is **M1 (Android BLE glue)**.

Verify everything: `cargo test --workspace && cargo run -p sim -- --nodes 200 --scenario broadcast`

---

## Active sequence (ralph loop)

Do these in order; test-first, verify, commit + push each before moving on. Core work is
simulator-verifiable; Android UI can only be compiled (no device for runtime test in the loop) —
mark such items and note it.

- [x] **1. Link dedup** — DONE. Core keeps one link per peer identity: a link is tagged from the
  TTL=1 link-local `SyncRequest` (its sender is the *direct* neighbour; relayed packets carry the
  originator's id, so tagging from general traffic wrongly merged distinct peers — fixed), then
  redundant links to the same fingerprint are closed via `Transport::close`. Android tears down the
  GATT connection + a 30 s reconnect cooldown (anti-thrash). Sim: `redundant_links_collapse_to_one`
  (5 links → 1, delivery still works); multi-hop/broadcast/partition unaffected. `cargo test
  --workspace` green (54 unit + 9 scenarios), APK builds. Addresses PERFORMANCE.md #1.
- [x] **2. Store-and-forward** — DONE. `send_dm` to a peer we can't currently reach (no
  `fp_to_eph` entry) holds the message as an encrypted envelope (`queue_envelope`) instead of
  dropping it; `handle_announce` drains held envelopes (`take_envelopes`) and delivers them when the
  peer reappears. Core-only (the FFI `send_dm` routes through it transparently); no rebuild. Sim:
  `store_and_forward_delivers_when_peer_returns` (held while offline, delivered on reconnect). Note:
  the DM UI still only exposes peers already discovered — DM-by-scanned-fingerprint (even offline)
  ties into item 3 (QR).
- [x] **3. QR verification + petnames** — DONE. Core: `verify_peer` / `set_petname` /
  `peer_verified` / `peer_petname`, persisted in the encrypted store; `handle_announce` now
  preserves verification + petname (never silently downgrades a face-to-face-verified peer). FFI
  exposes those + `my_fingerprint`. Android: a Verify dialog shows my fingerprint as a QR (ZXing)
  and scans the peer's QR (`ScanContract`) — on a match it calls `verify_peer`; a petname field and
  a verified badge in the DM view. Unit-tested core (`verify_and_petname_persist_and_survive_announce`);
  APK builds with the camera QR flow. Camera scan itself needs on-device validation (compile-only in
  the loop).

Output `<promise>SEQUENCE-DONE</promise>` only when all three are checked and `cargo test
--workspace` is green.

---

## M0 — Rust core + simulator

### Scaffolding
- [x] `docs/protocol.md` v0.1 (wire format, message types, all tunable numbers).
- [x] `docs/decisions/ADR-0001-rust-core-uniffi.md`.
- [x] Cargo workspace: root `Cargo.toml`, `rust-toolchain.toml`, `crates/{meshcore,sim}`. `cargo build --workspace` ok. *(meshcore-ffi crate deferred to M1 — see gated tail.)*
- [x] `.github/workflows/ci.yml` rust job: fmt + clippy `-D warnings` + test + vectors + sim smoke.

### meshcore — protocol core (pure, sans-IO)
- [x] `wire`: Header/Packet/MsgType/Tlv codec; unknown version rejected, unknown type preserved+relayed. 10 unit tests incl. fuzz-style short-buffer/oversize/trailing.
- [x] Golden vectors: `testvectors/packets.json` generated (`REGEN_VECTORS=1`) and verified by `tests/vectors.rs`; CI diffs against fresh bytes.
- [x] `frag`: fragmenter/reassembler sized to 182 B; 128 slots / 30 s / 1 MiB caps; out-of-order + duplicate + timeout tests.
- [x] `identity`: Ed25519+X25519, deterministic-from-seed, rotating eph id, hashcash `pow::mint`/`verify`.
- [x] `clock` (injected) + `config` (`Tunables` holds every protocol number, incl. relay probabilities).
- [x] `relay`: SeenCache (dedup+hear count), RateLimiter (token bucket + greylist), RelayScheduler (jitter + suppression), density-adaptive probability. All unit-tested.
- [x] `store`: `Store` trait + `MemoryStore` (history caps, envelopes, panic_wipe). Stores original packet bytes so sync preserves digests.
- [x] `channels`: name normalization + `SyncFilter` set-reconciliation.
- [x] `transport`: `Transport` trait + `TransportEvent`.
- [x] `node`: `MeshNode` orchestrator — announce, channel send/receive, sync, relay, tick; emits `MeshEvent`; sans-IO.

### sim — deterministic simulator
- [x] `SimTransport` + `SimClock` + `World` radio model (per-edge latency/loss/mtu). **Determinism** secured by using `BTreeMap`/`BTreeSet` in the core (HashMap iteration order was breaking reproducibility). Airtime-budget-per-cell is a documented follow-up (see Deviations).
- [x] Topology generators (`geometric` w/ forced connectivity via union-find, `line`, `clique`) + CLI (`cargo run -p sim -- --nodes N --scenario X`).
- [x] `two_nodes_hello` — green.
- [x] `broadcast_200_nodes` — 97.5% delivery, 0.69 rebroadcasts/node (well below a naive flood). *(≤0.5N remains a tuning target — see Deviations.)*
- [x] `partition_heal` — converges ≤ heal window, zero duplicate UI events.
- [x] `duplicate_storm` — 8 nodes echoing collapse to ≤20 transmissions.
- [x] `malicious_flooder` — honest message survives; flooder rate-limited (<50% of spam propagates).
- [x] `multi_hop_line_relay` — message survives 6 hops with the ends out of range.
- [x] `fragment_loss_timeout` — covered by `frag` unit test (`expired_partial_is_evicted`).
- [x] `zipbomb_rejected` — covered by `compress` unit tests (absolute-cap defense).
- [x] `ttl_clamp_dense` — covered by `config` unit test (`origin_ttl`).

### SQLCipher + FFI (M0 tail — toolchain-gated, deferred to M1)
- [x] `SqliteStore` (crate `meshcore-store`) via rusqlite + `bundled-sqlcipher-vendored-openssl` — **done & verified**. Ciphertext-at-rest; the vendored-OpenSSL build cross-compiles to all 3 Android ABIs. Wired into the FFI (`new_persistent(db_path, db_key)` + `channel_history`) with an Android Keystore **KeyVault** (hardware-wrapped DB key + identity seed). Verified on-device: a message + identity survive a full app force-stop/relaunch, and `strings mesh.db` finds no plaintext. Panic wipe = clear rows + destroy the keystore key (unrecoverable ciphertext). 6 store unit tests incl. persists-across-reopen and wrong-key-cannot-read.
- [!] `meshcore-ffi` UniFFI crate (`MeshNode` object + `BleTransport`/`KeyVault`/`EventListener` callbacks). **Gated/deferred to M1:** the sans-IO core is FFI-ready by construction; the FFI surface is most useful to build against the real Gradle/NDK integration.
- [!] **JVM smoke test.** **Gated:** no JRE/Kotlin toolchain in this environment. This is the concrete M1 de-risking step — do it first thing in M1.

**M0 done when:** core green ✅. Gated tail rolls into M1.

---

## M1 — Android BLE glue: two phones chat offline
The **toolchain and app are done and verified on the emulator** (the plumbing that the plan
called the "M1 choke point"). Only the literal BLE radio remains hardware-gated (emulators have
no Bluetooth).
- [x] `meshcore-ffi` UniFFI crate: concrete `FfiMeshNode` + `BleTransport`/`FfiEvent`. Compiles; Kotlin bindings generate.
- [x] Cross-compile to `arm64-v8a` via NDK 28 (`scripts/build-android-lib.sh`, env-var linker — no cargo-ndk needed).
- [x] Android Gradle app (AGP 8.5, Kotlin 1.9.24, Compose): builds a debug APK bundling the `.so` + JNA + bindings.
- [x] **Runs on the emulator**: two real `MeshNode`s exchange signed announces (peer discovery), and a channel message crosses from Ava → Ben **signature-verified** — the entire core runs on-device through UniFFI. (Loopback transport stands in for BLE.)
- [x] **Real BLE stack validated on hardware (Galaxy S23, Android 16)**: tapping "Start BLE mesh" (with runtime `BLUETOOTH_SCAN/CONNECT/ADVERTISE` + `POST_NOTIFICATIONS`) brings up the GATT server, advertising, and scanning with no crash — and a nearby central even connected to our advertised service (peripheral LINK UP). `BleMeshTransport` = one service UUID, RX write / TX notify, filtered scan + RSSI gate, status-133 close+retry, ≤5 links; `MeshForegroundService` = `connectedDevice`. The app has a "Real BLE" mode (permission flow + live status log) alongside the loopback demo.
- [x] **Two-phone chat over BLE — ACHIEVED (Galaxy S23 ↔ OnePlus CPH2613).** The phones discover each other, connect (dual central+peripheral), and exchange a channel message over Bluetooth with **no internet/servers**, delivered **✓ signature-verified** (Ed25519 against the key from the identity announce). Reconnect-after-restart also observed. This is the core M1 milestone.
  - Fix landed during testing: messages first arrived "unverified" because the link-up announce was dropped during GATT setup; now the app re-announces every ~3s so keys always propagate. (commit 80a6cd0)
  - Remaining polish (not blocking): full **bidirectional** verify needs both phones on the post-fix APK; **dedupe redundant links** to one peer (both sides advertise+scan+connect → ~5 links); a **GATT op queue** so the link-up announce write doesn't race the CCCD write; airplane-mode run.

- [x] **Always-on relay (foreground-service migration) — DONE & verified on S23.** The mesh
  (node + `BleMeshTransport` + ticker) now lives in a **process-lifetime `BleController` singleton**
  (`BleController.get`) with its own main-thread scope, kept alive by `MeshForegroundService`
  (`connectedDevice`, ongoing "Mesh active" notification + Stop action). The Activity binds by
  observing the same singleton's Compose state instead of owning the node in `lifecycleScope`, so
  the relay **survives backgrounding and screen-lock**. Verified on-device: FGS `isForeground=true
  types=0x10`; still scanning after HOME (17 scan hits/6 s backgrounded) and screen-off (7 hits).
- [x] **Battery duty-cycling (v1) — DONE.** `BleMeshTransport` registers a screen on/off receiver:
  screen-on → `SCAN_MODE_LOW_LATENCY` + `ADVERTISE_MODE_BALANCED`; screen-off → `SCAN_MODE_LOW_POWER`
  + `ADVERTISE_MODE_LOW_POWER` (keeps relaying in-pocket without draining). Both transitions logged
  and verified on S23. (Full 4-tier scheme incl. battery-level + link-count remains an M6 target.)
- [x] **FLAG_SECURE — DONE.** `MainActivity` sets `FLAG_SECURE` (blocks screenshots + hides the
  recents thumbnail). Verified: app window `fl=0x81812100` has the `0x2000` secure bit set.
- [x] **Panic-wipe button — DONE.** "Wipe" button in the live header → confirm dialog →
  `MeshForegroundService.panic` → `BleController.panicWipe` (`node.panic_wipe` clears the encrypted
  DB rows, then `KeyVault.wipe` destroys the hardware-wrapped key + deletes DB files = unrecoverable
  ciphertext) → service tears down. Path compiles + wired; not destructively run on-device (would
  erase the test identity). Duress-wipe / notification scrubbing remain M6.

- [x] **UI/UX redesign (design-system implementation) — DONE & verified on emulator.** Rebuilt the
  whole Android surface from the "Cockroach Chat" Claude Design project (imported via the
  `claude_design` MCP): warm near-black palette + paper-cream ink, Archivo (voice) / JetBrains Mono
  (fact) variable fonts bundled in `res/font`, and the full trust-badge language (signed/unverified
  per message; verified/not-met per peer; public-square vs E2E banners; no-false-confidence rule).
  New IA: bottom nav `FEED · PEOPLE · MAP · ME` with a three-tab feed `Announce / Nearby / Verified`.
  Screens: onboarding name, mesh-off/start, live shell, channel view, people, encrypted DM,
  in-person verification (QR + deterministic 4-word safety number + petname + mismatch), identity +
  hold-to-wipe panic, mesh status, offline-map placeholder (parked). Files: `ui/Theme.kt`,
  `ui/Components.kt`, `ui/App.kt`; `BleController` extended with the Announce (1/min throttle +
  cooldown), Nearby channels (`joinChannel`/channel-tagged events), persisted display name, and
  safety-number model. **No native rebuild** — all mapped onto the existing core (`FfiEvent.Message`
  already carries `channel`). Loopback demo dropped from the shipping UI. Verified on emulator: every
  screen renders (fonts + native lib load, no crashes), onboarding→start→live works, Announce send
  produced a signed bubble + the C3 cooldown card. Design source: claude.ai design project
  `958b9a4d-…`. Not yet installed on the physical phones (both were disconnected during the build).

Remaining M1 (SQLCipher store, iOS overflow-area filter) unchanged. See `docs/IMPLEMENTATION_PLAN.md` §M1.

## M2 — Relay live (multi-hop, store-and-forward) — *hardware-gated*  · see §M2
## M3 — Noise XX encrypted DMs
Core **built and simulator-verified**; UI + QR verification remain.
- [x] `noise.rs`: Noise XX session (snow) — handshake, transport encrypt/decrypt, identity binding via `remote_static()`. 5 unit tests (bidirectional messages, third-party-can't-decrypt, tampered-ciphertext-rejected, identity binding).
- [x] DM integration in `node.rs`: `send_dm(fingerprint)`, handshake auto-init + drive, `DirectMessage`/`NoiseHandshake` handling, announce carries X25519 key, **MITM-binding** (Noise remote static must equal announced X25519 key), inbound-DM buffering (handles a DM overtaking the final handshake message over a relay). Events: `DmReceived`, `DmSession`.
- [x] Simulator scenario `encrypted_dm_relays_and_eavesdropper_is_blind`: A→B DM relays through a middle node that forwards but **cannot decrypt**; delivered + verified.
- [x] FFI: `send_dm(peer_fp_hex, text)` + `FfiEvent::DirectMessage`/`DmSession`.
- [ ] DM UI on phones (tap a peer → encrypted DM thread); demoable on the two-phone rig.
- [ ] QR in-person verification + petnames; verified/unverified/unauthenticated UI states.
- [ ] Per-message ratchet with a skipped-key window (current: snow transport — forward-secure per session, but multiple DMs must arrive in order; out-of-order needs the ratchet). Persist sessions encrypted.

See §M3 in `docs/IMPLEMENTATION_PLAN.md`.
## M4 — Media (voice notes + images, offer-and-fetch) · see §M4
## M5 — iOS shell · see §M5
## M6 — Hardening · see §M6

---

## Blockers
_(none — M0 core complete; M1 requires Android toolchain + physical phones)_

## Decisions / deviations log
- **BTreeMap/BTreeSet in core over HashMap** — HashMap's randomized iteration order made the
  simulator non-reproducible (same seed, different delivery). The maps whose iteration affects
  behavior (links, seen cache, relay scheduler, reassembler, subscriptions) now use ordered
  collections. Lookup-only maps (eph_keys, rate buckets) stay HashMap.
- **Relay probability tiers 1.0/1.0/0.85, not the documented 1.0/0.7/0.45** — simulator finding:
  at the ~8-link BLE connection cap, aggressive probabilistic thinning under-covers because a
  low-degree cut vertex can drop the only path to a sub-crowd. Counter-based suppression does the
  thinning instead. `docs/protocol.md` notes this; final values are a tuning target.
- **≤0.5N rebroadcast target not yet met** — currently 0.69 rebroadcasts/node at 97.5% delivery.
  Hitting ≤0.5N *and* ≥95% delivery simultaneously is open tuning (candidate: lower suppression
  threshold with per-neighbour-distinct counting). Tracked, not blocking.
- **Per-radio-cell airtime budget** — the sim models per-edge latency/loss but not a shared
  ~10-frame/s airtime cap per cell yet. Adequate for the current scenarios (rate limiting is
  enforced at ingress); add before trusting congestion-collapse numbers.
- **meshcore-ffi / SQLCipher / JVM smoke deferred to M1** — see gated tail above.
