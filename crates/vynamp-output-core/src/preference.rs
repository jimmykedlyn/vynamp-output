//! Which host the facade tries first, as a pure function.
//!
//! The facade cannot build a `Box<dyn Host>` for a backend that was not
//! compiled, so the decision "which backends does this build contain" is
//! separated from "in what order do we try them". That makes the order
//! testable on every target, including the Windows-only branch, without a
//! Windows machine.
//!
//! The rule: a real OS backend beats the Null host, and the Null host is
//! always last and always present, because it is the only host that works
//! everywhere. `Capture` is an internal test backend and is never a
//! candidate for real playback.

/// Which backends this build contains.
///
/// The facade fills this from `cfg!`; tests fill it directly.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Preference {
    /// The WASAPI backend is compiled in.
    pub wasapi: bool,
    /// The Capture test backend is compiled in.
    pub capture: bool,
    /// The ALSA backend is compiled in.
    pub alsa: bool,
    /// The PipeWire backend is compiled in.
    pub pipewire: bool,
}

impl Preference {
    /// Builds a set from the four flags, in field order.
    pub const fn new(wasapi: bool, capture: bool, alsa: bool, pipewire: bool) -> Self {
        Preference {
            wasapi,
            capture,
            alsa,
            pipewire,
        }
    }
}

/// The host names to try, in order, for the backends in `p`.
///
/// Names are the stable lowercase ones from [`HostId::name`](crate::HostId::name),
/// so the facade can map them back without a second table. `null` is always
/// last and appears exactly once; `capture` never appears.
pub fn candidates(p: &Preference) -> Vec<&'static str> {
    let mut out = Vec::new();
    if p.wasapi {
        out.push("wasapi");
    }
    if p.alsa {
        out.push("alsa");
    }
    if p.pipewire {
        out.push("pipewire");
    }
    out.push("null");
    out
}
