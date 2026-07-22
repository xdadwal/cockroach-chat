# Governance

Small project, few rules — but the rules that exist are there because this is security software.

## Today

[@xdadwal](https://github.com/xdadwal) is the maintainer and has final say. That is a statement of
current fact, not an ambition: the project actively wants co-maintainers, and this document exists
so that path is written down before anyone needs it.

## Review requirements

| Change touches | Approvals |
|---|---|
| `crates/meshcore` crypto, wire codec, relay, or rate limiting | **2 maintainers** |
| `docs/protocol.md` or `testvectors/` | **2 maintainers** |
| `KeyVault.kt` (Android Keystore handling) | **2 maintainers** |
| Everything else | 1 maintainer |

`master` is protected: changes land by pull request with CI green. The two-approval rule is
deliberately slower than a project this size needs — a subtle bug in the relay or key handling
hurts people who have no way to detect it, and no author should be the only reader of that code.

Where two maintainers aren't available yet, the maintainer may ask an outside reviewer with relevant
expertise, and will say so in the PR. "I couldn't find a second reviewer" is a reason to wait, not
a reason to merge.

## Becoming a maintainer

There's no application. The path is:

1. Contribute meaningfully over time — code, review, or sustained hardware testing all count.
   Reviewing other people's PRs well is weighted heavily; it's the scarcest thing here.
2. Show good judgement on the [project invariants](CONTRIBUTING.md#project-invariants), especially
   knowing when to say "this needs more scrutiny than I can give it."
3. An existing maintainer invites you. If there are several maintainers, it takes agreement among
   them and no sustained objection.

Maintainers can step back at any time, and stepping back is normal — say so and we'll update the
list. Inactive maintainers may be moved to emeritus after a long quiet stretch; that's
bookkeeping, not a judgement, and the door stays open.

## How decisions get made

Ordinary things happen in issues and PRs by rough consensus. For anything that changes the wire
protocol, weakens a security property, or expands the project's scope, open an issue and let it sit
long enough for people to actually see it. Silence isn't agreement on those.

If consensus doesn't form, the maintainer decides and writes down why — in an
[ADR](docs/decisions/) when the decision is architectural.

## Scope

The scope is defined by [`docs/IMPLEMENTATION_PLAN.md`](docs/IMPLEMENTATION_PLAN.md), including its
v2 deferral list. Expanding scope is a decision, not a side effect of merging a PR. "We're not
building that yet" is a normal and friendly answer.

## Security decisions

Vulnerability handling follows [`SECURITY.md`](SECURITY.md). Embargoed fixes may be developed
privately and merged with less public discussion than usual; the reasoning gets published with the
advisory once it's out.

The commitment to an external audit before promoting this for real protest use is not something a
maintainer can quietly drop. Changing it requires an issue and a written rationale.

## Forking

MIT — fork freely, no permission needed. If you fork because something here isn't working, we'd
genuinely like to hear why, but you don't owe us that.
