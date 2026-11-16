//! Hand-written WASAPI and MMDevice FFI declarations and their safe wrappers.
//!
//! This is the only crate in the workspace allowed to contain `unsafe`
//! (besides the test-only testkit allocator). The declarations are written
//! from the public Microsoft documentation recorded in
//! [`docs/sources/wasapi.md`][sources]; no header or vendor source is copied.
//! The COM vtable order is the one the SDK documents for each interface, and
//! is checked against the permissively licensed `windows` crate (MIT OR
//! Apache-2.0), credited in the same file.
//!
//! [sources]: https://github.com/jimmykedlyn/vynamp-output/blob/develop/docs/sources/wasapi.md
//!
//! # What is safe here
//!
//! Everything in the `com`, `mmdevice` and `audioclient` modules is `unsafe`
//! and private to this crate. The wrappers in `device` and `render` are the
//! surface the backend uses: they own their COM references, never hand out
//! raw pointers to callers, and turn every `HRESULT` into [`Status`], which
//! is plain data.
//!
//! # Target
//!
//! Every declaration is bound to `cfg(target_os = "windows")`. On any other
//! target this crate compiles to the pure helpers in the `format` and
//! `status` modules only, so the workspace still builds and tests on Linux CI
//! without pretending a Windows backend exists there.

#![deny(unsafe_op_in_unsafe_fn)]
#![warn(clippy::undocumented_unsafe_blocks)]
#![warn(missing_docs)]

pub mod format;
pub mod status;

#[cfg(target_os = "windows")]
mod audioclient;
#[cfg(target_os = "windows")]
mod com;
#[cfg(target_os = "windows")]
mod device;
#[cfg(target_os = "windows")]
mod mmdevice;
#[cfg(target_os = "windows")]
mod render;

#[cfg(target_os = "windows")]
pub use device::{DeviceEntry, DeviceId, Enumerator};
#[cfg(target_os = "windows")]
pub use mmdevice::E_MULTIMEDIA as ROLE_MULTIMEDIA;
#[cfg(target_os = "windows")]
pub use mmdevice::{E_COMMUNICATIONS as ROLE_COMMUNICATIONS, E_CONSOLE as ROLE_CONSOLE};
#[cfg(target_os = "windows")]
pub use render::{ExclusiveStream, OpenError, OpenRequest, StreamFormat};

/// Whether this build contains the Windows backend at all.
///
/// The facade uses it to decide whether `hosts()` may list WASAPI; on a
/// non-Windows target it is `false` and the module is compiled out rather
/// than stubbed.
pub const fn compiled() -> bool {
    cfg!(target_os = "windows")
}

pub use format::{
    Endian, SampleKind, SpeakerPosition, WaveFormatExtensible, channel_mask,
    sample_format_supported,
};
pub use status::{Hresult, Status};
