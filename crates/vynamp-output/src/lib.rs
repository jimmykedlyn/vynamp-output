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
/// On Windows the WASAPI host comes first and the [`NullHost`] last; on every
/// other target only the [`NullHost`] exists, because no OS backend is
/// compiled there. The order is decided by [`candidates`], which is a pure
/// function and is tested on every target.
pub fn hosts() -> Vec<Box<dyn Host>> {
    let mut out: Vec<Box<dyn Host>> = Vec::new();
    for name in candidates(&preference()) {
        match name {
            #[cfg(all(target_os = "windows", feature = "wasapi"))]
            "wasapi" => out.push(Box::new(WasapiHost::new())),
            "null" => out.push(Box::new(NullHost::new(NullClock::Real))),
            // A name the build does not contain cannot appear: `preference`
            // only sets a flag for a backend that was compiled.
            _ => {}
        }
    }
    out
}

/// Which backends this build contains.
fn preference() -> Preference {
    Preference::new(
        cfg!(all(target_os = "windows", feature = "wasapi")),
        cfg!(feature = "test-capture"),
        false,
        false,
    )
}

/// The preferred host for this platform and how it was chosen.
///
/// The first entry of [`hosts`], with the hosts that were not chosen listed
/// in `skipped` and the reason each was passed over.
pub fn default_host() -> (Box<dyn Host>, HostSelection) {
    let mut all = hosts();
    let chosen = all.first().map(|h| h.id()).unwrap_or(HostId::Null);
    let skipped = all
        .iter()
        .skip(1)
        .map(|h| (h.id(), skip_reason(h.id())))
        .collect();
    let host = all
        .drain(..)
        .next()
        .unwrap_or_else(|| Box::new(NullHost::new(NullClock::Real)));
    (host, HostSelection { chosen, skipped })
}

/// Why a host that was not chosen was passed over.
fn skip_reason(host: HostId) -> &'static str {
    match host {
        HostId::Null => "fallback host: discards audio",
        _ => "a more specific backend was chosen first",
    }
}
