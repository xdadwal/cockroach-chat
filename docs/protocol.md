# Cockroach Chat Wire Protocol — v0.1

Normative spec for the on-air format. The Rust `meshcore` crate is the reference implementation;
`testvectors/packets.json` holds golden encodings that every implementation must reproduce
byte-for-byte. **Any change to this document changes `PROTOCOL_VERSION` and regenerates the
vectors.**

All integers are big-endian. Sizes are bytes.

## Layers

```
BLE GATT  →  Link Frame  →  Packet  →  Payload (per message type)
```

- A **Packet** is the routed unit (signed, deduplicated, relayed).
- A **Link Frame** carries a packet (or a fragment of one) across a single BLE connection, sized
  to the usable ATT payload.

## Link Frames

The usable ATT payload is assumed to be **182 bytes** (iOS floor: 185 MTU − 3) until a larger MTU
is negotiated. Two frame kinds:

| Kind | Byte 0 | Layout |
|------|--------|--------|
| Complete | `0x01` | `0x01 ‖ packet_bytes` (when `packet_bytes.len() < mtu`) |
| Fragment | `0x02` | `0x02 ‖ digest(8) ‖ index(2) ‖ total(2) ‖ chunk` |

Reassembly bounds (defended, since fragments are attacker-controlled): **128** concurrent
messages, **30 s** idle timeout, **1 MiB** max reassembled size, **16** fragments/link cap.
Fragments with `index ≥ total`, `total = 0`, or a changing `total` mid-message are rejected.

## Packet

```
+-----------+------+-------------------------------------------------+
| offset    | size | field                                           |
+-----------+------+-------------------------------------------------+
| 0         | 1    | version                (0x01; unknown ⇒ reject) |
| 1         | 1    | type                   (unknown ⇒ relay, keep)  |
| 2         | 1    | flags                                           |
| 3         | 1    | TTL       *** excluded from the signature ***   |
| 4         | 8    | timestamp_ms                                    |
| 12        | 8    | sender_eph_id                                   |
| 20        | 8    | recipient_eph_id   (only if flags.HAS_RECIPIENT)|
| 20/28     | 2    | payload_len                                     |
| ...       | N    | payload                                         |
| ...       | 64   | signature (Ed25519)                             |
+-----------+------+-------------------------------------------------+
```

**flags**: bit0 `HAS_RECIPIENT`, bit1 `COMPRESSED` (payload is LZ4), bit2 `RESERVED_PQ` (must be 0).

**Signature.** Ed25519 over the whole packet *with the TTL byte set to zero* and without the
signature trailer. A relay decrements TTL without re-signing; verification re-zeroes TTL. The
signer is the originator's long-term Ed25519 key; a verifier learns that key from the sender's
signed `Announce` (the `eph_id → key` binding).

**Dedup digest.** First 8 bytes of `SHA-256(signature)`. TTL-independent, so all copies of one
message share a digest.

**Decoder rules (untrusted input — must never panic):** reject on short buffer, `version ≠ 0x01`,
`payload_len + 64 > remaining`, or trailing bytes. Unknown `type` decodes to `Unknown(b)` and is
relayed but not parsed.

## Message Types

| Value | Type | Payload |
|-------|------|---------|
| 0x01 | Announce | `ed25519_pub(32) ‖ nick_len(1) ‖ nick(≤24)` (M0). TLV form is a later extension. |
| 0x02 | ChannelMessage | `compressed(1) ‖ chan_len(1) ‖ channel ‖ body` (body LZ4 iff `compressed=1`) |
| 0x03 | DirectMessage | Noise ciphertext (M3) |
| 0x04 | NoiseHandshake | Noise XX handshake message (M3) |
| 0x05 | Receipt | delivery/read receipt |
| 0x06 | SyncRequest | `chan_len(1) ‖ channel ‖ digest(8)*` — digests the requester holds |
| 0x07 | SyncResponse | reserved (M0 answers a SyncRequest by resending original ChannelMessage packets) |
| 0x10–0x13 | Media* | offer / fetch-request / chunk / fetch-ack (M4) |

## Relay & Flood Control

| Parameter | Default | Notes |
|-----------|---------|-------|
| TTL (origin) | 7 | clamped to 5 at local degree ≥ 6 |
| Jitter before rebroadcast | 10–220 ms uniform | |
| Suppression threshold | 3 | cancel a scheduled rebroadcast after hearing ≥3 copies |
| Rebroadcast probability | 1.0 / 1.0 / 0.85 → *see note* | by degree tier (≤3 / ≤6 / >6) |
| Seen cache | 1000 entries, 5 min TTL | keyed on digest |
| Split-horizon | on | never rebroadcast onto the arriving link |

> Note: the documented probabilistic-thinning tiers were 1.0/0.7/0.45. The simulator showed that
> at the ~8-link BLE connection cap, 0.45 under-covers (a low-degree cut vertex may drop the only
> path to a sub-crowd), so the reference default leans on counter-based suppression with higher
> rebroadcast probability. Final values are a tuning target tracked in `docs/PROGRESS.md`.

## Rate Limiting & Anti-Sybil

- Per **originating** sender: token bucket, burst **10**, sustained **30/min**; on exceed, drop
  and greylist **60 s**. Relayed copies carry the origin's id, so a flooder is throttled network-wide.
- Identity mint requires a hashcash **proof of work**: `SHA-256(pubkey ‖ nonce)` with **22** leading
  zero bits (~2–4 s on a mid-range phone). No-PoW peers get reduced quota and no store-and-forward.

## Compression

LZ4, applied only to payloads > 128 B, length-prepended (`u32` uncompressed size). Decode refuses
any blob whose declared output exceeds **4096 B** — the absolute cap is the zip-bomb defense
(output is bounded regardless of input size).

## Retention

Channel history 6 h / 1000 msgs; store-and-forward envelopes 24 h / 100 per peer; reassembly 30 s.

## Identity & Privacy

Long-term **Ed25519** (signing) + **X25519** (agreement); stable fingerprint = `SHA-256(ed25519_pub)`.
The 8-byte `sender_eph_id` is random per rotation and rotates with the BLE MAC (~15 min) so an
observer cannot link it to the fingerprint; peers relearn the binding from each signed Announce.
