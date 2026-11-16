//! The generic device-thread loop: one period per clock tick, exactly like an
//! OS audio callback would run it.
//!
//! RT module. Everything the loop touches is allocated before the thread
//! starts (period buffers, the boxed source, the shared atomics); per period
//! it only uses atomics, slices and the pre-allocated scratch (checked by
//! the static guard in `rt_guard.rs` and the counting-allocator tests).
//!
//! Waiting for the next period is the clock's job ([`PeriodClock`]): the Null
//! backend's clocks live in `null.rs`, OS backends will wait on their device
//! events. Where the delivered period goes is the sink's job ([`PeriodSink`]).

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Arc;
use std::sync::atomic::Ordering;

use crate::config::StreamConfig;
use crate::convert::convert_f32_to;
use crate::dop::pack;
use crate::format::SampleFormat;
use crate::render::{
    DeviceBuffer, MonotonicNanos, PresentationTiming, RenderContext, RenderSource, TimingSource,
};
use crate::request::ConversionSpec;
use crate::rtflags::RtFlags;
use crate::silence::fill_silence;

/// One period of device samples in the stream's native container format,
/// allocated once at open (`period_frames * channels` samples).
#[derive(Debug)]
pub(crate) enum NativeBuf {
    F32(Box<[f32]>),
    F64(Box<[f64]>),
    I16(Box<[i16]>),
    I24Packed(Box<[[u8; 3]]>),
    I32(Box<[i32]>),
    U8(Box<[u8]>),
}

impl NativeBuf {
    pub(crate) fn as_device(&mut self) -> DeviceBuffer<'_> {
        match self {
            NativeBuf::F32(b) => DeviceBuffer::F32(b),
            NativeBuf::F64(b) => DeviceBuffer::F64(b),
            NativeBuf::I16(b) => DeviceBuffer::I16(b),
            NativeBuf::I24Packed(b) => DeviceBuffer::I24Packed(b),
            NativeBuf::I32(b) => DeviceBuffer::I32(b),
            NativeBuf::U8(b) => DeviceBuffer::U8(b),
        }
    }
}

/// Where the source renders: straight into the device buffer, into an
/// `f32` scratch converted afterwards (`path.library_conversion`), or, for
/// DoP, into a DSD byte scratch packed afterwards (`dop.rs`).
#[derive(Debug)]
pub(crate) enum Pipeline {
    Direct,
    Convert {
        scratch: Box<[f32]>,
        spec: ConversionSpec,
    },
    /// `2 * period_frames * channels` DSD bytes.
    Dop {
        scratch: Box<[u8]>,
    },
}

/// Paces the device thread.
pub(crate) trait PeriodClock: Send + 'static {
    /// Blocks until the next period is due and returns its time, or `None`
    /// when the thread must leave (`flags.exit`).
    fn wait(&mut self, flags: &RtFlags) -> Option<MonotonicNanos>;
    /// On the device thread, after a tick's work and before it is counted.
    fn tick_done(&mut self) {}
    /// On the device thread, after the tick is counted in `flags.ticks`.
    fn after_tick(&mut self) {}
}

/// Receives each delivered period (an OS device, a capture ring, nothing).
pub(crate) trait PeriodSink: Send + 'static {
    fn deliver(&mut self, period: &NativeBuf);
}

/// A sink that drops every period (the Null device).
#[derive(Debug, Default)]
pub(crate) struct Discard;

impl PeriodSink for Discard {
    fn deliver(&mut self, _period: &NativeBuf) {}
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SourceState {
    Live,
    /// Reported `end_of_stream`; never called again.
    Ended,
    /// Panicked in `render`; never called again.
    Panicked,
}

/// State owned by the device thread.
pub(crate) struct Driver<C, K> {
    pub(crate) clock: C,
    pub(crate) sink: K,
    pub(crate) source: Box<dyn RenderSource>,
    pub(crate) flags: Arc<RtFlags>,
    pub(crate) config: StreamConfig,
    pub(crate) native: NativeBuf,
    pub(crate) pipeline: Pipeline,
    /// Latency reported in the timing snapshot.
    pub(crate) latency_frames: u64,
    device_frame: u64,
    source_state: SourceState,
    dop_phase: bool,
}

impl<C: PeriodClock, K: PeriodSink> Driver<C, K> {
    /// The same driver with a new clock and sink, and the same source, buffers
    /// and delivery position.
    ///
    /// Recovery uses this after a loss: the OS stream object is new, so its
    /// clock and sink are new too, but the `RenderSource`, the pre-allocated
    /// period buffers and `device_frame` must not be rebuilt (ADR-002).
    /// `sink` receives the old sink, so a new sink can take over the buffers
    /// it owned. Called on the control thread, between the old device thread
    /// returning the driver and the new one starting; never concurrently with
    /// a tick.
    ///
    /// Only the WASAPI backend needs it today (the Null/Capture device never
    /// really goes away, so their recovery reuses the same clock and sink);
    /// it is compiled on every target so the contract is testable there.
    #[cfg_attr(not(target_os = "windows"), allow(dead_code))]
    pub(crate) fn rebind<C2: PeriodClock, K2: PeriodSink>(
        self,
        clock: C2,
        sink: impl FnOnce(K) -> K2,
    ) -> Driver<C2, K2> {
        Driver {
            clock,
            sink: sink(self.sink),
            source: self.source,
            flags: self.flags,
            config: self.config,
            native: self.native,
            pipeline: self.pipeline,
            latency_frames: self.latency_frames,
            device_frame: self.device_frame,
            source_state: self.source_state,
            dop_phase: self.dop_phase,
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(
        clock: C,
        sink: K,
        source: Box<dyn RenderSource>,
        flags: Arc<RtFlags>,
        config: StreamConfig,
        native: NativeBuf,
        pipeline: Pipeline,
        latency_frames: u64,
    ) -> Self {
        Driver {
            clock,
            sink,
            source,
            flags,
            config,
            native,
            pipeline,
            latency_frames,
            device_frame: 0,
            source_state: SourceState::Live,
            dop_phase: false,
        }
    }

    /// One clock tick. Delivers a period only while running or draining.
    fn tick(&mut self, at: MonotonicNanos) {
        if !self.flags.is_rendering() {
            return;
        }
        let period = self.config.period_frames;
        let channels = usize::from(self.config.channels);
        let index = self.device_frame;
        let timing = PresentationTiming {
            device_frame: index,
            at,
            latency_frames: self.latency_frames,
            source: TimingSource::Estimated,
        };
        self.flags.timing.write(timing);

        let written = if self.source_state == SourceState::Live {
            let ctx = RenderContext {
                frames: period,
                device_frame_index: index,
                timing: Some(timing),
                config: &self.config,
            };
            let buf = work_buffer(&mut self.pipeline, &mut self.native);
            let source = &mut self.source;
            match catch_unwind(AssertUnwindSafe(|| source.render(buf, &ctx))) {
                Ok(r) => {
                    let frames = r.frames.min(period);
                    if r.end_of_stream {
                        self.source_state = SourceState::Ended;
                        self.flags
                            .mark_end_of_stream(index.wrapping_add(u64::from(frames)));
                    } else if frames < period {
                        self.flags.source_underruns.fetch_add(1, Ordering::Release);
                    }
                    frames
                }
                Err(payload) => {
                    // The payload was boxed by the panic machinery; dropping
                    // it only deallocates.
                    drop(payload);
                    self.source_state = SourceState::Panicked;
                    self.flags.source_panicked.store(true, Ordering::Release);
                    0
                }
            }
        } else {
            0
        };

        let format = match self.pipeline {
            Pipeline::Direct | Pipeline::Dop { .. } => self.config.format,
            Pipeline::Convert { .. } => SampleFormat::F32,
        };
        let mut buf = work_buffer(&mut self.pipeline, &mut self.native);
        fill_silence(
            &mut buf,
            written as usize,
            channels,
            format,
            &mut self.dop_phase,
        );
        let packed = match &self.pipeline {
            Pipeline::Direct => Ok(()),
            Pipeline::Convert { scratch, spec } => {
                convert_f32_to(scratch, &mut self.native.as_device(), *spec)
            }
            Pipeline::Dop { scratch } => pack(
                scratch,
                &mut self.native.as_device(),
                channels,
                &mut self.dop_phase,
            ),
        };
        if packed.is_err() {
            // validated at open; never deliver stale samples regardless
            fill_silence(
                &mut self.native.as_device(),
                0,
                channels,
                self.config.format,
                &mut self.dop_phase,
            );
        }
        self.sink.deliver(&self.native);

        self.device_frame = index.wrapping_add(u64::from(period));
        self.flags
            .frames_delivered
            .fetch_add(u64::from(period), Ordering::Release);
        self.flags.callbacks.fetch_add(1, Ordering::Release);
    }
}

fn work_buffer<'a>(pipeline: &'a mut Pipeline, native: &'a mut NativeBuf) -> DeviceBuffer<'a> {
    match pipeline {
        Pipeline::Direct => native.as_device(),
        Pipeline::Convert { scratch, .. } => DeviceBuffer::F32(scratch),
        Pipeline::Dop { scratch } => DeviceBuffer::Dsd(scratch),
    }
}

/// Drops a driver on the control thread, ignoring a panic in its `Drop`.
///
/// The driver owns the `RenderSource`, so on a normal `stop` it is dropped
/// here rather than on the device thread; a source whose `Drop` panics must
/// not turn `stop` into a panic (the device thread's panic was previously
/// swallowed by `JoinHandle::join`). Control thread only.
pub(crate) fn discard_driver<C, K>(driver: Option<Driver<C, K>>) {
    let Some(driver) = driver else { return };
    let _ = catch_unwind(AssertUnwindSafe(move || drop(driver)));
}

/// The device-thread body: runs ticks until the clock says to exit.
///
/// Returns the whole driver — source, pre-allocated buffers, pipeline,
/// counters and the clock's own state — when it left because the device was
/// lost, and `None` when it left because the stream was stopped. Recovery
/// ([`crate::stream::BackendStream::recover`]) hands the driver back to a new
/// thread, so the `RenderSource` survives the loss without a lock or an
/// allocation on this thread (ADR-002, the "respawn" mechanism: ownership
/// travels through the `JoinHandle`, never through shared memory).
pub(crate) fn run_device_thread<C: PeriodClock, K: PeriodSink>(
    mut driver: Driver<C, K>,
) -> Option<Driver<C, K>> {
    while let Some(at) = driver.clock.wait(&driver.flags) {
        driver.tick(at);
        driver.clock.tick_done();
        driver.flags.ticks.fetch_add(1, Ordering::Release);
        driver.clock.after_tick();
    }
    driver.flags.device_exited.store(true, Ordering::Release);
    driver.clock.after_tick();
    driver.flags.lost.load(Ordering::Acquire).then_some(driver)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::PathProperties;
    use crate::format::{ChannelLayout, HostId};
    use crate::render::Rendered;
    use crate::request::{DeviceId, ShareMode};
    use crate::rtflags::{ST_PAUSED, ST_RUNNING};
    use std::sync::Mutex;

    const P: u32 = 4;

    fn config(format: SampleFormat) -> StreamConfig {
        StreamConfig {
            host: HostId::Null,
            device: DeviceId("t".into()),
            mode: ShareMode::Shared,
            format,
            valid_bits: format.valid_bits(),
            rate: 48_000,
            channels: 1,
            layout: ChannelLayout::unknown(1),
            period_frames: P,
            buffer_frames: 2 * P,
            path: PathProperties::unknown(),
        }
    }

    /// Ticks `n` times, then exits.
    struct Ticks(u64);
    impl PeriodClock for Ticks {
        fn wait(&mut self, _flags: &RtFlags) -> Option<MonotonicNanos> {
            self.0 = self.0.checked_sub(1)?;
            Some(MonotonicNanos(0))
        }
    }

    #[derive(Clone, Default)]
    struct Record(Arc<Mutex<Vec<Vec<i32>>>>);
    impl PeriodSink for Record {
        fn deliver(&mut self, period: &NativeBuf) {
            let v = match period {
                NativeBuf::I32(b) => b.to_vec(),
                NativeBuf::I16(b) => b.iter().map(|&x| i32::from(x)).collect(),
                // the 24-bit word, unsigned
                NativeBuf::I24Packed(b) => b
                    .iter()
                    .map(|&[lo, mid, hi]| i32::from_le_bytes([lo, mid, hi, 0]))
                    .collect(),
                other => panic!("unexpected {other:?}"),
            };
            self.0.lock().unwrap().push(v);
        }
    }

    /// Writes `index + 1` per frame; `plan[call]` is (frames, end_of_stream)
    /// or a panic when `frames == u32::MAX`.
    struct Scripted {
        plan: Vec<(u32, bool)>,
        calls: Arc<Mutex<u32>>,
    }
    impl RenderSource for Scripted {
        fn render(&mut self, out: DeviceBuffer<'_>, ctx: &RenderContext<'_>) -> Rendered {
            let mut calls = self.calls.lock().unwrap();
            let (frames, eos) = self
                .plan
                .get(*calls as usize)
                .copied()
                .unwrap_or((P, false));
            *calls += 1;
            drop(calls);
            assert!(frames != u32::MAX, "scripted panic");
            let DeviceBuffer::I32(b) = out else {
                panic!("I32 only")
            };
            for (k, s) in b.iter_mut().enumerate() {
                *s = (ctx.device_frame_index + k as u64 + 1) as i32;
            }
            Rendered {
                frames,
                end_of_stream: eos,
            }
        }
    }

    fn run(plan: Vec<(u32, bool)>, ticks: u64) -> (Vec<Vec<i32>>, Arc<RtFlags>, u32) {
        let flags = Arc::new(RtFlags::new());
        flags.state.store(ST_RUNNING, Ordering::Relaxed);
        let rec = Record::default();
        let calls = Arc::new(Mutex::new(0));
        let src = Scripted {
            plan,
            calls: calls.clone(),
        };
        let d = Driver::new(
            Ticks(ticks),
            rec.clone(),
            Box::new(src),
            flags.clone(),
            config(SampleFormat::I32 { valid_bits: 32 }),
            NativeBuf::I32(vec![0; P as usize].into_boxed_slice()),
            Pipeline::Direct,
            0,
        );
        run_device_thread(d);
        let periods = rec.0.lock().unwrap().clone();
        let calls = *calls.lock().unwrap();
        (periods, flags, calls)
    }

    #[test]
    fn underrun_tail_is_silence_and_samples_are_never_replayed() {
        let (p, flags, _) = run(vec![(P, false), (1, false), (0, false), (P, false)], 4);
        assert_eq!(
            p,
            vec![
                vec![1, 2, 3, 4],
                vec![5, 0, 0, 0],
                vec![0, 0, 0, 0],
                vec![13, 14, 15, 16]
            ]
        );
        assert_eq!(flags.source_underruns.load(Ordering::Relaxed), 2);
        assert_eq!(flags.callbacks.load(Ordering::Relaxed), 4);
        assert_eq!(flags.ticks.load(Ordering::Relaxed), 4);
        assert!(flags.device_exited.load(Ordering::Relaxed));
    }

    #[test]
    fn end_of_stream_stops_pulling_and_delivers_silence() {
        let (p, flags, calls) = run(vec![(P, false), (2, true)], 4);
        assert_eq!(p[1], vec![5, 6, 0, 0]);
        assert_eq!(p[2], vec![0; 4]);
        assert_eq!(p[3], vec![0; 4]);
        assert_eq!(calls, 2, "source not called after end of stream");
        assert_eq!(flags.content_end(), Some(6));
        assert_eq!(flags.source_underruns.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn panic_gives_silence_from_then_on() {
        let (p, flags, calls) = run(vec![(P, false), (u32::MAX, false)], 4);
        assert_eq!(p.len(), 4, "cadence continues");
        assert_eq!(p[1], vec![0; 4], "the panicking period is silence");
        assert_eq!(p[3], vec![0; 4]);
        assert_eq!(calls, 2, "source never called again after panicking");
        assert!(flags.source_panicked.load(Ordering::Relaxed));
        assert_eq!(
            flags.frames_delivered.load(Ordering::Relaxed),
            4 * u64::from(P)
        );
    }

    #[test]
    fn over_reporting_source_is_clamped() {
        let (p, flags, _) = run(vec![(P + 10, false)], 1);
        assert_eq!(p[0], vec![1, 2, 3, 4]);
        assert_eq!(flags.source_underruns.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn ticks_outside_running_deliver_nothing() {
        let flags = Arc::new(RtFlags::new());
        flags.state.store(ST_PAUSED, Ordering::Relaxed);
        let rec = Record::default();
        let d = Driver::new(
            Ticks(3),
            rec.clone(),
            Box::new(Scripted {
                plan: vec![],
                calls: Arc::default(),
            }),
            flags.clone(),
            config(SampleFormat::I32 { valid_bits: 32 }),
            NativeBuf::I32(vec![0; P as usize].into_boxed_slice()),
            Pipeline::Direct,
            0,
        );
        run_device_thread(d);
        assert!(rec.0.lock().unwrap().is_empty());
        assert_eq!(flags.ticks.load(Ordering::Relaxed), 3);
        assert_eq!(flags.timing.read(), None);
    }

    struct Half;
    impl RenderSource for Half {
        fn render(&mut self, out: DeviceBuffer<'_>, _ctx: &RenderContext<'_>) -> Rendered {
            let DeviceBuffer::F32(b) = out else {
                panic!("F32 only")
            };
            b.fill(0.5);
            Rendered {
                frames: 3,
                end_of_stream: false,
            }
        }
    }

    #[test]
    fn library_conversion_converts_content_and_silence() {
        let flags = Arc::new(RtFlags::new());
        flags.state.store(ST_RUNNING, Ordering::Relaxed);
        let rec = Record::default();
        let spec = ConversionSpec {
            from: SampleFormat::F32,
            to: SampleFormat::I16,
        };
        let d = Driver::new(
            Ticks(1),
            rec.clone(),
            Box::new(Half),
            flags.clone(),
            config(SampleFormat::I16),
            NativeBuf::I16(vec![7; P as usize].into_boxed_slice()),
            Pipeline::Convert {
                scratch: vec![0.0; P as usize].into_boxed_slice(),
                spec,
            },
            0,
        );
        run_device_thread(d);
        assert_eq!(rec.0.lock().unwrap()[0], vec![16384, 16384, 16384, 0]);
        assert_eq!(flags.source_underruns.load(Ordering::Relaxed), 1);
    }

    /// Mono DSD: frame `n` carries bytes `(2n, 2n + 1)` of the stream; the
    /// first call is full, the second reports one frame.
    struct DsdCount(u32);
    impl RenderSource for DsdCount {
        fn render(&mut self, out: DeviceBuffer<'_>, ctx: &RenderContext<'_>) -> Rendered {
            let DeviceBuffer::Dsd(b) = out else {
                panic!("Dsd only")
            };
            for (k, byte) in b.iter_mut().enumerate() {
                *byte = (2 * ctx.device_frame_index + k as u64) as u8;
            }
            self.0 += 1;
            Rendered {
                frames: if self.0 == 1 { ctx.frames } else { 1 },
                end_of_stream: false,
            }
        }
    }

    #[test]
    fn dop_packs_into_i24_packed_with_continuous_markers() {
        let flags = Arc::new(RtFlags::new());
        flags.state.store(ST_RUNNING, Ordering::Relaxed);
        let rec = Record::default();
        // odd period: a phase reset per period would repeat 0x05 at frame 5
        const DP: u32 = 5;
        let mut cfg = config(SampleFormat::Dop {
            dsd_rate: 2_822_400,
        });
        cfg.period_frames = DP;
        let d = Driver::new(
            Ticks(2),
            rec.clone(),
            Box::new(DsdCount(0)),
            flags.clone(),
            cfg,
            NativeBuf::I24Packed(vec![[0; 3]; DP as usize].into_boxed_slice()),
            Pipeline::Dop {
                scratch: vec![0; 2 * DP as usize].into_boxed_slice(),
            },
            0,
        );
        run_device_thread(d);
        let p = rec.0.lock().unwrap().clone();
        assert_eq!(
            p,
            vec![
                vec![0x05_0001, 0xFA_0203, 0x05_0405, 0xFA_0607, 0x05_0809],
                vec![0xFA_0A0B, 0x05_6969, 0xFA_6969, 0x05_6969, 0xFA_6969]
            ]
        );
        assert_eq!(flags.source_underruns.load(Ordering::Relaxed), 1);
    }
}
