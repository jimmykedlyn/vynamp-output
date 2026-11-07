//! Format-correct silence for the unfilled tail of a device buffer.

use crate::dop::{IDLE, fill_idle};
use crate::format::SampleFormat;
use crate::render::DeviceBuffer;

/// Fills `buf` from frame `from_frame` (inclusive) to the end with silence.
///
/// PCM writes 0, `U8` writes `0x80`. A `from_frame` past the end is a no-op.
///
/// DoP: a `Dsd` buffer (the source's DSD bytes,
/// `format.dsd_bytes_per_frame() * channels` per frame) gets the idle byte
/// `0x69` and `dop_phase` is left alone, because the
/// packer advances it when it packs the period. A PCM container with
/// `format` `Dop` (`I24Packed`/`I32`) gets idle DoP words whose markers
/// continue from `dop_phase`, advancing it per frame.
pub(crate) fn fill_silence(
    buf: &mut DeviceBuffer<'_>,
    from_frame: usize,
    channels: usize,
    format: SampleFormat,
    dop_phase: &mut bool,
) {
    let start = from_frame.saturating_mul(channels);
    if let SampleFormat::Dop { .. } = format
        && !matches!(buf, DeviceBuffer::Dsd(_))
    {
        fill_idle(buf, from_frame, channels, dop_phase);
        return;
    }
    match buf {
        DeviceBuffer::F32(b) => tail(b, start, 0.0),
        DeviceBuffer::F64(b) => tail(b, start, 0.0),
        DeviceBuffer::I16(b) => tail(b, start, 0),
        DeviceBuffer::I24Packed(b) => tail(b, start, [0; 3]),
        DeviceBuffer::I32(b) => tail(b, start, 0),
        DeviceBuffer::U8(b) => tail(b, start, 0x80),
        // bytes per frame from the format; a non-DSD format here is a
        // mismatch: idle the whole buffer rather than leave stale bytes
        DeviceBuffer::Dsd(b) => match format.dsd_bytes_per_frame() {
            Some(n) => tail(b, start.saturating_mul(n), IDLE),
            None => b.fill(IDLE),
        },
    }
}

fn tail<T: Copy>(samples: &mut [T], start: usize, value: T) {
    if let Some(rest) = samples.get_mut(start..) {
        rest.fill(value);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PCM: SampleFormat = SampleFormat::I16;

    #[test]
    fn pcm_formats_fill_zero_from_frame() {
        let mut f32s = [1.0f32; 6];
        fill_silence(&mut DeviceBuffer::F32(&mut f32s), 1, 2, PCM, &mut false);
        assert_eq!(f32s, [1.0, 1.0, 0.0, 0.0, 0.0, 0.0]);
        let mut f64s = [1.0f64; 4];
        fill_silence(&mut DeviceBuffer::F64(&mut f64s), 0, 2, PCM, &mut false);
        assert_eq!(f64s, [0.0; 4]);
        let mut i16s = [7i16; 4];
        fill_silence(&mut DeviceBuffer::I16(&mut i16s), 1, 2, PCM, &mut false);
        assert_eq!(i16s, [7, 7, 0, 0]);
        let mut i24 = [[7u8; 3]; 4];
        fill_silence(
            &mut DeviceBuffer::I24Packed(&mut i24),
            1,
            2,
            PCM,
            &mut false,
        );
        assert_eq!(i24, [[7; 3], [7; 3], [0; 3], [0; 3]]);
        let mut i32s = [7i32; 4];
        fill_silence(&mut DeviceBuffer::I32(&mut i32s), 1, 2, PCM, &mut false);
        assert_eq!(i32s, [7, 7, 0, 0]);
    }

    #[test]
    fn u8_silence_is_midpoint() {
        let mut b = [0u8; 4];
        fill_silence(
            &mut DeviceBuffer::U8(&mut b),
            1,
            2,
            SampleFormat::U8,
            &mut false,
        );
        assert_eq!(b, [0, 0, 0x80, 0x80]);
    }

    #[test]
    fn from_frame_past_end_is_noop() {
        let mut b = [5i16; 4];
        fill_silence(&mut DeviceBuffer::I16(&mut b), 2, 2, PCM, &mut false);
        fill_silence(&mut DeviceBuffer::I16(&mut b), 99, 2, PCM, &mut false);
        fill_silence(
            &mut DeviceBuffer::I16(&mut b),
            usize::MAX,
            2,
            PCM,
            &mut false,
        );
        assert_eq!(b, [5; 4]);
    }
}
