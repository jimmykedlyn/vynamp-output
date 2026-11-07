//! vynamp-output: audio output with explicit modes, honest negotiation and
//! a real-time render contract.
//!
//! This facade re-exports [`vynamp_output_core`] and chooses a host. In this
//! release only the pure-Rust [`NullHost`] exists; OS backends (WASAPI,
//! ALSA, CoreAudio, AAudio, PipeWire, Pulse) arrive in later releases and
//! will be listed by [`hosts`] and chosen by [`default_host`].
//!
//! # Example
//!
//! Play two-channel silence for a few virtual periods on the Null host. The
//! virtual clock makes the run deterministic: periods happen only when
//! `advance` is called.
//!
//! ```
//! use vynamp_output::*;
//!
//! struct Quiet;
//! impl RenderSource for Quiet {
//!     fn render(&mut self, _out: DeviceBuffer<'_>, ctx: &RenderContext<'_>) -> Rendered {
//!         // Report no frames: the library fills the period with silence.
//!         let _ = ctx;
//!         Rendered { frames: 0, end_of_stream: false }
//!     }
//! }
//!
//! # fn main() -> Result<(), Error> {
//! let clock = VirtualClockHandle::new();
//! let host = NullHost::new(NullClock::Virtual(clock.clone()));
//! let req = StreamRequest {
//!     device: DeviceSelector::Default(Role::Multimedia),
//!     mode: ShareMode::Shared,
//!     format: SampleFormat::F32,
//!     rate: 48_000,
//!     channels: 2,
//!     layout: ChannelLayout::unknown(2),
//!     buffer: BufferRequest::Frames(256),
//!     on_unavailable: OnUnavailable::Error,
//!     conversion: Conversion::Forbid,
//!     allow_rate_change_of_device: false,
//! };
//! let mut stream = host.open(&req, Box::new(Quiet))?;
//! // The real configuration is always readable.
//! assert_eq!(stream.config().rate, 48_000);
//! stream.start()?;
//! clock.advance(4);
//! assert_eq!(stream.counters().callbacks, 4);
//! stream.stop()?;
//! # Ok(())
//! # }
//! ```
//!
//! Features:
//! - `test-capture` (not default): the internal `CaptureHost` test backend,
//!   which records the delivered bytes to a file. Not part of the supported
//!   API.

mod matrix;

pub use matrix::support_matrix_markdown;
pub use vynamp_output_core::*;

/// Which host [`default_host`] chose, and why the others were skipped.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostSelection {
    pub chosen: HostId,
    /// Hosts tried before `chosen`, with the reason each was skipped.
    pub skipped: Vec<(HostId, &'static str)>,
}

/// Every host available in this build, in preference order.
///
/// Currently only [`NullHost`] on the real clock.
pub fn hosts() -> Vec<Box<dyn Host>> {
    vec![Box::new(NullHost::new(NullClock::Real))]
}

/// The preferred host for this platform and how it was chosen.
///
/// Currently always [`NullHost`] on the real clock, with nothing skipped.
pub fn default_host() -> (Box<dyn Host>, HostSelection) {
    let host = Box::new(NullHost::new(NullClock::Real));
    let selection = HostSelection {
        chosen: HostId::Null,
        skipped: Vec::new(),
    };
    (host, selection)
}
