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
Security: [GitHub Security Advisories](https://github.com/jimmykedlyn/vynamp-output/security/advisories/new)
— see [SECURITY.md](SECURITY.md). This library writes to audio devices.
See [CONTRIBUTING.md](CONTRIBUTING.md) and [LEGAL.md](LEGAL.md).

# Português — vynamp-output

[English](#vynamp-output) · **Português**

Biblioteca de saída de áudio em Rust, sem dependências (só `std`): enumeração de
dispositivos, negociação de stream e renderização em tempo real para Windows,
Linux, macOS e Android, sem nenhuma dependência do crates.io. O `unsafe` está
confinado aos crates `*-sys` de cada plataforma.

**Estado:** pré-alpha, repositório público. As APIs são instáveis e nada foi
publicado. Irmã de [vynamp-decode](https://github.com/jimmykedlyn/vynamp-decode).

## Princípios

- Sem fallback silencioso: um pedido que não pode ser honrado é erro, ou um
  evento `Downgraded` explícito quando a degradação foi pedida; a configuração
  real é sempre legível.
- A biblioteca nunca aplica ganho, dither ou resample sem que uma conversão
  tenha sido explicitamente permitida.
- O caminho de tempo real nunca aloca, trava, bloqueia nem faz I/O.
- Clean-room: as declarações de FFI são escritas a partir de documentação
  pública de API, registradas em [docs/sources](docs/sources/README.md).

## Documentação

- Guia (fontes mdBook): [book/src](book/src/SUMMARY.md) — arquitetura, contrato
  de tempo real, como escrever um `RenderSource`, timing, modos e downgrade, DoP.
- [ADR-001](docs/adr/ADR-001-render-contract.md): o contrato de render.
- [Matriz de suporte](docs/support-matrix.md) (gerada) e
  [matriz de hardware](docs/hardware-matrix.md).
- `cargo run -p output-probe`: toca uma senoide no host (`null` ou `capture`) e
  imprime a configuração real, o timing, os contadores e as alocações da thread
  de device.

Licença: `MIT OR Apache-2.0` — [LICENSE](LICENSE) aponta para as duas,
[LICENSE-MIT](LICENSE-MIT) e [LICENSE-APACHE](LICENSE-APACHE).
Segurança: [GitHub Security Advisories](https://github.com/jimmykedlyn/vynamp-output/security/advisories/new)
— ver [SECURITY.md](SECURITY.md). Esta biblioteca escreve em dispositivos de áudio.
Ver [CONTRIBUTING.md](CONTRIBUTING.md) e [LEGAL.md](LEGAL.md).
