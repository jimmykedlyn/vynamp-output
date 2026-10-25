//! Zero heap allocations on the device thread after start, counted with the
//! testkit's per-thread counter through the virtual clock's tick probe (it
//! runs on the device thread at the end of every tick).
//!
//! The period in which a source panics is excluded: unwinding boxes the
//! panic payload and the default panic hook formats and prints a message.
//! Measurement resumes on the silent periods after it.

use vynamp_output_core::*;
use vynamp_output_testkit::alloc::{CountingAlloc, allocations_on_this_thread};
use vynamp_output_testkit::source::{ChannelRamp, Panics};

#[global_allocator]
static ALLOC: CountingAlloc = CountingAlloc;

const PERIOD: u32 = 32;

fn request(format: SampleFormat) -> StreamRequest {
    StreamRequest {
        device: DeviceSelector::Default(Role::Multimedia),
        mode: ShareMode::Exclusive,
        format,
        rate: 96_000,
        channels: 2,
        layout: ChannelLayout::unknown(2),
        buffer: BufferRequest::Frames(PERIOD),
        on_unavailable: OnUnavailable::Error,
        conversion: Conversion::Forbid,
        allow_rate_change_of_device: false,
    }
}

/// Full periods on even calls, a short one (underrun) on odd calls.
struct Alternating {
    calls: u64,
    inner: ChannelRamp,
}

impl RenderSource for Alternating {
    fn render(&mut self, out: DeviceBuffer<'_>, ctx: &RenderContext<'_>) -> Rendered {
        let r = self.inner.render(out, ctx);
        self.calls += 1;
        Rendered {
            frames: if self.calls.is_multiple_of(2) {
                r.frames
            } else {
                r.frames / 2
            },
            end_of_stream: false,
        }
    }
}

/// Opens on a fresh virtual clock with the allocation probe installed.
fn start<H: Host>(
    host: impl FnOnce(NullClock) -> H,
    req: &StreamRequest,
    src: impl RenderSource,
) -> (VirtualClockHandle, Stream) {
    let clock = VirtualClockHandle::new();
    assert!(clock.set_tick_probe(allocations_on_this_thread));
    let mut s = host(NullClock::Virtual(clock.clone()))
        .open(req, Box::new(src))
        .expect("open");
    s.start().unwrap();
    (clock, s)
}

#[test]
fn running_and_underrun_periods_do_not_allocate() {
    let src = Alternating {
        calls: 0,
        inner: ChannelRamp { start: 0, step: 1 },
    };
    let (clock, mut s) = start(NullHost::new, &request(SampleFormat::I24Packed), src);
    clock.advance(2);
    let base = clock.tick_probe();
    clock.advance(50);
    assert_eq!(clock.tick_probe() - base, 0, "device thread allocated");
    assert_eq!(s.counters().source_underruns, 26);
    s.stop().unwrap();
}

#[test]
fn library_conversion_does_not_allocate() {
    let only_i24 = |clock| {
        let entries = default_capability_entries()
            .into_iter()
            .filter(|e| e.format == SampleFormat::I24Packed)
            .collect();
        NullHost::with_capabilities(clock, entries)
    };
    let mut req = request(SampleFormat::F32);
    req.conversion = Conversion::Allow(ConversionSpec {
        from: SampleFormat::F32,
        to: SampleFormat::I24Packed,
    });
    let (clock, mut s) = start(
        only_i24,
        &req,
        ChannelRamp {
            start: 0,
            step: 256,
        },
    );
    assert_eq!(s.config().format, SampleFormat::I24Packed);
    clock.advance(2);
    let base = clock.tick_probe();
    clock.advance(50);
    assert_eq!(clock.tick_probe() - base, 0, "device thread allocated");
    s.stop().unwrap();
}

#[test]
fn silence_after_a_panic_does_not_allocate() {
    let src = Panics {
        after_frames: 4 * u64::from(PERIOD),
    };
    let (clock, mut s) = start(
        NullHost::new,
        &request(SampleFormat::I32 { valid_bits: 32 }),
        src,
    );
    clock.advance(2);
    let before = clock.tick_probe();
    clock.advance(2); // periods 2 and 3: content
    assert_eq!(clock.tick_probe() - before, 0, "device thread allocated");
    let pre_panic = clock.tick_probe();
    clock.advance(1); // period 4 panics: excluded
    let after_panic = clock.tick_probe();
    assert!(
        after_panic > pre_panic,
        "the probe must see the panic's allocations (counter is live)"
    );
    clock.advance(50);
    assert_eq!(
        clock.tick_probe() - after_panic,
        0,
        "silence periods allocated"
    );
    assert_eq!(s.events().try_recv(), Ok(Event::SourcePanicked));
    s.stop().unwrap();
}

#[test]
fn capture_ring_does_not_allocate() {
    let path = std::env::temp_dir().join(format!(
        "vynamp-output-zero-alloc-{}.raw",
        std::process::id()
    ));
    let p = path.clone();
    let host = move |clock| CaptureHost::new(p).with_clock(clock);
    let src = Alternating {
        calls: 0,
        inner: ChannelRamp { start: 0, step: 1 },
    };
    let (clock, mut s) = start(host, &request(SampleFormat::I24Packed), src);
    clock.advance(2);
    let base = clock.tick_probe();
    clock.advance(50);
    assert_eq!(clock.tick_probe() - base, 0, "device thread allocated");
    s.stop().unwrap();
    let len = std::fs::metadata(&path).unwrap().len();
    assert_eq!(len, 52 * u64::from(PERIOD) * 2 * 3);
    let mut m = path.into_os_string();
    let _ = std::fs::remove_file(&m);
    m.push(".manifest");
    let manifest = std::fs::read_to_string(&m).unwrap();
    assert!(
        manifest.ends_with(
            "overruns=0
"
        ),
        "{manifest}"
    );
    let _ = std::fs::remove_file(&m);
}
