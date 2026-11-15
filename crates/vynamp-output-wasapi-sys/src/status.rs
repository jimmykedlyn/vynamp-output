//! `HRESULT` as plain data, and the classification the backend needs.
//!
//! The backend must never see a raw `HRESULT`: it decides between "busy",
//! "device gone", "exclusive not allowed" and "unsupported format", and each
//! of those is a different `Error` on the other side of the boundary. The
//! classification lives here, next to the constants, so it is testable
//! without a device.

/// An `HRESULT` value.
///
/// Kept as `i32` because that is what the ABI passes; the named constants
/// below are the ones this backend distinguishes. `repr(transparent)` makes
/// it ABI-identical to the `LONG` the OS returns, so it can appear in the
/// `extern "system"` signatures directly.
#[repr(transparent)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Hresult(pub i32);

impl Hresult {
    /// `S_OK`.
    pub const OK: Hresult = Hresult(0);
    /// `AUDCLNT_E_DEVICE_IN_USE`: another client holds the endpoint.
    pub const DEVICE_IN_USE: Hresult = Hresult(0x8889_000A_u32 as i32);
    /// `AUDCLNT_E_DEVICE_INVALIDATED`: the endpoint was unplugged or
    /// reconfigured.
    pub const DEVICE_INVALIDATED: Hresult = Hresult(0x8889_0004_u32 as i32);
    /// `AUDCLNT_E_RESOURCES_INVALIDATED`: the stream was suspended (sleep,
    /// exclusive disconnect) or quiesced.
    pub const RESOURCES_INVALIDATED: Hresult = Hresult(0x8889_0026_u32 as i32);
    /// `AUDCLNT_E_EXCLUSIVE_MODE_NOT_ALLOWED`: the user disabled exclusive
    /// use of the device.
    pub const EXCLUSIVE_MODE_NOT_ALLOWED: Hresult = Hresult(0x8889_000E_u32 as i32);
    /// `AUDCLNT_E_UNSUPPORTED_FORMAT`.
    pub const UNSUPPORTED_FORMAT: Hresult = Hresult(0x8889_0008_u32 as i32);
    /// `AUDCLNT_E_BUFFER_SIZE_NOT_ALIGNED`: retry with the aligned size.
    pub const BUFFER_SIZE_NOT_ALIGNED: Hresult = Hresult(0x8889_0019_u32 as i32);
    /// `AUDCLNT_E_SERVICE_NOT_RUNNING`.
    pub const SERVICE_NOT_RUNNING: Hresult = Hresult(0x8889_0010_u32 as i32);
    /// `AUDCLNT_E_ENDPOINT_CREATE_FAILED`.
    pub const ENDPOINT_CREATE_FAILED: Hresult = Hresult(0x8889_000F_u32 as i32);
    /// `E_ACCESSDENIED`.
    pub const ACCESS_DENIED: Hresult = Hresult(0x8007_0005_u32 as i32);
    /// `E_POINTER`.
    pub const POINTER: Hresult = Hresult(0x8000_4003_u32 as i32);
    /// `E_INVALIDARG`.
    pub const INVALID_ARG: Hresult = Hresult(0x8007_0057_u32 as i32);
    /// `E_OUTOFMEMORY`.
    pub const OUT_OF_MEMORY: Hresult = Hresult(0x8007_000E_u32 as i32);
    /// `E_NOINTERFACE`.
    pub const NO_INTERFACE: Hresult = Hresult(0x8000_4002_u32 as i32);
    /// `E_NOTIMPL`.
    pub const NOT_IMPLEMENTED: Hresult = Hresult(0x8000_4001_u32 as i32);
    /// `E_FAIL`.
    pub const FAIL: Hresult = Hresult(0x8000_4005_u32 as i32);
    /// `CO_E_NOTINITIALIZED`: COM was not initialised on this thread.
    pub const COM_NOT_INITIALIZED: Hresult = Hresult(0x8004_01F0_u32 as i32);
    /// `E_NOTFOUND` (0x80070490): the OS has no default device for a role.
    pub const NOT_FOUND: Hresult = Hresult(0x8007_0490_u32 as i32);
    /// `RPC_E_CHANGED_MODE` (`0x80010106`, `rpcerror.h`): the thread is
    /// already in the other apartment.
    pub const RPC_CHANGED_MODE: Hresult = Hresult(0x8001_0106_u32 as i32);

    /// Whether the call succeeded (`>= 0`).
    pub const fn succeeded(self) -> bool {
        self.0 >= 0
    }

    /// Whether the call failed (`< 0`).
    pub const fn failed(self) -> bool {
        self.0 < 0
    }

    /// The raw value, for `Error::Os { code }`.
    pub const fn code(self) -> i64 {
        self.0 as i64
    }
}

/// What a failed call means to the backend.
///
/// This is the whole point of the module: the backend matches on this, never
/// on a numeric code, so a new code is a compile error here rather than a
/// silent fallthrough.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Status {
    /// The call succeeded.
    Ok,
    /// Another client holds the endpoint in exclusive mode.
    Busy,
    /// The endpoint was unplugged or reconfigured.
    DeviceGone,
    /// The stream's resources were invalidated (sleep, exclusive disconnect).
    ResourcesInvalidated,
    /// The user disabled exclusive use of this device.
    ExclusiveNotAllowed,
    /// The device does not support the requested format.
    UnsupportedFormat,
    /// The buffer size must be realigned and `Initialize` retried.
    BufferSizeNotAligned,
    /// The Windows audio service is not running.
    ServiceNotRunning,
    /// The endpoint could not be created (usually unplugged).
    EndpointCreateFailed,
    /// The OS denied access.
    AccessDenied,
    /// COM was not initialised on this thread, or in the wrong apartment.
    ComNotInitialized,
    /// The call was given a bad argument.
    InvalidArgument,
    /// Out of memory.
    OutOfMemory,
    /// The object does not implement the requested interface.
    NoInterface,
    /// The call is not implemented.
    NotImplemented,
    /// Any other failure, with the raw code.
    Other(i32),
}

impl Status {
    /// Classifies `hr`.
    pub const fn from_hresult(hr: Hresult) -> Status {
        if hr.succeeded() {
            return Status::Ok;
        }
        match hr.0 {
            x if x == Hresult::DEVICE_IN_USE.0 => Status::Busy,
            x if x == Hresult::DEVICE_INVALIDATED.0 => Status::DeviceGone,
            x if x == Hresult::RESOURCES_INVALIDATED.0 => Status::ResourcesInvalidated,
            x if x == Hresult::EXCLUSIVE_MODE_NOT_ALLOWED.0 => Status::ExclusiveNotAllowed,
            x if x == Hresult::UNSUPPORTED_FORMAT.0 => Status::UnsupportedFormat,
            x if x == Hresult::BUFFER_SIZE_NOT_ALIGNED.0 => Status::BufferSizeNotAligned,
            x if x == Hresult::SERVICE_NOT_RUNNING.0 => Status::ServiceNotRunning,
            x if x == Hresult::ENDPOINT_CREATE_FAILED.0 => Status::EndpointCreateFailed,
            x if x == Hresult::ACCESS_DENIED.0 => Status::AccessDenied,
            x if x == Hresult::COM_NOT_INITIALIZED.0 || x == Hresult::RPC_CHANGED_MODE.0 => {
                Status::ComNotInitialized
            }
            x if x == Hresult::INVALID_ARG.0 || x == Hresult::POINTER.0 => Status::InvalidArgument,
            x if x == Hresult::OUT_OF_MEMORY.0 => Status::OutOfMemory,
            x if x == Hresult::NO_INTERFACE.0 => Status::NoInterface,
            x if x == Hresult::NOT_IMPLEMENTED.0 => Status::NotImplemented,
            other => Status::Other(other),
        }
    }

    /// Whether the call succeeded.
    pub const fn is_ok(self) -> bool {
        matches!(self, Status::Ok)
    }

    /// Whether the device is gone and the stream must move to `Lost`.
    ///
    /// Both codes mean the endpoint the stream was opened on no longer
    /// exists; the backend reports `DeviceLost` and never reopens another
    /// device on its own.
    pub const fn is_device_gone(self) -> bool {
        matches!(self, Status::DeviceGone | Status::ResourcesInvalidated)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn success_is_ok() {
        assert_eq!(Status::from_hresult(Hresult::OK), Status::Ok);
        assert!(Hresult::OK.succeeded());
        assert!(!Hresult::OK.failed());
    }

    #[test]
    fn every_named_failure_maps_to_its_own_status() {
        for (hr, want) in [
            (Hresult::DEVICE_IN_USE, Status::Busy),
            (Hresult::DEVICE_INVALIDATED, Status::DeviceGone),
            (Hresult::RESOURCES_INVALIDATED, Status::ResourcesInvalidated),
            (
                Hresult::EXCLUSIVE_MODE_NOT_ALLOWED,
                Status::ExclusiveNotAllowed,
            ),
            (Hresult::UNSUPPORTED_FORMAT, Status::UnsupportedFormat),
            (
                Hresult::BUFFER_SIZE_NOT_ALIGNED,
                Status::BufferSizeNotAligned,
            ),
            (Hresult::SERVICE_NOT_RUNNING, Status::ServiceNotRunning),
            (
                Hresult::ENDPOINT_CREATE_FAILED,
                Status::EndpointCreateFailed,
            ),
            (Hresult::ACCESS_DENIED, Status::AccessDenied),
            (Hresult::COM_NOT_INITIALIZED, Status::ComNotInitialized),
            (Hresult::RPC_CHANGED_MODE, Status::ComNotInitialized),
            (Hresult::INVALID_ARG, Status::InvalidArgument),
            (Hresult::POINTER, Status::InvalidArgument),
            (Hresult::OUT_OF_MEMORY, Status::OutOfMemory),
            (Hresult::NO_INTERFACE, Status::NoInterface),
            (Hresult::NOT_IMPLEMENTED, Status::NotImplemented),
        ] {
            assert_eq!(Status::from_hresult(hr), want, "{hr:?}");
            assert!(hr.failed(), "{hr:?} must be a failure");
        }
    }

    #[test]
    fn rpc_e_changed_mode_is_the_documented_value() {
        // `rpcerror.h` documents `RPC_E_CHANGED_MODE` as `0x80010106`
        // (cannot change thread mode after it is set), recorded in
        // `docs/sources/wasapi.md`. The value is asserted numerically because
        // the classification below is only right for that exact code: with
        // any other value `Apartment::init` would treat a thread that is
        // already in an apartment as a failure.
        assert_eq!(Hresult::RPC_CHANGED_MODE.0, 0x8001_0106_u32 as i32);
        assert_eq!(
            Status::from_hresult(Hresult(0x8001_0106_u32 as i32)),
            Status::ComNotInitialized,
            "the documented code must classify as ComNotInitialized, not Other"
        );
    }

    #[test]
    fn an_unknown_failure_keeps_its_code() {
        let hr = Hresult(0x8000_4005_u32 as i32);
        assert_eq!(Status::from_hresult(hr), Status::Other(hr.0));
    }

    #[test]
    fn device_gone_covers_unplug_and_sleep() {
        assert!(Status::DeviceGone.is_device_gone());
        assert!(Status::ResourcesInvalidated.is_device_gone());
        assert!(!Status::Busy.is_device_gone());
        assert!(!Status::Ok.is_device_gone());
    }

    #[test]
    fn code_is_the_signed_raw_value() {
        // `Error::Os { code }` carries the signed HRESULT, as the OS reports
        // it; a failure is negative.
        assert_eq!(Hresult::DEVICE_IN_USE.code(), -0x7776_FFF6_i64);
        assert!(Hresult::DEVICE_IN_USE.code() < 0);
    }
}
