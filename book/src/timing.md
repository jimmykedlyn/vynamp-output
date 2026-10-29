# Timing

```rust,ignore
pub struct PresentationTiming {
    pub device_frame: u64,
    pub at: MonotonicNanos,
    pub latency_frames: u64,
    pub source: TimingSource, // Reported | Estimated
}
```

It reads: "frame `device_frame` of the stream's frame counter leaves the
converter at `at`." `at` is on the OS monotonic clock (QPC on Windows,
`CLOCK_MONOTONIC` on Linux, host time on macOS).

## The counter includes silence

`RenderContext::device_frame_index` and `PresentationTiming::device_frame`
count every frame delivered to the device, content and silence. A player maps
"content position presented" by subtracting the silence it knows it caused
(from `source_underruns` and the frames it did not supply), so the position is
not underestimated after an underrun or a drain.

## Reported or estimated

A backend that has an OS timestamp (WASAPI `IAudioClock`, ALSA `snd_pcm_status`,
CoreAudio `AudioTimeStamp`, ...) reports `Reported`. Without one the library
estimates from buffer and period size and marks the value `Estimated`. The
`Null` host always estimates.

## Where to read it

- In `render`: `ctx.timing` (an `Option`).
- Anywhere else: `Stream::timing()`, a snapshot taken without locks.

## Drain

`Stream::drain(timeout)` stops asking the source for data once it signals the
end, waits until the presented `device_frame` reaches the last content frame,
and only then stops. The outcome is `Drained` or `TimedOut`.
