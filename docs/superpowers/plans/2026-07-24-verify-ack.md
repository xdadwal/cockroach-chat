# Verify-Ack + Handshake Reliability Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Handshakes self-heal from packet loss and glare; verification produces a visible, honest two-sided acknowledgement; users can Unverify or Remove a contact.

**Architecture:** Two node.rs protocol fixes (responder retry coverage, fingerprint tie-breaker); DM plaintext gains a 1-byte kind (text/verify-notice/verify-ack) with control messages persisted as DMs so "confirmed" derives from history (no schema migration); new Store delete methods power Unverify/Remove; Android renders control kinds as system lines, adds badge states and long-press actions.

**Tech Stack:** Rust core (meshcore, meshcore-store, meshcore-ffi/uniffi), Kotlin/Compose Android.

**Spec:** `docs/superpowers/specs/2026-07-24-verify-ack-design.md`
**Branch:** `feat/verify-ack` (stacked on `feat/per-type-ttl`)

## Global Constraints
- Repro tests `scan_session_recovers_when_final_handshake_message_is_lost` and `simultaneous_mutual_scan_eventually_converges` already exist uncommitted in node.rs; they are committed WITH their fixes (Tasks 1-2), never alone.
- DM kinds: `0` text, `1` verify-notice, `2` verify-ack. Legacy heuristic (FFI + Android restore only): `body[0] <= 2` → framed, else legacy text.
- Every new Store method: MemoryStore + SqliteStore + FfiStore delegate. Every new/changed event: MeshEvent + to_ffi + FfiEvent + BleController `when` branch.
- Commit per task from repo root; end messages with `Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>`.

---

### Task 1: Responder stall recovery (Fix 1)
**Files:** Modify `crates/meshcore/src/node.rs` (`handle_noise_handshake`, `advance_handshake`).
- [ ] Run `cargo test -p meshcore scan_session_recovers` — FAILS (repro).
- [ ] In `handle_noise_handshake`, after creating the responder session: `self.handshake_last_ms.insert(fp, self.clock.now_ms()); self.handshake_attempts.entry(fp).or_insert(0);`. After every successful `read_handshake`: refresh `handshake_last_ms`.
- [ ] Test green; full suite green (watch `dm_retries_until_recipient_learns_sender`, `dm_recovers_after_one_side_restarts_midsession`).
- [ ] Commit: "Responders retry stalled handshakes so a lost msg3 can't wedge them".

### Task 2: Glare tie-breaker (Fix 2)
**Files:** Modify `crates/meshcore/src/node.rs` (`handle_noise_handshake`).
- [ ] Run `cargo test -p meshcore simultaneous_mutual_scan` — FAILS (repro).
- [ ] Before the responder-creation block: if a session exists, `is_handshaking()`, and `initiator`: `if self.fingerprint() < fp { return; }` else remove the session (keep attempts/last so retries stay budgeted) and fall through to create a responder.
- [ ] Add mirror test `simultaneous_mutual_scan_converges_with_reversed_fingerprints` using seeds picked so the winner is on the other side (swap which node is `a`); both orderings green.
- [ ] Full suite green. Commit: "Fingerprint tie-breaker resolves simultaneous handshake initiation".

### Task 3: DM kind framing + verify-notice/ack in core
**Files:** Modify `crates/meshcore/src/node.rs`.
- [ ] Constants `DM_KIND_TEXT/VERIFY_NOTICE/VERIFY_ACK: u8`. `MeshEvent::DmReceived` gains `kind: u8`.
- [ ] Outbound: `send_dm` frames plaintext as `[0] ++ text` (pending_dms/store-and-forward keep raw text; framing happens at `encrypt_and_send_dm(fp, kind, body)`); new `send_control_dm(fp, kind)` used by the notice/ack paths; control DMs persisted (`StoredDm.body = [kind]`).
- [ ] Inbound `decrypt_and_emit`: split kind; persist framed body for kinds 1/2 and plain text for kind 0 (keep stored text bodies UNframed so existing tests/history stay valid — only control rows carry the prefix); emit `DmReceived { kind, text }`; on kind 1 auto-send verify-ack; kind 2 never replies.
- [ ] Notice trigger: `verify_peer` sets in-memory `notice_pending: HashSet<Fingerprint>`; if session ready send immediately; `finish_session` (verified branch) sends+clears if flagged.
- [ ] Tests: notice/ack round-trip (A verifies B → B gets kind 1 → A gets kind 2, both persisted); ack-no-loop (exactly one kind-2 in each direction's history); text DMs still round-trip.
- [ ] Full suite green. Commit.

### Task 4: Unverify / Forget in core + store
**Files:** Modify `crates/meshcore/src/{node.rs,store.rs}`, `crates/meshcore-store/src/lib.rs`, `crates/meshcore-ffi/src/lib.rs` (FfiStore delegate only).
- [ ] `Store` trait: `fn delete_peer(&mut self, fp: &Fingerprint);` `fn delete_dms(&mut self, fp: &Fingerprint, only_control: bool);` (control = body.len()>=1 && body[0]==1||2 && framed rows only). Implement in MemoryStore + SqliteStore (`DELETE FROM dms WHERE peer=? [AND control predicate via fetched rows]`) + FfiStore delegate.
- [ ] Node: `unverify_peer(fp)` — persist verified=false, drop `noise_sessions/pending_dms/pending_inbound_dms/handshake_*/resync_pending/notice_pending` for fp, `delete_dms(fp, true)`. `forget_peer(fp)` — same plus `delete_dms(fp, false)` + `delete_peer(fp)` + drop `fp_to_eph` entry.
- [ ] Tests: unverify keeps texts+petname, clears control rows + verified; forget leaves no trace; sqlite store unit tests for both deletes.
- [ ] Full suite green. Commit.

### Task 5: FFI surface + bindings
**Files:** Modify `crates/meshcore-ffi/src/lib.rs`; regenerate bindings via `scripts/build-android-lib.sh` (Task 7 does the full build; here `cargo build -p meshcore-ffi` + bindgen only).
- [ ] `FfiEvent::DirectMessage` gains `kind: u8`; `FfiDmMessage` gains `kind: u8` with legacy heuristic applied in the `dm_history` mapper (strip prefix when `body[0] <= 2`, else kind=0, body unchanged).
- [ ] Export `unverify_peer(peer_fingerprint: String)`, `forget_peer(peer_fingerprint: String)`.
- [ ] `cargo test --workspace` green. Commit.

### Task 6: Android UI
**Files:** Modify `android/.../BleController.kt`, `ui/App.kt`, `ui/Components.kt`, `MeshController.kt` (ChatMessage), strings source (`Strings`/`s.` object — locate at implementation time), regenerated `meshcore_ffi.kt` from Task 5.
- [ ] `ChatMessage` gains `kind: Int = 0`; `Peer` gains `confirmed: Boolean = false`.
- [ ] `tickOnce` `DirectMessage` branch: kind 1 → system line + mark nothing (core acked); kind 2 → system line + `confirmed=true`; kind 0 → today's behavior. `DmSession` branch unchanged. Restore path (`restoreContacts`): derive `confirmed` from any inbound kind 1/2 in `dmHistory`.
- [ ] `MessageBubble`: kinds 1/2 render as a centered muted system line (new small composable, reuse theme colors), no bubble.
- [ ] Badge: `ShieldBadge` gains a `confirmed` visual variant (filled + double-ring or check+dot); `PeerListRow` + `DmScreen` header pass `confirmed`; DM subtitle "awaiting confirmation — re-scan if this persists" when verified && !confirmed.
- [ ] Long-press (`combinedClickable`) on `PeerListRow` → `AlertDialog`: Unverify / Remove / Cancel; Remove gets a second confirm dialog; actions call new controller fns `unverify(fp)` / `forget(fp)` (call FFI + update local state; forget also removes thread + navigates back if open).
- [ ] Build: `JAVA_HOME="/Applications/Android Studio.app/Contents/jbr/Contents/Home" ./gradlew assembleDebug` green. Commit.

### Task 7: End-to-end verify + PR
- [ ] `ABIS=arm64-v8a ./scripts/build-android-lib.sh` + `./gradlew assembleDebug`; `cargo test --workspace`, fmt, clippy all clean.
- [ ] Install on connected phones (adb, same flow as earlier today).
- [ ] PR stacked on `feat/per-type-ttl` describing fixes + feature + compat note.
