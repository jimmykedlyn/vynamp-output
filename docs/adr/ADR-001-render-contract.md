# ADR-001: The render contract (RenderSource, StreamConfig, PresentationTiming)

Status: accepted. Date: 2026-10-25.

## Context

Vynamp's audio spike used CPAL with a purpose-built callback renderer
(`Renderer::render(&mut [f32], latency_frames)`). CPAL cannot report the real
negotiated format, exclusive-mode facts or presentation timing, and brings 51
packages. vynamp-output replaces it with a std-only library whose API makes
the transport honest and the real-time path auditable.

## Decision

1. **`RenderSource`** is the only way audio enters the library:

   ```rust
   pub trait RenderSource: Send + 'static {
       fn render(&mut self, out: DeviceBuffer<'_>, ctx: &RenderContext<'_>) -> Rendered;
   }
   ```

   It runs on the device thread and must not allocate, lock, block or do I/O.
   `out` is a typed, interleaved slice in the stream's **real** format. The
   library fills what the source did not write with the format's silence and
   counts one `source_underruns` per short period, unless `end_of_stream` is
   set (a short final period is silence-filled but not an underrun); it never
   repeats samples.
   A panic is caught, the stream turns to silence and `Event::SourcePanicked`
   is emitted.

2. **`StreamConfig`** is what `open` actually produced, never what was asked:
   host, device, mode, container format, valid bits, rate, channels, layout,
   period and buffer, and `PathProperties` (OS mixer bypassed, OS resampling,
   library conversion, endpoint volume, transport, real-time priority). An unmet
   request is an error (`on_unavailable: Error`, default) or an explicit
   `Event::Downgraded` when `Downgrade` was requested. Conversions need
   `Conversion::Allow`, are deterministic (round half to even, clamp, no
   dither) and are reported. The library never applies gain or resamples.

3. **`PresentationTiming { device_frame, at, latency_frames, source }`** says
   when a frame leaves the converter, on the OS monotonic clock, marked
   `Reported` or `Estimated`. `device_frame` counts content **and** silence, so
   consumers can map content position without the underrun bias found in the
   spike. It is available inside `render` (`ctx.timing`) and from
   `Stream::timing()`.

4. **Verification levels** (`capture-verified`, `hardware-verified`,
   `integration`, `library-verified`, `compiled`) are declared per host and
   feature in code and published as a generated support matrix. A level not
   run is "not run", never "passed".

5. Hosts in this release: `Null` (real or virtual clock) and `Capture`
   (internal test backend). The generic device-thread driver is shared with
   future OS backends.

## Consequences

- Vynamp becomes an adapter: a `RenderSource` over its existing renderer
  (`F32` direct; `latency_frames` from `ctx.timing`), living in a
  `vynamp_audio::output` module. The old `crates/vynamp-output` leaves the
  Vynamp workspace (name collision) and the dependency is pinned by git `rev`.
  This is a separate task after the core layer.
- The transparent (integer) mode needs an integer ring in Vynamp; the library
  already accepts `I32`/`I24` buffers.
- Zero-allocation and silence guarantees are testable on `Null` in every CI
  run, which is why the counting allocator lives in the testkit.
- Adding a backend means implementing device I/O and timing around the existing
  driver; negotiation, state machine and events stay in the safe core.

## Alternatives rejected

- Keeping CPAL behind a feature: hides negotiation results and timing, so the
  honesty requirement cannot be met.
- A pull API that returns `Vec<f32>`: allocates on the device thread.
- Silent best-effort fallback on unmet requests: breaks bit-exact output
  without telling the user.
