//! Stream state machine, driver and watcher, driven deterministically through
//! the Null host's virtual clock.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use vynamp_output_core::*;
use vynamp_output_testkit::source::{ChannelRamp, Panics, StopAfter};

const PERIOD: u32 = 64;

fn request(format: SampleFormat) -> StreamRequest {
    StreamRequest {
        device: DeviceSelector::Default(Role::Multimedia),
        mode: ShareMode::Shared,
        format,
        rate: 48_000,
        channels: 2,
        layout: ChannelLayout::unknown(2),
        buffer: BufferRequest::Frames(PERIOD),
        on_unavailable: OnUnavailable::Error,
        conversion: Conversion::Forbid,
        allow_rate_change_of_device: false,
    }
}

fn open(src: impl RenderSource) -> (VirtualClockHandle, Stream) {
    let clock = VirtualClockHandle::new();
    let host = NullHost::new(NullClock::Virtual(clock.clone()));
    let stream = host
        .open(
            &request(SampleFormat::I32 { valid_bits: 32 }),
            Box::new(src),
        )
        .expect("open null stream");
    (clock, stream)
}

fn state_err() -> Error {
    Error::InvalidRequest { what: "state" }
}

fn events(s: &Stream) -> Vec<Event> {
    s.events().try_iter().collect()
}

/// Checks that every render sees a contiguous `device_frame_index` and
/// records what it saw.
struct Recorder {
    expected: u64,
    calls: Arc<AtomicU64>,
    gaps: Arc<AtomicU64>,
    /// Frames to report per call index (default: all requested).
    short: Option<(u64, u32)>,
}

impl RenderSource for Recorder {
    fn render(&mut self, out: DeviceBuffer<'_>, ctx: &RenderContext<'_>) -> Rendered {
        let call = self.calls.fetch_add(1, Ordering::Relaxed);
        if ctx.device_frame_index != self.expected {
            self.gaps.fetch_add(1, Ordering::Relaxed);
        }
        self.expected = ctx.device_frame_index + u64::from(ctx.frames);
        let _ = out;
        let frames = match self.short {
            Some((at, n)) if call >= at => n,
            _ => ctx.frames,
        };
        Rendered {
            frames,
            end_of_stream: false,
        }
    }
}

fn recorder(short: Option<(u64, u32)>) -> (Recorder, Arc<AtomicU64>, Arc<AtomicU64>) {
    let calls = Arc::new(AtomicU64::new(0));
    let gaps = Arc::new(AtomicU64::new(0));
    let r = Recorder {
        expected: 0,
        calls: calls.clone(),
        gaps: gaps.clone(),
        short,
    };
    (r, calls, gaps)
}

#[test]
fn state_machine_table() {
    let (clock, mut s) = open(ChannelRamp { start: 0, step: 1 });
    assert_eq!(s.state(), StreamState::Opened);
    assert_eq!(s.pause(), Err(state_err()));
    assert_eq!(s.resume(), Err(state_err()));
    assert_eq!(s.drain(Duration::ZERO), Err(state_err()));

    assert_eq!(s.start(), Ok(()));
    assert_eq!(s.state(), StreamState::Running);
    assert_eq!(s.start(), Err(state_err()));
    assert_eq!(s.resume(), Err(state_err()));

    assert_eq!(s.pause(), Ok(()));
    assert_eq!(s.state(), StreamState::Paused);
    assert_eq!(s.pause(), Err(state_err()));
    assert_eq!(s.start(), Err(state_err()));
    assert_eq!(s.drain(Duration::ZERO), Err(state_err()));

    assert_eq!(s.resume(), Ok(()));
    assert_eq!(s.state(), StreamState::Running);

    assert_eq!(s.drain(Duration::ZERO), Ok(DrainOutcome::TimedOut));
    assert_eq!(s.state(), StreamState::Draining);
    assert_eq!(s.pause(), Err(state_err()));
    assert_eq!(s.start(), Err(state_err()));

    assert_eq!(s.stop(), Ok(()));
    assert_eq!(s.state(), StreamState::Stopped);
    assert_eq!(s.stop(), Ok(()), "stop is idempotent");
    for r in [s.start(), s.pause(), s.resume()] {
        assert_eq!(r, Err(state_err()));
    }
    assert_eq!(s.drain(Duration::ZERO), Err(state_err()));
    clock.advance(2); // no stream bound any more: returns at once
}

#[test]
fn stop_from_opened_and_paused() {
    let (_c, mut s) = open(ChannelRamp { start: 0, step: 1 });
    assert_eq!(s.stop(), Ok(()));
    assert_eq!(s.state(), StreamState::Stopped);

    let (_c, mut s) = open(ChannelRamp { start: 0, step: 1 });
    s.start().unwrap();
    s.pause().unwrap();
    assert_eq!(s.stop(), Ok(()));
    assert_eq!(s.state(), StreamState::Stopped);
}

#[test]
fn periods_are_frame_exact_and_paused_ticks_deliver_nothing() {
    let (src, calls, gaps) = recorder(None);
    let (clock, mut s) = open(src);
    clock.advance(3); // Opened: device ticks, nothing is rendered
    assert_eq!(s.counters().callbacks, 0);
    assert_eq!(s.timing(), None);

    s.start().unwrap();
    clock.advance(5);
    let c = s.counters();
    assert_eq!(
        (c.callbacks, c.frames_delivered),
        (5, 5 * u64::from(PERIOD))
    );
    let t = s.timing().expect("timing after running ticks");
    assert_eq!(
        t.device_frame,
        4 * u64::from(PERIOD),
        "last tick presents frame 4P"
    );
    assert_eq!(t.source, TimingSource::Estimated);

    s.pause().unwrap();
    clock.advance(4);
    assert_eq!(s.counters().frames_delivered, 5 * u64::from(PERIOD));
    s.resume().unwrap();
    clock.advance(2);
    s.stop().unwrap();
    let c = s.counters();
    assert_eq!(c.frames_delivered, 7 * u64::from(PERIOD));
    assert_eq!(calls.load(Ordering::Relaxed), 7);
    assert_eq!(
        gaps.load(Ordering::Relaxed),
        0,
        "device_frame_index is contiguous"
    );
    assert_eq!(c.source_underruns, 0);
    assert!(events(&s).is_empty());
}

#[test]
fn underrun_is_counted_once_per_short_period() {
    // from call 2 on, the source renders only 10 frames per period
    let (src, _calls, _gaps) = recorder(Some((2, 10)));
    let (clock, mut s) = open(src);
    s.start().unwrap();
    clock.advance(2);
    assert_eq!(s.counters().source_underruns, 0);
    clock.advance(3);
    assert_eq!(s.counters().source_underruns, 3);
    // the watcher reports deltas; the total equals the counter
    let total: u64 = events(&s)
        .into_iter()
        .map(|e| match e {
            Event::SourceUnderrun { count } => count,
            other => panic!("unexpected event {other:?}"),
        })
        .sum();
    assert_eq!(total, 3);
    clock.advance(1);
    assert_eq!(events(&s), vec![Event::SourceUnderrun { count: 1 }]);
    // frames delivered count content + silence
    assert_eq!(s.counters().frames_delivered, 6 * u64::from(PERIOD));
    s.stop().unwrap();
}

#[test]
fn panic_gives_silence_one_event_and_keeps_cadence() {
    let (clock, mut s) = open(Panics {
        after_frames: 3 * u64::from(PERIOD),
    });
    s.start().unwrap();
    clock.advance(3);
    assert!(events(&s).is_empty());
    clock.advance(1); // the panicking period
    assert_eq!(events(&s), vec![Event::SourcePanicked]);
    clock.advance(5);
    assert!(events(&s).is_empty(), "SourcePanicked is emitted once");
    let c = s.counters();
    assert_eq!(c.callbacks, 9, "cadence continues after the panic");
    assert_eq!(c.frames_delivered, 9 * u64::from(PERIOD));
    assert_eq!(c.source_underruns, 0, "a panic is not an underrun");
    assert_eq!(s.state(), StreamState::Running);
    s.stop().unwrap();
}

#[test]
fn drain_completes_exactly_after_the_last_content_frame() {
    // content ends mid-period 5: last content frame index = 5.5 P
    let content = 5 * u64::from(PERIOD) + u64::from(PERIOD) / 2;
    let (clock, mut s) = open(StopAfter::new(ChannelRamp { start: 0, step: 1 }, content));
    s.start().unwrap();
    clock.advance(6); // periods 0..=5 rendered; timing presents frame 5P
    assert_eq!(
        s.drain(Duration::from_millis(5)),
        Ok(DrainOutcome::TimedOut)
    );
    assert_eq!(s.state(), StreamState::Draining);

    let c2 = clock.clone();
    let t = std::thread::spawn(move || c2.advance(1)); // tick 6 presents 6P >= 5.5P
    let outcome = s.drain(Duration::from_secs(5));
    t.join().unwrap();
    assert_eq!(outcome, Ok(DrainOutcome::Drained));
    assert_eq!(s.state(), StreamState::Stopped);
    let c = s.counters();
    assert_eq!(c.frames_delivered, 7 * u64::from(PERIOD));
    assert_eq!(c.source_underruns, 0, "end of stream is not an underrun");
}

#[test]
fn drain_times_out_when_the_source_never_ends() {
    let (clock, mut s) = open(ChannelRamp { start: 0, step: 1 });
    s.start().unwrap();
    clock.advance(2);
    let t0 = std::time::Instant::now();
    assert_eq!(
        s.drain(Duration::from_millis(20)),
        Ok(DrainOutcome::TimedOut)
    );
    assert!(t0.elapsed() >= Duration::from_millis(20));
    assert_eq!(s.state(), StreamState::Draining);
    s.stop().unwrap();
}

#[test]
fn device_lost_comes_only_from_the_backend() {
    let (clock, mut s) = open(ChannelRamp { start: 0, step: 1 });
    s.start().unwrap();
    clock.advance(2);
    clock.inject_device_lost();
    clock.advance(3);
    assert_eq!(s.state(), StreamState::Lost);
    assert_eq!(events(&s), vec![Event::DeviceLost]);
    assert_eq!(s.counters().callbacks, 2, "a lost device renders nothing");
    assert_eq!(s.start(), Err(state_err()));
    assert_eq!(s.resume(), Err(state_err()));
    assert_eq!(s.stop(), Ok(()));
    assert_eq!(s.state(), StreamState::Stopped);
    clock.inject_device_lost(); // after stop: no effect
    assert_eq!(s.state(), StreamState::Stopped);
}

#[test]
fn drop_stops_the_stream() {
    let (clock, mut s) = open(ChannelRamp { start: 0, step: 1 });
    s.start().unwrap();
    clock.advance(1);
    drop(s);
    clock.advance(1); // device thread gone: must not hang
}

#[test]
fn a_virtual_clock_drives_one_stream_at_a_time() {
    let clock = VirtualClockHandle::new();
    let host = NullHost::new(NullClock::Virtual(clock.clone()));
    let req = request(SampleFormat::F32);
    let mut a = host
        .open(&req, Box::new(ChannelRamp { start: 0, step: 1 }))
        .unwrap();
    let busy = host.open(&req, Box::new(ChannelRamp { start: 0, step: 1 }));
    assert_eq!(
        busy.err(),
        Some(Error::InvalidRequest {
            what: "virtual clock already drives a stream"
        })
    );
    a.stop().unwrap();
    let mut b = host
        .open(&req, Box::new(ChannelRamp { start: 0, step: 1 }))
        .unwrap();
    b.start().unwrap();
    clock.advance(2);
    assert_eq!(b.counters().callbacks, 2);
}
