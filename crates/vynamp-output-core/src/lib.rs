//! Core API types, errors, events and format negotiation for vynamp-output.
//!
//! This crate is std-only and has no backend code: it defines what a stream
//! request is, what a negotiated configuration looks like, and how a request
//! is matched against a device's [`Capabilities`] (see [`negotiate`]).

mod config;
mod error;
mod event;
mod format;
mod negotiate;
mod request;

pub use config::{EndpointVolume, PathProperties, RtPriority, StreamConfig, Transport, Tri};
pub use error::Error;
pub use event::Event;
pub use format::{ChannelLayout, ChannelPosition, HostId, Justify, SampleFormat};
pub use negotiate::{Capabilities, CapabilityEntry, CapabilityOrigin, Negotiated, negotiate};
pub use request::{
    BufferRequest, Conversion, ConversionSpec, DeviceId, DeviceSelector, IdStability,
    OnUnavailable, Role, ShareMode, StreamRequest,
};
