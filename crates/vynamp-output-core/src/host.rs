//! The host (backend) API.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{RecvTimeoutError, channel};
use std::time::Duration;

use crate::config::Transport;
use crate::error::Error;
use crate::format::HostId;
use crate::negotiate::Capabilities;
use crate::render::RenderSource;
use crate::request::{DeviceId, IdStability, Role, ShareMode, StreamRequest};
use crate::stream::Stream;

/// An audio backend: enumerates devices and opens streams on them.
pub trait Host: Send + Sync {
    fn id(&self) -> HostId;
    /// Lists output devices. May block on a misbehaving driver; see
    /// `enumerate_with_timeout` for a bounded variant.
    fn enumerate(&self) -> Result<Vec<DeviceInfo>, Error>;
    /// The system default device for `role`, if any.
    fn default_device(&self, role: Role) -> Result<Option<DeviceInfo>, Error>;
    /// What `device` supports in `mode`.
    fn capabilities(&self, device: &DeviceId, mode: ShareMode) -> Result<Capabilities, Error>;
    /// Negotiates `req` and opens a stream in the `Opened` state; `source`
    /// is called on the device thread once the stream is started.
    fn open(&self, req: &StreamRequest, source: Box<dyn RenderSource>) -> Result<Stream, Error>;
}

/// An output device as reported by a host.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceInfo {
    pub id: DeviceId,
    pub id_stability: IdStability,
    pub name: String,
    pub host: HostId,
    /// Roles for which this device is the system default.
    pub default_for: Vec<Role>,
    pub transport: Transport,
}

static ABANDONED_ENUMERATIONS: AtomicU64 = AtomicU64::new(0);

/// Number of enumeration threads [`enumerate_with_timeout`] has abandoned
/// (still blocked in a host) since the process started.
pub fn abandoned_enumeration_threads() -> u64 {
    ABANDONED_ENUMERATIONS.load(Ordering::Relaxed)
}

/// Runs `host.enumerate()` on an auxiliary thread and waits at most
/// `timeout`.
///
/// On timeout returns [`Error::Timeout`]; the blocked thread is abandoned
/// (never killed), keeps its reference to the host, and is counted in
/// [`abandoned_enumeration_threads`]. The host is passed as an `Arc` because
/// the abandoned thread may outlive the caller's borrow.
pub fn enumerate_with_timeout(
    host: Arc<dyn Host>,
    timeout: Duration,
) -> Result<Vec<DeviceInfo>, Error> {
    let id = host.id();
    let (tx, rx) = channel();
    std::thread::Builder::new()
        .name("vynamp-output-enumerate".into())
        .spawn(move || {
            let _ = tx.send(host.enumerate());
        })
        .map_err(|e| Error::Os {
            backend: id,
            code: i64::from(e.raw_os_error().unwrap_or(-1)),
            call: "std::thread::Builder::spawn",
        })?;
    match rx.recv_timeout(timeout) {
        Ok(result) => result,
        Err(RecvTimeoutError::Timeout) => {
            ABANDONED_ENUMERATIONS.fetch_add(1, Ordering::Relaxed);
            Err(Error::Timeout)
        }
        Err(RecvTimeoutError::Disconnected) => Err(Error::Os {
            backend: id,
            code: -1,
            call: "Host::enumerate panicked",
        }),
    }
}
