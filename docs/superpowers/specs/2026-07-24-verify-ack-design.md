# Reliable handshakes + visible verification ack + peer reset actions

**Date:** 2026-07-24
**Status:** approved (design discussed and accepted in session)

## Problems

1. **Lost msg3 wedges the responder forever** (reproduced:
   `scan_session_recovers_when_final_handshake_message_is_lost`). Responders get no
   `handshake_attempts` entry, so `retry_stalled_handshakes` never drives them; the #32
   recovery requires holding no session. The initiator flips "verified" on *writing* msg3,
   so one side shows verified while the other never completes.
2. **Simultaneous initiation (glare) has no tie-breaker** (reproduced:
   `simultaneous_mutual_scan_eventually_converges`). Crossed msg1s destroy both sessions;
   both retry on the same fixed 2 s cadence.
3. **No acknowledgement**: after a QR scan there is no proof on the scanner's phone that
   the peer's side completed; no way to reset a bad verification state.

## Decisions

### Fix 1 — responder stall recovery
When a responder session is created in `handle_noise_handshake`, record
`handshake_last_ms` and a `handshake_attempts` entry (0), and refresh `handshake_last_ms`
on every successful handshake read. The existing `retry_stalled_handshakes` then covers
responders: a session stalled >= `DM_HANDSHAKE_RETRY_MS` is dropped and re-initiated
(as initiator). The peer's stale "ready" session is superseded by the existing
`resync_pending` path (threshold 2). Heal time for a lost msg3: roughly 6 s.

### Fix 2 — glare tie-breaker
In `handle_noise_handshake`, when a handshake packet arrives while we hold an in-flight
session that `is_handshaking()` **and** `initiator == true`:
- if `self.fingerprint() < peer_fp` (lexicographic byte order): we win — ignore the
  packet, keep initiating; the peer will yield when our msg1 arrives.
- else: yield — drop our initiator session, create a responder, process their msg1.
Responder-role in-flight sessions keep today's behavior (read; on error drop, recovery
via Fix 1).

### Feature — DM plaintext kind framing + verification messages
DM plaintext becomes `kind(1 byte) || body`:
- `0` text (body = UTF-8 message)
- `1` verify-notice ("I verified you in person"), empty body
- `2` verify-ack (auto-reply to a notice), empty body

Sender path: `verify_peer` marks the peer; when the session is (or becomes) ready, the
node auto-sends one verify-notice (in-memory pending flag; acceptable to lose on process
restart — UI offers re-scan). Receiver path: on decrypting a verify-notice, persist it,
emit it, and auto-send a verify-ack (acks never trigger replies — no loops). Control
messages are persisted as `StoredDm` with the kind prefix in `body`, so "confirmed both
ends" is derivable from history with **no store schema change**: a contact is
*confirmed* iff `verified` and the thread contains an inbound kind-1 or kind-2 message.
`MeshEvent::DmReceived` gains `kind: u8`. Legacy DB rows (no prefix) are treated as text
via the FFI mapper heuristic: `body[0] <= 2` means framed, else legacy text (UTF-8 text
never begins with control bytes 0x00-0x02).

Compat: DM plaintext framing changes — all devices update together (same caveat as the
nonce fix; channels unaffected). `PROTOCOL_VERSION` unchanged.

### Feature — Unverify / Remove
Two new node + FFI operations, surfaced in Android via long-press on a contact row
(Compose `AlertDialog`, destructive action double-confirmed):
- `unverify_peer(fp)`: verified=false (persisted), drop session/pending/attempts/resync
  state, delete stored control messages for the peer (so "confirmed" derivation resets);
  keep text history and petname. UI prompts re-scan.
- `forget_peer(fp)`: everything above plus delete the peer record and full DM thread.
New `Store` methods (implemented in MemoryStore, SqliteStore, FfiStore delegate):
`delete_peer(&mut self, fp)`, `delete_dms(&mut self, fp, only_control: bool)`.

### Android UI
- `ChatMessage` gains `kind`; kinds 1/2 render as centered system lines
  ("🔒 <name> verified you" / "✅ <name> acknowledged your verification").
- `Peer` gains `confirmed`; badge states: unverified → verified-by-you (existing shield)
  → confirmed-both-ends (filled shield + check variant). DM header subtitle shows
  "awaiting confirmation…" when verified && !confirmed, with a "re-scan" affordance.
- `BleController` handles the new event kinds; restore path derives `confirmed` from
  `dm_history` kinds.

## Testing
- Existing repro tests flip to green (they are the regression tests for fixes 1-2).
- Core: notice/ack round-trip marks both ends confirmed-derivable; ack does not trigger
  a reply; unverify clears control history but keeps texts; forget deletes thread+peer;
  glare with both orderings of fingerprints; responder stall with msg2 loss too.
- Full workspace suite, fmt, clippy; on-device smoke test on the 3-phone fleet.
