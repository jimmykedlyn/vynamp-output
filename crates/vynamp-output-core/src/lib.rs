//! Core of vynamp-output: API types, errors, events, format negotiation,
//! the stream state machine, the generic device-thread driver and the
//! pure-Rust virtual hosts: [`NullHost`] (discards audio, real or virtual
//! clock) and, with the `capture` feature, `CaptureHost` (records the
//! delivered bytes and a manifest; an internal test backend).
//!
//! This crate is std-only and has no OS backend code. It defines what a
//! stream request is, what a negotiated configuration looks like, how a
//! request is matched against a device's [`Capabilities`] (see
//! [`negotiate`]), and how a [`Stream`] runs a [`RenderSource`] on a
//! device thread under the real-time contract.
//!
//! # Panics in `render`
//!
//! A panic in [`RenderSource::render`] is caught (`catch_unwind`); the
//! stream delivers silence from then on and emits [`Event::SourcePanicked`]
//! once. The process-wide panic hook still runs first on the device thread
//! (by default it prints to stderr, which allocates and does I/O). The
//! library does not replace the global hook; an application that needs a
//! fully silent audio thread installs its own. With `panic = "abort"`
//! nothing can be caught.

#[cfg(feature = "capture")]
mod capture;
mod config;
mod convert;
mod dop;
mod driver;
mod error;
mod event;
mod format;
mod host;
mod negotiate;
mod null;
mod render;
mod request;
#[cfg(feature = "capture")]
mod ring;
#[cfg(test)]
mod rt_guard;
mod rtflags;
mod silence;
mod stream;
mod watcher;
#[cfg(test)]
mod zero_alloc_tests;

#[cfg(feature = "capture")]
pub use capture::CaptureHost;
pub use config::{EndpointVolume, PathProperties, RtPriority, StreamConfig, Transport, Tri};
pub use dop::dop_pcm_rate;
pub use error::Error;
pub use event::Event;
pub use format::{ChannelLayout, ChannelPosition, HostId, Justify, SampleFormat};
pub use host::{DeviceInfo, Host, abandoned_enumeration_threads, enumerate_with_timeout};
pub use negotiate::{Capabilities, CapabilityEntry, CapabilityOrigin, Negotiated, negotiate};
pub use null::{NullClock, NullHost, VirtualClockHandle, default_capability_entries};
pub use render::{
    DeviceBuffer, MonotonicNanos, PresentationTiming, RenderContext, RenderSource, Rendered,
    TimingSource,
};
pub use request::{
    BufferRequest, Conversion, ConversionSpec, DeviceId, DeviceSelector, IdStability,
    OnUnavailable, Role, ShareMode, StreamRequest,
};
pub use stream::{Counters, DrainOutcome, Stream, StreamState};
