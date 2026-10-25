//! Matching a request against device capabilities.

use crate::config::{PathProperties, StreamConfig};
use crate::error::Error;
use crate::format::HostId;
use crate::format::{ChannelLayout, SampleFormat};
use crate::request::DeviceId;
use crate::request::{Conversion, ConversionSpec, OnUnavailable, ShareMode, StreamRequest};

/// How a capability entry was learned.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CapabilityOrigin {
    Advertised,
    Probed,
}

/// One (mode, format) combination with the rates and channel counts it supports.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapabilityEntry {
    pub mode: ShareMode,
    pub format: SampleFormat,
    pub rates: Vec<u32>,
    pub channels: Vec<u16>,
    pub origin: CapabilityOrigin,
}

/// What a device can do, with the identity needed to build a [`StreamConfig`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Capabilities {
    pub host: HostId,
    pub device: DeviceId,
    pub entries: Vec<CapabilityEntry>,
}

/// Result of negotiation: a config for the backend to complete.
///
/// In `config_template`, `period_frames` and `buffer_frames` are 0 (the
/// backend fills them) and `path` holds only what negotiation knows.
#[derive(Debug, Clone, PartialEq)]
pub struct Negotiated {
    pub config_template: StreamConfig,
    /// `Some(reason)` when the request was downgraded to the nearest match.
    pub downgraded: Option<&'static str>,
}

fn invalid(what: &'static str) -> Error {
    Error::InvalidRequest { what }
}

fn validate(req: &StreamRequest) -> Result<(), Error> {
    if req.rate == 0 {
        return Err(invalid("rate is zero"));
    }
    if req.channels == 0 {
        return Err(invalid("channels is zero"));
    }
    if req.layout.positions.len() != usize::from(req.channels) {
        return Err(invalid("layout length differs from channels"));
    }
    if let SampleFormat::I32 { valid_bits } = req.format
        && !(1..=32).contains(&valid_bits)
    {
        return Err(invalid("valid bits out of range"));
    }
    if let SampleFormat::Dop { dsd_rate: 0 } = req.format {
        return Err(invalid("dsd rate is zero"));
    }
    if let Conversion::Allow(spec) = req.conversion {
        if spec.from != req.format {
            return Err(invalid("conversion source differs from requested format"));
        }
        if let SampleFormat::I32 { valid_bits } = spec.to
            && !(1..=32).contains(&valid_bits)
        {
            return Err(invalid("valid bits out of range"));
        }
        match (spec.from, spec.to) {
            (SampleFormat::Dop { dsd_rate: a }, SampleFormat::Dop { dsd_rate: b }) => {
                if a != b {
                    return Err(invalid("dop conversion changes dsd rate"));
                }
            }
            // PCM -> DoP would be DSD modulation; the library never synthesises.
            (_, SampleFormat::Dop { .. }) => {
                return Err(invalid("conversion cannot turn pcm into dop"));
            }
            // DoP -> PCM is only a container repack: integer, >= 24 valid bits.
            (SampleFormat::Dop { .. }, pcm)
                if matches!(pcm, SampleFormat::F32 | SampleFormat::F64)
                    || pcm.valid_bits() < 24 =>
            {
                return Err(invalid("dop requires a 24-bit capable container"));
            }
            _ => {}
        }
    }
    Ok(())
}

fn template(
    caps: &Capabilities,
    req: &StreamRequest,
    format: SampleFormat,
    rate: u32,
    channels: u16,
    conversion: Option<ConversionSpec>,
) -> StreamConfig {
    let layout = if channels == req.channels {
        req.layout.clone()
    } else {
        ChannelLayout::unknown(channels)
    };
    let mut path = PathProperties::unknown();
    path.library_conversion = conversion;
    StreamConfig {
        host: caps.host,
        device: caps.device.clone(),
        mode: req.mode,
        format,
        valid_bits: format.valid_bits(),
        rate,
        channels,
        layout,
        period_frames: 0,
        buffer_frames: 0,
        path,
    }
}

fn supports(
    entry: &CapabilityEntry,
    mode: ShareMode,
    format: SampleFormat,
    rate: u32,
    ch: u16,
) -> bool {
    entry.mode == mode
        && entry.format == format
        && entry.rates.contains(&rate)
        && entry.channels.contains(&ch)
}

/// Smallest value `>= want`, else the largest value `< want`.
fn closest_up_first(values: impl Iterator<Item = u32>, want: u32) -> Option<u32> {
    let mut above: Option<u32> = None;
    let mut below: Option<u32> = None;
    for v in values {
        if v >= want {
            above = Some(above.map_or(v, |a| a.min(v)));
        } else {
            below = Some(below.map_or(v, |b| b.max(v)));
        }
    }
    above.or(below)
}

fn is_dop(format: SampleFormat) -> bool {
    matches!(format, SampleFormat::Dop { .. })
}

/// Nearest supported (format, rate, channels) in the request's mode.
///
/// DoP and PCM are incompatible families and are never mixed: a DoP request
/// only gets a DoP nearest (possibly with another `dsd_rate`) and a PCM
/// request only a PCM one, because DoP marker words played as PCM are loud
/// noise. Entries with empty `rates` or `channels` are ignored. Channels are
/// taken from the chosen entry only. With no candidate the result is `None`.
///
/// Order: rate (closest >= requested, else closest <), then container width
/// (same, else closest, ties to wider), then format (same, else first listed),
/// then channels (same, else closest >=, else closest <).
fn nearest(req: &StreamRequest, caps: &Capabilities) -> Option<(SampleFormat, u32, u16)> {
    let same_mode = || {
        caps.entries.iter().filter(|e| {
            e.mode == req.mode
                && is_dop(e.format) == is_dop(req.format)
                && !e.rates.is_empty()
                && !e.channels.is_empty()
        })
    };
    let rate = closest_up_first(same_mode().flat_map(|e| e.rates.iter().copied()), req.rate)?;
    let at_rate = || same_mode().filter(move |e| e.rates.contains(&rate));
    let want_w = i16::from(req.format.container_bytes());
    let best_w = at_rate()
        .map(|e| i16::from(e.format.container_bytes()))
        .min_by_key(|w| ((w - want_w).abs(), -w))?;
    let at_width = || at_rate().filter(move |e| i16::from(e.format.container_bytes()) == best_w);
    let entry = at_width()
        .find(|e| e.format == req.format)
        .or_else(|| at_width().next())?;
    let channels = closest_up_first(
        entry.channels.iter().map(|&c| u32::from(c)),
        u32::from(req.channels),
    )?;
    let channels = u16::try_from(channels).ok()?;
    Some((entry.format, rate, channels))
}

/// Reports the first differing cause, checked in the order rate, format, channels.
fn downgrade_reason(req: &StreamRequest, got: (SampleFormat, u32, u16)) -> &'static str {
    if got.1 != req.rate {
        "rate not supported"
    } else if got.0 != req.format {
        "format not supported"
    } else {
        "channel count not supported"
    }
}

/// Matches `req` against `caps`.
///
/// 1. Exact match (mode, format, rate, channels) gives the config as asked.
/// 2. Else, with `Conversion::Allow(spec)` and the device supporting
///    `spec.to` at the requested rate and channels, opens in `spec.to` and
///    records the conversion in `path.library_conversion`.
/// 3. Else the nearest configuration: returned in the error as
///    [`Error::FormatUnsupported`] by default, or as a successful downgrade
///    (with a reason) when `OnUnavailable::Downgrade` was requested.
pub fn negotiate(req: &StreamRequest, caps: &Capabilities) -> Result<Negotiated, Error> {
    validate(req)?;

    let exact = |format| {
        caps.entries
            .iter()
            .any(|e| supports(e, req.mode, format, req.rate, req.channels))
    };
    if exact(req.format) {
        return Ok(Negotiated {
            config_template: template(caps, req, req.format, req.rate, req.channels, None),
            downgraded: None,
        });
    }
    if let Conversion::Allow(spec) = req.conversion
        && exact(spec.to)
    {
        return Ok(Negotiated {
            config_template: template(caps, req, spec.to, req.rate, req.channels, Some(spec)),
            downgraded: None,
        });
    }

    match nearest(req, caps) {
        None => Err(Error::FormatUnsupported { nearest: None }),
        Some((format, rate, channels)) => {
            let config = template(caps, req, format, rate, channels, None);
            match req.on_unavailable {
                OnUnavailable::Error => Err(Error::FormatUnsupported {
                    nearest: Some(Box::new(config)),
                }),
                OnUnavailable::Downgrade => Ok(Negotiated {
                    config_template: config,
                    downgraded: Some(downgrade_reason(req, (format, rate, channels))),
                }),
            }
        }
    }
}
