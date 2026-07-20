# ADR-0001: Shared Rust core via UniFFI

**Status:** Accepted · **Date:** 2026-07 · **Milestone:** M0

## Context

Cockroach Chat ships on both iOS and Android. The protocol, cryptography, relay logic, and
store-and-forward are security-critical and identical on both platforms. The obvious alternative —
bitchat's approach — implements the protocol twice (Swift + Kotlin) and keeps them in sync by
hand. bitchat's own issue tracker shows the cost: chronic iOS↔Android interop bugs where one side
silently drops the other's packets, plus two independent chances to get the Noise handshake wrong.

## Decision

One **sans-IO Rust core** (`meshcore`) owns the packet codec, fragmentation, identity/PoW, Noise
sessions, relay/flood-control, store, and media state machines. It is exposed to native apps
through **Mozilla UniFFI**. Native code owns only two thin things: the BLE radio (CoreBluetooth /
Android BLE) behind a `Transport` trait, and the UI.

"Sans-IO" is load-bearing: the core takes time from an injected `Clock` and does all I/O through
the `Transport`/`Store`/`KeyVault` traits. No threads, no async runtime, no syscalls inside the
core. This is what lets the desktop simulator run hundreds of real nodes deterministically, so
~95% of the logic is tested without a phone.

## Consequences

**Positive:** one implementation of every wire/crypto rule (no drift, half the audit surface);
cross-platform conformance guaranteed by shared golden test vectors; the bulk of development and
testing happens off-device in fast, deterministic Rust.

**Negative / costs:** UniFFI + Gradle/NDK + Xcode plumbing is real up-front friction — de-risked
by a JVM smoke test in M0 before the Android app depends on it. Async platform BLE callbacks must
be marshalled into the single-threaded core (handled at the FFI boundary). Contributors need some
Rust.

**Rejected alternatives:** native ×2 (the drift/audit tax above); Flutter/React Native (Dart/JS
are suspended in the background, killing relay exactly when it's needed); Kotlin Multiplatform (no
precedent for dual-role BLE meshes, immature iOS peripheral path); closed mesh SDKs like Bridgefy
(offline license checks, repeatedly-broken crypto, not auditable).
