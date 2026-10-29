//! Verification levels and the per-host support declaration.
//!
//! Every host declares, per [`Feature`], the strongest [`VerificationLevel`]
//! it has reached (spec §5.10). `docs/support-matrix.md` is generated from
//! [`support`], so the matrix can never claim more than the code declares.

use crate::format::HostId;

/// How a (host, feature) pair has been verified, strongest first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum VerificationLevel {
    /// Delivered bytes compared bit for bit at a point that includes the OS path.
    CaptureVerified,
    /// Manual hardware matrix entry (`docs/hardware-matrix.md`).
    HardwareVerified,
    /// Opened, played, drained and lost on the real OS stack in CI.
    Integration,
    /// Proven by the Null (real or virtual clock) and Capture hosts in every CI run.
    LibraryVerified,
    /// The target builds; nothing was run.
    Compiled,
}

impl VerificationLevel {
    /// Stable lowercase name used in the support matrix.
    pub fn name(self) -> &'static str {
        match self {
            VerificationLevel::CaptureVerified => "capture-verified",
            VerificationLevel::HardwareVerified => "hardware-verified",
            VerificationLevel::Integration => "integration",
            VerificationLevel::LibraryVerified => "library-verified",
            VerificationLevel::Compiled => "compiled",
        }
    }
}

/// A capability whose verification level is tracked per host.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Feature {
    SharedMode,
    ExclusiveMode,
    /// Integer PCM containers (16, 24 packed, 24-in-32, 32).
    IntegerPcm,
    Dop,
    /// `Stream::timing` and `RenderContext::timing`.
    Timing,
    /// Underrun, device-lost and panic events.
    Events,
}

impl Feature {
    /// Every feature, in matrix column order.
    pub const ALL: [Feature; 6] = [
        Feature::SharedMode,
        Feature::ExclusiveMode,
        Feature::IntegerPcm,
        Feature::Dop,
        Feature::Timing,
        Feature::Events,
    ];

    /// Column title in the support matrix.
    pub fn name(self) -> &'static str {
        match self {
            Feature::SharedMode => "shared",
            Feature::ExclusiveMode => "exclusive",
            Feature::IntegerPcm => "integer PCM",
            Feature::Dop => "DoP",
            Feature::Timing => "timing",
            Feature::Events => "events",
        }
    }
}

impl HostId {
    /// Every host, in matrix row order.
    pub const ALL: [HostId; 8] = [
        HostId::Wasapi,
        HostId::Alsa,
        HostId::PipeWire,
        HostId::Pulse,
        HostId::CoreAudio,
        HostId::AAudio,
        HostId::Null,
        HostId::Capture,
    ];
}

/// The strongest verification `host` has reached for `feature`, or `None`
/// when the host does not implement it in this release.
///
/// The OS backends are not implemented yet. `Null` and `Capture` are
/// library-verified for every feature: their tests run in every CI build.
pub fn support(host: HostId, _feature: Feature) -> Option<VerificationLevel> {
    match host {
        HostId::Null | HostId::Capture => Some(VerificationLevel::LibraryVerified),
        HostId::Wasapi
        | HostId::Alsa
        | HostId::PipeWire
        | HostId::Pulse
        | HostId::CoreAudio
        | HostId::AAudio => None,
    }
}
