//! Verification levels and the per-host support declaration.
//!
//! Every host declares, per [`Feature`], the strongest [`VerificationLevel`]
//! it has reached (spec §5.10). `docs/support-matrix.md` is generated from
//! [`support`], so the matrix can never claim more than the code declares.

use crate::format::HostId;

/// How a (host, feature) pair has been verified, strongest first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[non_exhaustive]
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
#[non_exhaustive]
pub enum Feature {
    /// Opening streams in `ShareMode::Shared`.
    SharedMode,
    /// Opening streams in `ShareMode::Exclusive`.
    ExclusiveMode,
    /// Integer PCM containers (16, 24 packed, 24-in-32, 32).
    IntegerPcm,
    /// DSD over PCM (`SampleFormat::Dop`).
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

/// The strongest verification `host` has reached for `feature`, or `None`
/// when the host does not implement it in this release.
///
/// `Null` and `Capture` are library-verified for every feature: their tests
/// run in every CI build.
///
/// `Wasapi` is `Compiled`: the backend is in the crate and the Windows target
/// compiles it in CI, and nothing was ever run. `Compiled` is the weakest
/// level and the only honest one — no WASAPI test has run on hardware, so
/// nothing above it is claimed. The hardware criteria (`OUTPUT-AC-16` to
/// `OUTPUT-AC-18`, `OUTPUT-BK-AC-15` to `AC-19`) are `#[ignore]` until a DAC
/// and a Windows 11 machine run them; when they do, this is raised to
/// `HardwareVerified` and the evidence goes in `docs/hardware-matrix.md`.
///
/// The answer is the same on every host target: `docs/support-matrix.md` is
/// one committed file, so it must not change when the doc generator runs on a
/// different OS. The `Compiled` claim is backed by the `wasapi` job in
/// `.github/workflows/ci.yml`, which builds this crate for
/// `x86_64-pc-windows-msvc` on every run.
///
/// The other OS backends are not implemented: `None`.
pub fn support(host: HostId, _feature: Feature) -> Option<VerificationLevel> {
    match host {
        HostId::Null | HostId::Capture => Some(VerificationLevel::LibraryVerified),
        HostId::Wasapi => Some(VerificationLevel::Compiled),
        HostId::Alsa | HostId::PipeWire | HostId::Pulse | HostId::CoreAudio | HostId::AAudio => {
            None
        }
    }
}
