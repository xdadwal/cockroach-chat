# Per-Message-Type TTL Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Each floodable message type originates with its own TTL — announce 15 (12 dense), channel 10 (7 dense), DM/handshake 10 (7 dense) — replacing the single `ttl_default`/`ttl_dense_clamp` pair.

**Architecture:** `Tunables` gains six per-type TTL fields and `origin_ttl(msg_type, degree)`; the three origination sites in `node.rs` pass their message type. Relay path, wire format, and SyncRequest (hardcoded TTL 1) are untouched.

**Tech Stack:** Rust (crates/meshcore, crates/sim). Tests via `cargo test --workspace`.

**Spec:** `docs/superpowers/specs/2026-07-24-per-type-ttl-design.md`

## Global Constraints

- TTL values (normal/dense): Announce 15/12, ChannelMessage 10/7, DirectMessage 10/7, NoiseHandshake 10/7 (must always equal DirectMessage).
- Dense means local degree >= `dense_degree` (unchanged, 6).
- No wire-format change; `PROTOCOL_VERSION` stays 1.
- SyncRequest stays hardcoded TTL 1 and must NOT go through `origin_ttl`.
- All commits run from repo root `/Users/akshay/Code/cockroach-chat`; end commit messages with `Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>`.
- NOTE: the working tree already contains uncommitted DM-nonce-fix changes in `crates/meshcore/src/{noise.rs,node.rs}`. Commit ONLY the files each task names (`git add <exact paths>` — never `git add -A`).

---

### Task 1: Per-type TTL in `Tunables` + call-site updates

Single task because changing `origin_ttl`'s signature breaks all three call sites and the sim config at once — the crate must compile as one sweep.

**Files:**
- Modify: `crates/meshcore/src/config.rs` (fields at lines 21-23, defaults at 74-75, `origin_ttl` at 101-110, new test module at end)
- Modify: `crates/meshcore/src/node.rs:210` (`send_channel_message`), `node.rs:249` (`announce`), `node.rs:592` (`send_directed`)
- Modify: `crates/sim/src/scenarios.rs:22-24` (`crowd_cfg`)

**Interfaces:**
- Produces: `Tunables::origin_ttl(&self, msg_type: MsgType, degree: usize) -> u8`; fields `ttl_announce`, `ttl_announce_dense`, `ttl_channel`, `ttl_channel_dense`, `ttl_dm`, `ttl_dm_dense` (all `u8`). Fields `ttl_default`/`ttl_dense_clamp` are REMOVED.

- [ ] **Step 1: Write the failing tests** — append to `crates/meshcore/src/config.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::wire::MsgType;

    #[test]
    fn origin_ttl_is_per_message_type() {
        let c = Tunables::default();
        // Sparse (degree < dense_degree).
        assert_eq!(c.origin_ttl(MsgType::Announce, 2), 15);
        assert_eq!(c.origin_ttl(MsgType::ChannelMessage, 2), 10);
        assert_eq!(c.origin_ttl(MsgType::DirectMessage, 2), 10);
        assert_eq!(c.origin_ttl(MsgType::NoiseHandshake, 2), 10);
    }

    #[test]
    fn origin_ttl_clamps_in_dense_crowds() {
        let c = Tunables::default();
        assert_eq!(c.origin_ttl(MsgType::Announce, 6), 12);
        assert_eq!(c.origin_ttl(MsgType::ChannelMessage, 6), 7);
        assert_eq!(c.origin_ttl(MsgType::DirectMessage, 6), 7);
        assert_eq!(c.origin_ttl(MsgType::NoiseHandshake, 6), 7);
    }

    #[test]
    fn handshake_ttl_always_equals_dm_ttl() {
        // DMs cannot exist without a completed handshake, so the two must never diverge.
        let c = Tunables::default();
        for degree in [0, 3, 6, 12] {
            assert_eq!(
                c.origin_ttl(MsgType::NoiseHandshake, degree),
                c.origin_ttl(MsgType::DirectMessage, degree),
            );
        }
    }

    #[test]
    fn unlisted_types_default_to_link_local() {
        // Safe-by-default: a future type must be listed explicitly to flood.
        let c = Tunables::default();
        assert_eq!(c.origin_ttl(MsgType::SyncRequest, 2), 1);
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `PATH="$HOME/.cargo/bin:$PATH" cargo test -p meshcore config:: 2>&1 | tail -20`
Expected: COMPILE ERROR (`origin_ttl` takes 1 argument, fields don't exist) — that is the failure mode for a signature change.

- [ ] **Step 3: Implement** — in `crates/meshcore/src/config.rs`:

Add import at top (config.rs currently only imports `crate::clock::Millis`; `wire.rs` does not import config, so no cycle):

```rust
use crate::wire::MsgType;
```

Replace the two fields (lines 20-23):

```rust
    /// Origination TTLs, per message type. Announce defines presence/discovery radius (largest);
    /// channels serve a local crowd; DM covers DirectMessage AND NoiseHandshake (a DM cannot
    /// exist without its handshake, so they must never diverge). The `_dense` variants apply at
    /// local degree >= `dense_degree`, where flooding is cheap and airtime is scarce.
    pub ttl_announce: u8,
    pub ttl_announce_dense: u8,
    pub ttl_channel: u8,
    pub ttl_channel_dense: u8,
    pub ttl_dm: u8,
    pub ttl_dm_dense: u8,
```

Replace the two defaults (lines 74-75):

```rust
            ttl_announce: 15,
            ttl_announce_dense: 12,
            ttl_channel: 10,
            ttl_channel_dense: 7,
            ttl_dm: 10,
            ttl_dm_dense: 7,
```

Replace `origin_ttl` (lines 101-110):

```rust
impl Tunables {
    /// The TTL to originate a packet of `msg_type` with, given current local node degree.
    /// Types not listed here are link-local (TTL 1) by design — list a type explicitly to
    /// let it flood.
    pub fn origin_ttl(&self, msg_type: MsgType, degree: usize) -> u8 {
        let dense = degree >= self.dense_degree;
        match msg_type {
            MsgType::Announce => {
                if dense {
                    self.ttl_announce_dense
                } else {
                    self.ttl_announce
                }
            }
            MsgType::ChannelMessage => {
                if dense {
                    self.ttl_channel_dense
                } else {
                    self.ttl_channel
                }
            }
            MsgType::DirectMessage | MsgType::NoiseHandshake => {
                if dense {
                    self.ttl_dm_dense
                } else {
                    self.ttl_dm
                }
            }
            _ => 1,
        }
    }
}
```

Update the three call sites in `crates/meshcore/src/node.rs`:

`send_channel_message` (line 210):
```rust
        let ttl = self.cfg.origin_ttl(MsgType::ChannelMessage, self.links.len());
```

`announce` (line 249, inside `Packet::new`):
```rust
            self.cfg.origin_ttl(MsgType::Announce, self.links.len()),
```

`send_directed` (line 592 — `msg_type` is already a parameter of this function and covers both `DirectMessage` and `NoiseHandshake`):
```rust
        let ttl = self.cfg.origin_ttl(msg_type, self.links.len());
```

Update `crowd_cfg` in `crates/sim/src/scenarios.rs` (lines 22-24) — the simulator lifts every TTL to cross its synthetic graph; keep that intent for all types:

```rust
    Tunables {
        ttl_announce: 24,
        ttl_announce_dense: 24,
        ttl_channel: 24,
        ttl_channel_dense: 24,
        ttl_dm: 24,
        ttl_dm_dense: 24,
```
(the comment above it referencing "default TTL (7, clamped 5)" should be reworded to "the default TTLs model a physically small crowd")

- [ ] **Step 4: Run the full suite**

Run: `PATH="$HOME/.cargo/bin:$PATH" cargo test --workspace 2>&1 | grep -E "test result|error"`
Expected: all `ok`, 4 new config tests included. If any other test referenced the removed fields, the compiler lists it — fix it with the per-type equivalents.

- [ ] **Step 5: Commit**

```bash
git add crates/meshcore/src/config.rs crates/meshcore/src/node.rs crates/sim/src/scenarios.rs
git commit -m "Per-message-type origination TTL (announce 15/12, channel+DM 10/7)

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

---

### Task 2: Node-level regression tests — each type leaves with its TTL

Proves the wiring end-to-end at the node boundary (Task 1's tests only cover the config function) and pins the relay decrement + no-clamp-anywhere behavior for TTL 15.

**Files:**
- Modify: `crates/meshcore/src/node.rs` (append to the existing `#[cfg(test)] mod tests`)

**Interfaces:**
- Consumes: `Tunables::origin_ttl(MsgType, usize)` from Task 1; existing test helpers `node(seed)`, `drain(&node)`, `RecordingTransport`.

- [ ] **Step 1: Write the failing tests** — append inside `mod tests` in `crates/meshcore/src/node.rs`:

```rust
    /// Decode captured outbound frames back into packets (single-fragment frames only, which
    /// is all these small test packets produce).
    fn decode_frames(frames: &[Vec<u8>]) -> Vec<Packet> {
        let mut r = Reassembler::new(Tunables::default());
        frames
            .iter()
            .filter_map(|f| r.ingest(f, 0).ok().flatten())
            .filter_map(|bytes| Packet::decode(&bytes).ok())
            .collect()
    }

    /// Every floodable type must originate with its own configured TTL (announce needs the
    /// largest discovery radius; channels and DMs a smaller one).
    #[test]
    fn origination_ttl_is_per_message_type() {
        let mut a = node(1);
        let mut b = node(2);
        a.on_transport_event(TransportEvent::LinkUp {
            link: 1,
            mtu: 182,
            peer_hint: None,
        });
        b.on_transport_event(TransportEvent::LinkUp {
            link: 1,
            mtu: 182,
            peer_hint: None,
        });
        for f in drain(&b) {
            a.on_transport_event(TransportEvent::FrameReceived { link: 1, frame: f });
        }
        let b_fp = a
            .take_events()
            .iter()
            .find_map(|e| match e {
                MeshEvent::PeerAppeared { fingerprint, .. } => Some(*fingerprint),
                _ => None,
            })
            .expect("A should learn B");
        let _ = drain(&a);

        a.announce();
        let pkts = decode_frames(&drain(&a));
        assert!(
            pkts.iter()
                .any(|p| p.msg_type == MsgType::Announce && p.ttl == 15),
            "announce should originate with TTL 15, got {:?}",
            pkts.iter().map(|p| (p.msg_type, p.ttl)).collect::<Vec<_>>()
        );

        a.send_channel_message("#general", "hello");
        let pkts = decode_frames(&drain(&a));
        assert!(
            pkts.iter()
                .any(|p| p.msg_type == MsgType::ChannelMessage && p.ttl == 10),
            "channel messages should originate with TTL 10"
        );

        // send_dm to a fresh peer emits the first Noise handshake packet — same TTL rule as DMs.
        a.send_dm(b_fp, "hi");
        let pkts = decode_frames(&drain(&a));
        assert!(
            pkts.iter()
                .any(|p| p.msg_type == MsgType::NoiseHandshake && p.ttl == 10),
            "noise handshakes should originate with TTL 10"
        );
    }

    /// In a dense crowd (degree >= 6) origination TTLs clamp to the dense values.
    #[test]
    fn origination_ttl_clamps_when_dense() {
        let mut a = node(1);
        for link in 1..=6 {
            a.on_transport_event(TransportEvent::LinkUp {
                link,
                mtu: 182,
                peer_hint: None,
            });
        }
        let _ = drain(&a);

        a.announce();
        let pkts = decode_frames(&drain(&a));
        assert!(
            pkts.iter()
                .any(|p| p.msg_type == MsgType::Announce && p.ttl == 12),
            "dense announce should clamp to TTL 12"
        );

        a.send_channel_message("#general", "hello");
        let pkts = decode_frames(&drain(&a));
        assert!(
            pkts.iter()
                .any(|p| p.msg_type == MsgType::ChannelMessage && p.ttl == 7),
            "dense channel messages should clamp to TTL 7"
        );
    }

    /// A relayed packet is decremented by exactly 1 — no clamp anywhere rejects the new
    /// higher-than-7 TTLs (wire.rs carries TTL as a raw byte; relay only decrements).
    #[test]
    fn relay_decrements_high_ttl_by_one() {
        let mut b = node(2);
        b.on_transport_event(TransportEvent::LinkUp {
            link: 1,
            mtu: 182,
            peer_hint: None,
        });
        b.on_transport_event(TransportEvent::LinkUp {
            link: 2,
            mtu: 182,
            peer_hint: None,
        });
        let _ = drain(&b);

        // Hand-craft a TTL-15 announce from a third identity and feed it to B on link 1.
        let id = LocalIdentity::from_seed(&[7u8; 32]);
        let payload = encode_announce(
            id.verifying_key().as_bytes(),
            id.dh_public().as_bytes(),
            "stranger",
        );
        let mut pkt = Packet::new(
            MsgType::Announce,
            15,
            1000,
            id.eph_id(),
            None,
            payload,
        );
        pkt.sign(id.signing_key());
        for f in frag::split(&pkt.encode(), pkt.digest(), 182).unwrap() {
            b.on_transport_event(TransportEvent::FrameReceived { link: 1, frame: f });
        }

        // Degree 2 => sparse => relay probability 1.0, so the rebroadcast is always scheduled.
        // Fire it by advancing past the jitter window.
        b.clock.advance(1000);
        b.tick();
        let relayed = decode_frames(&drain(&b));
        assert!(
            relayed
                .iter()
                .any(|p| p.msg_type == MsgType::Announce && p.ttl == 14),
            "a TTL-15 packet must relay with TTL 14, got {:?}",
            relayed
                .iter()
                .map(|p| (p.msg_type, p.ttl))
                .collect::<Vec<_>>()
        );
    }
```

Note for the implementer: `frag::split(packet_bytes, digest, mtu)` is defined at `crates/meshcore/src/frag.rs:21`. For `LocalIdentity` accessor names, mirror how `announce()` at node.rs:240 builds its payload.

- [ ] **Step 2: Run tests to verify they fail... or pass**

Run: `PATH="$HOME/.cargo/bin:$PATH" cargo test -p meshcore origination_ttl relay_decrements 2>&1 | tail -10`
Expected: PASS if Task 1 landed correctly (these are wiring-regression tests, written after the change per the plan's task split). If any FAILS, Task 1's wiring is wrong — fix Task 1, not the test.

- [ ] **Step 3: Run the full suite**

Run: `PATH="$HOME/.cargo/bin:$PATH" cargo test --workspace 2>&1 | grep -E "test result|error"`
Expected: all `ok`.

- [ ] **Step 4: Commit**

```bash
git add crates/meshcore/src/node.rs
git commit -m "Node-level TTL regression tests: per-type origination, dense clamp, relay decrement

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

---

### Task 3: Documentation + final verification

**Files:**
- Modify: `docs/protocol.md:84` (defaults table)

**Interfaces:** none.

- [ ] **Step 1: Update the defaults table** — in `docs/protocol.md`, replace line 84:

```markdown
| TTL (origin) | 7 | clamped to 5 at local degree ≥ 6 |
```

with:

```markdown
| TTL, announce (origin) | 15 | clamped to 12 at local degree ≥ 6 |
| TTL, channel (origin) | 10 | clamped to 7 at local degree ≥ 6 |
| TTL, DM + handshake (origin) | 10 | clamped to 7 at local degree ≥ 6; always equal for both types |
```

- [ ] **Step 2: Full verification**

Run: `PATH="$HOME/.cargo/bin:$PATH" cargo test --workspace 2>&1 | grep -E "test result" && cargo fmt --all --check && cargo clippy --workspace --all-targets 2>&1 | grep -cE "^(warning|error)" || true`
Expected: all `test result: ok`, fmt clean (exit 0), clippy count 0. If fmt fails, run `cargo fmt --all` and include the touched files in the commit.

- [ ] **Step 3: Commit**

```bash
git add docs/protocol.md
git commit -m "Document per-type TTL defaults in protocol.md

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```
