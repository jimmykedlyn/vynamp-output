# Architecture

vynamp-output is a std-only audio output library. This release contains the
platform-independent core and two pure-Rust hosts; OS backends (WASAPI, ALSA,
PipeWire, Pulse, CoreAudio, AAudio) arrive in later releases.

```text
Host -> enumerate() -> DeviceInfo -> open(StreamRequest, RenderSource) -> Stream
                         \-> Receiver<Event> (underrun, device lost, downgrade, panic, ...)
Stream: start / pause / resume / drain(timeout) / stop, config(), timing(), counters()
```

## Crates

- `vynamp-output` is the facade. It re-exports the core and chooses a host
  (`hosts()`, `default_host()` and the reason in `HostSelection`).
- `vynamp-output-core` holds the API types, negotiation, the stream state
  machine, the generic device-thread driver, the DoP packer and the `Null`
  host (plus `Capture`, an internal test backend behind the `capture` feature,
  exposed by the facade as `test-capture`).
- `vynamp-output-testkit` is test-only: the per-thread counting allocator and
  deterministic render sources. The core uses it only as a dev-dependency.
- `tools/output-probe` is a development tool; see below.

`unsafe` is forbidden everywhere except future per-platform `*-sys` crates and
the testkit's allocator.

## Principles

- No silent fallback: an unmet request is an error, or an explicit
  `Event::Downgraded` when a downgrade was requested. `Stream::config()` is
  always the real configuration.
- The library never applies gain, dithers or resamples. Conversions happen only
  when allowed and are reported in `PathProperties::library_conversion`.
- The real-time path never allocates, locks, blocks or does I/O.

## Hosts in this release

`NullHost` discards audio. With `NullClock::Real` it paces periods on the wall
clock; with `NullClock::Virtual` periods happen only when a test calls
`VirtualClockHandle::advance`, which makes runs deterministic. Both run the
same device-thread driver an OS callback would, so the contract is exercised
in CI.

## Verification

Each host declares a verification level per feature. The generated
[support matrix](../../docs/support-matrix.md) lists them;
[hardware-matrix.md](../../docs/hardware-matrix.md) holds the manual hardware
entries. Decisions are recorded in [ADR-001](../../docs/adr/ADR-001-render-contract.md).

## The probe

`cargo run -p output-probe` opens a host, plays a sine and prints the real
`StreamConfig`, five timing samples, the counters and the number of heap
allocations on the device thread (expected: 0). Hosts: `null` and `capture`
(the facade's `test-capture` feature, enabled in the probe; `--capture-file`
sets the output file). Options: `--host null|capture`, `--device`,
`--clock virtual|real`, `--rate`, `--channels`, `--period-frames`, `--periods`.
