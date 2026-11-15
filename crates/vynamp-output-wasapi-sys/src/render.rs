//! The exclusive-mode render stream: open a device in WASAPI exclusive with
//! event-driven buffering, pull a period, write it, release it.
//!
//! This is the only place that owns a device; everything the caller sees is
//! plain data or [`Status`]. The stream never falls back to shared mode and
//! never reopens another device: if the endpoint goes away the call returns a
//! status whose [`Status::is_device_gone`] is true, and the caller decides.
//!
//! Declarations and their order are from the public documentation recorded in
//! `docs/sources/wasapi.md`.

use std::ffi::c_void;

use crate::Hresult;
use crate::audioclient::{
    AudioClient, AudioRenderClient, IID_IAUDIO_CLIENT, SHAREMODE_EXCLUSIVE, SHAREMODE_SHARED,
    STREAMFLAGS_EVENTCALLBACK, STREAMFLAGS_NOPERSIST,
};
use crate::com::ComPtr;
use crate::device::{DeviceId, Enumerator};
use crate::format::WaveFormatExtensible;
use crate::mmdevice;
use crate::status::Status;

/// `REFERENCE_TIME` units per second (100 ns each).
const HNS_PER_SEC: i64 = 10_000_000;

/// How a stream is opened.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OpenRequest {
    /// Exclusive or shared. The backend never changes this on its own.
    pub exclusive: bool,
    /// The format to negotiate.
    pub format: WaveFormatExtensible,
    /// Requested period, in frames; the OS may clamp it.
    pub period_frames: u32,
}

/// The format a stream ended up with after the OS clamped the period.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StreamFormat {
    /// The format the device accepted.
    pub format: WaveFormatExtensible,
    /// The period the OS actually granted, in frames.
    pub period_frames: u32,
    /// The endpoint buffer the OS allocated, in frames (two periods in
    /// exclusive event-driven mode).
    pub buffer_frames: u32,
}

/// Why a stream could not be opened.
///
/// The distinction matters on the other side of the boundary: `Busy` is
/// `Error::DeviceBusy`, `ExclusiveNotAllowed` is
/// `Error::ExclusiveUnavailable`, `UnsupportedFormat` is
/// `Error::FormatUnsupported`, and none of them is retried in shared mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpenError {
    /// Another client holds the endpoint in exclusive mode.
    Busy,
    /// The user disabled exclusive use of this device.
    ExclusiveNotAllowed,
    /// The device does not accept the requested format.
    UnsupportedFormat,
    /// The device is gone (unplugged before the stream opened).
    DeviceGone,
    /// The device id is not an active render endpoint.
    NotFound,
    /// Any other OS failure, with its status.
    Failed(Status),
}

impl OpenError {
    /// Classifies a failed open.
    fn from_status(s: Status) -> Self {
        match s {
            Status::Busy => OpenError::Busy,
            Status::ExclusiveNotAllowed => OpenError::ExclusiveNotAllowed,
            Status::UnsupportedFormat => OpenError::UnsupportedFormat,
            s if s.is_device_gone() => OpenError::DeviceGone,
            s => OpenError::Failed(s),
        }
    }
}

/// An open render stream.
///
/// Owns the `IAudioClient`, the `IAudioRenderClient`, the event handle and a
/// reference to the device, so the endpoint stays open until this is dropped.
pub struct ExclusiveStream {
    _device: ComPtr<mmdevice::ImmDevice>,
    client: ComPtr<AudioClient>,
    render: ComPtr<AudioRenderClient>,
    event: *mut c_void,
    format: StreamFormat,
}

// SAFETY: the interfaces are used from one thread at a time (the caller's
// device thread); moving the stream between threads moves every handle with
// it, and no raw pointer is shared. The COM objects are apartment-neutral
// under `COINIT_MULTITHREADED`.
unsafe impl Send for ExclusiveStream {}
// SAFETY: the stream is shared between the control thread (start/stop) and
// the device thread (wait/write). WASAPI documents `IAudioClient::Start`,
// `Stop` and the render client's `GetBuffer`/`ReleaseBuffer` as callable
// from any thread of the initialised apartment, and the event handle is a
// kernel object that is safe to wait on from another thread. The backend
// never calls `start`/`stop` concurrently with `wait_period`/`write_period`
// on the same stream: `start`/`stop` happen on the control thread only
// while the device thread is parked in `wait_period`, and the OS serialises
// the two.
unsafe impl Sync for ExclusiveStream {}

impl ExclusiveStream {
    /// Opens `device` according to `req`.
    ///
    /// Exclusive mode is requested exactly when `req.exclusive` is true; a
    /// failed exclusive open is returned as an error, never retried in shared
    /// mode. The period is taken from the request and clamped by the OS, and
    /// the granted values are reported in [`StreamFormat`].
    pub fn open(
        enumerator: &Enumerator,
        device: &DeviceId,
        req: OpenRequest,
    ) -> Result<Self, OpenError> {
        let raw = enumerator
            .device_by_id(device)
            .map_err(|hr| OpenError::from_status(Status::from_hresult(hr)))?
            .ok_or(OpenError::NotFound)?;
        Self::open_on(raw, req)
    }

    /// The format and timings the stream actually got.
    pub fn format(&self) -> StreamFormat {
        self.format
    }

    /// Whether `device` accepts `format` in `exclusive` (or shared) mode,
    /// without opening a stream.
    ///
    /// This is `IAudioClient::IsFormatSupported`, which the OS documents as
    /// callable before `Initialize`. It is how the backend builds
    /// `Capabilities` from what the device advertises instead of guessing.
    pub fn is_format_supported(
        enumerator: &Enumerator,
        device: &DeviceId,
        format: &WaveFormatExtensible,
        exclusive: bool,
    ) -> Result<bool, Status> {
        let raw = enumerator
            .device_by_id(device)
            .map_err(Status::from_hresult)?
            .ok_or(Status::Other(Hresult::NOT_FOUND.0))?;
        let client = raw
            .activate(&IID_IAUDIO_CLIENT)
            .map_err(Status::from_hresult)?;
        let share = if exclusive {
            SHAREMODE_EXCLUSIVE
        } else {
            SHAREMODE_SHARED
        };
        let hr = client.is_format_supported(share, &format.to_bytes());
        if hr.succeeded() {
            return Ok(true);
        }
        match Status::from_hresult(hr) {
            Status::UnsupportedFormat => Ok(false),
            other => Err(other),
        }
    }

    /// Starts the device. After this the event is signalled once per period.
    pub fn start(&self) -> Status {
        Status::from_hresult(self.client.start())
    }

    /// Stops the device. Queued audio is discarded by the OS.
    pub fn stop(&self) -> Status {
        Status::from_hresult(self.client.stop())
    }

    /// Blocks until the device signals that one period is ready to fill.
    ///
    /// This is the device thread's wait; it performs no I/O and allocates
    /// nothing. Returns the OS status, so a vanished endpoint surfaces as
    /// [`Status::is_device_gone`].
    pub fn wait_period(&self) -> Status {
        Status::from_hresult(wait_for_event(self.event))
    }

    /// Copies one period of raw interleaved bytes into the device buffer and
    /// releases it.
    ///
    /// `src` must hold exactly `period_frames * block_align` bytes. The copy
    /// is the only work on the device thread besides the render itself.
    pub fn write_period(&self, src: &[u8]) -> Result<(), Status> {
        let frames = self.format.period_frames;
        let want = frames as usize * usize::from(self.format.format.block_align());
        if src.len() != want {
            return Err(Status::InvalidArgument);
        }
        // SAFETY: `self.render` is a live render client, and `frames` is the
        // period the OS granted for this stream, so `GetBuffer` returns a
        // pointer valid for `want` bytes until the matching `ReleaseBuffer`.
        let dst = unsafe { self.render.get_buffer(frames) }.map_err(Status::from_hresult)?;
        // SAFETY: as above, `dst` is valid for `want` bytes; `src` is exactly
        // `want` bytes (checked above) and does not overlap `dst`, which
        // lives in OS-owned memory.
        unsafe {
            std::ptr::copy_nonoverlapping(src.as_ptr(), dst, want);
        }
        let released = Status::from_hresult(self.render.release_buffer(frames, 0));
        if !released.is_ok() {
            return Err(released);
        }
        Ok(())
    }

    /// Opens `raw` with `req`.
    fn open_on(raw: ComPtr<mmdevice::ImmDevice>, req: OpenRequest) -> Result<Self, OpenError> {
        let client = raw
            .activate(&IID_IAUDIO_CLIENT)
            .map_err(|hr| OpenError::from_status(Status::from_hresult(hr)))?;
        let bytes = req.format.to_bytes();
        let share = if req.exclusive {
            SHAREMODE_EXCLUSIVE
        } else {
            SHAREMODE_SHARED
        };

        // Exclusive event-driven streams need buffer == period, both nonzero;
        // shared streams take 0/0 and let the engine size the buffer.
        let hns = hns_for_frames(req.period_frames, req.format.rate);
        let (buffer_hns, period_hns) = if req.exclusive { (hns, hns) } else { (0, 0) };
        let flags = STREAMFLAGS_EVENTCALLBACK | STREAMFLAGS_NOPERSIST;

        let mut hr = client.initialize(share, flags, buffer_hns, period_hns, &bytes);
        // The OS can ask for a realigned buffer in exclusive event mode; the
        // documented answer is to read the size it wants and retry once.
        if hr == Hresult::BUFFER_SIZE_NOT_ALIGNED && req.exclusive {
            let frames = client
                .buffer_size()
                .map_err(|e| OpenError::from_status(Status::from_hresult(e)))?;
            let aligned = hns_for_frames(frames, req.format.rate);
            hr = client.initialize(share, flags, aligned, aligned, &bytes);
        }
        if hr.failed() {
            return Err(OpenError::from_status(Status::from_hresult(hr)));
        }

        let event = create_event().map_err(OpenError::from_status)?;
        let set = client.set_event_handle(event);
        if set.failed() {
            close_event(event);
            return Err(OpenError::from_status(Status::from_hresult(set)));
        }

        let render = match client.render_client() {
            Ok(r) => r,
            Err(hr) => {
                close_event(event);
                return Err(OpenError::from_status(Status::from_hresult(hr)));
            }
        };

        let buffer_frames = client
            .buffer_size()
            .map_err(|e| OpenError::from_status(Status::from_hresult(e)))?;
        // In exclusive event mode the packet is always the whole buffer; in
        // shared mode the caller reads padding per callback instead.
        let period_frames = if req.exclusive {
            buffer_frames
        } else {
            req.period_frames
        };
        Ok(ExclusiveStream {
            _device: raw,
            client,
            render,
            event,
            format: StreamFormat {
                format: req.format,
                period_frames,
                buffer_frames,
            },
        })
    }
}

impl Drop for ExclusiveStream {
    fn drop(&mut self) {
        let _ = self.client.stop();
        let _ = self.client.reset();
        close_event(self.event);
        // `render`, `client` and `_device` release their COM references in
        // that field order when this struct drops.
    }
}

/// Frames to `REFERENCE_TIME` (100 ns units), rounded to nearest.
fn hns_for_frames(frames: u32, rate: u32) -> i64 {
    if rate == 0 {
        return 0;
    }
    (i64::from(frames) * HNS_PER_SEC + i64::from(rate) / 2) / i64::from(rate)
}

unsafe extern "system" {
    fn CreateEventW(
        attributes: *const c_void,
        manual_reset: i32,
        initial_state: i32,
        name: *const u16,
    ) -> *mut c_void;
    fn WaitForSingleObject(handle: *mut c_void, milliseconds: u32) -> u32;
    fn CloseHandle(handle: *mut c_void) -> i32;
}

/// `WAIT_OBJECT_0`.
const WAIT_OBJECT_0: u32 = 0;
/// `WAIT_TIMEOUT`.
const WAIT_TIMEOUT: u32 = 258;
/// How long one period may take before the device is considered stalled.
const PERIOD_WAIT_MS: u32 = 2_000;

/// Creates an auto-reset event.
fn create_event() -> Result<*mut c_void, Status> {
    // SAFETY: all four arguments are documented as nullable/default;
    // `CreateEventW` returns null on failure.
    let h = unsafe { CreateEventW(std::ptr::null(), 0, 0, std::ptr::null()) };
    if h.is_null() {
        Err(Status::Other(Hresult::FAIL.0))
    } else {
        Ok(h)
    }
}

/// Closes an event handle created by `create_event`.
fn close_event(h: *mut c_void) {
    if !h.is_null() {
        // SAFETY: `h` came from `CreateEventW` above and is closed once here.
        unsafe {
            CloseHandle(h);
        }
    }
}

/// Waits for the device event, returning a success `HRESULT` or a timeout
/// mapped to `AUDCLNT_E_BUFFER_ERROR`.
fn wait_for_event(h: *mut c_void) -> Hresult {
    // SAFETY: `h` is a live event handle owned by the stream.
    let r = unsafe { WaitForSingleObject(h, PERIOD_WAIT_MS) };
    match r {
        WAIT_OBJECT_0 => Hresult::OK,
        WAIT_TIMEOUT => Hresult(0x8889_0018_u32 as i32),
        _ => Hresult::FAIL,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hns_rounds_to_nearest_frame() {
        // 256 frames at 48 kHz is 5.333 ms = 53_333.3 hns.
        assert_eq!(hns_for_frames(256, 48_000), 53_333);
        // 192 frames at 48 kHz is exactly 4 ms.
        assert_eq!(hns_for_frames(192, 48_000), 40_000);
        // 96 frames at 96 kHz is exactly 1 ms.
        assert_eq!(hns_for_frames(96, 96_000), 10_000);
    }

    #[test]
    fn hns_of_a_zero_rate_is_zero() {
        assert_eq!(hns_for_frames(256, 0), 0);
    }

    #[test]
    fn open_error_classifies_the_statuses_that_matter() {
        assert_eq!(OpenError::from_status(Status::Busy), OpenError::Busy);
        assert_eq!(
            OpenError::from_status(Status::ExclusiveNotAllowed),
            OpenError::ExclusiveNotAllowed
        );
        assert_eq!(
            OpenError::from_status(Status::UnsupportedFormat),
            OpenError::UnsupportedFormat
        );
        assert_eq!(
            OpenError::from_status(Status::DeviceGone),
            OpenError::DeviceGone
        );
        assert_eq!(
            OpenError::from_status(Status::ResourcesInvalidated),
            OpenError::DeviceGone
        );
        assert_eq!(
            OpenError::from_status(Status::AccessDenied),
            OpenError::Failed(Status::AccessDenied)
        );
    }
}
