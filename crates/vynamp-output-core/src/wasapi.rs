//! The translation between this crate's format types and the primitives the
//! WASAPI backend takes.
//!
//! The boundary rule (AGENTS.md §1.3) is that no domain type crosses a repo
//! boundary: the backend crate gets `(channels, rate, kind, container_bits,
//! valid_bits, channel_mask)` and returns the same. This module is that
//! translation, and it is pure, so it is tested on every target rather than
//! only where the backend runs.
//!
//! It lives in the core, not in the `-sys` crate, because it is about this
//! crate's types; the `-sys` crate never sees a `SampleFormat`.

use crate::error::Error;
use crate::format::{ChannelLayout, ChannelPosition, SampleFormat};

/// Whether the samples are integers or IEEE floats.
///
/// Mirrors the OS distinction; the width and the valid bits are separate
/// fields, exactly as in `WAVEFORMATEXTENSIBLE`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SampleKind {
    /// Integer PCM.
    Integer,
    /// IEEE floating point.
    Float,
}

/// A format as the WASAPI backend takes it: primitives only.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct WasapiFormat {
    /// Integer or float.
    pub kind: SampleKind,
    /// Channel count.
    pub channels: u16,
    /// Sample rate in Hz.
    pub rate: u32,
    /// Container width in bits.
    pub container_bits: u16,
    /// Meaningful bits.
    pub valid_bits: u16,
    /// `dwChannelMask`; 0 means "positions unspecified".
    pub channel_mask: u32,
}

impl WasapiFormat {
    /// The six primitives, in field order.
    pub fn into_primitives(self) -> (u16, u32, SampleKind, u16, u16, u32) {
        (
            self.channels,
            self.rate,
            self.kind,
            self.container_bits,
            self.valid_bits,
            self.channel_mask,
        )
    }

    /// Rebuilds a format from the six primitives.
    pub fn from_primitives(
        channels: u16,
        rate: u32,
        kind: SampleKind,
        container_bits: u16,
        valid_bits: u16,
        channel_mask: u32,
    ) -> Self {
        WasapiFormat {
            kind,
            channels,
            rate,
            container_bits,
            valid_bits,
            channel_mask,
        }
    }
}

/// The `dwChannelMask` bit of a channel position.
///
/// `Unknown` contributes no bit, which is why a layout of unknown positions
/// asks for mask 0 ("positions unspecified") rather than a wrong guess.
pub fn channel_mask(positions: &[ChannelPosition]) -> u32 {
    positions.iter().fold(0, |m, p| m | position_bit(*p))
}

/// The bit of one position.
fn position_bit(p: ChannelPosition) -> u32 {
    match p {
        ChannelPosition::FL => 0x1,
        ChannelPosition::FR => 0x2,
        ChannelPosition::FC => 0x4,
        ChannelPosition::LFE => 0x8,
        ChannelPosition::BL => 0x10,
        ChannelPosition::BR => 0x20,
        ChannelPosition::FLC => 0x40,
        ChannelPosition::FRC => 0x80,
        ChannelPosition::BC => 0x100,
        ChannelPosition::SL => 0x200,
        ChannelPosition::SR => 0x400,
        ChannelPosition::Unknown(_) => 0,
    }
}

/// Translates `format` into the backend's primitives.
///
/// `DoP` has no WASAPI representation: it is a PCM carrier, and the caller
/// must ask for the carrier format (`I24Packed` or `I32`) instead. Asking
/// for `Dop` here is `InvalidRequest`, not a silent substitution.
///
/// A layout whose length differs from `channels` is `InvalidRequest`, because
/// the mask would then describe a different channel count than the stream.
pub fn to_wasapi(
    format: SampleFormat,
    channels: u16,
    rate: u32,
    layout: &ChannelLayout,
) -> Result<WasapiFormat, Error> {
    if layout.positions.len() != usize::from(channels) {
        return Err(Error::InvalidRequest {
            what: "layout length differs from channels",
        });
    }
    let (kind, container_bits, valid_bits) = match format {
        SampleFormat::F32 => (SampleKind::Float, 32, 32),
        SampleFormat::F64 => (SampleKind::Float, 64, 64),
        SampleFormat::I16 => (SampleKind::Integer, 16, 16),
        SampleFormat::I24Packed => (SampleKind::Integer, 24, 24),
        SampleFormat::I24In32 { .. } => (SampleKind::Integer, 32, 24),
        SampleFormat::I32 { valid_bits } => (SampleKind::Integer, 32, u16::from(valid_bits)),
        SampleFormat::U8 => (SampleKind::Integer, 8, 8),
        SampleFormat::Dop { .. } => {
            return Err(Error::InvalidRequest {
                what: "dop has no wasapi format; ask for the pcm carrier",
            });
        }
    };
    Ok(WasapiFormat {
        kind,
        channels,
        rate,
        container_bits,
        valid_bits,
        channel_mask: channel_mask(&layout.positions),
    })
}
