# Threat model

> **This document describes design intent, not audited fact.** Cockroach Chat has had **no external
> security audit**. Every "mitigated" row below is a claim about what the code is *built to do*,
> verified only by our own tests and hardware runs. Treat the "Residual risk" and
> [Not defended](#what-we-do-not-defend-against) sections as the load-bearing parts.

Referenced by `docs/IMPLEMENTATION_PLAN.md`. Normative protocol numbers live in
[`protocol.md`](protocol.md); the prior-art failures that shaped these choices are in
[`research-brief.md`](research-brief.md).

---

## Who this is for

A person in a crowd — protest, disaster zone, blackout — whose phone has no working internet or
cell service, who needs to reach people nearby, and for whom being **identified** as the author of
a message can carry consequences well beyond the message itself.

That last clause is why "the messages are encrypted" is not a sufficient answer anywhere in this
document. **Metadata is the risk.** Radio presence is metadata we cannot fully hide.

## Assets

| Asset | Why it matters |
|---|---|
| Message **content** (DMs) | Directly incriminating. |
| **Social graph** — who talks to whom | Usually more dangerous than content; maps organisational structure. |
| **Presence** — that you are running this app, here, now | Being a user can itself be treated as evidence. |
| **Long-term identity keys** | Compromise means durable impersonation. |
| **Message history at rest** | The seized-phone scenario. |

---

## Adversaries

### A1 — Passive co-located radio observer
*Someone within BLE range with an SDR or a scanner: IMSI-catcher operator, surveillance van, or a person with a laptop.*

- **Mitigated:** DM content is Noise XX encrypted. The 8-byte `sender_eph_id` is random and rotates
  with the BLE MAC roughly every 15 minutes, so a naive log of identifiers doesn't trivially link
  back to your long-term fingerprint ([`protocol.md`](protocol.md) § Identity & Privacy).
- **Residual risk — HIGH, and not solvable at this layer.** An observer with continuous coverage can
  correlate RSSI, packet timing, and rotation boundaries to re-link identifiers and track a device
  across rotations. Transmission itself reveals presence and rough location. Flood routing means
  *every relay in range* sees that a packet exists, its size, and its timing.
  **This app does not make you anonymous over the air, and cannot.**

### A2 — Active MITM on DM setup
*The Bridgefy and launch-day bitchat break.*

- **Mitigated:** Noise XX with identity binding — the encrypted session must match the peer's
  announced static key, so a substituted key fails the handshake rather than silently downgrading.
  In-person QR fingerprint verification plus a 4-word safety number is the out-of-band check. There
  is no silent downgrade path.
- **Residual risk:** Until you verify in person, you are trusting first-use. An attacker present at
  first contact who can suppress the real peer is the classic TOFU gap. **Verify in person** is not
  decoration; it's the only thing that closes this.

### A3 — Impersonation / forged messages
- **Mitigated:** every packet is Ed25519-signed over the full packet with the TTL byte zeroed, so
  relays can decrement TTL without invalidating the signature. Identity fingerprint is
  `SHA-256(ed25519_pub)`.
- **Residual risk:** a signature proves key custody, not human identity. Only QR verification binds
  a key to a person you actually saw.

### A4 — Denial of service / resource exhaustion
*A zip bomb killed Bridgefy's entire mesh; a parser overflow broke bitchat.*

- **Mitigated:** decompression refuses any blob declaring output over 4096 B (an absolute cap, so
  output is bounded regardless of input); reassembly is capped at 1 MiB / 16 fragments per link with
  a 30 s idle timeout; per-sender rate limits with a 60 s greylist, applied network-wide because
  relayed copies carry the origin's id; seen-cache dedup on a TTL-independent digest. The
  `malicious_flooder`, `duplicate_storm`, and `zipbomb_rejected` simulator scenarios cover these.
  Fuzz targets exist for the three parsers (`crates/meshcore/fuzz`): `wire_decode`,
  `frag_reassemble`, and `decompress`.
- **Residual risk:** those targets run **60 s each on pull requests and nowhere else**. There is no
  scheduled long-running fuzz job and no corpus is carried between runs, so CI catches regressions
  that crash quickly — it does not search for new bugs. Sustained fuzzing happens only when someone
  runs it by hand. M6 sets the bar at ≥8 h clean per target and we are nowhere near it. The Noise
  handshake and the store have no targets at all. **Treat "the parsers are fuzzed" as a statement
  about tooling that exists, not about assurance that has been earned.**
  A determined attacker with several radios can still degrade a local cell regardless; BLE airtime
  is finite and no protocol fixes that.

### A5 — Sybil / flooding the mesh with fake identities
- **Partially mitigated:** minting an identity costs a 22-bit hashcash proof of work (~2–4 s on a
  mid-range phone), and no-PoW peers get reduced quota and no store-and-forward.
- **Residual risk — HIGH.** PoW raises the cost; it does not stop a motivated adversary with
  hardware. **There is no defence against Sybil attacks without out-of-band trust.** Verified
  contacts are the only real trust signal in the system.

### A6 — Seized device, locked
- **Mitigated:** messages and identity live in a SQLCipher database keyed by a hardware-wrapped
  Android Keystore key; ciphertext-only at rest. `FLAG_SECURE` blocks screenshots and the recents
  thumbnail. Panic wipe destroys the keystore key, rendering the database permanently unreadable.
- **Residual risk:** depends entirely on your lock screen and the device's hardware keystore holding
  up against the forensic tooling of whoever seized it. Cellebrite-class tooling against an unpatched
  or older device is a real threat we do not claim to beat.

### A7 — Seized device, unlocked, or coercion
- **NOT DEFENDED.** If the phone is unlocked and in someone else's hands, they have your messages,
  your contacts, and your identity key. Panic wipe only helps if you trigger it *before* losing
  control of the device. There is no duress PIN, no hidden volume, and no plausible deniability.

### A8 — Compromised OS, rooted device, or malicious app with accessibility access
- **NOT DEFENDED.** We are an app; we cannot defend against the platform underneath us.

### A9 — Network-level censorship or shutdown
- **Structurally immune** — this is the reason the project exists. No servers, no accounts, no DNS,
  no internet dependency. Nothing to block, subpoena, or shut off.
- **Residual risk:** the *distribution* channel is still centralised and blockable. Getting the APK
  into hands during a shutdown is an unsolved problem here.

### A10 — RF jamming
- **NOT DEFENDED.** Broadband jamming of the 2.4 GHz band stops the mesh. No mitigation exists at
  this layer.

### A11 — Supply chain / build integrity
- **Partially mitigated:** releases are signed; the signing certificate fingerprint is published in
  [`SECURITY.md`](../SECURITY.md) so a downloaded APK can be checked with
  `apksigner verify --print-certs`.
- **Residual risk:** builds are **not yet reproducible**, so you are trusting our build machine.
  Reproducible builds and F-Droid are on the roadmap.

---

## What we do *not* defend against

Stated plainly, because a security tool that is vague here is lying:

1. **Anonymity over the air.** This is not Tor. Transmitting reveals presence; a co-located
   observer with sustained coverage can track and correlate you (A1).
2. **Traffic analysis and social-graph inference** by an adversary who can watch the radio (A1).
3. **Sybil attacks** without out-of-band verification (A5).
4. **An unlocked seized device, or coercion** (A7). No duress mode exists.
5. **A compromised operating system** (A8).
6. **Jamming** (A10).
7. **Forward secrecy for stored history.** Noise gives transport forward secrecy, but delivered
   messages are written to the local database. Compromising the device compromises the history that
   is still within the retention window (channel history 6 h / 1000 msgs; envelopes 24 h / 100 per
   peer).
8. **Public channels.** `#general` and friends are ownerless and unencrypted by design — anyone in
   radio range reads them, including police. There is no lock, and there never will be. This is a
   product decision, not a gap.

## If your safety depends on this

Don't rely on it yet. The software is unaudited, the parsers are not fuzzed, and the metadata
exposure in A1 is inherent to broadcast radio rather than a bug we can fix. Use tools that have been
audited and have a track record. Revisit this document after an external audit exists.

## Reporting

Vulnerabilities: see [`SECURITY.md`](../SECURITY.md) — please use private reporting, never a public
issue. Corrections to *this document* are equally welcome; a threat model that overstates its
defences is itself a vulnerability.
