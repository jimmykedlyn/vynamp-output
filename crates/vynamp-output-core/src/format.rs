//! Hosts, sample formats and channel layouts.

use std::fmt;

/// Identifies an audio backend (host).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HostId {
    Wasapi,
    Alsa,
    PipeWire,
    Pulse,
    CoreAudio,
    AAudio,
    Null,
    Capture,
}

impl HostId {
    /// Short lowercase name, stable for logs and messages.
    pub fn name(self) -> &'static str {
        match self {
            HostId::Wasapi => "wasapi",
            HostId::Alsa => "alsa",
            HostId::PipeWire => "pipewire",
            HostId::Pulse => "pulse",
            HostId::CoreAudio => "coreaudio",
            HostId::AAudio => "aaudio",
            HostId::Null => "null",
            HostId::Capture => "capture",
        }
    }
}

impl fmt::Display for HostId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// Where the meaningful bits sit inside a wider container.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Justify {
    Msb,
    Lsb,
}

/// Sample format. Container width and valid bits are always separate concepts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SampleFormat {
    F32,
    F64,
    I16,
    /// 24-bit samples in 3 bytes.
    I24Packed,
    /// 24-bit samples in a 32-bit container.
    I24In32 {
        justify: Justify,
    },
    /// 32-bit container with `valid_bits` meaningful bits (1..=32).
    I32 {
        valid_bits: u8,
    },
    U8,
    /// DSD over PCM; `dsd_rate` is the DSD bit rate (e.g. 2_822_400).
    Dop {
        dsd_rate: u32,
    },
}

impl SampleFormat {
    /// Bytes per sample of the container.
    ///
    /// `Dop` counts as 4 bytes, like `I24In32` (DoP-in-32 convention: a
    /// 24-bit DoP word carried in a 32-bit container).
    pub fn container_bytes(self) -> u8 {
        match self {
            SampleFormat::U8 => 1,
            SampleFormat::I16 => 2,
            SampleFormat::I24Packed => 3,
            SampleFormat::I24In32 { .. }
            | SampleFormat::I32 { .. }
            | SampleFormat::F32
            | SampleFormat::Dop { .. } => 4,
            SampleFormat::F64 => 8,
        }
    }

    /// DSD bytes per channel per frame in the source's
    /// [`DeviceBuffer::Dsd`](crate::DeviceBuffer::Dsd) buffer: `Dop` 2 (16
    /// DSD bits per carrier frame), `None` for PCM formats. A native DSD
    /// format (DSD_U8/U16/U32) adds its own width here.
    pub fn dsd_bytes_per_frame(self) -> Option<usize> {
        match self {
            SampleFormat::Dop { .. } => Some(2),
            _ => None,
        }
    }

    /// Meaningful bits of the format (`F32` 32, `F64` 64, `Dop` 24).
    pub fn valid_bits(self) -> u8 {
        match self {
            SampleFormat::F32 => 32,
            SampleFormat::F64 => 64,
            SampleFormat::I16 => 16,
            SampleFormat::I24Packed | SampleFormat::I24In32 { .. } | SampleFormat::Dop { .. } => 24,
            SampleFormat::I32 { valid_bits } => valid_bits,
            SampleFormat::U8 => 8,
        }
    }
}

/// Position of one channel in the output layout.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChannelPosition {
    FL,
    FR,
    FC,
    LFE,
    BL,
    BR,
    SL,
    SR,
    FLC,
    FRC,
    BC,
    Unknown(u16),
}

/// Ordered channel positions; its length must equal the channel count.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChannelLayout {
    pub positions: Vec<ChannelPosition>,
}

impl ChannelLayout {
    /// A layout of `n` channels with unspecified positions (`Unknown(0..n)`).
    pub fn unknown(n: u16) -> Self {
        ChannelLayout {
            positions: (0..n).map(ChannelPosition::Unknown).collect(),
        }
    }
}
