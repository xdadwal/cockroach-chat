# Per-message-type TTL

**Date:** 2026-07-24
**Status:** approved

## Problem

Every packet type originates with the same TTL (`ttl_default = 7`, clamped to 5 in dense
crowds). But the types have different reach requirements: announcements define how far
presence is visible (largest area), channel chat serves a local crowd, and DMs need to
reach known contacts. One knob can't express that.

## Decision

Replace the single `ttl_default` / `ttl_dense_clamp` pair in `Tunables` with per-type
values, applied at origination:

| Type                 | Normal | Dense crowd (degree >= `dense_degree`, still 6) |
|----------------------|--------|--------------------------------------------------|
| Announce             | 15     | 12                                                |
| ChannelMessage       | 10     | 7                                                 |
| DirectMessage        | 10     | 7                                                 |
| NoiseHandshake       | 10     | 7 (must always equal DM — DMs can't exist without a completed handshake) |
| SyncRequest          | 1 (hardcoded, link-local, unchanged) | — |

`Tunables::origin_ttl(degree)` becomes `origin_ttl(msg_type, degree)`. Explicit per-type
clamp values, not a scaling formula — easier to read and tune.

Known accepted trade-off: DM TTL (10) < announce TTL (15), so peers 11–15 hops out are
visible in the contact list but not DM-reachable. Accepted by the user in design
discussion (store-and-forward still queues for peers with no known location, but a peer
whose announce arrived appears reachable and the DM flood simply dies out).

## Scope of change

- `config.rs`: new per-type TTL fields + `origin_ttl(msg_type, degree)`; drop
  `ttl_default` / `ttl_dense_clamp`.
- `node.rs`: pass the message type at the three origination sites — `announce()`, the
  channel send path, and `send_directed()` (covers DM + handshake). Relay path untouched
  (pure decrement). SyncRequest untouched.
- Wire format: none (TTL is already a `u8`); old and new versions interop — old nodes
  relay TTL-15 packets fine.
- Verify during implementation that nothing in `wire.rs` decode or the relay path
  rejects or clamps TTL > 7.

## Testing

- Unit: each origination type gets its configured TTL, normal and dense.
- Unit: relayed packets still decrement by exactly 1 regardless of type.
- Existing suite stays green (some tests may assume TTL 7 at origination — update those
  assertions to the per-type values).
