//! `WAVEFORMATEXTENSIBLE` as plain data, plus the channel-mask rules.
//!
//! The struct is built and read here as a fixed 40-byte little-endian array
//! rather than as a `#[repr(C)]` struct with a `GUID` field. Two reasons:
//! the layout is then testable on any target (the tests in `tests/format.rs`
//! run on Linux CI), and there is no padding or alignment question to get
//! wrong when the array is handed to the OS as a `*const WAVEFORMATEX`.
//!
//! Layout, from the documented C definition (`mmreg.h`, recorded in
//! `docs/sources/wasapi.md`):
//!
//! ```text
//! offset  size  field
//!      0     2  wFormatTag            (WAVE_FORMAT_EXTENSIBLE = 0xFFFE)
//!      2     2  nChannels
//!      4     4  nSamplesPerSec
//!      8     4  nAvgBytesPerSec
//!     12     2  nBlockAlign
//!     14     2  wBitsPerSample        (container size)
//!     16     2  cbSize                (22)
//!     18     2  Samples.wValidBitsPerSample
//!     20     4  dwChannelMask
//!     24    16  SubFormat             (GUID)
//! ```

/// `WAVE_FORMAT_EXTENSIBLE`.
const WAVE_FORMAT_EXTENSIBLE: u16 = 0xFFFE;
/// `cbSize` of a `WAVEFORMATEXTENSIBLE`: the struct minus `WAVEFORMATEX`.
const CB_SIZE: u16 = 22;

/// `KSDATAFORMAT_SUBTYPE_PCM` in memory order.
const SUBTYPE_PCM: [u8; 16] = [
    0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x10, 0x00, 0x80, 0x00, 0x00, 0xAA, 0x00, 0x38, 0x9B, 0x71,
];
/// `KSDATAFORMAT_SUBTYPE_IEEE_FLOAT` in memory order.
const SUBTYPE_FLOAT: [u8; 16] = [
    0x03, 0x00, 0x00, 0x00, 0x00, 0x00, 0x10, 0x00, 0x80, 0x00, 0x00, 0xAA, 0x00, 0x38, 0x9B, 0x71,
];

/// Whether the samples are integers or IEEE floats.
///
/// This is the only distinction the OS format descriptor makes; the width
/// and the valid bits are separate fields.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SampleKind {
    /// `KSDATAFORMAT_SUBTYPE_PCM`.
    Integer,
    /// `KSDATAFORMAT_SUBTYPE_IEEE_FLOAT`.
    Float,
}

/// Byte order of the samples. WASAPI is little-endian on every supported
/// Windows target; the enum exists so a caller cannot silently assume it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Endian {
    /// Least significant byte first.
    Little,
}

/// A speaker position, as the `dwChannelMask` bits name them.
///
/// Only the positions the library can produce from a `ChannelLayout` are
/// listed; an unknown position contributes no bit, which is why
/// [`channel_mask`] can return 0.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SpeakerPosition {
    /// `SPEAKER_FRONT_LEFT` (0x1).
    FrontLeft,
    /// `SPEAKER_FRONT_RIGHT` (0x2).
    FrontRight,
    /// `SPEAKER_FRONT_CENTER` (0x4).
    FrontCenter,
    /// `SPEAKER_LOW_FREQUENCY` (0x8).
    LowFrequency,
    /// `SPEAKER_BACK_LEFT` (0x10).
    BackLeft,
    /// `SPEAKER_BACK_RIGHT` (0x20).
    BackRight,
    /// `SPEAKER_FRONT_LEFT_OF_CENTER` (0x40).
    FrontLeftOfCenter,
    /// `SPEAKER_FRONT_RIGHT_OF_CENTER` (0x80).
    FrontRightOfCenter,
    /// `SPEAKER_BACK_CENTER` (0x100).
    BackCenter,
    /// `SPEAKER_SIDE_LEFT` (0x200).
    SideLeft,
    /// `SPEAKER_SIDE_RIGHT` (0x400).
    SideRight,
}

impl SpeakerPosition {
    /// The `dwChannelMask` bit of this position.
    pub const fn bit(self) -> u32 {
        match self {
            SpeakerPosition::FrontLeft => 0x1,
            SpeakerPosition::FrontRight => 0x2,
            SpeakerPosition::FrontCenter => 0x4,
            SpeakerPosition::LowFrequency => 0x8,
            SpeakerPosition::BackLeft => 0x10,
            SpeakerPosition::BackRight => 0x20,
            SpeakerPosition::FrontLeftOfCenter => 0x40,
            SpeakerPosition::FrontRightOfCenter => 0x80,
            SpeakerPosition::BackCenter => 0x100,
            SpeakerPosition::SideLeft => 0x200,
            SpeakerPosition::SideRight => 0x400,
        }
    }
}

/// The `dwChannelMask` for `positions`, OR-ing their bits.
///
/// An empty list gives 0, which the OS reads as "positions unspecified".
pub fn channel_mask(positions: &[SpeakerPosition]) -> u32 {
    positions.iter().fold(0, |m, p| m | p.bit())
}

/// A `WAVEFORMATEXTENSIBLE` as plain data.
///
/// `container_bits` is `wBitsPerSample` and `valid_bits` is
/// `Samples.wValidBitsPerSample`; they are deliberately separate, because a
/// 32-bit container carrying 24 meaningful bits is a different request from
/// a 24-bit container.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct WaveFormatExtensible {
    /// Integer or float samples.
    pub kind: SampleKind,
    /// Channel count.
    pub channels: u16,
    /// Sample rate in Hz.
    pub rate: u32,
    /// Container width in bits (8, 16, 24 or 32).
    pub container_bits: u16,
    /// Meaningful bits; 0 is read as "same as the container".
    pub valid_bits: u16,
    /// `dwChannelMask`.
    pub channel_mask: u32,
}

impl WaveFormatExtensible {
    /// Size of the struct in bytes.
    pub const SIZE: usize = 40;

    /// Builds a descriptor. `valid_bits` of 0 means "same as the container".
    pub const fn new(
        kind: SampleKind,
        channels: u16,
        rate: u32,
        container_bits: u16,
        valid_bits: u16,
        channel_mask: u32,
    ) -> Self {
        WaveFormatExtensible {
            kind,
            channels,
            rate,
            container_bits,
            valid_bits,
            channel_mask,
        }
    }

    /// Bytes per sample of the container.
    pub const fn container_bytes(self) -> u16 {
        self.container_bits / 8
    }

    /// Bytes of one frame (all channels).
    pub const fn block_align(self) -> u16 {
        self.channels * self.container_bytes()
    }

    /// `nAvgBytesPerSec`.
    pub const fn avg_bytes_per_sec(self) -> u32 {
        self.rate * self.block_align() as u32
    }

    /// The effective valid bits: `valid_bits`, or the container when 0.
    pub const fn effective_valid_bits(self) -> u16 {
        if self.valid_bits == 0 {
            self.container_bits
        } else {
            self.valid_bits
        }
    }

    /// The 40 bytes the OS reads, little-endian.
    pub fn to_bytes(self) -> [u8; Self::SIZE] {
        let mut b = [0u8; Self::SIZE];
        b[0..2].copy_from_slice(&WAVE_FORMAT_EXTENSIBLE.to_le_bytes());
        b[2..4].copy_from_slice(&self.channels.to_le_bytes());
        b[4..8].copy_from_slice(&self.rate.to_le_bytes());
        b[8..12].copy_from_slice(&self.avg_bytes_per_sec().to_le_bytes());
        b[12..14].copy_from_slice(&self.block_align().to_le_bytes());
        b[14..16].copy_from_slice(&self.container_bits.to_le_bytes());
        b[16..18].copy_from_slice(&CB_SIZE.to_le_bytes());
        b[18..20].copy_from_slice(&self.valid_bits.to_le_bytes());
        b[20..24].copy_from_slice(&self.channel_mask.to_le_bytes());
        let sub = match self.kind {
            SampleKind::Integer => SUBTYPE_PCM,
            SampleKind::Float => SUBTYPE_FLOAT,
        };
        b[24..40].copy_from_slice(&sub);
        b
    }

    /// Reads a descriptor back, or `None` when the tag is not
    /// `WAVE_FORMAT_EXTENSIBLE` or the subformat is neither PCM nor float.
    pub fn from_bytes(b: &[u8; Self::SIZE]) -> Option<Self> {
        let tag = u16::from_le_bytes([b[0], b[1]]);
        if tag != WAVE_FORMAT_EXTENSIBLE {
            return None;
        }
        let sub: [u8; 16] = b[24..40].try_into().ok()?;
        let kind = if sub == SUBTYPE_PCM {
            SampleKind::Integer
        } else if sub == SUBTYPE_FLOAT {
            SampleKind::Float
        } else {
            return None;
        };
        Some(WaveFormatExtensible {
            kind,
            channels: u16::from_le_bytes([b[2], b[3]]),
            rate: u32::from_le_bytes([b[4], b[5], b[6], b[7]]),
            container_bits: u16::from_le_bytes([b[14], b[15]]),
            valid_bits: u16::from_le_bytes([b[18], b[19]]),
            channel_mask: u32::from_le_bytes([b[20], b[21], b[22], b[23]]),
        })
    }

    /// Whether this descriptor is exactly the requested format.
    ///
    /// A device that reports 0 valid bits is read as "the whole container",
    /// which is how some drivers answer; that is a match for the full
    /// container and a mismatch for anything narrower.
    pub fn accepts(
        self,
        channels: u16,
        rate: u32,
        kind: SampleKind,
        container_bits: u16,
        valid_bits: u16,
    ) -> bool {
        self.channels == channels
            && self.rate == rate
            && self.kind == kind
            && self.container_bits == container_bits
            && self.effective_valid_bits() == valid_bits
    }
}

/// Whether `device` is exactly the requested format.
///
/// A free function so the backend can call it with primitives only, keeping
/// domain types on the other side of the boundary.
pub fn sample_format_supported(
    device: &WaveFormatExtensible,
    channels: u16,
    rate: u32,
    kind: SampleKind,
    container_bits: u16,
    valid_bits: u16,
) -> bool {
    device.accepts(channels, rate, kind, container_bits, valid_bits)
}
