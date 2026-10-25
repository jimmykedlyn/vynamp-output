//! vynamp-output: audio output with explicit modes, honest negotiation and
//! a real-time render contract.
//!
//! This facade re-exports [`vynamp_output_core`] and chooses a host. In this
//! release only the pure-Rust [`NullHost`] exists; OS backends (WASAPI,
//! ALSA, CoreAudio, AAudio, PipeWire, Pulse) arrive in later releases and
//! will be listed by [`hosts`] and chosen by [`default_host`].
//!
//! Features:
//! - `test-capture` (not default): the internal `CaptureHost` test backend,
//!   which records the delivered bytes to a file. Not part of the supported
//!   API.

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
