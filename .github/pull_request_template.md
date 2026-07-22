<!--
  Security vulnerability? Don't open a PR against master — it discloses the bug.
  Use https://github.com/xdadwal/cockroach-chat/security/advisories/new instead.
-->

## What and why

<!-- What changes, and what problem it solves. Link the issue if there is one. -->

## Verification

<!--
  Paste the commands you actually ran and their results. "Should work" isn't verification —
  see the honesty rules in CONTRIBUTING.md.
-->

```
```

- [ ] `cargo test --workspace`
- [ ] `cargo clippy --workspace --all-targets -- -D warnings`
- [ ] `cargo fmt --all -- --check`
- [ ] Built the Android app (if Kotlin/resources changed)
- [ ] Verified on a physical phone (if BLE or UI changed) — say which devices

## Invariants

- [ ] No hand-rolled crypto; new parsers have size and decompression caps
- [ ] `meshcore` stays sans-IO — no threads, async runtime, or clock syscalls
- [ ] Wire-format changes bump `PROTOCOL_VERSION` **and** regenerate golden vectors in this commit
- [ ] Stays within the ~50–500 local-cluster scope

## Anything you're unsure about

<!--
  Genuinely useful — especially on security-relevant code. Flagged uncertainty gets attention;
  quiet uncertainty is how people get hurt. "None" is a fine answer.
-->
