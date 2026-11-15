//! Safe device enumeration: raw COM in, plain data out.
//!
//! The backend never sees a `ComPtr`. It gets [`DeviceEntry`] values with a
//! `String` id, a name and a role list, which is everything `DeviceInfo`
//! needs on the other side of the boundary.

use std::ffi::c_void;

use crate::Hresult;
use crate::com::{self, COINIT_MULTITHREADED, ComPtr, RPC_E_CHANGED_MODE};
use crate::mmdevice::{E_COMMUNICATIONS, E_CONSOLE, E_MULTIMEDIA, ImmDevice, ImmDeviceEnumerator};

/// The MMDevice endpoint id, as a `String`.
///
/// WASAPI ids are stable across reboots for a given endpoint
/// (`{0.0.0.00000000}.{...}`); they are what `DeviceSelector::Id` carries.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct DeviceId(pub String);

/// One render endpoint, as the backend needs it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceEntry {
    /// Endpoint id.
    pub id: DeviceId,
    /// Friendly name, when the property store yields one; the id otherwise.
    pub name: String,
    /// Roles this device is the system default for.
    pub roles: Vec<i32>,
}

/// Owns `CoUninitialize` for the thread that created it.
///
/// `CoInitializeEx` is per-thread and reference-counted; this guard releases
/// exactly the reference it added, and none when the thread was already in
/// another apartment (`RPC_E_CHANGED_MODE`).
struct Apartment {
    /// Whether `init` added a reference that `drop` must release.
    owns_reference: bool,
}

impl Apartment {
    /// Initialises COM on this thread as multithreaded.
    ///
    /// `RPC_E_CHANGED_MODE` is not an error: the thread is already in a
    /// different apartment, and every interface used here is usable from it.
    fn init() -> Result<Self, Hresult> {
        // SAFETY: `CoInitializeEx` takes no out-pointers; a null reserved
        // pointer is documented as allowed.
        let hr = unsafe { com::CoInitializeEx(std::ptr::null(), COINIT_MULTITHREADED) };
        if hr == RPC_E_CHANGED_MODE {
            return Ok(Apartment {
                owns_reference: false,
            });
        }
        if hr.failed() {
            return Err(hr);
        }
        Ok(Apartment {
            owns_reference: true,
        })
    }
}

impl Drop for Apartment {
    fn drop(&mut self) {
        if self.owns_reference {
            // SAFETY: balanced with the successful `CoInitializeEx` in
            // `init`, and only when that call added a reference.
            unsafe { com::CoUninitialize() };
        }
    }
}

/// An open MMDevice enumerator, holding the COM apartment for its lifetime.
pub struct Enumerator {
    inner: ComPtr<ImmDeviceEnumerator>,
    _apartment: Apartment,
}

impl Enumerator {
    /// Creates the MMDevice enumerator on this thread.
    pub fn new() -> Result<Self, Hresult> {
        let apartment = Apartment::init()?;
        let inner = crate::mmdevice::enumerator()?;
        Ok(Enumerator {
            inner,
            _apartment: apartment,
        })
    }

    /// Every active render endpoint, in the order the OS lists them.
    ///
    /// Each entry's `roles` is filled by asking the OS which device is the
    /// default for each role and comparing ids, so a device that is the
    /// default for two roles lists both.
    pub fn render_devices(&self) -> Result<Vec<DeviceEntry>, Hresult> {
        let defaults = self.default_ids();
        let collection = self.inner.render_endpoints()?;
        let count = collection.count()?;
        let mut out = Vec::with_capacity(count as usize);
        for index in 0..count {
            let device = collection.item(index)?;
            let mut entry = entry_of(&device)?;
            entry.roles = defaults
                .iter()
                .filter(|(_, id)| *id == entry.id)
                .map(|(role, _)| *role)
                .collect();
            out.push(entry);
        }
        Ok(out)
    }

    /// The system default render endpoint for `role` (`E_CONSOLE`,
    /// `E_MULTIMEDIA`, `E_COMMUNICATIONS`), if the OS has one.
    pub fn default_device(&self, role: i32) -> Result<Option<DeviceEntry>, Hresult> {
        match self.inner.default_render(role) {
            Ok(device) => {
                let mut entry = entry_of(&device)?;
                entry.roles = vec![role];
                Ok(Some(entry))
            }
            // The OS reports "no default for this role" as E_NOTFOUND.
            Err(hr) if hr == Hresult::NOT_FOUND => Ok(None),
            Err(hr) => Err(hr),
        }
    }

    /// The device with `id`, or `None` when no active render endpoint has it.
    ///
    /// The lookup goes through `IMMDeviceEnumerator::GetDevice`, so a device
    /// that vanished since enumeration is reported as absent rather than
    /// returned as a stale pointer.
    pub fn device_by_id(&self, id: &DeviceId) -> Result<Option<ComPtr<ImmDevice>>, Hresult> {
        let wide: Vec<u16> = id.0.encode_utf16().chain(std::iter::once(0)).collect();
        let mut out: *mut c_void = std::ptr::null_mut();
        // SAFETY: `self.inner` is a live enumerator; `wide` is a
        // null-terminated UTF-16 string that outlives the call; `out` is a
        // valid out-pointer.
        let hr = unsafe {
            let v = *self
                .inner
                .as_raw()
                .cast::<*const crate::mmdevice::ImmDeviceEnumeratorVtbl>();
            ((*v).get_device)(self.inner.as_raw(), wide.as_ptr(), &mut out)
        };
        if hr == Hresult::NOT_FOUND {
            return Ok(None);
        }
        if hr.failed() {
            return Err(hr);
        }
        // SAFETY: on success the OS returned a device with one reference.
        Ok(unsafe { ComPtr::from_raw(out) })
    }

    /// `(role, id)` for every role that has a default device.
    fn default_ids(&self) -> Vec<(i32, DeviceId)> {
        [E_CONSOLE, E_MULTIMEDIA, E_COMMUNICATIONS]
            .into_iter()
            .filter_map(|role| {
                let device = self.inner.default_render(role).ok()?;
                let id = entry_of(&device).ok()?.id;
                Some((role, id))
            })
            .collect()
    }
}

/// Reads a device's id into a [`DeviceEntry`] with no roles yet.
fn entry_of(device: &ComPtr<ImmDevice>) -> Result<DeviceEntry, Hresult> {
    let ptr = device.id_ptr()?;
    if ptr.is_null() {
        return Err(Hresult::FAIL);
    }
    // SAFETY: `GetId` succeeded and returned an OS-allocated,
    // null-terminated UTF-16 string. This block reads it to the terminator
    // and then frees it with the matching allocator, exactly once.
    let id = unsafe {
        let mut len = 0usize;
        while *ptr.add(len) != 0 {
            len += 1;
        }
        let s = String::from_utf16_lossy(std::slice::from_raw_parts(ptr, len));
        com::CoTaskMemFree(ptr.cast::<c_void>());
        s
    };
    Ok(DeviceEntry {
        name: id.clone(),
        id: DeviceId(id),
        roles: Vec::new(),
    })
}
