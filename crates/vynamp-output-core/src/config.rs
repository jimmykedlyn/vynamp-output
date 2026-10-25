//! What the stream actually got after negotiation.

use crate::format::{ChannelLayout, HostId, SampleFormat};
use crate::request::{ConversionSpec, DeviceId, ShareMode};

/// Three-valued answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tri {
    Yes,
    No,
    Unknown,
}

/// State of the endpoint (hardware/OS) volume.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum EndpointVolume {
    Unity,
    NotUnity(f32),
    Unknown,
}

/// Physical or logical transport of the device.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Transport {
    Usb,
    Bluetooth,
    Hdmi,
    BuiltIn,
    Virtual,
    Unknown,
}

/// Outcome of the real-time priority request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RtPriority {
    Granted(&'static str),
    Denied(&'static str),
}

/// Facts about the signal path, reported rather than assumed.
#[derive(Debug, Clone, PartialEq)]
pub struct PathProperties {
    pub os_mixer_bypassed: Tri,
    pub os_resampling: bool,
    pub library_conversion: Option<ConversionSpec>,
    pub endpoint_volume: EndpointVolume,
    pub transport: Transport,
    pub rt_priority: RtPriority,
}

impl PathProperties {
    /// Nothing known yet: all `Unknown`/`false`/`None`, RT priority not started.
    pub fn unknown() -> Self {
        PathProperties {
            os_mixer_bypassed: Tri::Unknown,
            os_resampling: false,
            library_conversion: None,
            endpoint_volume: EndpointVolume::Unknown,
            transport: Transport::Unknown,
            rt_priority: RtPriority::Denied("not started"),
        }
    }
}

/// The real configuration of a stream, after negotiation.
#[derive(Debug, Clone, PartialEq)]
pub struct StreamConfig {
    pub host: HostId,
    pub device: DeviceId,
    pub mode: ShareMode,
    pub format: SampleFormat,
    pub valid_bits: u8,
    pub rate: u32,
    pub channels: u16,
    pub layout: ChannelLayout,
    pub period_frames: u32,
    pub buffer_frames: u32,
    pub path: PathProperties,
}
