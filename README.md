# vynamp-output

Dependency-free audio output library in Rust (std only): device enumeration, stream
negotiation and real-time rendering for Windows, Linux, macOS and Android, with no
crates.io dependencies. `unsafe` is confined to per-platform `*-sys` crates.

**Status:** pre-alpha, public repository. APIs are unstable and nothing is published.
Sibling of [vynamp-decode](https://github.com/jimmykedlyn/vynamp-decode).

## Principles

- No silent fallback: a request that cannot be honoured is an error, or an explicit
  `Downgraded` event when a downgrade was requested; the real configuration is always readable.
- The library never applies gain, dithers or resamples unless a conversion is explicitly allowed.
- The real-time path never allocates, locks, blocks or performs I/O.
- Clean-room: FFI declarations are written from public API documentation, recorded in
  [docs/sources](docs/sources/README.md).

## Documentation

- Guide (mdBook sources): [book/src](book/src/SUMMARY.md) - architecture, real-time contract,
  writing a `RenderSource`, timing, modes and downgrade, DoP.
- [ADR-001](docs/adr/ADR-001-render-contract.md): the render contract.
- [Support matrix](docs/support-matrix.md) (generated) and [hardware matrix](docs/hardware-matrix.md).
- `cargo run -p output-probe`: plays a sine on a host (`null` or `capture`) and prints the real config, timing,
  counters and device-thread allocations.

Licence: `MIT OR Apache-2.0` — [LICENSE](LICENSE) points to both,
[LICENSE-MIT](LICENSE-MIT) and [LICENSE-APACHE](LICENSE-APACHE).
See [CONTRIBUTING.md](CONTRIBUTING.md), [LEGAL.md](LEGAL.md) and [SECURITY.md](SECURITY.md).
