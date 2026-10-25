//! The host (backend) API.

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
