//! Deterministic render sources for tests.
//!
//! Sources write interleaved samples for `min(ctx.frames, buffer frames)`
//! frames. [`Silence`], [`Ramp`] and [`SineF32`] write the same value into
//! every channel of a frame; [`ChannelRamp`] gives every sample its own value.
//! Values depend only on `ctx.device_frame_index` plus the frame offset, so
//! output is reproducible. A source panics when given a buffer variant it
//! does not support (a test bug, not a runtime condition).

use vynamp_output_core::{DeviceBuffer, RenderContext, RenderSource, Rendered};

/// Frames to write: bounded by the request and by the buffer capacity.
fn frame_count(samples: usize, ctx: &RenderContext<'_>) -> usize {
    let channels = usize::from(ctx.config.channels);
    (samples.checked_div(channels).unwrap_or(0)).min(ctx.frames as usize)
}

/// Writes `value(frame_offset)` into every channel of each frame.
fn fill<T: Copy>(out: &mut [T], channels: usize, frames: usize, mut value: impl FnMut(u64) -> T) {
    if channels == 0 {
        return;
    }
    for (n, frame) in out.chunks_exact_mut(channels).take(frames).enumerate() {
        frame.fill(value(n as u64));
    }
}

fn rendered(frames: usize) -> Rendered {
    Rendered {
        frames: frames as u32,
        end_of_stream: false,
    }
}

/// Renders digital silence in every buffer variant (PCM 0, `U8` 0x80, DSD idle
/// 0x69, two bytes per channel per frame).
#[derive(Debug, Clone, Copy, Default)]
pub struct Silence;

impl RenderSource for Silence {
    fn render(&mut self, out: DeviceBuffer<'_>, ctx: &RenderContext<'_>) -> Rendered {
        let ch = usize::from(ctx.config.channels);
        let n = frame_count(out.sample_count(), ctx);
        match out {
            DeviceBuffer::F32(b) => fill(b, ch, n, |_| 0.0),
            DeviceBuffer::F64(b) => fill(b, ch, n, |_| 0.0),
            DeviceBuffer::I16(b) => fill(b, ch, n, |_| 0),
            DeviceBuffer::I24Packed(b) => fill(b, ch, n, |_| [0; 3]),
            DeviceBuffer::I32(b) => fill(b, ch, n, |_| 0),
            DeviceBuffer::U8(b) => fill(b, ch, n, |_| 0x80),
            DeviceBuffer::Dsd(b) => {
                // DSD frames hold 2 bytes per channel (DoP layout)
                let n = frame_count(b.len() / 2, ctx);
                fill(b, 2 * ch, n, |_| 0x69);
                return rendered(n);
            }
        }
        rendered(n)
    }
}

/// Writes `value(frame_offset, channel)` into each sample, mapped to the
/// buffer type the way [`Ramp`] documents (clamp for integers, `/ 2^31` for
/// floats). `Dsd` is not supported.
fn fill_ints(
    out: DeviceBuffer<'_>,
    ch: usize,
    n: usize,
    value: impl Fn(u64, usize) -> i64,
    who: &str,
) {
    const SCALE: f64 = 2_147_483_648.0;
    fn put<T: Copy>(
        out: &mut [T],
        ch: usize,
        n: usize,
        value: &impl Fn(u64, usize) -> i64,
        map: impl Fn(i64) -> T,
    ) {
        if ch == 0 {
            return;
        }
        for (f, frame) in out.chunks_exact_mut(ch).take(n).enumerate() {
            for (c, s) in frame.iter_mut().enumerate() {
                *s = map(value(f as u64, c));
            }
        }
    }
    match out {
        DeviceBuffer::F32(b) => put(b, ch, n, &value, |x| (x as f64 / SCALE) as f32),
        DeviceBuffer::F64(b) => put(b, ch, n, &value, |x| x as f64 / SCALE),
        DeviceBuffer::I16(b) => put(b, ch, n, &value, |x| {
            x.clamp(i16::MIN.into(), i16::MAX.into()) as i16
        }),
        DeviceBuffer::I24Packed(b) => put(b, ch, n, &value, |x| {
            let [lo, mid, hi, _] = (x.clamp(-8_388_608, 8_388_607) as i32).to_le_bytes();
            [lo, mid, hi]
        }),
        DeviceBuffer::I32(b) => put(b, ch, n, &value, |x| {
            x.clamp(i32::MIN.into(), i32::MAX.into()) as i32
        }),
        DeviceBuffer::U8(b) => put(b, ch, n, &value, |x| x.clamp(0, 255) as u8),
        DeviceBuffer::Dsd(_) => panic!("{who} does not support Dsd buffers"),
    }
}

/// Integer ramp: frame `f` carries `start + step * (device_frame_index + f)`.
///
/// Integer buffers (`I16`, `I24Packed`, `I32`, `U8`) receive that integer
/// directly, clamped to the container range (`U8`: 0..=255 as raw byte value,
/// `I24Packed`: 24-bit signed, little-endian). Float buffers receive
/// `value / 2^31`. `Dsd` is not supported.
#[derive(Debug, Clone, Copy)]
pub struct Ramp {
    pub start: i64,
    pub step: i64,
}

impl RenderSource for Ramp {
    fn render(&mut self, out: DeviceBuffer<'_>, ctx: &RenderContext<'_>) -> Rendered {
        let ch = usize::from(ctx.config.channels);
        let n = frame_count(out.sample_count(), ctx);
        let (start, step, base) = (self.start, self.step, ctx.device_frame_index);
        let v = move |f: u64, _c: usize| {
            start.wrapping_add(step.wrapping_mul(base.wrapping_add(f) as i64))
        };
        fill_ints(out, ch, n, v, "Ramp");
        rendered(n)
    }
}

/// Per-channel ramp over the interleaved sample index: channel `c` of
/// absolute frame `f` carries `start + step * (f * channels + c)`, where
/// `f = device_frame_index + frame offset`. Every sample differs, so channel
/// swaps, dropped and repeated samples are all detectable. Clamping and float
/// scaling as in [`Ramp`]; `Dsd` is not supported.
#[derive(Debug, Clone, Copy)]
pub struct ChannelRamp {
    pub start: i64,
    pub step: i64,
}

impl ChannelRamp {
    /// The integer value of channel `c` of absolute frame `f` with `channels` channels.
    pub fn value(&self, f: u64, c: usize, channels: usize) -> i64 {
        let index = f.wrapping_mul(channels as u64).wrapping_add(c as u64) as i64;
        self.start.wrapping_add(self.step.wrapping_mul(index))
    }
}

impl RenderSource for ChannelRamp {
    fn render(&mut self, out: DeviceBuffer<'_>, ctx: &RenderContext<'_>) -> Rendered {
        let ch = usize::from(ctx.config.channels);
        let n = frame_count(out.sample_count(), ctx);
        let (this, base) = (*self, ctx.device_frame_index);
        fill_ints(
            out,
            ch,
            n,
            move |f, c| this.value(base.wrapping_add(f), c, ch),
            "ChannelRamp",
        );
        rendered(n)
    }
}

/// Renders [`ChannelRamp`] `{ start: 1, step: 1 }` content and panics on the
/// first `render` call whose `ctx.device_frame_index >= after_frames`.
///
/// Stateless: the decision depends only on the device frame index, so with
/// a period-aligned `after_frames` the panicking period is exact.
#[derive(Debug, Clone, Copy)]
pub struct Panics {
    pub after_frames: u64,
}

impl RenderSource for Panics {
    fn render(&mut self, out: DeviceBuffer<'_>, ctx: &RenderContext<'_>) -> Rendered {
        if ctx.device_frame_index >= self.after_frames {
            panic!("testkit Panics source: deliberate panic");
        }
        ChannelRamp { start: 1, step: 1 }.render(out, ctx)
    }
}

/// Sine wave of `freq` Hz at sample rate `rate`, amplitude 1.0, phase from
/// `device_frame_index`. Supports `F32` and `F64` buffers only.
#[derive(Debug, Clone, Copy)]
pub struct SineF32 {
    pub freq: f32,
    pub rate: u32,
}

impl RenderSource for SineF32 {
    fn render(&mut self, out: DeviceBuffer<'_>, ctx: &RenderContext<'_>) -> Rendered {
        let ch = usize::from(ctx.config.channels);
        let n = frame_count(out.sample_count(), ctx);
        let (w, base) = (
            std::f64::consts::TAU * f64::from(self.freq) / f64::from(self.rate),
            ctx.device_frame_index,
        );
        let s = move |f: u64| (w * base.wrapping_add(f) as f64).sin();
        match out {
            DeviceBuffer::F32(b) => fill(b, ch, n, |f| s(f) as f32),
            DeviceBuffer::F64(b) => fill(b, ch, n, s),
            _ => panic!("SineF32 supports only F32 and F64 buffers"),
        }
        rendered(n)
    }
}

/// Wraps a source and ends the stream after `frames` frames in total.
///
/// The render whose cumulative count reaches `frames` reports only the
/// remaining frames and `end_of_stream: true`; later renders report 0 frames.
#[derive(Debug, Clone)]
pub struct StopAfter<S> {
    pub inner: S,
    pub frames: u64,
    pub rendered: u64,
}

impl<S> StopAfter<S> {
    pub fn new(inner: S, frames: u64) -> Self {
        StopAfter {
            inner,
            frames,
            rendered: 0,
        }
    }
}

impl<S: RenderSource> RenderSource for StopAfter<S> {
    fn render(&mut self, out: DeviceBuffer<'_>, ctx: &RenderContext<'_>) -> Rendered {
        let remaining = self.frames.saturating_sub(self.rendered);
        let take = remaining.min(u64::from(ctx.frames)) as u32;
        let limited = RenderContext {
            frames: take,
            ..*ctx
        };
        let inner = if take == 0 {
            Rendered {
                frames: 0,
                end_of_stream: false,
            }
        } else {
            self.inner.render(out, &limited)
        };
        let frames = inner.frames.min(take);
        self.rendered += u64::from(frames);
        Rendered {
            frames,
            end_of_stream: inner.end_of_stream || self.rendered >= self.frames,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use vynamp_output_core::{
        ChannelLayout, DeviceId, HostId, PathProperties, SampleFormat, ShareMode, StreamConfig,
    };

    fn config(channels: u16) -> StreamConfig {
        StreamConfig {
            host: HostId::Null,
            device: DeviceId("test".into()),
            mode: ShareMode::Shared,
            format: SampleFormat::F32,
            valid_bits: 32,
            rate: 48_000,
            channels,
            layout: ChannelLayout::unknown(channels),
            period_frames: 4,
            buffer_frames: 8,
            path: PathProperties::unknown(),
        }
    }

    fn ctx(cfg: &StreamConfig, frames: u32, index: u64) -> RenderContext<'_> {
        RenderContext {
            frames,
            device_frame_index: index,
            timing: None,
            config: cfg,
        }
    }

    #[test]
    fn silence_writes_format_silence() {
        let cfg = config(2);
        let mut f = [1.0f32; 4];
        let r = Silence.render(DeviceBuffer::F32(&mut f), &ctx(&cfg, 2, 0));
        assert_eq!((f, r.frames, r.end_of_stream), ([0.0; 4], 2, false));
        let mut u = [0u8; 6];
        // buffer holds 3 frames; only the 2 requested are written
        let r = Silence.render(DeviceBuffer::U8(&mut u), &ctx(&cfg, 2, 0));
        assert_eq!((u, r.frames), ([0x80, 0x80, 0x80, 0x80, 0, 0], 2));
        // DSD: 2 bytes per channel per frame; buffer of 3 frames, 2 requested
        let mut d = [0u8; 12];
        let r = Silence.render(DeviceBuffer::Dsd(&mut d), &ctx(&cfg, 2, 0));
        assert_eq!(
            (d, r.frames),
            (
                [0x69, 0x69, 0x69, 0x69, 0x69, 0x69, 0x69, 0x69, 0, 0, 0, 0],
                2
            )
        );
    }

    #[test]
    fn ramp_integer_float_and_clamp() {
        let cfg = config(2);
        let mut ramp = Ramp {
            start: 100,
            step: 10,
        };
        let mut i = [0i16; 6];
        let r = ramp.render(DeviceBuffer::I16(&mut i), &ctx(&cfg, 3, 1));
        assert_eq!((i, r.frames), ([110, 110, 120, 120, 130, 130], 3));
        let mut p = [[0u8; 3]; 2];
        Ramp { start: -1, step: 0 }.render(DeviceBuffer::I24Packed(&mut p), &ctx(&cfg, 1, 0));
        assert_eq!(p, [[0xFF; 3]; 2]);
        let mut c = [0i16; 2];
        Ramp {
            start: 1 << 40,
            step: 0,
        }
        .render(DeviceBuffer::I16(&mut c), &ctx(&cfg, 1, 0));
        assert_eq!(c, [i16::MAX; 2]);
        let mut f = [0f32; 2];
        Ramp {
            start: 1 << 30,
            step: 0,
        }
        .render(DeviceBuffer::F32(&mut f), &ctx(&cfg, 1, 0));
        assert_eq!(f, [0.5; 2]);
    }

    #[test]
    fn sine_is_deterministic_and_interleaved() {
        let cfg = config(2);
        let mut sine = SineF32 {
            freq: 12_000.0,
            rate: 48_000,
        };
        let mut a = [0f32; 8];
        sine.render(DeviceBuffer::F32(&mut a), &ctx(&cfg, 4, 0));
        // 12 kHz at 48 kHz: 0, 1, 0, -1 per frame, same on both channels
        let expect = [0.0, 0.0, 1.0, 1.0, 0.0, 0.0, -1.0, -1.0];
        for (x, e) in a.iter().zip(expect) {
            assert!((x - e).abs() < 1e-6, "{a:?}");
        }
        let mut b = [0f32; 8];
        sine.render(DeviceBuffer::F32(&mut b), &ctx(&cfg, 4, 0));
        assert_eq!(a, b);
    }

    #[test]
    fn stop_after_ends_on_the_reaching_render() {
        let cfg = config(1);
        let mut src = StopAfter::new(Ramp { start: 1, step: 1 }, 5);
        let mut buf = [0i32; 4];
        let r = src.render(DeviceBuffer::I32(&mut buf), &ctx(&cfg, 4, 0));
        assert_eq!((r.frames, r.end_of_stream), (4, false));
        let r = src.render(DeviceBuffer::I32(&mut buf), &ctx(&cfg, 4, 4));
        assert_eq!((r.frames, r.end_of_stream), (1, true));
        assert_eq!(buf[0], 5);
        let r = src.render(DeviceBuffer::I32(&mut buf), &ctx(&cfg, 4, 8));
        assert_eq!((r.frames, r.end_of_stream), (0, true));
    }

    #[test]
    fn channel_ramp_uses_the_interleaved_index() {
        let cfg = config(2);
        let mut src = ChannelRamp { start: 10, step: 3 };
        let mut i = [0i32; 6];
        let r = src.render(DeviceBuffer::I32(&mut i), &ctx(&cfg, 3, 4));
        // frame 4 starts at interleaved index 8
        assert_eq!((i, r.frames), ([34, 37, 40, 43, 46, 49], 3));
        let mut p = [[0u8; 3]; 2];
        ChannelRamp { start: -2, step: 1 }
            .render(DeviceBuffer::I24Packed(&mut p), &ctx(&cfg, 1, 0));
        assert_eq!(p, [[0xFE, 0xFF, 0xFF], [0xFF, 0xFF, 0xFF]]);
        let mut c = [0i16; 2];
        ChannelRamp {
            start: 40_000,
            step: -80_000,
        }
        .render(DeviceBuffer::I16(&mut c), &ctx(&cfg, 1, 0));
        assert_eq!(c, [i16::MAX, i16::MIN]);
        let mut f = [0f32; 2];
        ChannelRamp {
            start: 1 << 30,
            step: 1 << 29,
        }
        .render(DeviceBuffer::F32(&mut f), &ctx(&cfg, 1, 0));
        assert_eq!(f, [0.5, 0.75]);
    }

    #[test]
    fn panics_source_renders_then_panics() {
        let cfg = config(1);
        let mut src = Panics { after_frames: 4 };
        let mut b = [0i32; 4];
        let r = src.render(DeviceBuffer::I32(&mut b), &ctx(&cfg, 4, 0));
        assert_eq!((b, r.frames), ([1, 2, 3, 4], 4));
        let caught = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            src.render(DeviceBuffer::I32(&mut b), &ctx(&cfg, 4, 4))
        }));
        assert!(caught.is_err());
    }
}
