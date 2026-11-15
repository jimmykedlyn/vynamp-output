//! The WASAPI host: exclusive-mode playback on Windows.
//!
//! This module is compiled only on Windows and only with the `wasapi`
//! feature. It is the `Host` implementation the facade lists first; the FFI
//! itself lives in `vynamp-output-wasapi-sys`, and this file is the
//! translation between that crate's primitives and this crate's types.
//!
//! # What it does not do
//!
//! - It never falls back to shared mode. A failed exclusive open is an
//!   error, and `OnUnavailable::Downgrade` only ever picks another format,
//!   never another mode.
//! - It never reopens another device. When the endpoint goes away the stream
//!   moves to `Lost` and emits `DeviceLost`; recovery is an explicit
//!   `resume` after the device returns, or a new `open`.
//! - It does not resample. The rate is the one the device accepted, and a
//!   device that cannot take it is `FormatUnsupported`.

use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::sync::mpsc::channel;
use std::thread::{self, JoinHandle};

use crate::config::{EndpointVolume, PathProperties, RtPriority, StreamConfig, Transport, Tri};
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
use crate::wasapi::{SampleKind, to_wasapi};
use crate::watcher;

use vynamp_output_wasapi_sys as sys;

/// The rates the backend probes on a device, in ascending order.
const PROBE_RATES: [u32; 10] = [
    44_100, 48_000, 88_200, 96_000, 176_400, 192_000, 352_800, 384_000, 705_600, 768_000,
];

/// The channel counts the backend probes.
const PROBE_CHANNELS: [u16; 8] = [1, 2, 3, 4, 5, 6, 7, 8];

/// The formats the backend probes, as `(kind, container, valid)`.
const PROBE_FORMATS: [(SampleKind, u16, u16); 5] = [
    (SampleKind::Integer, 16, 16),
    (SampleKind::Integer, 24, 24),
    (SampleKind::Integer, 32, 24),
    (SampleKind::Integer, 32, 32),
    (SampleKind::Float, 32, 32),
];

/// A WASAPI host.
///
/// Holds no COM object between calls: each enumeration and each open creates
/// its own enumerator, so a device that appears or disappears between calls
/// is seen as it is, and no apartment is held on a thread that is not using
/// it.
#[derive(Debug, Clone, Copy, Default)]
pub struct WasapiHost;

impl WasapiHost {
    /// A host with no configuration; the backend has no knobs yet.
    pub fn new() -> Self {
        WasapiHost
    }

    /// The MMDevice enumerator for one call.
    fn enumerator() -> Result<sys::Enumerator, Error> {
        sys::Enumerator::new().map_err(|hr| Error::Os {
            backend: HostId::Wasapi,
            code: hr.code(),
            call: "CoCreateInstance(MMDeviceEnumerator)",
        })
    }

    /// The `DeviceInfo` for a raw entry.
    fn info(entry: &sys::DeviceEntry) -> DeviceInfo {
        DeviceInfo {
            id: DeviceId(entry.id.0.clone()),
            // MMDevice ids are stable across reboots for a given endpoint.
            id_stability: IdStability::Persistent,
            name: entry.name.clone(),
            host: HostId::Wasapi,
            default_for: entry.roles.iter().filter_map(|r| role_of(*r)).collect(),
            transport: Transport::Unknown,
        }
    }

    /// Probes `device` for what it accepts in `mode`.
    ///
    /// Every entry is `Probed`, not `Advertised`: WASAPI has no capability
    /// list to read, so the only honest answer is what `IsFormatSupported`
    /// said for each format the backend tried.
    fn probe(device: &DeviceId, mode: ShareMode) -> Result<Capabilities, Error> {
        let enumerator = Self::enumerator()?;
        let raw_device = sys::DeviceId(device.0.clone());
        let exclusive = mode == ShareMode::Exclusive;
        let entries = probe_entries(mode, |kind, container, valid, ch, rate| {
            let format =
                sys::WaveFormatExtensible::new(sys_kind(kind), ch, rate, container, valid, 0);
            sys::ExclusiveStream::is_format_supported(&enumerator, &raw_device, &format, exclusive)
                .map_err(|s| os_error("IAudioClient::IsFormatSupported", s))
        })?;
        Ok(Capabilities {
            host: HostId::Wasapi,
            device: device.clone(),
            entries,
        })
    }
}

/// Builds the capability entries from a `supported` predicate.
///
/// One entry per `(format, channel count)`, with exactly the rates that work
/// at that channel count. The split matters: `CapabilityEntry` holds `rates`
/// and `channels` as independent lists, and negotiation treats them as a
/// cross product, so a single entry per format would claim 8 channels at a
/// rate the device only does in stereo. That over-claim would then fail at
/// `open`, which is exactly the "promise more than the code delivers" the
/// support matrix exists to prevent.
///
/// Pure, so it is tested on every target with a fake predicate.
fn probe_entries(
    mode: ShareMode,
    mut supported: impl FnMut(SampleKind, u16, u16, u16, u32) -> Result<bool, Error>,
) -> Result<Vec<CapabilityEntry>, Error> {
    let mut entries = Vec::new();
    for (kind, container, valid) in PROBE_FORMATS {
        for ch in PROBE_CHANNELS {
            let mut rates = Vec::new();
            for rate in PROBE_RATES {
                if supported(kind, container, valid, ch, rate)? {
                    rates.push(rate);
                }
            }
            if !rates.is_empty() {
                entries.push(CapabilityEntry {
                    mode,
                    format: sample_format(kind, container, valid),
                    rates,
                    channels: vec![ch],
                    origin: CapabilityOrigin::Probed,
                });
            }
        }
    }
    Ok(entries)
}

/// The `-sys` crate's `SampleKind` for this crate's.
///
/// The two enums are deliberately separate: the boundary rule says no domain
/// type crosses a repo boundary, and `SampleKind` is the `-sys` crate's own
/// primitive vocabulary. This is the one place they meet.
fn sys_kind(kind: SampleKind) -> sys::SampleKind {
    match kind {
        SampleKind::Integer => sys::SampleKind::Integer,
        SampleKind::Float => sys::SampleKind::Float,
    }
}

/// Maps an `ERole` value to this crate's `Role`.
fn role_of(role: i32) -> Option<Role> {
    match role {
        sys::ROLE_CONSOLE => Some(Role::Console),
        sys::ROLE_MULTIMEDIA => Some(Role::Multimedia),
        sys::ROLE_COMMUNICATIONS => Some(Role::Communications),
        _ => None,
    }
}

/// The `ERole` value for a `Role`.
fn role_value(role: Role) -> i32 {
    match role {
        Role::Console => sys::ROLE_CONSOLE,
        Role::Multimedia => sys::ROLE_MULTIMEDIA,
        Role::Communications => sys::ROLE_COMMUNICATIONS,
    }
}

/// The `SampleFormat` for a probed `(kind, container, valid)`.
fn sample_format(kind: SampleKind, container: u16, valid: u16) -> SampleFormat {
    match (kind, container, valid) {
        (SampleKind::Float, 32, _) => SampleFormat::F32,
        (SampleKind::Float, 64, _) => SampleFormat::F64,
        (SampleKind::Integer, 8, _) => SampleFormat::U8,
        (SampleKind::Integer, 16, _) => SampleFormat::I16,
        (SampleKind::Integer, 24, _) => SampleFormat::I24Packed,
        (SampleKind::Integer, 32, 24) => SampleFormat::I24In32 {
            justify: crate::format::Justify::Msb,
        },
        (SampleKind::Integer, 32, n) => SampleFormat::I32 {
            valid_bits: n as u8,
        },
        _ => SampleFormat::I32 { valid_bits: 32 },
    }
}

/// An OS failure as this crate's error.
fn os_error(call: &'static str, status: sys::Status) -> Error {
    match status {
        sys::Status::Busy => Error::DeviceBusy,
        sys::Status::ExclusiveNotAllowed => Error::ExclusiveUnavailable {
            reason: "exclusive mode is disabled for this device",
        },
        sys::Status::UnsupportedFormat => Error::FormatUnsupported { nearest: None },
        sys::Status::AccessDenied => Error::PermissionDenied,
        s if s.is_device_gone() => Error::DeviceLost,
        sys::Status::Other(code) => Error::Os {
            backend: HostId::Wasapi,
            code: i64::from(code),
            call,
        },
        _ => Error::Os {
            backend: HostId::Wasapi,
            code: -1,
            call,
        },
    }
}

/// The `Error` for a failed open.
fn open_error(e: sys::OpenError) -> Error {
    match e {
        sys::OpenError::Busy => Error::DeviceBusy,
        sys::OpenError::ExclusiveNotAllowed => Error::ExclusiveUnavailable {
            reason: "exclusive mode is disabled for this device",
        },
        sys::OpenError::UnsupportedFormat => Error::FormatUnsupported { nearest: None },
        sys::OpenError::DeviceGone => Error::DeviceLost,
        sys::OpenError::NotFound => Error::DeviceNotFound,
        sys::OpenError::Failed(s) => os_error("IAudioClient::Initialize", s),
    }
}

impl Host for WasapiHost {
    fn id(&self) -> HostId {
        HostId::Wasapi
    }

    fn enumerate(&self) -> Result<Vec<DeviceInfo>, Error> {
        let enumerator = Self::enumerator()?;
        let devices = enumerator.render_devices().map_err(|hr| Error::Os {
            backend: HostId::Wasapi,
            code: hr.code(),
            call: "IMMDeviceEnumerator::EnumAudioEndpoints",
        })?;
        Ok(devices.iter().map(Self::info).collect())
    }

    fn default_device(&self, role: Role) -> Result<Option<DeviceInfo>, Error> {
        let enumerator = Self::enumerator()?;
        let device = enumerator
            .default_device(role_value(role))
            .map_err(|hr| Error::Os {
                backend: HostId::Wasapi,
                code: hr.code(),
                call: "IMMDeviceEnumerator::GetDefaultAudioEndpoint",
            })?;
        Ok(device.as_ref().map(Self::info))
    }

    fn capabilities(&self, device: &DeviceId, mode: ShareMode) -> Result<Capabilities, Error> {
        Self::probe(device, mode)
    }

    fn open(&self, req: &StreamRequest, source: Box<dyn RenderSource>) -> Result<Stream, Error> {
        let device = match &req.device {
            DeviceSelector::Id(id) => id.clone(),
            DeviceSelector::Default(role) => {
                self.default_device(*role)?.ok_or(Error::DeviceNotFound)?.id
            }
        };
        let caps = self.capabilities(&device, req.mode)?;
        let negotiated = negotiate(req, &caps)?;
        let mut config = negotiated.config_template;
        let wasapi = to_wasapi(config.format, config.channels, config.rate, &config.layout)?;
        let period = period_frames(req, config.rate)?;
        let enumerator = Self::enumerator()?;
        let stream = sys::ExclusiveStream::open(
            &enumerator,
            &sys::DeviceId(device.0.clone()),
            sys::OpenRequest {
                exclusive: req.mode == ShareMode::Exclusive,
                format: sys::WaveFormatExtensible::new(
                    sys_kind(wasapi.kind),
                    wasapi.channels,
                    wasapi.rate,
                    wasapi.container_bits,
                    wasapi.valid_bits,
                    wasapi.channel_mask,
                ),
                period_frames: period,
            },
        )
        .map_err(open_error)?;
        let granted = stream.format();
        config.period_frames = granted.period_frames;
        config.buffer_frames = granted.buffer_frames;
        config.path = PathProperties {
            // Exclusive mode is the only mode this backend opens, and it is
            // the mode in which the OS mixer is out of the path.
            os_mixer_bypassed: match req.mode {
                ShareMode::Exclusive => Tri::Yes,
                ShareMode::Shared => Tri::No,
            },
            os_resampling: false,
            library_conversion: config.path.library_conversion,
            // WASAPI does not expose endpoint volume through this path; the
            // honest answer is `Unknown`, never a guess of unity.
            endpoint_volume: EndpointVolume::Unknown,
            transport: Transport::Unknown,
            rt_priority: RtPriority::Denied("not requested by this backend yet"),
        };
        launch(req, config, negotiated.downgraded, source, stream)
    }
}

/// The period in frames for `req`, or `InvalidRequest`.
fn period_frames(req: &StreamRequest, rate: u32) -> Result<u32, Error> {
    let frames = match req.buffer {
        BufferRequest::Frames(n) => u64::from(n),
        BufferRequest::Duration(d) => {
            let f = u128::from(rate) * d.as_nanos() / 1_000_000_000;
            u64::try_from(f).unwrap_or(u64::MAX)
        }
        BufferRequest::BackendDefault => u64::from(rate / 100).max(1),
    };
    if frames == 0 {
        return Err(Error::InvalidRequest {
            what: "buffer is zero",
        });
    }
    u32::try_from(frames).map_err(|_| Error::InvalidRequest {
        what: "buffer too large",
    })
}

/// The device thread's clock: waits on the WASAPI event.
struct WasapiClock {
    stream: Arc<sys::ExclusiveStream>,
}

impl PeriodClock for WasapiClock {
    fn wait(&mut self, flags: &RtFlags) -> Option<MonotonicNanos> {
        loop {
            if flags.exit.load(Ordering::Acquire) {
                return None;
            }
            let status = self.stream.wait_period();
            if status.is_ok() {
                return Some(monotonic_now());
            }
            if status.is_device_gone() {
                // The endpoint vanished: report it and leave the loop. The
                // stream moves to `Lost`; nothing reopens another device.
                flags.notify_lost();
                return None;
            }
            // A transient failure (a stalled period) is not fatal; the next
            // wait retries, and the exit flag is re-checked at the top.
        }
    }
}

/// The device thread's sink: copies the period into the OS buffer.
struct WasapiSink {
    stream: Arc<sys::ExclusiveStream>,
    bytes: Box<[u8]>,
    flags: Arc<RtFlags>,
}

impl PeriodSink for WasapiSink {
    fn deliver(&mut self, period: &NativeBuf) {
        let n = write_native(period, &mut self.bytes);
        let Some(slice) = self.bytes.get(..n) else {
            return;
        };
        match self.stream.write_period(slice) {
            Ok(()) => {}
            // The endpoint vanished between the wait and the write. Report it
            // the same way the wait path does, so the stream moves to `Lost`
            // and the watcher emits `DeviceLost`; the next wait then leaves
            // the loop. Swallowing it would keep the thread spinning on a
            // dead device until the 2 s wait timeout.
            Err(status) if status.is_device_gone() => self.flags.notify_lost(),
            // A transient write failure (a stalled period) is not fatal: the
            // next wait retries, exactly as in `WasapiClock::wait`.
            Err(_) => {}
        }
    }
}

/// Serialises `period` into `dst` as little-endian interleaved bytes, and
/// returns how many bytes were written.
fn write_native(period: &NativeBuf, dst: &mut [u8]) -> usize {
    let mut at = 0usize;
    let mut put = |bytes: &[u8]| {
        if let Some(slot) = dst.get_mut(at..at + bytes.len()) {
            slot.copy_from_slice(bytes);
            at += bytes.len();
        }
    };
    match period {
        NativeBuf::F32(b) => b.iter().for_each(|s| put(&s.to_le_bytes())),
        NativeBuf::F64(b) => b.iter().for_each(|s| put(&s.to_le_bytes())),
        NativeBuf::I16(b) => b.iter().for_each(|s| put(&s.to_le_bytes())),
        NativeBuf::I24Packed(b) => b.iter().for_each(|s| put(s)),
        NativeBuf::I32(b) => b.iter().for_each(|s| put(&s.to_le_bytes())),
        NativeBuf::U8(b) => put(b),
    }
    at
}

/// The control-side handle of a WASAPI stream.
struct WasapiBackend {
    stream: Arc<sys::ExclusiveStream>,
    device: Option<JoinHandle<()>>,
    flags: Arc<RtFlags>,
}

impl BackendStream for WasapiBackend {
    fn start(&mut self) -> Result<(), Error> {
        let status = self.stream.start();
        if status.is_ok() {
            Ok(())
        } else {
            Err(os_error("IAudioClient::Start", status))
        }
    }

    fn pause(&mut self) -> Result<(), Error> {
        let status = self.stream.stop();
        if status.is_ok() {
            Ok(())
        } else {
            Err(os_error("IAudioClient::Stop", status))
        }
    }

    fn stop(&mut self) -> Result<(), Error> {
        self.flags.exit.store(true, Ordering::Release);
        // We expect `IAudioClient::Stop` to make the OS signal the stream's
        // event, which is what the device thread is blocked on in
        // `wait_period`; Microsoft does not document that, so it is an
        // assumption, not a fact. If it does not hold, the thread sits out
        // the bounded 2 s `WaitForSingleObject` timeout before it re-reads
        // `exit`, which is the fallback. `unpark` below cannot wake that wait.
        let status = self.stream.stop();
        if let Some(h) = self.device.take() {
            h.thread().unpark();
            let _ = h.join();
        }
        if status.is_ok() {
            Ok(())
        } else {
            Err(os_error("IAudioClient::Stop", status))
        }
    }
}

/// Monotonic time for the timing snapshot.
fn monotonic_now() -> MonotonicNanos {
    use std::sync::OnceLock;
    use std::time::Instant;
    static EPOCH: OnceLock<Instant> = OnceLock::new();
    let epoch = *EPOCH.get_or_init(Instant::now);
    MonotonicNanos(u64::try_from(epoch.elapsed().as_nanos()).unwrap_or(u64::MAX))
}

/// Starts the watcher and the device thread and returns the stream.
fn launch(
    req: &StreamRequest,
    config: StreamConfig,
    downgraded: Option<&'static str>,
    source: Box<dyn RenderSource>,
    stream: sys::ExclusiveStream,
) -> Result<Stream, Error> {
    let flags = Arc::new(RtFlags::new());
    let (tx, rx) = channel();
    if let Some(reason) = downgraded {
        let _ = tx.send(Event::Downgraded {
            requested: Box::new(req.clone()),
            actual: Box::new(config.clone()),
            reason,
        });
    }
    let stream = Arc::new(stream);
    let samples = config.period_frames as usize * usize::from(config.channels);
    let native = native_buffer(config.format, samples);
    let pipeline = match config.path.library_conversion {
        None => Pipeline::Direct,
        Some(spec) => Pipeline::Convert {
            scratch: vec![0.0; samples].into_boxed_slice(),
            spec,
        },
    };
    let bytes =
        vec![0u8; samples * usize::from(config.format.container_bytes())].into_boxed_slice();
    let watcher =
        watcher::spawn(flags.clone(), tx, "vynamp-output-wasapi-watcher").map_err(|e| {
            Error::Os {
                backend: HostId::Wasapi,
                code: i64::from(e.raw_os_error().unwrap_or(-1)),
                call: "std::thread::Builder::spawn",
            }
        })?;
    let driver = Driver::new(
        WasapiClock {
            stream: stream.clone(),
        },
        WasapiSink {
            stream: stream.clone(),
            bytes,
            flags: flags.clone(),
        },
        source,
        flags.clone(),
        config.clone(),
        native,
        pipeline,
        0,
    );
    let device = thread::Builder::new()
        .name("vynamp-output-wasapi-device".into())
        .spawn(move || run_device_thread(driver))
        .map_err(|e| Error::Os {
            backend: HostId::Wasapi,
            code: i64::from(e.raw_os_error().unwrap_or(-1)),
            call: "std::thread::Builder::spawn",
        })?;
    let backend = WasapiBackend {
        stream,
        device: Some(device),
        flags: flags.clone(),
    };
    // Recorded here, at the single point where this backend opens a device:
    // the list can only ever hold the endpoint `open` actually opened, so a
    // fallback to another device would have to add a second id to be read
    // back, and there is no code path that does.
    let open_attempts = vec![config.device.clone()];
    Ok(Stream::new(
        config,
        flags,
        Box::new(backend),
        watcher,
        rx,
        open_attempts,
    ))
}

/// One period of native samples, zeroed.
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sample_format_maps_the_probed_tuples() {
        assert_eq!(
            sample_format(SampleKind::Integer, 16, 16),
            SampleFormat::I16
        );
        assert_eq!(
            sample_format(SampleKind::Integer, 24, 24),
            SampleFormat::I24Packed
        );
        assert_eq!(
            sample_format(SampleKind::Integer, 32, 24),
            SampleFormat::I24In32 {
                justify: crate::format::Justify::Msb
            }
        );
        assert_eq!(
            sample_format(SampleKind::Integer, 32, 32),
            SampleFormat::I32 { valid_bits: 32 }
        );
        assert_eq!(sample_format(SampleKind::Float, 32, 32), SampleFormat::F32);
    }

    #[test]
    fn roles_round_trip() {
        for role in [Role::Console, Role::Multimedia, Role::Communications] {
            assert_eq!(role_of(role_value(role)), Some(role));
        }
        assert_eq!(role_of(99), None);
    }

    #[test]
    fn write_native_serialises_little_endian() {
        let mut dst = [0u8; 8];
        let n = write_native(
            &NativeBuf::I16(vec![0x0102, -2].into_boxed_slice()),
            &mut dst,
        );
        assert_eq!(n, 4);
        assert_eq!(&dst[..4], &[0x02, 0x01, 0xFE, 0xFF]);
    }

    #[test]
    fn probe_entries_never_claim_a_rate_at_a_channel_count_that_lacks_it() {
        // A device that does 8 channels only at 48 kHz and stereo at 96 kHz.
        // One entry per format would claim 8 ch at 96 kHz, which `open`
        // would then fail; the split must keep them apart.
        let entries = probe_entries(ShareMode::Exclusive, |kind, container, valid, ch, rate| {
            if kind != SampleKind::Integer || container != 24 || valid != 24 {
                return Ok(false);
            }
            Ok(matches!(
                (ch, rate),
                (2, 48_000) | (2, 96_000) | (8, 48_000)
            ))
        })
        .unwrap();
        let stereo = entries
            .iter()
            .find(|e| e.channels == vec![2])
            .expect("a stereo entry");
        assert_eq!(stereo.rates, vec![48_000, 96_000]);
        let eight = entries
            .iter()
            .find(|e| e.channels == vec![8])
            .expect("an 8-channel entry");
        assert_eq!(eight.rates, vec![48_000], "8 ch must not claim 96 kHz");
    }

    #[test]
    fn probe_entries_skip_a_format_the_device_rejects_entirely() {
        let entries = probe_entries(ShareMode::Exclusive, |_, _, _, _, _| Ok(false)).unwrap();
        assert!(entries.is_empty());
    }

    #[test]
    fn probe_entries_are_probed_not_advertised() {
        let entries =
            probe_entries(ShareMode::Shared, |_, _, _, _, rate| Ok(rate == 48_000)).unwrap();
        assert!(!entries.is_empty());
        for e in &entries {
            assert_eq!(e.origin, CapabilityOrigin::Probed);
            assert_eq!(e.mode, ShareMode::Shared);
        }
    }

    #[test]
    fn probe_entries_propagate_a_probe_failure() {
        let err = probe_entries(ShareMode::Exclusive, |_, _, _, _, _| Err(Error::DeviceLost));
        assert_eq!(err, Err(Error::DeviceLost));
    }

    #[test]
    fn write_native_stops_at_the_end_of_dst() {
        let mut dst = [0u8; 3];
        let n = write_native(&NativeBuf::I16(vec![1, 2, 3].into_boxed_slice()), &mut dst);
        assert_eq!(n, 2, "only one whole sample fits");
    }
}
