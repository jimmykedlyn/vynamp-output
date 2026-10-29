# The real-time contract

```rust,ignore
pub trait RenderSource: Send + 'static {
    fn render(&mut self, out: DeviceBuffer<'_>, ctx: &RenderContext<'_>) -> Rendered;
}
```

`render` runs on the device thread, once per period. Inside it you must not
allocate, lock, block, perform I/O or log. Everything the library touches in
that path is allocated in `open()`; the callback uses atomics, slices and
pre-allocated scratch only.

## What the library guarantees

- `out` is interleaved and in the stream's real format (`DeviceBuffer::F32`,
  `I32`, `I24Packed`, `Dsd`, ...), never the requested one.
- If you report `Rendered { frames }` smaller than `ctx.frames`, the library
  fills the rest with the format's correct silence (zero PCM, `0x80` for `U8`,
  DSD idle `0x69` with continuous DoP markers) and counts one
  `source_underruns` for that period. It never repeats samples.
- `ctx.device_frame_index` counts every frame delivered to the device, content
  and silence.
- `end_of_stream: true` tells the library to stop asking for data; `drain`
  then waits until the last content frame has been presented.

## Panics

A panic in `render` is caught. The stream delivers silence from then on and
emits `Event::SourcePanicked` once. The process-wide panic hook still runs
first on the device thread (by default it prints, which allocates and does
I/O); the library does not replace it. An application that needs a silent audio
thread installs its own hook. With `panic = "abort"` nothing can be caught.

## How it is checked

- A per-thread counting allocator proves zero allocations on the device thread
  in `Null` and `Capture` runs (running, underrun and panic-recovery periods).
- `output-probe` prints the same count for any host it opens.
- The facade's doc examples run on `Null`.
