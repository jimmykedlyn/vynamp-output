# Contributing

## Global rules

- No crates.io dependencies at all, development dependencies included. Only `std`.
- `#![forbid(unsafe_code)]` everywhere, enforced by the workspace lint
  `unsafe_code = "forbid"`, except in the per-platform `*-sys` crates (see below).
- Licence: `MIT OR Apache-2.0`.
- The core must never depend on `vynamp-output-testkit`. The testkit may depend on the core.
- Real-time path (anything the device thread runs per period): no allocation, no locks,
  no blocking, no I/O, no reachable panics.
- Commits end with the `Co-Authored-By:` trailer of the agent that authored them.

### `unsafe` only in `*-sys` crates

`unsafe` is allowed only in the platform `*-sys` crates. Each of them uses
`#![deny(unsafe_op_in_unsafe_fn)]` and `#![warn(clippy::undocumented_unsafe_blocks)]`
(an error in CI), and every `unsafe` block carries a `// SAFETY:` comment citing the API
rule it relies on (valid pointer, lifetime, thread, COM reference count). They expose
minimal safe wrappers so invariants are proven once, inside the crate. Invariants are
recorded in [docs/ffi-safety.md](docs/ffi-safety.md).

**Review rule:** every change that touches `crates/*-sys/**` requires a review by a
clean-context Opus subagent, recorded in the PR and in `docs/ffi-safety.md`.

### The testkit exception

`crates/vynamp-output-testkit/src/alloc.rs` implements `GlobalAlloc` for the allocation
counter, which requires `unsafe impl`. The testkit crate (`publish = false`, test-only)
therefore sets `unsafe_code = "allow"` in its own `[lints.rust]` instead of inheriting the
workspace lint. Every `unsafe` block there carries a `// SAFETY:` comment and delegates to
`std::alloc::System`. No other non-`-sys` crate may do this.

## Clean-room

Never copy or translate code from CPAL, RtAudio, PortAudio, Oboe, miniaudio, vendor SDKs or
system headers. FFI declarations are written from public API documentation; the sources
consulted are recorded per backend in `docs/sources/<backend>.md`. Permissively licensed
projects may be studied for architecture and quirks, with credit.

## TDD

Write the failing test first, watch it fail for the expected reason, then implement.

## Local gate (required before every commit)

```
cargo fmt --check && cargo clippy --workspace --all-targets --locked -- -D warnings && cargo test --workspace --locked
```

At the end of each task also run
`RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --locked`.
`Cargo.lock` is committed.
