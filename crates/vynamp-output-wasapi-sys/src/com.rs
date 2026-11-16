//! The minimum COM scaffolding the backend needs: an owning smart pointer
//! over an `IUnknown`-derived interface, and the OS entry points that create
//! and release them.
//!
//! Declarations come from the public documentation recorded in
//! `docs/sources/wasapi.md` (`unknwn.h`, `combaseapi.h`, `objbase.h`). The
//! vtable shape is the COM ABI: `QueryInterface`, `AddRef`, `Release`, then
//! the interface's own methods in declaration order.

use std::ffi::c_void;
use std::marker::PhantomData;
use std::ptr::NonNull;

use crate::Hresult;

/// A 128-bit COM class or interface identifier.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Guid {
    /// First component.
    pub data1: u32,
    /// Second component.
    pub data2: u16,
    /// Third component.
    pub data3: u16,
    /// Remaining eight bytes.
    pub data4: [u8; 8],
}

impl Guid {
    /// Builds a GUID from its documented textual parts.
    pub const fn new(data1: u32, data2: u16, data3: u16, data4: [u8; 8]) -> Self {
        Guid {
            data1,
            data2,
            data3,
            data4,
        }
    }
}

/// The three `IUnknown` methods, in vtable order.
///
/// Every COM interface's vtable begins with these three function pointers;
/// an interface struct embeds this as its first field so the layouts agree.
#[repr(C)]
#[derive(Debug)]
pub struct UnknownVtbl {
    /// `QueryInterface`.
    pub query_interface:
        unsafe extern "system" fn(*mut c_void, *const Guid, *mut *mut c_void) -> Hresult,
    /// `AddRef`.
    pub add_ref: unsafe extern "system" fn(*mut c_void) -> u32,
    /// `Release`.
    pub release: unsafe extern "system" fn(*mut c_void) -> u32,
}

unsafe extern "system" {
    /// `CoInitializeEx`: initialises COM on the calling thread.
    pub fn CoInitializeEx(reserved: *const c_void, coinit: u32) -> Hresult;
    /// `CoUninitialize`: closes the COM library on the calling thread.
    pub fn CoUninitialize();
    /// `CoCreateInstance`: creates a single uninitialised object of the
    /// given class on the given thread or process.
    pub fn CoCreateInstance(
        clsid: *const Guid,
        outer: *const c_void,
        clsctx: u32,
        iid: *const Guid,
        out: *mut *mut c_void,
    ) -> Hresult;
    /// `CoTaskMemFree`: frees memory the OS allocated for us (the format
    /// descriptor and device id strings).
    pub fn CoTaskMemFree(ptr: *const c_void);
}

/// `CLSCTX_ALL` (`CLSCTX_INPROC_SERVER | INPROC_HANDLER | LOCAL_SERVER |
/// REMOTE_SERVER`).
pub const CLSCTX_ALL: u32 = 0x17;

/// `COINIT_MULTITHREADED`.
pub const COINIT_MULTITHREADED: u32 = 0x0;

/// `RPC_E_CHANGED_MODE` (`0x80010106`, `rpcerror.h`), which
/// `CoInitializeEx` returns when the thread is already initialised in the
/// other apartment. It is not a failure for us: COM is usable.
pub const RPC_E_CHANGED_MODE: Hresult = Hresult(0x8001_0106_u32 as i32);

/// An owning reference to a COM object of interface `I`.
///
/// `I` is a zero-sized marker whose only job is to keep two different
/// interfaces from being mixed up at compile time; the concrete vtables live
/// in `mmdevice.rs` and `audioclient.rs`.
///
/// Dropping calls `Release`. `Clone` calls `AddRef`. The pointer is never
/// handed to code outside this crate.
pub struct ComPtr<I> {
    ptr: NonNull<c_void>,
    _iface: PhantomData<I>,
}

// SAFETY: a COM object obtained with an apartment-neutral or multithreaded
// initialisation is free-threaded by the COM rules the backend relies on
// (`CoInitializeEx(COINIT_MULTITHREADED)` on the control thread, and the
// WASAPI interfaces are documented as callable from any thread of the
// initialised apartment). `AddRef`/`Release` are atomic in the OS.
unsafe impl<I> Send for ComPtr<I> {}
// SAFETY: same as `Send`: reference counting is atomic and the interfaces
// used here are documented thread-safe within an MTA.
unsafe impl<I> Sync for ComPtr<I> {}

impl<I> ComPtr<I> {
    /// Takes ownership of `ptr`, which must be a valid interface pointer with
    /// a reference already held by the caller.
    ///
    /// # Safety
    ///
    /// `ptr` must be null or a valid `I` pointer whose reference count the
    /// caller is transferring. Passing a pointer that is already owned
    /// elsewhere double-releases.
    pub(crate) unsafe fn from_raw(ptr: *mut c_void) -> Option<Self> {
        NonNull::new(ptr).map(|ptr| ComPtr {
            ptr,
            _iface: PhantomData,
        })
    }

    /// The raw pointer, for passing back to the OS.
    pub(crate) fn as_raw(&self) -> *mut c_void {
        self.ptr.as_ptr()
    }
}

impl<I> Clone for ComPtr<I> {
    fn clone(&self) -> Self {
        // SAFETY: `self.ptr` is a live COM interface pointer, so its vtable
        // starts with `IUnknown`. Reading the first vtable slot is the COM
        // ABI; `AddRef` on a live object is safe and atomic.
        unsafe {
            let vtbl = *self.ptr.as_ptr().cast::<*const UnknownVtbl>();
            ((*vtbl).add_ref)(self.ptr.as_ptr());
        }
        ComPtr {
            ptr: self.ptr,
            _iface: PhantomData,
        }
    }
}

impl<I> Drop for ComPtr<I> {
    fn drop(&mut self) {
        // SAFETY: as in `clone`, `self.ptr` is a live interface pointer whose
        // vtable begins with `IUnknown`, and this `ComPtr` owns exactly one
        // reference (it was created by `from_raw` or `clone`), so releasing
        // it here cannot leave a dangling alias.
        unsafe {
            let vtbl = *self.ptr.as_ptr().cast::<*const UnknownVtbl>();
            ((*vtbl).release)(self.ptr.as_ptr());
        }
    }
}

impl<I> std::fmt::Debug for ComPtr<I> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("ComPtr").field(&self.ptr).finish()
    }
}
