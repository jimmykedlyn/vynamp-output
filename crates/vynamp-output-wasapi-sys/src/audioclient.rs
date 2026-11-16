//! `IAudioClient` and `IAudioRenderClient`: the stream itself.
//!
//! Declarations from the public documentation recorded in
//! `docs/sources/wasapi.md` (`audioclient.h`). Method order is the documented
//! declaration order, which is the vtable order.

use std::ffi::c_void;

use crate::Hresult;
use crate::com::{ComPtr, Guid, UnknownVtbl};

/// `IID_IAudioClient`.
pub const IID_IAUDIO_CLIENT: Guid = Guid::new(
    0x1CB9_AD4C,
    0xDBFA,
    0x4C32,
    [0xB1, 0x78, 0xC2, 0xF5, 0x68, 0xA7, 0x03, 0xB2],
);
/// `IID_IAudioRenderClient`.
pub const IID_IAUDIO_RENDER_CLIENT: Guid = Guid::new(
    0xF294_ACFC,
    0x3146,
    0x4483,
    [0xA7, 0xBF, 0xAD, 0xDC, 0xA7, 0xC2, 0x60, 0xE2],
);

/// `AUDCLNT_SHAREMODE_SHARED`.
pub const SHAREMODE_SHARED: i32 = 0;
/// `AUDCLNT_SHAREMODE_EXCLUSIVE`.
pub const SHAREMODE_EXCLUSIVE: i32 = 1;

/// `AUDCLNT_STREAMFLAGS_EVENTCALLBACK`.
pub const STREAMFLAGS_EVENTCALLBACK: u32 = 0x0004_0000;
/// `AUDCLNT_STREAMFLAGS_NOPERSIST`.
pub const STREAMFLAGS_NOPERSIST: u32 = 0x0008_0000;

/// Marker type for `IAudioClient`.
pub struct AudioClient;
/// Marker type for `IAudioRenderClient`.
pub struct AudioRenderClient;

/// `IAudioClient` vtable.
#[repr(C)]
pub struct AudioClientVtbl {
    /// `IUnknown`.
    pub base: UnknownVtbl,
    /// `Initialize`.
    pub initialize: unsafe extern "system" fn(
        *mut c_void,
        i32,
        u32,
        i64,
        i64,
        *const u8,
        *const Guid,
    ) -> Hresult,
    /// `GetBufferSize`.
    pub get_buffer_size: unsafe extern "system" fn(*mut c_void, *mut u32) -> Hresult,
    /// `GetStreamLatency`.
    pub get_stream_latency: unsafe extern "system" fn(*mut c_void, *mut i64) -> Hresult,
    /// `GetCurrentPadding`.
    pub get_current_padding: unsafe extern "system" fn(*mut c_void, *mut u32) -> Hresult,
    /// `IsFormatSupported`.
    pub is_format_supported:
        unsafe extern "system" fn(*mut c_void, i32, *const u8, *mut *mut u8) -> Hresult,
    /// `GetMixFormat`.
    pub get_mix_format: unsafe extern "system" fn(*mut c_void, *mut *mut u8) -> Hresult,
    /// `GetDevicePeriod`.
    pub get_device_period: unsafe extern "system" fn(*mut c_void, *mut i64, *mut i64) -> Hresult,
    /// `Start`.
    pub start: unsafe extern "system" fn(*mut c_void) -> Hresult,
    /// `Stop`.
    pub stop: unsafe extern "system" fn(*mut c_void) -> Hresult,
    /// `Reset`.
    pub reset: unsafe extern "system" fn(*mut c_void) -> Hresult,
    /// `SetEventHandle`.
    pub set_event_handle: unsafe extern "system" fn(*mut c_void, *mut c_void) -> Hresult,
    /// `GetService`.
    pub get_service:
        unsafe extern "system" fn(*mut c_void, *const Guid, *mut *mut c_void) -> Hresult,
}

/// `IAudioRenderClient` vtable.
#[repr(C)]
pub struct AudioRenderClientVtbl {
    /// `IUnknown`.
    pub base: UnknownVtbl,
    /// `GetBuffer`.
    pub get_buffer: unsafe extern "system" fn(*mut c_void, u32, *mut *mut u8) -> Hresult,
    /// `ReleaseBuffer`.
    pub release_buffer: unsafe extern "system" fn(*mut c_void, u32, u32) -> Hresult,
}

/// Reads the vtable of `ptr` as `V`.
///
/// # Safety
///
/// `ptr` must be a live COM interface pointer whose vtable is a `V`.
unsafe fn vtbl<V>(ptr: *mut c_void) -> *const V {
    // SAFETY: the caller guarantees `ptr` is a live interface pointer, so
    // its first word is the vtable pointer, and the caller guarantees the
    // vtable is a `V`.
    unsafe { *ptr.cast::<*const V>() }
}

impl ComPtr<AudioClient> {
    /// `Initialize(share_mode, flags, buffer, period, format, null)`.
    ///
    /// `format` points at a 40-byte `WAVEFORMATEXTENSIBLE`; the OS reads it
    /// during the call and does not keep the pointer.
    pub fn initialize(
        &self,
        share_mode: i32,
        flags: u32,
        buffer_hns: i64,
        period_hns: i64,
        format: &[u8; 40],
    ) -> Hresult {
        // SAFETY: `self` is a live `IAudioClient`; `format` is a live
        // 40-byte array that outlives the call, and the OS only reads it
        // during the call; the session GUID is documented as nullable.
        unsafe {
            let v = vtbl::<AudioClientVtbl>(self.as_raw());
            ((*v).initialize)(
                self.as_raw(),
                share_mode,
                flags,
                buffer_hns,
                period_hns,
                format.as_ptr(),
                std::ptr::null(),
            )
        }
    }

    /// `GetBufferSize`.
    pub fn buffer_size(&self) -> Result<u32, Hresult> {
        let mut n = 0u32;
        // SAFETY: `self` is a live client; `n` is a valid out-pointer.
        let hr = unsafe {
            let v = vtbl::<AudioClientVtbl>(self.as_raw());
            ((*v).get_buffer_size)(self.as_raw(), &mut n)
        };
        if hr.failed() { Err(hr) } else { Ok(n) }
    }

    /// `GetDevicePeriod(default, minimum)`.
    pub fn device_period(&self) -> Result<(i64, i64), Hresult> {
        let (mut default, mut minimum) = (0i64, 0i64);
        // SAFETY: `self` is a live client; both out-pointers are valid.
        let hr = unsafe {
            let v = vtbl::<AudioClientVtbl>(self.as_raw());
            ((*v).get_device_period)(self.as_raw(), &mut default, &mut minimum)
        };
        if hr.failed() {
            Err(hr)
        } else {
            Ok((default, minimum))
        }
    }

    /// `GetStreamLatency`.
    pub fn stream_latency(&self) -> Result<i64, Hresult> {
        let mut n = 0i64;
        // SAFETY: `self` is a live client; `n` is a valid out-pointer.
        let hr = unsafe {
            let v = vtbl::<AudioClientVtbl>(self.as_raw());
            ((*v).get_stream_latency)(self.as_raw(), &mut n)
        };
        if hr.failed() { Err(hr) } else { Ok(n) }
    }

    /// `IsFormatSupported(share_mode, format, null)`.
    ///
    /// The closest-match out-pointer is null, so the OS does not allocate a
    /// descriptor we would have to free.
    pub fn is_format_supported(&self, share_mode: i32, format: &[u8; 40]) -> Hresult {
        // SAFETY: `self` is a live client; `format` is a live 40-byte array
        // read only during the call; a null closest-match pointer is
        // documented as allowed.
        unsafe {
            let v = vtbl::<AudioClientVtbl>(self.as_raw());
            ((*v).is_format_supported)(
                self.as_raw(),
                share_mode,
                format.as_ptr(),
                std::ptr::null_mut(),
            )
        }
    }

    /// `SetEventHandle`.
    pub fn set_event_handle(&self, event: *mut c_void) -> Hresult {
        // SAFETY: `self` is a live client; `event` is a live event handle
        // owned by the caller for at least as long as the stream.
        unsafe {
            let v = vtbl::<AudioClientVtbl>(self.as_raw());
            ((*v).set_event_handle)(self.as_raw(), event)
        }
    }

    /// `Start`.
    pub fn start(&self) -> Hresult {
        // SAFETY: `self` is a live client.
        unsafe {
            let v = vtbl::<AudioClientVtbl>(self.as_raw());
            ((*v).start)(self.as_raw())
        }
    }

    /// `Stop`.
    pub fn stop(&self) -> Hresult {
        // SAFETY: `self` is a live client.
        unsafe {
            let v = vtbl::<AudioClientVtbl>(self.as_raw());
            ((*v).stop)(self.as_raw())
        }
    }

    /// `Reset`.
    pub fn reset(&self) -> Hresult {
        // SAFETY: `self` is a live client.
        unsafe {
            let v = vtbl::<AudioClientVtbl>(self.as_raw());
            ((*v).reset)(self.as_raw())
        }
    }

    /// `GetService(IID_IAudioRenderClient)`.
    pub fn render_client(&self) -> Result<ComPtr<AudioRenderClient>, Hresult> {
        let mut out: *mut c_void = std::ptr::null_mut();
        // SAFETY: `self` is a live client; the IID is the documented one;
        // `out` is a valid out-pointer.
        let hr = unsafe {
            let v = vtbl::<AudioClientVtbl>(self.as_raw());
            ((*v).get_service)(self.as_raw(), &IID_IAUDIO_RENDER_CLIENT, &mut out)
        };
        if hr.failed() {
            return Err(hr);
        }
        // SAFETY: on success the OS returned an interface with one reference.
        unsafe { ComPtr::from_raw(out) }.ok_or(Hresult::FAIL)
    }
}

impl ComPtr<AudioRenderClient> {
    /// `GetBuffer(frames)`, returning the writable pointer.
    ///
    /// # Safety
    ///
    /// The returned pointer is valid for `frames * block_align` bytes until
    /// the matching `ReleaseBuffer`. The caller must not hold it across
    /// calls and must release exactly once.
    pub unsafe fn get_buffer(&self, frames: u32) -> Result<*mut u8, Hresult> {
        let mut out: *mut u8 = std::ptr::null_mut();
        // SAFETY: `self` is a live render client; `out` is a valid
        // out-pointer; the caller upholds the buffer lifetime contract.
        let hr = unsafe {
            let v = vtbl::<AudioRenderClientVtbl>(self.as_raw());
            ((*v).get_buffer)(self.as_raw(), frames, &mut out)
        };
        if hr.failed() { Err(hr) } else { Ok(out) }
    }

    /// `ReleaseBuffer(frames, flags)`.
    pub fn release_buffer(&self, frames: u32, flags: u32) -> Hresult {
        // SAFETY: `self` is a live render client; the caller passes the same
        // frame count it got from `get_buffer`.
        unsafe {
            let v = vtbl::<AudioRenderClientVtbl>(self.as_raw());
            ((*v).release_buffer)(self.as_raw(), frames, flags)
        }
    }
}
