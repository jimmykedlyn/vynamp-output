//! What the caller asks for.

use std::time::Duration;

use crate::format::{ChannelLayout, SampleFormat};

/// Device sharing mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShareMode {
    Shared,
    Exclusive,
}

/// Behaviour when the exact request cannot be honoured.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OnUnavailable {
    Error,
    Downgrade,
}

/// Library-side sample conversion policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Conversion {
    Forbid,
    Allow(ConversionSpec),
}

/// A format conversion: round-half-even with clamp, no dither.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ConversionSpec {
    pub from: SampleFormat,
    pub to: SampleFormat,
}

/// Which device to open.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeviceSelector {
    Id(DeviceId),
    Default(Role),
}

/// System default-device role.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    Console,
    Multimedia,
    Communications,
}

/// Backend-specific device identifier.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct DeviceId(pub String);

/// How long a [`DeviceId`] stays valid.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IdStability {
    Persistent,
    Session,
}

/// Requested buffer size.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BufferRequest {
    Frames(u32),
    Duration(Duration),
    BackendDefault,
}

/// A complete request to open an output stream.
#[derive(Debug, Clone, PartialEq)]
pub struct StreamRequest {
    pub device: DeviceSelector,
    pub mode: ShareMode,
    pub format: SampleFormat,
    pub rate: u32,
    pub channels: u16,
    pub layout: ChannelLayout,
    pub buffer: BufferRequest,
    pub on_unavailable: OnUnavailable,
    pub conversion: Conversion,
    pub allow_rate_change_of_device: bool,
}
