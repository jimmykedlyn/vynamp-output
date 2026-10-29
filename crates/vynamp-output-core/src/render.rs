//! The real-time render contract: [`RenderSource`], [`DeviceBuffer`] and friends.
//!
//! Everything here is called on the audio thread. No allocation, locking,
//! blocking, I/O or logging is allowed in `RenderSource::render`.

use crate::config::StreamConfig;

/// Produces audio on the device's real-time thread.
pub trait RenderSource: Send + 'static {
    /// Called on the audio thread. Must not allocate, lock, block, or do I/O/logging.
    ///
    /// Fill `out` (interleaved, in the stream's real format) with up to
    /// `ctx.frames` frames and report how many were written. The library
    /// fills the remainder with the format's silence; it never repeats samples.
    ///
    /// For a DoP stream `out` is [`DeviceBuffer::Dsd`] (raw DSD bytes, layout
    /// documented there) and frames are DoP (PCM carrier) frames.
    fn render(&mut self, out: DeviceBuffer<'_>, ctx: &RenderContext<'_>) -> Rendered;
}

/// Interleaved device samples in the stream's real container format.
#[derive(Debug)]
pub enum DeviceBuffer<'a> {
    F32(&'a mut [f32]),
    F64(&'a mut [f64]),
    I16(&'a mut [i16]),
    /// 24-bit samples, little-endian `[lo, mid, hi]`, two's complement.
    I24Packed(&'a mut [[u8; 3]]),
    /// Shared by `I24In32` and `I32 { valid_bits }`.
    I32(&'a mut [i32]),
    /// Unsigned 8-bit, midpoint `0x80`.
    U8(&'a mut [u8]),
    /// Raw DSD bytes, packed into DoP by the library.
    ///
    /// Per DoP frame (one frame at the PCM carrier rate `dsd_rate / 16`):
    /// 2 bytes per channel, grouped by channel (`[c0 first, c0 second, c1
    /// first, c1 second, …]`), each channel's bytes in time order. Within a
    /// byte the MSB is the earliest DSD bit (DFF order); sources reading DSF
    /// (LSB first) must bit-reverse each byte. The idle (silence) byte is
    /// `0x69`.
    Dsd(&'a mut [u8]),
}

impl DeviceBuffer<'_> {
    /// Number of samples (all channels) in the buffer; for `Dsd`, the number
    /// of bytes (2 per channel per frame for DoP).
    pub fn sample_count(&self) -> usize {
        match self {
            DeviceBuffer::F32(b) => b.len(),
            DeviceBuffer::F64(b) => b.len(),
            DeviceBuffer::I16(b) => b.len(),
            DeviceBuffer::I24Packed(b) => b.len(),
            DeviceBuffer::I32(b) => b.len(),
            DeviceBuffer::U8(b) | DeviceBuffer::Dsd(b) => b.len(),
        }
    }
}

/// What the source is being asked to render.
#[derive(Debug, Clone, Copy)]
pub struct RenderContext<'a> {
    /// Frames requested for this call (DoP: carrier frames, each holding 2
    /// DSD bytes per channel).
    pub frames: u32,
    /// Monotonic count of all frames delivered to the device so far (content + silence).
    pub device_frame_index: u64,
    pub timing: Option<PresentationTiming>,
    pub config: &'a StreamConfig,
}

/// Result of one `render` call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rendered {
    /// Frames actually written (the rest is filled with silence); counted
    /// like [`RenderContext::frames`].
    pub frames: u32,
    pub end_of_stream: bool,
}

/// "Frame `device_frame` leaves the converter at `at`."
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PresentationTiming {
    pub device_frame: u64,
    pub at: MonotonicNanos,
    pub latency_frames: u64,
    pub source: TimingSource,
}

/// Nanoseconds on the OS monotonic clock.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct MonotonicNanos(pub u64);

/// Whether the OS reported the timing or the library estimated it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TimingSource {
    Reported,
    Estimated,
}
