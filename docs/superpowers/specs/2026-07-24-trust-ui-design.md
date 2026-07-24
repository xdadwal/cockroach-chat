# Trust UI: one meaning for the shield

**Date:** 2026-07-24
**Status:** approved (list reviewed and accepted in session)

## Problem

One badge currently does four jobs: signature validity, identity continuity, in-person
verification, and both-ends confirmation — shown per message in channels and DMs, which
is noise that trains users to ignore badges.

## Principles (approved)

1. The shield means a human trust decision (scan / ack) — never mere signature validity.
2. Badge the exception, not the norm: per-message signals only for failure, never success.
3. Trust belongs to an identity, shown once per contact/conversation, not per bubble.
4. Trust state appears only where the user can act on it.
5. Unverified is neutral (quoted names, dashed avatar), not an alarm.
6. Downgrades are loud: a known contact whose announced key changes gets a visible warning.

## Decisions

### Core
- `handle_channel`: a message whose sender key is KNOWN and whose signature is INVALID is
  **dropped** (forgery/collision), not delivered-with-flag. `verified` on
  `MessageReceived` now means "cryptographically attributed" vs "sender not yet known" —
  a neutral distinction, never a warning.
- Key-change detection: in `handle_announce`, if a peer's announced X25519 key differs
  from the stored `peer_static_dh` for the same fingerprint, emit
  `MeshEvent::PeerKeyChanged { peer_fp }` and persist a local marker row
  (`StoredDm { body: [3], mine: false }`, never transmitted). DM kind 3 =
  KEY_CHANGED marker. `is_control_dm` covers kinds 1..=3 (unverify clears markers too);
  sqlite predicate updated to include x'03'.

### Android
- Channel/announce bubbles: remove shield + VERIFIED/UNVERIFIED chips. Attributed sender
  -> bold plain name; unattributed -> quoted "name" with the existing subtle gray
  "can't confirm sender" footer. Tiny shield only when the sender resolves to a scanned
  contact (`ChatMessage.fromContact`, resolved eph->fp->peer at event/restore time).
  Keep the solid-vs-dashed left bar as the quiet texture for attributed/unattributed.
- DM bubbles: no name row, no badges, no "you · signed" — body + timestamp only
  (`MessageBubble(inDm = true)`); trust lives in the header.
- DM header: unchanged 3-state display, now TAPPABLE — verified peer -> contact options
  dialog (existing PeerActionsDialog); unverified -> QR scan flow.
- Key-changed: kind-3 rows render as a red/amber system line "identity changed —
  re-verify"; a contact with an unresolved kind-3 marker shows the same warning as the
  row subtitle and the DM header subtitle (overrides other states). Re-verifying
  (verify_peer after a fresh scan) clears markers via the unverify-style control purge
  on scan — concretely: `verify()` first calls `delete_dms(fp, only_control=true)`
  semantics through a new FFI-exposed `clear_trust_markers(fp)`? NO — keep it simple:
  `verify_peer` in core deletes kind-3 rows for that peer (one store call) so a fresh
  scan resets the warning. Unverify already clears them via is_control_dm.

## Out of scope
- Distinguishing "no key" vs "invalid" historically (old stored rows keep their flag).
- Any new persisted peer columns (all derived from DM rows, as before).

## Testing
- Core: forged channel message (bad sig, known key) is dropped; unknown-sender message
  still delivers unattributed. Key change emits event + persists marker; verify_peer
  clears markers; unverify clears them too (is_control_dm 1..=3).
- Android: assembleDebug; manual on-fleet pass.
