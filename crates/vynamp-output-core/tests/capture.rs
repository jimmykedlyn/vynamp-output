//! The Capture host records exactly what the device thread delivered:
//! bytes compared bit-exactly with the `ChannelRamp` oracle, including the
//! silence of an underrun, plus the manifest.

use std::path::PathBuf;

use vynamp_output_core::*;
use vynamp_output_testkit::source::ChannelRamp;

const PERIOD: u32 = 64;
const CHANNELS: u16 = 2;
const PERIODS: u64 = 5;
/// Call index whose render is short, and its frame count.
const SHORT_CALL: u64 = 2;
const SHORT_FRAMES: u32 = 10;
const RAMP: ChannelRamp = ChannelRamp {
    start: -100_000,
    step: 4099,
};

/// `ChannelRamp`, but call `SHORT_CALL` reports only `SHORT_FRAMES` frames.
struct ShortOnce {
    inner: ChannelRamp,
    calls: u64,
}

impl RenderSource for ShortOnce {
    fn render(&mut self, out: DeviceBuffer<'_>, ctx: &RenderContext<'_>) -> Rendered {
        let r = self.inner.render(out, ctx);
        self.calls += 1;
        Rendered {
            frames: if self.calls - 1 == SHORT_CALL {
                SHORT_FRAMES
            } else {
                r.frames
            },
            end_of_stream: false,
        }
    }
}

fn temp_path(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "vynamp-output-capture-{}-{name}.raw",
        std::process::id()
    ))
}

fn manifest_path(path: &std::path::Path) -> PathBuf {
    let mut s = path.as_os_str().to_owned();
    s.push(".manifest");
    PathBuf::from(s)
}

/// Runs `PERIODS` periods on a virtual clock and returns (bytes, manifest).
fn record(
    name: &str,
    host: impl FnOnce(PathBuf) -> CaptureHost,
    req: StreamRequest,
    src: impl RenderSource,
) -> (Vec<u8>, String, Stream) {
    let path = temp_path(name);
    let clock = VirtualClockHandle::new();
    let host = host(path.clone()).with_clock(NullClock::Virtual(clock.clone()));
    assert_eq!(host.id(), HostId::Capture);
    let mut s = host.open(&req, Box::new(src)).expect("open capture");
    s.start().unwrap();
    clock.advance(PERIODS);
    s.stop().unwrap();
    let bytes = std::fs::read(&path).expect("capture file");
    let manifest = std::fs::read_to_string(manifest_path(&path)).expect("manifest");
    let _ = std::fs::remove_file(&path);
    let _ = std::fs::remove_file(manifest_path(&path));
    (bytes, manifest, s)
}

fn request(format: SampleFormat) -> StreamRequest {
    StreamRequest {
        device: DeviceSelector::Default(Role::Multimedia),
        mode: ShareMode::Exclusive,
        format,
        rate: 48_000,
        channels: CHANNELS,
        layout: ChannelLayout::unknown(CHANNELS),
        buffer: BufferRequest::Frames(PERIOD),
        on_unavailable: OnUnavailable::Error,
        conversion: Conversion::Forbid,
        allow_rate_change_of_device: false,
    }
}

/// The oracle: `Some(value)` for content samples, `None` for silence.
fn expected_values(ramp: ChannelRamp) -> Vec<Option<i64>> {
    let ch = usize::from(CHANNELS);
    let mut out = Vec::new();
    for f in 0..PERIODS * u64::from(PERIOD) {
        let in_period = f % u64::from(PERIOD);
        let silent = f / u64::from(PERIOD) == SHORT_CALL && in_period >= u64::from(SHORT_FRAMES);
        for c in 0..ch {
            out.push((!silent).then(|| ramp.value(f, c, ch)));
        }
    }
    out
}

fn short_ramp() -> ShortOnce {
    ShortOnce {
        inner: RAMP,
        calls: 0,
    }
}

#[test]
fn i24_packed_bytes_are_bit_exact_with_underrun_silence() {
    let (bytes, manifest, s) = record(
        "i24",
        CaptureHost::new,
        request(SampleFormat::I24Packed),
        short_ramp(),
    );
    let expect: Vec<u8> = expected_values(RAMP)
        .into_iter()
        .flat_map(|v| {
            let [a, b, c, _] = (v.unwrap_or(0) as i32).to_le_bytes();
            [a, b, c]
        })
        .collect();
    assert_eq!(bytes.len(), expect.len());
    assert!(bytes == expect, "I24Packed bytes differ from the oracle");
    assert_eq!(s.counters().source_underruns, 1);
    assert_eq!(
        manifest,
        "format=I24Packed\ncontainer_bytes=3\nvalid_bits=24\nbyte_order=little\n\
         rate=48000\nchannels=2\nperiod_frames=64\nframes=320\noverruns=0\n"
    );
}

#[test]
fn i32_bytes_are_bit_exact_with_underrun_silence() {
    let (bytes, manifest, _) = record(
        "i32",
        CaptureHost::new,
        request(SampleFormat::I32 { valid_bits: 32 }),
        short_ramp(),
    );
    let expect: Vec<u8> = expected_values(RAMP)
        .into_iter()
        .flat_map(|v| (v.unwrap_or(0) as i32).to_le_bytes())
        .collect();
    assert!(bytes == expect, "I32 bytes differ from the oracle");
    assert!(manifest.starts_with("format=I32\ncontainer_bytes=4\nvalid_bits=32\n"));
    assert!(manifest.ends_with("frames=320\noverruns=0\n"), "{manifest}");
}

#[test]
fn f32_bytes_are_bit_exact_with_underrun_silence() {
    let (bytes, manifest, _) = record(
        "f32",
        CaptureHost::new,
        request(SampleFormat::F32),
        short_ramp(),
    );
    let expect: Vec<u8> = expected_values(RAMP)
        .into_iter()
        .flat_map(|v| {
            let x = v.map_or(0.0, |v| (v as f64 / 2_147_483_648.0) as f32);
            x.to_bits().to_le_bytes()
        })
        .collect();
    assert!(bytes == expect, "F32 bytes differ from the oracle");
    assert!(manifest.starts_with("format=F32\ncontainer_bytes=4\nvalid_bits=32\n"));
    assert!(manifest.ends_with("overruns=0\n"), "{manifest}");
}

#[test]
fn library_conversion_f32_to_i24_is_recorded() {
    // value / 2^31 in f32, times 2^23, is value / 256 exactly
    let ramp = ChannelRamp {
        start: -256 * 1000,
        step: 256 * 3,
    };
    let only_i24 = |path| {
        let entries = default_capability_entries()
            .into_iter()
            .filter(|e| e.format == SampleFormat::I24Packed)
            .collect();
        CaptureHost::with_capabilities(path, entries)
    };
    let mut req = request(SampleFormat::F32);
    req.conversion = Conversion::Allow(ConversionSpec {
        from: SampleFormat::F32,
        to: SampleFormat::I24Packed,
    });
    let src = ShortOnce {
        inner: ramp,
        calls: 0,
    };
    let (bytes, manifest, s) = record("conv", only_i24, req, src);
    assert_eq!(
        s.config().path.library_conversion.map(|c| c.to),
        Some(SampleFormat::I24Packed)
    );
    let expect: Vec<u8> = expected_values(ramp)
        .into_iter()
        .flat_map(|v| {
            let [a, b, c, _] = (v.unwrap_or(0) as i32 / 256).to_le_bytes();
            [a, b, c]
        })
        .collect();
    assert!(bytes == expect, "converted bytes differ from the oracle");
    assert!(manifest.starts_with("format=I24Packed\n"), "{manifest}");
}

#[test]
fn channels_are_not_swapped() {
    // ChannelRamp gives each channel its own value: sample 0 is channel 0
    let (bytes, _, _) = record(
        "swap",
        CaptureHost::new,
        request(SampleFormat::I32 { valid_bits: 32 }),
        ChannelRamp { start: 1, step: 1 },
    );
    let first: Vec<i32> = bytes
        .as_chunks::<4>()
        .0
        .iter()
        .take(4)
        .map(|b| i32::from_le_bytes(*b))
        .collect();
    assert_eq!(first, [1, 2, 3, 4]);
}

#[test]
fn unwritable_path_is_an_os_error() {
    let dir = std::env::temp_dir().join(format!("vynamp-missing-{}", std::process::id()));
    let host = CaptureHost::new(dir.join("sub").join("x.raw"));
    let r = host.open(&request(SampleFormat::F32), Box::new(RAMP));
    assert!(
        matches!(
            r,
            Err(Error::Os {
                backend: HostId::Capture,
                ..
            })
        ),
        "{r:?}"
    );
}
