# Cockroach Chat — Ralph build loop prompt

You are building **Cockroach Chat**, a decentralized, serverless BLE-mesh messenger for
protests / network blackouts. Shared Rust core (`meshcore`) exposed to native apps via
UniFFI; Android first, then iOS. Full context: `docs/IMPLEMENTATION_PLAN.md`. Hard
constraints: `docs/research-brief.md`. Wire spec: `docs/protocol.md`.

## Each iteration, do exactly this

1. **Read `docs/PROGRESS.md`.** It is the source of truth for what's done and next.
2. **Pick the single smallest next unchecked `[ ]` task.** One task. Do not batch a whole
   milestone. Prefer tasks that are verifiable in this repo without hardware.
3. **Follow TDD** (see `superpowers:test-driven-development` if available): write the failing
   test first, then the implementation, then make it pass.
4. **Verify before claiming done.** Run the real command (`cargo test --workspace`,
   `cargo clippy -- -D warnings`, the specific sim scenario). Paste/keep the evidence. If it
   fails, that failure is your next work — debug and fix (`superpowers:systematic-debugging`).
5. **Update `docs/PROGRESS.md`:** check the box `[x]` with a one-line note incl. the command
   you ran. If blocked, mark `[!]` and fill in **Blockers**, then pick another independent task.
6. **Commit** with a focused message describing the one task. Keep commits small.

## Rules

- **Vetted crypto only** (`snow`, `ed25519-dalek`, `x25519-dalek`, `chacha20poly1305`,
  RustCrypto). Never hand-roll a primitive. Parsers get decompression/size caps and are
  fuzzable.
- **Sans-IO core:** no threads, no async runtime, no clock syscalls inside `meshcore` — time
  is injected. This is what keeps the simulator deterministic. Don't break it.
- **Don't drift from `docs/protocol.md`.** If you must change the wire format, bump its
  version and update the golden test vectors in the same commit; log it in PROGRESS.
- **Respect the plan's scope.** Target is local clusters (50–500), not city scale. The v2
  deferral list in IMPLEMENTATION_PLAN is enforced — don't build deferred features.

## Escape hatches (do NOT fake progress to exit)

- **Hardware-gated tasks (M1+):** BLE glue on real phones cannot be verified by an autonomous
  loop. When the next task requires physical devices, do the code + JVM/unit-level verification
  you *can*, then mark the task `[!]` in PROGRESS with note "needs on-device verification",
  and stop — this is a legitimate handoff to the human, not a failure.
- **Genuinely stuck** after a real attempt: write the blocker + what you tried + a suggested
  approach into PROGRESS **Blockers**, commit that, and continue with a different independent task.

## Completion

Output `<promise>COCKROACH_CHAT_COMPLETE</promise>` **only when every `[ ]` in PROGRESS.md is
`[x]` and `cargo test --workspace` is green** — i.e. the whole plan through M6 is done and
verified. Never output it to escape the loop. If only the software-verifiable milestones remain
and everything left is hardware-gated, say so explicitly and stop (do not output the promise).
