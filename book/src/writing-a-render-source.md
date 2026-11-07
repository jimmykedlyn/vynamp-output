# Writing a RenderSource

A source fills the buffer it is given. Keep state in the struct, set it up in
the constructor, and do nothing else that could block.

```rust
use vynamp_output::*;

struct Sine {
    phase: f64,
}

impl RenderSource for Sine {
    fn render(&mut self, out: DeviceBuffer<'_>, ctx: &RenderContext<'_>) -> Rendered {
        let DeviceBuffer::F32(samples) = out else {
            // The stream was opened with another format: end it.
            return Rendered { frames: 0, end_of_stream: true };
        };
        let step = std::f64::consts::TAU * 440.0 / f64::from(ctx.config.rate);
        let channels = usize::from(ctx.config.channels).max(1);
        let mut frames = 0;
        for frame in samples.chunks_exact_mut(channels).take(ctx.frames as usize) {
            frame.fill((self.phase.sin() * 0.25) as f32);
            self.phase = (self.phase + step) % std::f64::consts::TAU;
            frames += 1;
        }
        Rendered { frames, end_of_stream: false }
    }
}

# fn main() -> Result<(), Error> {
let clock = VirtualClockHandle::new();
let host = NullHost::new(NullClock::Virtual(clock.clone()));
let req = StreamRequest {
    device: DeviceSelector::Default(Role::Multimedia),
    mode: ShareMode::Shared,
    format: SampleFormat::F32,
    rate: 48_000,
    channels: 2,
    layout: ChannelLayout::unknown(2),
    buffer: BufferRequest::Frames(256),
    on_unavailable: OnUnavailable::Error,
    conversion: Conversion::Forbid,
    allow_rate_change_of_device: false,
};
let mut stream = host.open(&req, Box::new(Sine { phase: 0.0 }))?;
stream.start()?;
clock.advance(10);
assert_eq!(stream.counters().frames_delivered, 2560);
stream.stop()?;
# Ok(())
# }
```

## Rules of thumb

- Write at most `ctx.frames` frames and at most what the slice holds.
- Match on the `DeviceBuffer` variant you expect. The variant is the stream's
  real format, so check `Stream::config()` after `open`.
- Never fill with garbage to "avoid" an underrun: report the frames you have
  and let the library produce silence.
- Pass data in from other threads with a lock-free single-producer ring, not a
  `Mutex` or an allocating channel.
- Test a source on the `Null` host with a virtual clock, and use the testkit's
  allocation counter to assert zero allocations.
