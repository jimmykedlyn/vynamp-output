//! DoP end to end: the Capture host records the packed 24-bit words exactly
//! as the device thread delivered them (I32, MSB-justified, little-endian),
//! with the 0x05/0xFA marker continuous across periods, underrun and
//! silence; opening DoP is refused on a mixed or shared path or with a
//! library conversion.
//!
//! End-to-end coverage uses the I32 container (the Null/Capture DoP
//! container); the I24Packed container is covered at unit level (`dop.rs`)
//! and through the driver (`driver.rs`). The period is odd, so a marker
//! phase reset at a period boundary would be caught.

use std::path::PathBuf;

use vynamp_output_core::*;

const DSD64: u32 = 2_822_400;
const DOP64: SampleFormat = SampleFormat::Dop { dsd_rate: DSD64 };
const PERIOD: u32 = 7;
const CHANNELS: u16 = 2;
/// Call index whose render is short (underrun), and its frame count.
const SHORT_CALL: u64 = 1;
const SHORT_FRAMES: u32 = 3;

fn request(format: SampleFormat, rate: u32) -> StreamRequest {
    StreamRequest {
        device: DeviceSelector::Default(Role::Multimedia),
        mode: ShareMode::Exclusive,
        format,
        rate,
        channels: CHANNELS,
        layout: ChannelLayout::unknown(CHANNELS),
        buffer: BufferRequest::Frames(PERIOD),
        on_unavailable: OnUnavailable::Error,
        conversion: Conversion::Forbid,
        allow_rate_change_of_device: false,
    }
}

/// The DSD byte at PCM frame `f`, channel `c`, byte `k` (0 = first in time).
fn dsd_byte(f: u64, c: usize, k: usize) -> u8 {
    (f as u8).wrapping_mul(4).wrapping_add((c * 2 + k) as u8) ^ 0x30
}

/// Writes `dsd_byte` patterns; call `SHORT_CALL` reports `SHORT_FRAMES`.
struct DsdPattern {
    calls: u64,
}

impl RenderSource for DsdPattern {
    fn render(&mut self, out: DeviceBuffer<'_>, ctx: &RenderContext<'_>) -> Rendered {
        let DeviceBuffer::Dsd(b) = out else {
            panic!("DoP streams render DSD bytes")
        };
        let ch = usize::from(ctx.config.channels);
        for (n, frame) in b.chunks_exact_mut(2 * ch).enumerate() {
            for (c, pair) in frame.as_chunks_mut::<2>().0.iter_mut().enumerate() {
                for (k, byte) in pair.iter_mut().enumerate() {
                    *byte = dsd_byte(ctx.device_frame_index + n as u64, c, k);
                }
            }
        }
        let call = self.calls;
        self.calls += 1;
        Rendered {
            frames: if call == SHORT_CALL {
                SHORT_FRAMES
            } else {
                ctx.frames
            },
            end_of_stream: false,
        }
    }
}

/// Reports end of stream at once: everything delivered is DoP silence.
struct EndsAtOnce;

impl RenderSource for EndsAtOnce {
    fn render(&mut self, _out: DeviceBuffer<'_>, _ctx: &RenderContext<'_>) -> Rendered {
        Rendered {
            frames: 0,
            end_of_stream: true,
        }
    }
}

fn temp_path(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "vynamp-output-dop-{}-{name}.raw",
        std::process::id()
    ))
}

/// Runs `periods` periods through Capture on a virtual clock; returns the
/// recorded little-endian `u32` words, the manifest and the stream.
fn record(name: &str, periods: u64, src: impl RenderSource) -> (Vec<u32>, String, Stream) {
    let path = temp_path(name);
    let clock = VirtualClockHandle::new();
    let host = CaptureHost::new(path.clone()).with_clock(NullClock::Virtual(clock.clone()));
    let mut s = host
        .open(&request(DOP64, 176_400), Box::new(src))
        .expect("open DoP capture");
    s.start().unwrap();
    clock.advance(periods);
    s.stop().unwrap();
    let bytes = std::fs::read(&path).expect("capture file");
    let mut m = path.clone().into_os_string();
    m.push(".manifest");
    let manifest = std::fs::read_to_string(&m).expect("manifest");
    let _ = std::fs::remove_file(&path);
    let _ = std::fs::remove_file(&m);
    let words = bytes
        .as_chunks::<4>()
        .0
        .iter()
        .map(|&w| u32::from_le_bytes(w))
        .collect();
    (words, manifest, s)
}

fn marker(frame: u64) -> u32 {
    if frame.is_multiple_of(2) { 0x05 } else { 0xFA }
}

#[test]
fn markers_continue_across_periods_and_an_underrun() {
    let (words, manifest, s) = record("cont", 3, DsdPattern { calls: 0 });
    let ch = usize::from(CHANNELS);
    assert_eq!(words.len(), 3 * PERIOD as usize * ch);
    for (i, &w) in words.iter().enumerate() {
        let f = (i / ch) as u64;
        let c = i % ch;
        let silent =
            f / u64::from(PERIOD) == SHORT_CALL && f % u64::from(PERIOD) >= u64::from(SHORT_FRAMES);
        let (b0, b1) = if silent {
            (0x69, 0x69)
        } else {
            (dsd_byte(f, c, 0), dsd_byte(f, c, 1))
        };
        let expect = (marker(f) << 24) | (u32::from(b0) << 16) | (u32::from(b1) << 8);
        assert_eq!(w, expect, "frame {f} channel {c}: {w:#010x}");
    }
    assert_eq!(s.counters().source_underruns, 1);
    assert_eq!(s.config().format, DOP64);
    assert_eq!(s.config().rate, 176_400);
    assert_eq!(s.config().valid_bits, 24);
    assert_eq!(
        manifest,
        "format=Dop2822400\ncontainer_bytes=4\nvalid_bits=24\nbyte_order=little\n\
         rate=176400\nchannels=2\nperiod_frames=7\nframes=21\noverruns=0\n"
    );
}

#[test]
fn silence_is_idle_pattern_with_alternating_markers() {
    let (words, _, _) = record("silence", 3, EndsAtOnce);
    assert_eq!(words.len(), 3 * PERIOD as usize * usize::from(CHANNELS));
    for (i, &w) in words.iter().enumerate() {
        let f = (i / usize::from(CHANNELS)) as u64;
        assert_eq!(w, (marker(f) << 24) | 0x0069_6900, "frame {f}");
    }
}

#[test]
fn pcm_rate_is_dsd_rate_over_16() {
    assert_eq!(dop_pcm_rate(2_822_400), Some(176_400));
    assert_eq!(dop_pcm_rate(5_644_800), Some(352_800));
    assert_eq!(dop_pcm_rate(11_289_600), Some(705_600));
    assert_eq!(dop_pcm_rate(0), None);
    assert_eq!(dop_pcm_rate(2_822_401), None);
}

#[test]
fn null_opens_dsd64_128_256_at_their_carrier_rates() {
    for (dsd_rate, rate) in [
        (2_822_400, 176_400),
        (5_644_800, 352_800),
        (11_289_600, 705_600),
    ] {
        let format = SampleFormat::Dop { dsd_rate };
        let mut s = NullHost::new(NullClock::Real)
            .open(&request(format, rate), Box::new(EndsAtOnce))
            .unwrap_or_else(|e| panic!("DSD {dsd_rate}: {e}"));
        assert_eq!((s.config().format, s.config().rate), (format, rate));
        assert_eq!(s.config().path.os_mixer_bypassed, Tri::Yes);
        assert_eq!(s.config().path.library_conversion, None);
        s.stop().unwrap();
    }
}

#[test]
fn dop_rate_must_match_dsd_rate() {
    let e = NullHost::new(NullClock::Real)
        .open(&request(DOP64, 352_800), Box::new(EndsAtOnce))
        .unwrap_err();
    assert_eq!(
        e,
        Error::InvalidRequest {
            what: "dop rate differs from dsd rate / 16"
        }
    );
}

#[test]
fn refused_on_a_mixed_or_unknown_path() {
    for mixer in [Tri::No, Tri::Unknown] {
        let e = NullHost::new(NullClock::Real)
            .with_os_mixer_bypassed(mixer)
            .open(&request(DOP64, 176_400), Box::new(EndsAtOnce))
            .unwrap_err();
        assert!(
            matches!(e, Error::ExclusiveUnavailable { .. }),
            "{mixer:?}: {e:?}"
        );
        let e = CaptureHost::new(temp_path("mixed"))
            .with_os_mixer_bypassed(mixer)
            .open(&request(DOP64, 176_400), Box::new(EndsAtOnce))
            .unwrap_err();
        assert!(matches!(e, Error::ExclusiveUnavailable { .. }), "{e:?}");
    }
    // PCM is still fine on a mixed path
    let mut s = NullHost::new(NullClock::Real)
        .with_os_mixer_bypassed(Tri::No)
        .open(&request(SampleFormat::I16, 48_000), Box::new(EndsAtOnce))
        .unwrap();
    assert_eq!(s.config().path.os_mixer_bypassed, Tri::No);
    s.stop().unwrap();
}

#[test]
fn refused_when_endpoint_volume_is_not_unity() {
    let e = NullHost::new(NullClock::Real)
        .with_endpoint_volume(EndpointVolume::NotUnity(0.5))
        .open(&request(DOP64, 176_400), Box::new(EndsAtOnce))
        .unwrap_err();
    assert!(matches!(e, Error::ExclusiveUnavailable { .. }), "{e:?}");
}

#[test]
fn unknown_endpoint_volume_opens_with_a_warning() {
    let mut s = NullHost::new(NullClock::Real)
        .with_endpoint_volume(EndpointVolume::Unknown)
        .open(&request(DOP64, 176_400), Box::new(EndsAtOnce))
        .unwrap();
    assert_eq!(s.config().path.endpoint_volume, EndpointVolume::Unknown);
    // queued at open, before the first callback
    assert_eq!(s.events().try_recv(), Ok(Event::DopVolumeUnverified));
    assert!(s.events().try_recv().is_err());
    s.stop().unwrap();
    // unity volume: no warning
    let mut s = NullHost::new(NullClock::Real)
        .open(&request(DOP64, 176_400), Box::new(EndsAtOnce))
        .unwrap();
    assert!(s.events().try_recv().is_err());
    s.stop().unwrap();
}

#[test]
fn refused_on_a_shared_path() {
    let shared_dop = vec![CapabilityEntry {
        mode: ShareMode::Shared,
        format: DOP64,
        rates: vec![176_400],
        channels: vec![2],
        origin: CapabilityOrigin::Advertised,
    }];
    let mut req = request(DOP64, 176_400);
    req.mode = ShareMode::Shared;
    let e = NullHost::with_capabilities(NullClock::Real, shared_dop.clone())
        .open(&req, Box::new(EndsAtOnce))
        .unwrap_err();
    assert!(matches!(e, Error::ExclusiveUnavailable { .. }), "{e:?}");
    let e = CaptureHost::with_capabilities(temp_path("shared"), shared_dop)
        .open(&req, Box::new(EndsAtOnce))
        .unwrap_err();
    assert!(matches!(e, Error::ExclusiveUnavailable { .. }), "{e:?}");
    // a shared virtual stream reports the mixer as not bypassed
    let mut pcm = request(SampleFormat::I16, 48_000);
    pcm.mode = ShareMode::Shared;
    let mut s = NullHost::new(NullClock::Real)
        .open(&pcm, Box::new(EndsAtOnce))
        .unwrap();
    assert_eq!(s.config().path.os_mixer_bypassed, Tri::No);
    s.stop().unwrap();
}

#[test]
fn dsd128_downgrades_to_dsd64_on_a_dsd64_only_device() {
    let dsd64_only = vec![CapabilityEntry {
        mode: ShareMode::Exclusive,
        format: DOP64,
        rates: vec![176_400],
        channels: vec![2],
        origin: CapabilityOrigin::Advertised,
    }];
    let mut req = request(
        SampleFormat::Dop {
            dsd_rate: 5_644_800,
        },
        352_800,
    );
    req.on_unavailable = OnUnavailable::Downgrade;
    let mut s = NullHost::with_capabilities(NullClock::Real, dsd64_only)
        .open(&req, Box::new(EndsAtOnce))
        .unwrap();
    assert_eq!((s.config().format, s.config().rate), (DOP64, 176_400));
    match s.events().try_recv() {
        Ok(Event::Downgraded { actual, reason, .. }) => {
            assert_eq!(reason, "rate not supported");
            assert_eq!((actual.format, actual.rate), (DOP64, 176_400));
        }
        other => panic!("expected Downgraded, got {other:?}"),
    }
    s.stop().unwrap();
}

#[test]
fn refused_with_library_conversion() {
    let only_i24 = default_capability_entries()
        .into_iter()
        .filter(|e| e.format == SampleFormat::I24Packed)
        .collect();
    let mut req = request(DOP64, 176_400);
    req.conversion = Conversion::Allow(ConversionSpec {
        from: DOP64,
        to: SampleFormat::I24Packed,
    });
    let e = NullHost::with_capabilities(NullClock::Real, only_i24)
        .open(&req, Box::new(EndsAtOnce))
        .unwrap_err();
    assert_eq!(e, Error::FormatUnsupported { nearest: None });
}
