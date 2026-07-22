# Security policy

## Status: unaudited

Cockroach Chat has **not had an external security audit**. It implements cryptography (Noise XX,
Ed25519, X25519, SQLCipher) using vetted libraries rather than hand-rolled primitives, and it has a
written [threat model](docs/threat-model.md) — but nobody independent has reviewed the result.

The app is built to be used in the field, and shipping it beats withholding it — but it is under
active development and **may not work as expected**, particularly under the conditions it exists
for: dense crowds, interference, jamming, low battery, unfamiliar hardware. Delivery is
best-effort. Keep a fallback that doesn't depend on it.

An external audit is a commitment we intend to keep, not a box already ticked.

Read [`docs/threat-model.md`](docs/threat-model.md) before deciding this is appropriate for your
situation. It is explicit about what is *not* defended: over-the-air anonymity, traffic analysis,
Sybil attacks without out-of-band verification, an unlocked seized device, a compromised OS, and
jamming.

## Reporting a vulnerability

**Please report privately. Do not open a public issue.**

Use GitHub's private vulnerability reporting:
**[Report a vulnerability](https://github.com/xdadwal/cockroach-chat/security/advisories/new)**
(Security tab → Report a vulnerability). It's private to the maintainers and gives us a place to
collaborate on a fix and issue a CVE if warranted.

If that is unavailable to you, contact the maintainer through their
[GitHub profile](https://github.com/xdadwal).

Helpful to include: affected component, reproduction steps or a proof of concept, the impact you
think it has, and whether you plan to disclose publicly on a timeline.

### What to expect

This is a small volunteer project — these are honest targets, not an SLA:

| Stage | Target |
|---|---|
| Acknowledgement | 5 days |
| Initial assessment | 14 days |
| Fix or documented mitigation | Depends on severity; we'll tell you what we're aiming for |
| Public disclosure | Coordinated, default 90 days from report |

We'll credit you in the advisory and changelog unless you'd rather stay anonymous. **There is no
bug bounty** — no money, just genuine thanks and credit.

If we go quiet past these targets, disclose. A dead maintainer inbox is not a reason to sit on a
vulnerability that affects users.

## Scope

**In scope**

- `crates/meshcore` — wire codec, fragmentation, relay, rate limiting, identity, PoW, compression
- `crates/meshcore-store` — SQLCipher persistence
- `crates/meshcore-ffi` — the UniFFI boundary
- `android/app/src/main/java/chat/cockroach/KeyVault.kt` — Android Keystore handling
- `android/app/src/main/java/chat/cockroach/ble/` — BLE transport and the foreground service
- Anything that lets an attacker read DM content, impersonate an identity, escalate out of the
  documented rate limits, or crash a peer remotely

**Out of scope**

- The residual risks already documented in [`docs/threat-model.md`](docs/threat-model.md) —
  RF traffic analysis, presence disclosure, Sybil without out-of-band trust, unlocked-device
  seizure, compromised OS, jamming. These are known and stated; a report restating them isn't a
  vulnerability. A report showing we're **worse than documented** absolutely is.
- Public channels being readable. They are unencrypted by design.
- Social engineering, physical attacks on a device you already control, and third-party
  dependency issues without a demonstrated path through our code (report those upstream, but do
  tell us so we can bump).

## Verifying a release

Release APKs are signed. Check what you downloaded:

```bash
apksigner verify --print-certs cockroach-chat-<version>.apk
sha256sum -c SHA256SUMS
```

The certificate SHA-256 must match the fingerprint published below.

> **Signing certificate SHA-256:** _not yet published — no signed release exists._
> This will be filled in with the first tagged release. Until then, build from source.

Builds are **not yet reproducible**, so a signature proves the artifact came from our build machine
and nothing more. Reproducible builds are on the roadmap.

## Cryptography and export

This project distributes cryptographic software. Depending on your jurisdiction, redistributing it
may carry notification or compliance obligations. We are not lawyers and this is not legal advice —
if you plan to mirror or repackage it, check your local rules.
