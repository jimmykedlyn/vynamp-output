# vynamp-output

Dependency-free audio output library in Rust (std only): device enumeration, stream
negotiation and real-time rendering for Windows, Linux, macOS and Android, with no
crates.io dependencies. `unsafe` is confined to per-platform `*-sys` crates.

**Status:** pre-alpha, private repository. APIs are unstable and nothing is published.
Sibling of [vynamp-decode](https://github.com/jimmykedlyn/vynamp-decode).

## Principles

- No silent fallback: a request that cannot be honoured is an error, or an explicit
  `Downgraded` event when a downgrade was requested; the real configuration is always readable.
- The library never applies gain, dithers or resamples unless a conversion is explicitly allowed.
- The real-time path never allocates, locks, blocks or performs I/O.
- Clean-room: FFI declarations are written from public API documentation, recorded in
  [docs/sources](docs/sources/README.md).

See [CONTRIBUTING.md](CONTRIBUTING.md), [LEGAL.md](LEGAL.md) and [SECURITY.md](SECURITY.md).
