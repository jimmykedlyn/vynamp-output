//! DoP (DSD over PCM) packer (spec §5.9).
//!
//! RT module. Each PCM frame carries 16 DSD bits per channel in a 24-bit
//! word: bits 23..16 are the marker (`0x05` and `0xFA`, alternating per
//! frame, the same for every channel of a frame), bits 15..0 the two DSD
//! bytes of that channel, the first in time in bits 15..8. The PCM rate is
//! `dsd_rate / 16` (DSD64 2.8224 MHz → 176.4 kHz).
//!
//! The source renders raw DSD bytes ([`DeviceBuffer::Dsd`]): per PCM frame
//! `2 * channels` bytes, channel by channel, each channel's two bytes in time
//! order. The driver stages them in a pre-allocated scratch, fills the
//! unrendered tail with the idle byte `0x69` and packs the whole period, so
//! the marker phase runs on across periods, underruns and silence. The phase
//! (`false` = next marker `0x05`) lives in the driver for the stream's life.
//!
//! Output containers: `I24Packed` (little-endian `[lo, mid, hi]`) and `I32`
//! (MSB-justified: `word << 8`, the low byte 0).

use crate::config::{EndpointVolume, PathProperties, Tri};
use crate::error::Error;
use crate::render::DeviceBuffer;

/// Marker of frames with `phase == false`.
pub(crate) const MARKER_A: u8 = 0x05;
/// Marker of frames with `phase == true`.
pub(crate) const MARKER_B: u8 = 0xFA;
/// DSD idle (silence) byte.
pub(crate) const IDLE: u8 = 0x69;
/// DSD bytes per channel per PCM frame.
pub(crate) const BYTES_PER_FRAME: usize = 2;

/// PCM carrier rate of DoP for a DSD bit rate: `dsd_rate / 16`, or `None`
/// when `dsd_rate` is zero or not a multiple of 16.
pub fn dop_pcm_rate(dsd_rate: u32) -> Option<u32> {
    (dsd_rate != 0 && dsd_rate.is_multiple_of(16)).then_some(dsd_rate / 16)
}

/// Whether a path may carry DoP, for any backend to call at open: the OS
/// mixer must be bypassed (a mixer turns DoP into noise:
/// `ExclusiveUnavailable`), endpoint volume unity or unknown
/// (`NotUnity` is `ExclusiveUnavailable`; with `Unknown` the stream opens
/// and the backend emits [`Event::DopVolumeUnverified`](crate::Event::DopVolumeUnverified)),
/// and no library conversion (`FormatUnsupported`).
pub fn check_dop_path(path: &PathProperties) -> Result<(), Error> {
    if path.os_mixer_bypassed != Tri::Yes {
        return Err(Error::ExclusiveUnavailable {
            reason: "dop requires a path that bypasses the os mixer",
        });
    }
    if let EndpointVolume::NotUnity(_) = path.endpoint_volume {
        return Err(Error::ExclusiveUnavailable {
            reason: "dop requires unity endpoint volume",
        });
    }
    if path.library_conversion.is_some() {
        return Err(Error::FormatUnsupported { nearest: None });
    }
    Ok(())
}

/// Returns the marker for the current frame and advances the phase.
fn next_marker(phase: &mut bool) -> u8 {
    let m = if *phase { MARKER_B } else { MARKER_A };
    *phase = !*phase;
    m
}

/// The 24-bit DoP word in a `u32`.
fn word(marker: u8, first: u8, second: u8) -> u32 {
    u32::from_be_bytes([0, marker, first, second])
}

/// Writes `frames` of DoP into `out` (frames of `channels` samples), taking
/// each channel's byte pair from `pair(frame, channel)`.
fn write_frames(
    out: &mut DeviceBuffer<'_>,
    from_frame: usize,
    channels: usize,
    phase: &mut bool,
    mut pair: impl FnMut(usize, usize) -> (u8, u8),
) {
    match out {
        DeviceBuffer::I24Packed(o) => {
            for (f, frame) in o.chunks_exact_mut(channels).enumerate().skip(from_frame) {
                let m = next_marker(phase);
                for (c, s) in frame.iter_mut().enumerate() {
                    let (first, second) = pair(f, c);
                    *s = [second, first, m];
                }
            }
        }
        DeviceBuffer::I32(o) => {
            for (f, frame) in o.chunks_exact_mut(channels).enumerate().skip(from_frame) {
                let m = next_marker(phase);
                for (c, s) in frame.iter_mut().enumerate() {
                    let (first, second) = pair(f, c);
                    // MSB-justified; the marker bit 31 makes the i32 negative
                    *s = (word(m, first, second) << 8) as i32;
                }
            }
        }
        _ => {}
    }
}

fn invalid(what: &'static str) -> Error {
    Error::InvalidRequest { what }
}

/// Packs one period of DSD bytes (`dsd`, layout in the module docs) into
/// `out`, advancing `phase` once per frame.
///
/// Errors (`InvalidRequest`, nothing written, phase unchanged): `out` not
/// `I24Packed`/`I32`, zero channels, or sizes that do not match (`dsd` must
/// hold exactly `2 * channels` bytes per frame of `out`).
pub(crate) fn pack(
    dsd: &[u8],
    out: &mut DeviceBuffer<'_>,
    channels: usize,
    phase: &mut bool,
) -> Result<(), Error> {
    if !matches!(out, DeviceBuffer::I24Packed(_) | DeviceBuffer::I32(_)) {
        return Err(invalid("dop target must be I24Packed or I32"));
    }
    let samples = out.sample_count();
    if channels == 0
        || !samples.is_multiple_of(channels)
        || Some(dsd.len()) != samples.checked_mul(BYTES_PER_FRAME)
    {
        return Err(invalid("dop buffer sizes do not match"));
    }
    let stride = channels * BYTES_PER_FRAME;
    write_frames(out, 0, channels, phase, |f, c| {
        let at = f * stride + c * BYTES_PER_FRAME;
        match dsd.get(at..at + BYTES_PER_FRAME) {
            Some(&[first, second]) => (first, second),
            _ => (IDLE, IDLE),
        }
    });
    Ok(())
}

/// DoP silence in a PCM container: idle words (`0x69 0x69`) from
/// `from_frame` to the end, with the marker phase continuing. Buffers other
/// than `I24Packed`/`I32` are left untouched.
pub(crate) fn fill_idle(
    out: &mut DeviceBuffer<'_>,
    from_frame: usize,
    channels: usize,
    phase: &mut bool,
) {
    if channels != 0 {
        write_frames(out, from_frame, channels, phase, |_, _| (IDLE, IDLE));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{EndpointVolume, PathProperties, Tri};
    use crate::error::Error;
    use crate::format::SampleFormat;
    use crate::render::DeviceBuffer;
    use crate::request::ConversionSpec;
    use crate::silence::fill_silence;

    /// Two stereo frames, DSD bytes in time order per channel.
    const DSD: [u8; 8] = [0x12, 0x34, 0xAB, 0xCD, 0x01, 0x02, 0xFE, 0xFF];

    #[test]
    fn packs_i24_packed_little_endian_with_markers() {
        let mut out = [[0u8; 3]; 4];
        let mut phase = false;
        pack(&DSD, &mut DeviceBuffer::I24Packed(&mut out), 2, &mut phase).unwrap();
        // word = marker << 16 | first byte << 8 | second byte, stored [lo, mid, hi]
        assert_eq!(
            out,
            [
                [0x34, 0x12, 0x05],
                [0xCD, 0xAB, 0x05],
                [0x02, 0x01, 0xFA],
                [0xFF, 0xFE, 0xFA]
            ]
        );
        assert!(!phase, "two frames: next marker is 0x05 again");
    }

    #[test]
    fn packs_i32_msb_justified() {
        let mut out = [0i32; 4];
        let mut phase = false;
        pack(&DSD, &mut DeviceBuffer::I32(&mut out), 2, &mut phase).unwrap();
        let words: Vec<u32> = out.iter().map(|&w| w as u32).collect();
        assert_eq!(words, [0x0512_3400, 0x05AB_CD00, 0xFA01_0200, 0xFAFE_FF00]);
    }

    #[test]
    fn phase_carries_across_calls() {
        let mut phase = false;
        let mut out = [0i32; 3];
        // mono, 3 frames, then 3 more: markers 05 FA 05 | FA 05 FA
        pack(
            &[1, 2, 3, 4, 5, 6],
            &mut DeviceBuffer::I32(&mut out),
            1,
            &mut phase,
        )
        .unwrap();
        assert_eq!(out.map(|w| (w as u32) >> 24), [0x05, 0xFA, 0x05]);
        assert!(phase);
        pack(
            &[1, 2, 3, 4, 5, 6],
            &mut DeviceBuffer::I32(&mut out),
            1,
            &mut phase,
        )
        .unwrap();
        assert_eq!(out.map(|w| (w as u32) >> 24), [0xFA, 0x05, 0xFA]);
        assert!(!phase);
    }

    #[test]
    fn rejects_other_targets_and_size_mismatch() {
        let mut phase = false;
        let mut f = [0.0f32; 4];
        assert!(pack(&DSD, &mut DeviceBuffer::F32(&mut f), 2, &mut phase).is_err());
        let mut short = [0i32; 2];
        assert!(pack(&DSD, &mut DeviceBuffer::I32(&mut short), 2, &mut phase).is_err());
        let mut out = [0i32; 4];
        assert!(pack(&DSD, &mut DeviceBuffer::I32(&mut out), 0, &mut phase).is_err());
        assert!(!phase, "a rejected call does not advance the phase");
    }

    #[test]
    fn dsd_bytes_per_frame_comes_from_the_format() {
        assert_eq!(DOP.dsd_bytes_per_frame(), Some(2));
        assert_eq!(
            SampleFormat::I32 { valid_bits: 24 }.dsd_bytes_per_frame(),
            None
        );
        // a Dsd buffer with a non-DSD format is filled entirely (never stale)
        let mut b = [0x12u8; 4];
        fill_silence(
            &mut DeviceBuffer::Dsd(&mut b),
            1,
            1,
            SampleFormat::I16,
            &mut false,
        );
        assert_eq!(b, [IDLE; 4]);
    }

    #[test]
    fn pcm_rate_mapping() {
        assert_eq!(dop_pcm_rate(2_822_400), Some(176_400));
        assert_eq!(dop_pcm_rate(5_644_800), Some(352_800));
        assert_eq!(dop_pcm_rate(11_289_600), Some(705_600));
        assert_eq!(dop_pcm_rate(15), None);
        assert_eq!(dop_pcm_rate(0), None);
    }

    const DOP: SampleFormat = SampleFormat::Dop {
        dsd_rate: 2_822_400,
    };

    #[test]
    fn silence_in_dsd_buffer_is_idle_bytes_without_phase_change() {
        let mut b = [0x12u8; 8];
        let mut phase = true;
        fill_silence(&mut DeviceBuffer::Dsd(&mut b), 1, 2, DOP, &mut phase);
        assert_eq!(b, [0x12, 0x12, 0x12, 0x12, 0x69, 0x69, 0x69, 0x69]);
        assert!(phase, "the packer advances the phase, not the byte fill");
    }

    #[test]
    fn silence_in_pcm_container_is_idle_words_with_markers() {
        let mut out = [7i32; 6];
        let mut phase = true;
        fill_silence(&mut DeviceBuffer::I32(&mut out), 1, 2, DOP, &mut phase);
        let words: Vec<u32> = out.iter().map(|&w| w as u32).collect();
        assert_eq!(
            words,
            [7, 7, 0xFA69_6900, 0xFA69_6900, 0x0569_6900, 0x0569_6900]
        );
        assert!(phase, "two idle frames from 0xFA: next is 0xFA");
        let mut p = [[0u8; 3]; 2];
        let mut phase = false;
        fill_silence(&mut DeviceBuffer::I24Packed(&mut p), 0, 1, DOP, &mut phase);
        assert_eq!(p, [[0x69, 0x69, 0x05], [0x69, 0x69, 0xFA]]);
        assert!(!phase);
    }

    fn path(mixer: Tri, volume: EndpointVolume) -> PathProperties {
        let mut p = PathProperties::unknown();
        p.os_mixer_bypassed = mixer;
        p.endpoint_volume = volume;
        p
    }

    #[test]
    fn path_check() {
        assert_eq!(
            check_dop_path(&path(Tri::Yes, EndpointVolume::Unity)),
            Ok(())
        );
        assert_eq!(
            check_dop_path(&path(Tri::Yes, EndpointVolume::Unknown)),
            Ok(())
        );
        for p in [
            path(Tri::No, EndpointVolume::Unity),
            path(Tri::Unknown, EndpointVolume::Unity),
            path(Tri::Yes, EndpointVolume::NotUnity(0.5)),
        ] {
            assert!(
                matches!(check_dop_path(&p), Err(Error::ExclusiveUnavailable { .. })),
                "{p:?}"
            );
        }
        let mut p = path(Tri::Yes, EndpointVolume::Unity);
        p.library_conversion = Some(ConversionSpec {
            from: DOP,
            to: SampleFormat::I24Packed,
        });
        assert_eq!(
            check_dop_path(&p),
            Err(Error::FormatUnsupported { nearest: None })
        );
    }
}
