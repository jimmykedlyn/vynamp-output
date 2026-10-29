//! The Capture host: a virtual device that records exactly the bytes the
//! device thread delivered, for bit-exact verification in tests.
//!
//! The device thread copies each period into a pre-allocated SPSC byte ring
//! (`ring.rs`); a non-RT writer thread flushes the ring to `<path>` as raw
//! little-endian interleaved samples, and on `stop` writes
//! `<path>.manifest`, a plain `key=value` text file:
//!
//! ```text
//! format=I24Packed
//! container_bytes=3
//! valid_bits=24
//! byte_order=little
//! rate=48000
//! channels=2
//! period_frames=64
//! frames=320
//! overruns=0
//! ```
//!
//! The ring holds at least 2 s of audio (and at least 4 periods). If it is
//! ever full, the period is dropped (never waited for) and counted in
//! `overruns`. One stream at a time per path.
//!
//! Internal test backend (spec §9.6): behind the core feature `capture`,
//! exposed by the facade only with its `test-capture` feature.

use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU64, AtomicUsize, Ordering};
use std::thread;
use std::time::Duration;

use crate::config::{EndpointVolume, StreamConfig, Tri};
use crate::error::Error;
use crate::format::{HostId, Justify, SampleFormat};
use crate::host::{DeviceInfo, Host};
use crate::negotiate::{Capabilities, CapabilityEntry};
use crate::null::{Launch, NullClock, VirtualDevice, default_capability_entries, launch};
use crate::render::RenderSource;
use crate::request::{DeviceId, Role, ShareMode, StreamRequest};
use crate::ring::{ByteRing, RingSink};
use crate::stream::Stream;

/// Largest ring the host allocates.
const MAX_RING_BYTES: u64 = 1 << 30;
/// Writer poll interval when the ring is empty.
const WRITER_POLL: Duration = Duration::from_millis(2);

impl ByteRing {
    /// A ring of at least `min_bytes` (rounded up to a power of two).
    pub(crate) fn with_capacity(min_bytes: usize) -> Self {
        let cap = min_bytes.max(1).next_power_of_two();
        ByteRing {
            slots: (0..cap).map(|_| AtomicU8::new(0)).collect(),
            mask: cap - 1,
            head: AtomicUsize::new(0),
            tail: AtomicUsize::new(0),
            overruns: AtomicU64::new(0),
            finish: AtomicBool::new(false),
        }
    }

    /// Consumer: appends every readable byte to `out` and frees the space.
    pub(crate) fn pop_into(&self, out: &mut Vec<u8>) {
        let tail = self.tail.load(Ordering::Relaxed);
        let head = self.head.load(Ordering::Acquire);
        let n = head.wrapping_sub(tail);
        out.reserve(n);
        out.extend(
            (0..n).map(|i| self.slots[tail.wrapping_add(i) & self.mask].load(Ordering::Relaxed)),
        );
        self.tail.store(head, Ordering::Release);
    }
}

/// Manifest name of a sample format.
fn format_name(f: SampleFormat) -> String {
    match f {
        SampleFormat::F32 => "F32".into(),
        SampleFormat::F64 => "F64".into(),
        SampleFormat::I16 => "I16".into(),
        SampleFormat::I24Packed => "I24Packed".into(),
        SampleFormat::I24In32 {
            justify: Justify::Msb,
        } => "I24In32Msb".into(),
        SampleFormat::I24In32 {
            justify: Justify::Lsb,
        } => "I24In32Lsb".into(),
        SampleFormat::I32 { .. } => "I32".into(),
        SampleFormat::U8 => "U8".into(),
        SampleFormat::Dop { dsd_rate } => format!("Dop{dsd_rate}"),
    }
}

fn io_error(call: &'static str) -> impl Fn(std::io::Error) -> Error {
    move |e| Error::Os {
        backend: HostId::Capture,
        code: i64::from(e.raw_os_error().unwrap_or(-1)),
        call,
    }
}

fn manifest_path(path: &std::path::Path) -> PathBuf {
    let mut s = path.as_os_str().to_owned();
    s.push(".manifest");
    PathBuf::from(s)
}

/// The writer thread: flushes the ring to the file until asked to finish,
/// then writes the manifest. Leaves without a manifest if the stream was
/// never created (all other ring owners dropped).
fn write_capture(
    ring: Arc<ByteRing>,
    file: File,
    path: PathBuf,
    config: StreamConfig,
) -> Result<(), Error> {
    let mut out = BufWriter::new(file);
    let mut chunk = Vec::new();
    let mut bytes = 0u64;
    let abandoned = loop {
        let finishing = ring.finish.load(Ordering::Acquire);
        let orphan = Arc::strong_count(&ring) == 1;
        chunk.clear();
        ring.pop_into(&mut chunk);
        if !chunk.is_empty() {
            out.write_all(&chunk).map_err(io_error("write capture"))?;
            bytes += chunk.len() as u64;
        } else if finishing || orphan {
            break orphan && !finishing;
        } else {
            thread::park_timeout(WRITER_POLL);
        }
    };
    out.flush().map_err(io_error("flush capture"))?;
    if abandoned {
        return Ok(());
    }
    let frame_bytes = u64::from(config.format.container_bytes()) * u64::from(config.channels);
    let manifest = format!(
        "format={}\ncontainer_bytes={}\nvalid_bits={}\nbyte_order=little\nrate={}\n\
         channels={}\nperiod_frames={}\nframes={}\noverruns={}\n",
        format_name(config.format),
        config.format.container_bytes(),
        config.valid_bits,
        config.rate,
        config.channels,
        config.period_frames,
        bytes / frame_bytes.max(1),
        ring.overruns.load(Ordering::Acquire),
    );
    std::fs::write(manifest_path(&path), manifest).map_err(io_error("write manifest"))
}

/// A host with one virtual device (`"capture"`) that records to a file.
#[derive(Debug, Clone)]
pub struct CaptureHost {
    path: PathBuf,
    clock: NullClock,
    device: VirtualDevice,
}

impl CaptureHost {
    /// Records to `path` (and `<path>.manifest`) on the real clock, with
    /// [`default_capability_entries`].
    pub fn new(path: PathBuf) -> Self {
        Self::with_capabilities(path, default_capability_entries())
    }

    /// Like [`new`](Self::new), advertising `entries`.
    pub fn with_capabilities(path: PathBuf, entries: Vec<CapabilityEntry>) -> Self {
        CaptureHost {
            path,
            clock: NullClock::Real,
            device: VirtualDevice::new(HostId::Capture, "capture", "Capture output", entries),
        }
    }

    /// Paces the device with `clock` (e.g. a virtual clock in tests).
    pub fn with_clock(mut self, clock: NullClock) -> Self {
        self.clock = clock;
        self
    }

    /// Reports `os_mixer_bypassed` on opened streams (default `Yes`), e.g.
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

impl Host for CaptureHost {
    fn id(&self) -> HostId {
        HostId::Capture
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
        let frame_bytes = u64::from(config.format.container_bytes()) * u64::from(config.channels);
        let two_seconds = 2 * u64::from(config.rate) * frame_bytes;
        let four_periods = 4 * u64::from(config.period_frames) * frame_bytes;
        let want = two_seconds.max(four_periods);
        if want > MAX_RING_BYTES {
            return Err(Error::InvalidRequest {
                what: "capture ring too large",
            });
        }
        let ring = Arc::new(ByteRing::with_capacity(want as usize));
        let file = File::create(&self.path).map_err(io_error("File::create"))?;
        let (r, path, cfg) = (ring.clone(), self.path.clone(), config.clone());
        let writer = thread::Builder::new()
            .name("vynamp-output-capture".into())
            .spawn(move || write_capture(r, file, path, cfg))
            .map_err(io_error("std::thread::Builder::spawn"))?;
        let r = ring.clone();
        let finish = Box::new(move || {
            r.finish.store(true, Ordering::Release);
            writer.thread().unpark();
            writer.join().unwrap_or(Err(Error::Os {
                backend: HostId::Capture,
                code: -1,
                call: "capture writer panicked",
            }))
        });
        let args = Launch {
            clock: self.clock.clone(),
            sink: RingSink(ring),
            finish: Some(finish),
        };
        launch(req, config, downgraded, source, args)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::driver::NativeBuf;

    fn period(v: &[u8]) -> NativeBuf {
        NativeBuf::U8(v.to_vec().into_boxed_slice())
    }

    #[test]
    fn full_ring_drops_whole_periods_and_wraps_in_order() {
        let ring = ByteRing::with_capacity(6); // rounds up to 8
        assert_eq!(ring.slots.len(), 8);
        assert!(ring.push_period(&period(&[1, 2, 3])));
        assert!(ring.push_period(&period(&[4, 5, 6])));
        assert!(!ring.push_period(&period(&[7, 8, 9])), "only 2 bytes free");
        let mut out = Vec::new();
        ring.pop_into(&mut out);
        assert_eq!(out, [1, 2, 3, 4, 5, 6]);
        assert!(
            ring.push_period(&period(&[7, 8, 9, 10, 11])),
            "wraps around"
        );
        out.clear();
        ring.pop_into(&mut out);
        assert_eq!(out, [7, 8, 9, 10, 11]);
    }

    #[test]
    fn samples_are_little_endian() {
        let ring = ByteRing::with_capacity(64);
        assert!(ring.push_period(&NativeBuf::I16(vec![0x0102, -2].into_boxed_slice())));
        assert!(ring.push_period(&NativeBuf::I24Packed(vec![[1, 2, 3]].into_boxed_slice())));
        assert!(ring.push_period(&NativeBuf::I32(vec![0x0A0B_0C0D].into_boxed_slice())));
        assert!(ring.push_period(&NativeBuf::F32(vec![1.0].into_boxed_slice())));
        let mut out = Vec::new();
        ring.pop_into(&mut out);
        assert_eq!(
            out,
            [
                2, 1, 0xFE, 0xFF, 1, 2, 3, 0x0D, 0x0C, 0x0B, 0x0A, 0, 0, 0x80, 0x3F
            ]
        );
    }

    #[test]
    fn format_names() {
        assert_eq!(format_name(SampleFormat::I32 { valid_bits: 24 }), "I32");
        assert_eq!(
            format_name(SampleFormat::I24In32 {
                justify: Justify::Msb
            }),
            "I24In32Msb"
        );
        assert_eq!(
            format_name(SampleFormat::Dop {
                dsd_rate: 2_822_400
            }),
            "Dop2822400"
        );
    }
}
