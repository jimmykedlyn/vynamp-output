//! The Null host: a virtual device that consumes periods on a real or a
//! virtual (test-driven) clock and discards them.
//!
//! The device thread runs the generic driver (`driver.rs`) exactly like an
//! OS callback would, so the real-time contract is exercised in CI. The
//! clocks below are the only place where the device thread waits: the real
//! clock parks until the next period deadline, the virtual clock parks until
//! a test grants ticks with [`VirtualClockHandle::advance`]. Waiting uses
//! only atomics and `park`/`unpark` (no locks, no allocation).

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::channel;
use std::sync::{Arc, Mutex, OnceLock, PoisonError};
use std::thread::{self, JoinHandle, Thread};
use std::time::{Duration, Instant};

use crate::config::{EndpointVolume, PathProperties, RtPriority, StreamConfig, Transport, Tri};
use crate::dop::{self, check_dop_path, dop_pcm_rate};
use crate::driver::{Driver, NativeBuf, PeriodClock, PeriodSink, Pipeline, run_device_thread};
use crate::error::Error;
use crate::event::Event;
use crate::format::{HostId, SampleFormat};
use crate::host::{DeviceInfo, Host};
use crate::negotiate::{Capabilities, CapabilityEntry, CapabilityOrigin, negotiate};
use crate::render::{MonotonicNanos, RenderSource};
use crate::request::{
    BufferRequest, DeviceId, DeviceSelector, IdStability, Role, ShareMode, StreamRequest,
};
use crate::rtflags::RtFlags;
use crate::stream::{BackendStream, Stream};
use crate::watcher;

/// Largest accepted period, in frames.
const MAX_PERIOD_FRAMES: u64 = 1 << 20;

/// How a Null (or Capture) device paces its periods.
#[derive(Debug, Clone)]
pub enum NullClock {
    /// One period per `period_frames / rate` of wall-clock time.
    Real,
    /// Periods happen only when a test calls [`VirtualClockHandle::advance`].
    Virtual(VirtualClockHandle),
}

/// Test handle for a virtual clock. Clones share the same clock.
///
/// A handle drives at most one open stream at a time; once that stream is
/// stopped, a new one can be opened on it. Ticks are counted per stream from 0.
///
/// The handle uses a `Mutex` on the control/test side only (binding a stream,
/// [`advance`](Self::advance)); the device thread never locks it.
#[derive(Debug, Clone, Default)]
pub struct VirtualClockHandle {
    inner: Arc<VirtualInner>,
}

#[derive(Debug, Default)]
struct VirtualInner {
    binding: Mutex<Option<Arc<Binding>>>,
    probe: OnceLock<fn() -> u64>,
    probe_value: AtomicU64,
}

/// One stream driven by a virtual clock.
#[derive(Debug)]
struct Binding {
    flags: Arc<RtFlags>,
    /// Ticks granted by `advance`.
    granted: AtomicU64,
    device: OnceLock<Thread>,
    watcher: OnceLock<Thread>,
}

impl VirtualClockHandle {
    pub fn new() -> Self {
        Self::default()
    }

    fn current(&self) -> Option<Arc<Binding>> {
        self.inner
            .binding
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    /// Lets `periods` device periods elapse and returns once the device
    /// thread has processed them and the watcher has turned their flags into
    /// events. Without an open stream it returns at once.
    ///
    /// Ticks while the stream is not running (`Opened`, `Paused`, `Lost`)
    /// pass without delivering frames, like a stopped device clock.
    pub fn advance(&self, periods: u64) {
        let Some(b) = self.current() else { return };
        let target = b.granted.fetch_add(periods, Ordering::AcqRel) + periods;
        if let Some(t) = b.device.get() {
            t.unpark();
        }
        loop {
            let f = &b.flags;
            if f.device_exited.load(Ordering::Acquire) {
                return;
            }
            if f.ticks.load(Ordering::Acquire) >= target
                && f.watched_ticks.load(Ordering::Acquire) >= target
            {
                return;
            }
            thread::yield_now();
        }
    }

    /// Test hook: simulates the backend reporting the device as gone.
    #[doc(hidden)]
    pub fn inject_device_lost(&self) {
        if let Some(b) = self.current() {
            b.flags.notify_lost();
            if let Some(w) = b.watcher.get() {
                w.unpark();
            }
        }
    }

    /// Test hook: `probe` runs on the device thread at the end of every tick
    /// (before `advance` can return) and its result is kept for
    /// [`tick_probe`](Self::tick_probe). Used with the testkit's
    /// `allocations_on_this_thread` to count device-thread allocations.
    /// Can be set once per handle; returns false if already set.
    #[doc(hidden)]
    pub fn set_tick_probe(&self, probe: fn() -> u64) -> bool {
        self.inner.probe.set(probe).is_ok()
    }

    /// Test hook: the value the tick probe returned at the latest tick.
    #[doc(hidden)]
    pub fn tick_probe(&self) -> u64 {
        self.inner.probe_value.load(Ordering::Acquire)
    }

    fn bind(&self, flags: Arc<RtFlags>) -> Result<Arc<Binding>, Error> {
        let mut slot = self
            .inner
            .binding
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if let Some(b) = slot.as_ref()
            && !b.flags.device_exited.load(Ordering::Acquire)
        {
            return Err(Error::InvalidRequest {
                what: "virtual clock already drives a stream",
            });
        }
        let b = Arc::new(Binding {
            flags,
            granted: AtomicU64::new(0),
            device: OnceLock::new(),
            watcher: OnceLock::new(),
        });
        *slot = Some(b.clone());
        Ok(b)
    }
}

/// Nanoseconds since a process-wide epoch, initialised at the first open
/// (control thread), so the device thread only reads it.
///
/// Until the OS monotonic clock arrives (plan 02), the Null host's
/// `MonotonicNanos` is relative to this library-internal epoch.
fn monotonic_now() -> MonotonicNanos {
    static EPOCH: OnceLock<Instant> = OnceLock::new();
    let epoch = *EPOCH.get_or_init(Instant::now);
    MonotonicNanos(u64::try_from(epoch.elapsed().as_nanos()).unwrap_or(u64::MAX))
}

fn frames_to_nanos(frames: u64, rate: u32) -> u64 {
    let n = u128::from(frames) * 1_000_000_000 / u128::from(rate.max(1));
    u64::try_from(n).unwrap_or(u64::MAX)
}

/// Wall-clock pacing. Parks until each deadline (`stop` unparks it).
struct RealClock {
    start: Option<Instant>,
    k: u64,
    period_frames: u64,
    rate: u32,
}

impl PeriodClock for RealClock {
    fn wait(&mut self, flags: &RtFlags) -> Option<MonotonicNanos> {
        let start = *self.start.get_or_insert_with(Instant::now);
        let due =
            start + Duration::from_nanos(frames_to_nanos(self.k * self.period_frames, self.rate));
        loop {
            if flags.exit.load(Ordering::Acquire) {
                return None;
            }
            let now = Instant::now();
            if now >= due {
                break;
            }
            thread::park_timeout(due - now);
        }
        self.k += 1;
        Some(monotonic_now())
    }
}

/// Test-driven pacing: one tick per granted period, virtual time
/// `tick * period / rate`.
struct VirtualClock {
    binding: Arc<Binding>,
    inner: Arc<VirtualInner>,
    taken: u64,
    period_frames: u64,
    rate: u32,
}

impl PeriodClock for VirtualClock {
    fn wait(&mut self, flags: &RtFlags) -> Option<MonotonicNanos> {
        loop {
            if flags.exit.load(Ordering::Acquire) {
                return None;
            }
            if self.binding.granted.load(Ordering::Acquire) > self.taken {
                let at = frames_to_nanos(self.taken * self.period_frames, self.rate);
                self.taken += 1;
                return Some(MonotonicNanos(at));
            }
            thread::park();
        }
    }

    fn tick_done(&mut self) {
        if let Some(probe) = self.inner.probe.get() {
            self.inner.probe_value.store(probe(), Ordering::Release);
        }
    }

    fn after_tick(&mut self) {
        if let Some(w) = self.binding.watcher.get() {
            w.unpark();
        }
    }
}

/// Control-side handle of a device thread (Null and Capture).
pub(crate) struct ThreadBackend {
    flags: Arc<RtFlags>,
    device: Option<JoinHandle<()>>,
    /// Runs after the device thread is joined (Capture: flush + manifest).
    finish: Option<Box<dyn FnOnce() -> Result<(), Error> + Send>>,
}

impl BackendStream for ThreadBackend {
    // The device thread follows `flags.state`; nothing to do per transition.
    fn start(&mut self) -> Result<(), Error> {
        Ok(())
    }

    fn pause(&mut self) -> Result<(), Error> {
        Ok(())
    }

    /// Joins the device thread. A virtual device holds no queued audio, so
    /// there is nothing else to discard.
    fn stop(&mut self) -> Result<(), Error> {
        self.flags.exit.store(true, Ordering::Release);
        if let Some(h) = self.device.take() {
            h.thread().unpark();
            // A panic outside `render` (e.g. in the source's Drop) only ends
            // the device thread; the stream is stopped either way.
            let _ = h.join();
        }
        self.finish.take().map_or(Ok(()), |f| f())
    }
}

/// The capabilities of the default Null/Capture device: Shared and
/// Exclusive; F32, I16, I24Packed, I32{24}, I32{32}; 44.1–768 kHz; 1–8 ch;
/// plus, in Exclusive only, DoP for DSD64/128/256 at 176.4/352.8/705.6 kHz.
pub fn default_capability_entries() -> Vec<CapabilityEntry> {
    const RATES: [u32; 10] = [
        44_100, 48_000, 88_200, 96_000, 176_400, 192_000, 352_800, 384_000, 705_600, 768_000,
    ];
    const FORMATS: [SampleFormat; 5] = [
        SampleFormat::F32,
        SampleFormat::I16,
        SampleFormat::I24Packed,
        SampleFormat::I32 { valid_bits: 24 },
        SampleFormat::I32 { valid_bits: 32 },
    ];
    let mut entries = Vec::new();
    for mode in [ShareMode::Shared, ShareMode::Exclusive] {
        for format in FORMATS {
            entries.push(CapabilityEntry {
                mode,
                format,
                rates: RATES.to_vec(),
                channels: (1..=8).collect(),
                origin: CapabilityOrigin::Advertised,
            });
        }
    }
    for dsd_rate in [2_822_400, 5_644_800, 11_289_600] {
        entries.push(CapabilityEntry {
            mode: ShareMode::Exclusive,
            format: SampleFormat::Dop { dsd_rate },
            rates: dop_pcm_rate(dsd_rate).into_iter().collect(),
            channels: (1..=8).collect(),
            origin: CapabilityOrigin::Advertised,
        });
    }
    entries
}

/// Identity and capabilities of one virtual device.
#[derive(Debug, Clone)]
pub(crate) struct VirtualDevice {
    pub(crate) caps: Capabilities,
    pub(crate) name: &'static str,
    /// Reported as `path.os_mixer_bypassed` (default `Yes`).
    pub(crate) os_mixer_bypassed: Tri,
    /// Reported as `path.endpoint_volume` (default `Unity`).
    pub(crate) endpoint_volume: EndpointVolume,
}

impl VirtualDevice {
    pub(crate) fn new(
        host: HostId,
        id: &str,
        name: &'static str,
        entries: Vec<CapabilityEntry>,
    ) -> Self {
        VirtualDevice {
            caps: Capabilities {
                host,
                device: DeviceId(id.into()),
                entries,
            },
            name,
            os_mixer_bypassed: Tri::Yes,
            endpoint_volume: EndpointVolume::Unity,
        }
    }

    pub(crate) fn info(&self) -> DeviceInfo {
        DeviceInfo {
            id: self.caps.device.clone(),
            id_stability: IdStability::Persistent,
            name: self.name.into(),
            host: self.caps.host,
            default_for: vec![Role::Console, Role::Multimedia, Role::Communications],
            transport: Transport::Virtual,
        }
    }

    pub(crate) fn capabilities(
        &self,
        device: &DeviceId,
        mode: ShareMode,
    ) -> Result<Capabilities, Error> {
        if *device != self.caps.device {
            return Err(Error::DeviceNotFound);
        }
        Ok(Capabilities {
            entries: self
                .caps
                .entries
                .iter()
                .filter(|e| e.mode == mode)
                .cloned()
                .collect(),
            ..self.caps.clone()
        })
    }

    /// Negotiates `req` and completes the config: period from the request,
    /// buffer = 2 periods, virtual path properties. Returns the config and
    /// the downgrade reason, if any.
    ///
    /// A shared stream reports `os_mixer_bypassed: No` (a shared endpoint is
    /// a mixed path). DoP needs a path that carries it unaltered
    /// ([`check_dop_path`]): a mixed path or non-unity volume is
    /// `ExclusiveUnavailable`.
    pub(crate) fn prepare(
        &self,
        req: &StreamRequest,
    ) -> Result<(StreamConfig, Option<&'static str>), Error> {
        if let DeviceSelector::Id(id) = &req.device
            && *id != self.caps.device
        {
            return Err(Error::DeviceNotFound);
        }
        let n = negotiate(req, &self.caps)?;
        let mut config = n.config_template;
        if let Some(spec) = config.path.library_conversion
            && spec.from != SampleFormat::F32
        {
            return Err(Error::InvalidRequest {
                what: "conversion source must be F32",
            });
        }
        let period = match req.buffer {
            BufferRequest::Frames(n) => u64::from(n),
            BufferRequest::Duration(d) => {
                let f = u128::from(config.rate) * d.as_nanos() / 1_000_000_000;
                u64::try_from(f).unwrap_or(u64::MAX)
            }
            BufferRequest::BackendDefault => u64::from(config.rate / 100).max(1),
        };
        if period == 0 {
            return Err(Error::InvalidRequest {
                what: "buffer is zero",
            });
        }
        if period > MAX_PERIOD_FRAMES {
            return Err(Error::InvalidRequest {
                what: "buffer too large",
            });
        }
        config.period_frames = period as u32;
        config.buffer_frames = 2 * period as u32;
        let conversion = config.path.library_conversion;
        config.path = PathProperties {
            os_mixer_bypassed: match config.mode {
                ShareMode::Shared => Tri::No,
                ShareMode::Exclusive => self.os_mixer_bypassed,
            },
            os_resampling: false,
            library_conversion: conversion,
            endpoint_volume: self.endpoint_volume,
            transport: Transport::Virtual,
            rt_priority: RtPriority::Denied("virtual device: no real-time priority requested"),
        };
        if is_dop(config.format) {
            check_dop_path(&config.path)?;
        }
        Ok((config, n.downgraded))
    }
}

fn is_dop(format: SampleFormat) -> bool {
    matches!(format, SampleFormat::Dop { .. })
}

fn spawn_error(host: HostId) -> impl Fn(std::io::Error) -> Error {
    move |e| Error::Os {
        backend: host,
        code: i64::from(e.raw_os_error().unwrap_or(-1)),
        call: "std::thread::Builder::spawn",
    }
}

/// Allocates one period of native samples (zeroed: PCM silence; `U8` 0x80).
/// DoP uses an `I32` container, MSB-justified (the packer writes every word).
fn native_buffer(format: SampleFormat, samples: usize) -> NativeBuf {
    match format {
        SampleFormat::F32 => NativeBuf::F32(vec![0.0; samples].into_boxed_slice()),
        SampleFormat::F64 => NativeBuf::F64(vec![0.0; samples].into_boxed_slice()),
        SampleFormat::I16 => NativeBuf::I16(vec![0; samples].into_boxed_slice()),
        SampleFormat::I24Packed => NativeBuf::I24Packed(vec![[0; 3]; samples].into_boxed_slice()),
        SampleFormat::U8 => NativeBuf::U8(vec![0x80; samples].into_boxed_slice()),
        SampleFormat::I24In32 { .. } | SampleFormat::I32 { .. } | SampleFormat::Dop { .. } => {
            NativeBuf::I32(vec![0; samples].into_boxed_slice())
        }
    }
}

/// Everything `launch` needs besides the config.
pub(crate) struct Launch<K> {
    pub(crate) clock: NullClock,
    pub(crate) sink: K,
    pub(crate) finish: Option<Box<dyn FnOnce() -> Result<(), Error> + Send>>,
}

/// Starts the watcher and the device thread for `config` and returns the
/// stream in the `Opened` state. A downgrade is reported as the first event.
pub(crate) fn launch<K: PeriodSink>(
    req: &StreamRequest,
    config: StreamConfig,
    downgraded: Option<&'static str>,
    source: Box<dyn RenderSource>,
    l: Launch<K>,
) -> Result<Stream, Error> {
    let host = config.host;
    let flags = Arc::new(RtFlags::new());
    let (tx, rx) = channel();
    if let Some(reason) = downgraded {
        let _ = tx.send(Event::Downgraded {
            requested: Box::new(req.clone()),
            actual: Box::new(config.clone()),
            reason,
        });
    }
    if is_dop(config.format) && config.path.endpoint_volume == EndpointVolume::Unknown {
        let _ = tx.send(Event::DopVolumeUnverified);
    }
    let samples = config.period_frames as usize * usize::from(config.channels);
    let native = native_buffer(config.format, samples);
    let pipeline = match config.path.library_conversion {
        None if is_dop(config.format) => Pipeline::Dop {
            scratch: vec![dop::IDLE; samples * dop::BYTES_PER_FRAME].into_boxed_slice(),
        },
        None => Pipeline::Direct,
        Some(spec) => Pipeline::Convert {
            scratch: vec![0.0; samples].into_boxed_slice(),
            spec,
        },
    };
    let _ = monotonic_now(); // initialise the epoch off the device thread

    let binding = match &l.clock {
        NullClock::Virtual(h) => Some((h.clone(), h.bind(flags.clone())?)),
        NullClock::Real => None,
    };
    let abandon = |flags: &RtFlags| flags.device_exited.store(true, Ordering::Release);

    let watcher = match watcher::spawn(flags.clone(), tx, "vynamp-output-watcher") {
        Ok(w) => w,
        Err(e) => {
            abandon(&flags);
            return Err(spawn_error(host)(e));
        }
    };
    if let Some((_, b)) = &binding {
        let _ = b.watcher.set(watcher.thread().clone());
    }

    let period_frames = u64::from(config.period_frames);
    let rate = config.rate;
    let name = "vynamp-output-device";
    let builder = || thread::Builder::new().name(name.into());
    let spawned = match binding {
        Some((handle, b)) => {
            let clock = VirtualClock {
                binding: b.clone(),
                inner: handle.inner.clone(),
                taken: 0,
                period_frames,
                rate,
            };
            let d = Driver::new(
                clock,
                l.sink,
                source,
                flags.clone(),
                config.clone(),
                native,
                pipeline,
                0,
            );
            builder().spawn(move || run_device_thread(d)).inspect(|h| {
                let _ = b.device.set(h.thread().clone());
            })
        }
        None => {
            let clock = RealClock {
                start: None,
                k: 0,
                period_frames,
                rate,
            };
            let d = Driver::new(
                clock,
                l.sink,
                source,
                flags.clone(),
                config.clone(),
                native,
                pipeline,
                0,
            );
            builder().spawn(move || run_device_thread(d))
        }
    };
    let device = match spawned {
        Ok(h) => h,
        Err(e) => {
            abandon(&flags);
            flags.watcher_exit.store(true, Ordering::Release);
            watcher.thread().unpark();
            let _ = watcher.join();
            return Err(spawn_error(host)(e));
        }
    };
    let backend = ThreadBackend {
        flags: flags.clone(),
        device: Some(device),
        finish: l.finish,
    };
    Ok(Stream::new(config, flags, Box::new(backend), watcher, rx))
}

/// A host with one virtual output device (`"null"`) that discards audio.
///
/// ```
/// use vynamp_output_core::*;
///
/// let host = NullHost::new(NullClock::Real);
/// let devices = host.enumerate().unwrap();
/// assert_eq!(devices.len(), 1);
/// assert_eq!(devices[0].host, HostId::Null);
/// let caps = host.capabilities(&devices[0].id, ShareMode::Exclusive).unwrap();
/// assert!(!caps.entries.is_empty());
/// ```
#[derive(Debug, Clone)]
pub struct NullHost {
    clock: NullClock,
    device: VirtualDevice,
}

impl NullHost {
    /// A Null host with [`default_capability_entries`].
    pub fn new(clock: NullClock) -> Self {
        Self::with_capabilities(clock, default_capability_entries())
    }

    /// A Null host whose device advertises `entries` (to test negotiation
    /// and downgrade paths).
    pub fn with_capabilities(clock: NullClock, entries: Vec<CapabilityEntry>) -> Self {
        NullHost {
            clock,
            device: VirtualDevice::new(HostId::Null, "null", "Null output", entries),
        }
    }

    /// Reports `os_mixer_bypassed` on opened exclusive streams (default
    /// `Yes`; shared streams always report `No`), e.g.
    /// `No` to model a mixed path, where DoP is refused.
    pub fn with_os_mixer_bypassed(mut self, bypassed: Tri) -> Self {
        self.device.os_mixer_bypassed = bypassed;
        self
    }

    /// Reports `endpoint_volume` on opened streams (default `Unity`); DoP is
    /// refused with `NotUnity`.
    pub fn with_endpoint_volume(mut self, volume: EndpointVolume) -> Self {
        self.device.endpoint_volume = volume;
        self
    }
}

impl Host for NullHost {
    fn id(&self) -> HostId {
        HostId::Null
    }

    fn enumerate(&self) -> Result<Vec<DeviceInfo>, Error> {
        Ok(vec![self.device.info()])
    }

    fn default_device(&self, _role: Role) -> Result<Option<DeviceInfo>, Error> {
        Ok(Some(self.device.info()))
    }

    fn capabilities(&self, device: &DeviceId, mode: ShareMode) -> Result<Capabilities, Error> {
        self.device.capabilities(device, mode)
    }

    fn open(&self, req: &StreamRequest, source: Box<dyn RenderSource>) -> Result<Stream, Error> {
        let (config, downgraded) = self.device.prepare(req)?;
        let launch_args = Launch {
            clock: self.clock.clone(),
            sink: crate::driver::Discard,
            finish: None,
        };
        launch(req, config, downgraded, source, launch_args)
    }
}
