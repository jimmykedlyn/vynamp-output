//! The MMDevice API: enumerating render endpoints and getting their ids.
//!
//! Declarations from the public documentation recorded in
//! `docs/sources/wasapi.md` (`mmdeviceapi.h`). Method order is the documented
//! declaration order, which is the vtable order.

use std::ffi::c_void;

use crate::Hresult;
use crate::com::{CLSCTX_ALL, ComPtr, Guid, UnknownVtbl};

/// `EDataFlow::eRender`.
pub const E_RENDER: i32 = 0;
/// `ERole::eConsole`.
pub const E_CONSOLE: i32 = 0;
/// `ERole::eMultimedia`.
pub const E_MULTIMEDIA: i32 = 1;
/// `ERole::eCommunications`.
pub const E_COMMUNICATIONS: i32 = 2;
/// `DEVICE_STATE_ACTIVE`.
pub const DEVICE_STATE_ACTIVE: u32 = 0x1;

/// `CLSID_MMDeviceEnumerator`.
pub const CLSID_MM_DEVICE_ENUMERATOR: Guid = Guid::new(
    0xBCDE_0395,
    0xE52F,
    0x467C,
    [0x8E, 0x3D, 0xC4, 0x57, 0x92, 0x91, 0x69, 0x2E],
);
/// `IID_IMMDeviceEnumerator`.
pub const IID_IMM_DEVICE_ENUMERATOR: Guid = Guid::new(
    0xA956_64D2,
    0x9614,
    0x4F35,
    [0xA7, 0x46, 0xDE, 0x8D, 0xB6, 0x36, 0x17, 0xE6],
);
/// Marker type for `IMMDeviceEnumerator`.
pub struct ImmDeviceEnumerator;
/// Marker type for `IMMDeviceCollection`.
pub struct ImmDeviceCollection;
/// Marker type for `IMMDevice`.
pub struct ImmDevice;

/// `IMMDeviceEnumerator` vtable.
#[repr(C)]
pub struct ImmDeviceEnumeratorVtbl {
    /// `IUnknown`.
    pub base: UnknownVtbl,
    /// `EnumAudioEndpoints`.
    pub enum_audio_endpoints:
        unsafe extern "system" fn(*mut c_void, i32, u32, *mut *mut c_void) -> Hresult,
    /// `GetDefaultAudioEndpoint`.
    pub get_default_audio_endpoint:
        unsafe extern "system" fn(*mut c_void, i32, i32, *mut *mut c_void) -> Hresult,
    /// `GetDevice`.
    pub get_device: unsafe extern "system" fn(*mut c_void, *const u16, *mut *mut c_void) -> Hresult,
    /// `RegisterEndpointNotificationCallback`.
    pub register_endpoint_notification_callback:
        unsafe extern "system" fn(*mut c_void, *mut c_void) -> Hresult,
    /// `UnregisterEndpointNotificationCallback`.
    pub unregister_endpoint_notification_callback:
        unsafe extern "system" fn(*mut c_void, *mut c_void) -> Hresult,
}

/// `IMMDeviceCollection` vtable.
#[repr(C)]
pub struct ImmDeviceCollectionVtbl {
    /// `IUnknown`.
    pub base: UnknownVtbl,
    /// `GetCount`.
    pub get_count: unsafe extern "system" fn(*mut c_void, *mut u32) -> Hresult,
    /// `Item`.
    pub item: unsafe extern "system" fn(*mut c_void, u32, *mut *mut c_void) -> Hresult,
}

/// `IMMDevice` vtable.
#[repr(C)]
pub struct ImmDeviceVtbl {
    /// `IUnknown`.
    pub base: UnknownVtbl,
    /// `Activate`.
    pub activate: unsafe extern "system" fn(
        *mut c_void,
        *const Guid,
        u32,
        *const c_void,
        *mut *mut c_void,
    ) -> Hresult,
    /// `OpenPropertyStore`.
    pub open_property_store:
        unsafe extern "system" fn(*mut c_void, u32, *mut *mut c_void) -> Hresult,
    /// `GetId`.
    pub get_id: unsafe extern "system" fn(*mut c_void, *mut *mut u16) -> Hresult,
    /// `GetState`.
    pub get_state: unsafe extern "system" fn(*mut c_void, *mut u32) -> Hresult,
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

/// Creates the MMDevice enumerator.
pub fn enumerator() -> Result<ComPtr<ImmDeviceEnumerator>, Hresult> {
    let mut out: *mut c_void = std::ptr::null_mut();
    // SAFETY: `CLSID_MMDeviceEnumerator` and `IID_IMMDeviceEnumerator` are
    // the documented constants for this object; `out` is a valid out-pointer
    // and the OS writes a reference into it on success.
    let hr = unsafe {
        crate::com::CoCreateInstance(
            &CLSID_MM_DEVICE_ENUMERATOR,
            std::ptr::null(),
            CLSCTX_ALL,
            &IID_IMM_DEVICE_ENUMERATOR,
            &mut out,
        )
    };
    if hr.failed() {
        return Err(hr);
    }
    // SAFETY: on success the OS returned a valid interface pointer with one
    // reference, which this `ComPtr` now owns.
    unsafe { ComPtr::from_raw(out) }.ok_or(Hresult::FAIL)
}

impl ComPtr<ImmDeviceEnumerator> {
    /// `EnumAudioEndpoints(eRender, DEVICE_STATE_ACTIVE)`.
    pub fn render_endpoints(&self) -> Result<ComPtr<ImmDeviceCollection>, Hresult> {
        let mut out: *mut c_void = std::ptr::null_mut();
        // SAFETY: `self` is a live `IMMDeviceEnumerator`; the vtable is the
        // documented one; `out` is a valid out-pointer.
        let hr = unsafe {
            let v = vtbl::<ImmDeviceEnumeratorVtbl>(self.as_raw());
            ((*v).enum_audio_endpoints)(self.as_raw(), E_RENDER, DEVICE_STATE_ACTIVE, &mut out)
        };
        if hr.failed() {
            return Err(hr);
        }
        // SAFETY: on success the OS returned a collection with one reference.
        unsafe { ComPtr::from_raw(out) }.ok_or(Hresult::FAIL)
    }

    /// `GetDefaultAudioEndpoint(eRender, role)`.
    pub fn default_render(&self, role: i32) -> Result<ComPtr<ImmDevice>, Hresult> {
        let mut out: *mut c_void = std::ptr::null_mut();
        // SAFETY: as in `render_endpoints`; `role` is one of the documented
        // `ERole` values.
        let hr = unsafe {
            let v = vtbl::<ImmDeviceEnumeratorVtbl>(self.as_raw());
            ((*v).get_default_audio_endpoint)(self.as_raw(), E_RENDER, role, &mut out)
        };
        if hr.failed() {
            return Err(hr);
        }
        // SAFETY: on success the OS returned a device with one reference.
        unsafe { ComPtr::from_raw(out) }.ok_or(Hresult::FAIL)
    }
}

impl ComPtr<ImmDeviceCollection> {
    /// `GetCount`.
    pub fn count(&self) -> Result<u32, Hresult> {
        let mut n = 0u32;
        // SAFETY: `self` is a live collection; `n` is a valid out-pointer.
        let hr = unsafe {
            let v = vtbl::<ImmDeviceCollectionVtbl>(self.as_raw());
            ((*v).get_count)(self.as_raw(), &mut n)
        };
        if hr.failed() { Err(hr) } else { Ok(n) }
    }

    /// `Item(index)`.
    pub fn item(&self, index: u32) -> Result<ComPtr<ImmDevice>, Hresult> {
        let mut out: *mut c_void = std::ptr::null_mut();
        // SAFETY: `self` is a live collection; `index` is below `count()`
        // when the caller follows the documented contract; `out` is valid.
        let hr = unsafe {
            let v = vtbl::<ImmDeviceCollectionVtbl>(self.as_raw());
            ((*v).item)(self.as_raw(), index, &mut out)
        };
        if hr.failed() {
            return Err(hr);
        }
        // SAFETY: on success the OS returned a device with one reference.
        unsafe { ComPtr::from_raw(out) }.ok_or(Hresult::FAIL)
    }
}

impl ComPtr<ImmDevice> {
    /// `GetId`, as a UTF-16 string the caller must free with
    /// `CoTaskMemFree`.
    ///
    /// Returns the raw pointer so the caller controls the free; the wrapper
    /// in `device.rs` converts it to a `String` and frees it.
    pub fn id_ptr(&self) -> Result<*mut u16, Hresult> {
        let mut out: *mut u16 = std::ptr::null_mut();
        // SAFETY: `self` is a live device; `out` is a valid out-pointer.
        let hr = unsafe {
            let v = vtbl::<ImmDeviceVtbl>(self.as_raw());
            ((*v).get_id)(self.as_raw(), &mut out)
        };
        if hr.failed() { Err(hr) } else { Ok(out) }
    }

    /// `Activate(iid, CLSCTX_ALL, null, out)`.
    pub fn activate(&self, iid: &Guid) -> Result<ComPtr<crate::audioclient::AudioClient>, Hresult> {
        let mut out: *mut c_void = std::ptr::null_mut();
        // SAFETY: `self` is a live device; `iid` is a documented interface
        // id; `out` is a valid out-pointer.
        let hr = unsafe {
            let v = vtbl::<ImmDeviceVtbl>(self.as_raw());
            ((*v).activate)(self.as_raw(), iid, CLSCTX_ALL, std::ptr::null(), &mut out)
        };
        if hr.failed() {
            return Err(hr);
        }
        // SAFETY: on success the OS returned an interface with one reference.
        unsafe { ComPtr::from_raw(out) }.ok_or(Hresult::FAIL)
    }
}
