//! Translating a `StreamRequest` into the primitives the WASAPI backend
//! takes, and back.
//!
//! The boundary rule is that no domain type crosses a repo boundary: the
//! backend gets `(channels, rate, kind, container_bits, valid_bits, mask)`
//! and returns the same. This module is the translation, and it is pure, so
//! it is tested here on every target rather than only on Windows.

use vynamp_output_core::wasapi::{SampleKind, WasapiFormat, channel_mask, to_wasapi};
use vynamp_output_core::{ChannelLayout, ChannelPosition, SampleFormat};

#[test]
fn i24_packed_is_24_bits_in_a_24_bit_container() {
    let f = to_wasapi(
        SampleFormat::I24Packed,
        2,
        96_000,
        &ChannelLayout::unknown(2),
    )
    .unwrap();
    assert_eq!(f.kind, SampleKind::Integer);
    assert_eq!(f.container_bits, 24);
    assert_eq!(f.valid_bits, 24);
    assert_eq!(f.channels, 2);
    assert_eq!(f.rate, 96_000);
}

#[test]
fn i24_in_32_keeps_container_and_valid_bits_apart() {
    let f = to_wasapi(
        SampleFormat::I24In32 {
            justify: vynamp_output_core::Justify::Msb,
        },
        2,
        192_000,
        &ChannelLayout::unknown(2),
    )
    .unwrap();
    assert_eq!(f.container_bits, 32);
    assert_eq!(f.valid_bits, 24);
}

#[test]
fn i32_with_valid_bits_reports_them() {
    let f = to_wasapi(
        SampleFormat::I32 { valid_bits: 24 },
        2,
        48_000,
        &ChannelLayout::unknown(2),
    )
    .unwrap();
    assert_eq!(f.container_bits, 32);
    assert_eq!(f.valid_bits, 24);
}

#[test]
fn f32_is_a_float_container() {
    let f = to_wasapi(SampleFormat::F32, 2, 48_000, &ChannelLayout::unknown(2)).unwrap();
    assert_eq!(f.kind, SampleKind::Float);
    assert_eq!(f.container_bits, 32);
    assert_eq!(f.valid_bits, 32);
}

#[test]
fn dop_has_no_wasapi_representation() {
    // DoP is a PCM carrier; the backend must be asked for the carrier
    // format, never for a "DoP format" the OS does not have.
    let err = to_wasapi(
        SampleFormat::Dop {
            dsd_rate: 2_822_400,
        },
        2,
        176_400,
        &ChannelLayout::unknown(2),
    );
    assert!(err.is_err(), "DoP must not map to a WASAPI format");
}

#[test]
fn a_known_stereo_layout_becomes_the_front_lr_mask() {
    let layout = ChannelLayout {
        positions: vec![ChannelPosition::FL, ChannelPosition::FR],
    };
    let f = to_wasapi(SampleFormat::I24Packed, 2, 96_000, &layout).unwrap();
    assert_eq!(f.channel_mask, 0x3);
}

#[test]
fn an_unknown_layout_asks_for_no_mask() {
    let f = to_wasapi(
        SampleFormat::I24Packed,
        2,
        96_000,
        &ChannelLayout::unknown(2),
    )
    .unwrap();
    assert_eq!(f.channel_mask, 0, "positions unspecified");
}

#[test]
fn a_layout_whose_length_disagrees_with_channels_is_rejected() {
    let layout = ChannelLayout {
        positions: vec![ChannelPosition::FL],
    };
    assert!(to_wasapi(SampleFormat::I24Packed, 2, 96_000, &layout).is_err());
}

#[test]
fn channel_mask_maps_every_position_the_layout_can_hold() {
    let all = [
        ChannelPosition::FL,
        ChannelPosition::FR,
        ChannelPosition::FC,
        ChannelPosition::LFE,
        ChannelPosition::BL,
        ChannelPosition::BR,
        ChannelPosition::FLC,
        ChannelPosition::FRC,
        ChannelPosition::BC,
        ChannelPosition::SL,
        ChannelPosition::SR,
    ];
    assert_eq!(channel_mask(&all), 0x7FF);
}

#[test]
fn an_unknown_position_contributes_no_bit() {
    assert_eq!(channel_mask(&[ChannelPosition::Unknown(0)]), 0);
}

#[test]
fn wasapi_format_round_trips_through_primitives() {
    let f = WasapiFormat {
        kind: SampleKind::Integer,
        channels: 2,
        rate: 96_000,
        container_bits: 32,
        valid_bits: 24,
        channel_mask: 0x3,
    };
    let (ch, rate, kind, container, valid, mask) = f.into_primitives();
    assert_eq!(
        (ch, rate, kind, container, valid, mask),
        (2, 96_000, SampleKind::Integer, 32, 24, 0x3)
    );
    assert_eq!(
        WasapiFormat::from_primitives(ch, rate, kind, container, valid, mask),
        f
    );
}
