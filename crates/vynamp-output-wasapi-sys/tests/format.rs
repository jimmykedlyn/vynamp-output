//! The `WAVEFORMATEXTENSIBLE` the backend hands to `IAudioClient::Initialize`
//! must match the layout the OS documents, field by field and byte by byte.
//!
//! Oracle: the documented C struct (offsets and widths from Microsoft Learn,
//! recorded in `docs/sources/wasapi.md`), written out here as an independent
//! table rather than derived from the implementation.

use vynamp_output_wasapi_sys::{
    SampleKind, SpeakerPosition, WaveFormatExtensible, channel_mask, sample_format_supported,
};

/// `WAVEFORMATEX` field offsets inside a `WAVEFORMATEXTENSIBLE`, per the
/// documented C layout: 18-byte `WAVEFORMATEX`, then the `Samples` union,
/// `dwChannelMask` and the `SubFormat` GUID.
const OFF_FORMAT_TAG: usize = 0;
const OFF_CHANNELS: usize = 2;
const OFF_RATE: usize = 4;
const OFF_AVG_BYTES: usize = 8;
const OFF_BLOCK_ALIGN: usize = 12;
const OFF_BITS: usize = 14;
const OFF_CB_SIZE: usize = 16;
const OFF_VALID_BITS: usize = 18;
const OFF_MASK: usize = 20;
const OFF_SUBFORMAT: usize = 24;
const SIZE: usize = 40;

const WAVE_FORMAT_PCM: u16 = 0x0001;
const WAVE_FORMAT_EXTENSIBLE: u16 = 0xFFFE;

/// `KSDATAFORMAT_SUBTYPE_PCM`, little-endian as the GUID is laid out in
/// memory (`Data1` u32, `Data2`/`Data3` u16, then 8 bytes).
const SUBTYPE_PCM: [u8; 16] = [
    0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x10, 0x00, 0x80, 0x00, 0x00, 0xAA, 0x00, 0x38, 0x9B, 0x71,
];
/// `KSDATAFORMAT_SUBTYPE_IEEE_FLOAT`, same layout.
const SUBTYPE_FLOAT: [u8; 16] = [
    0x03, 0x00, 0x00, 0x00, 0x00, 0x00, 0x10, 0x00, 0x80, 0x00, 0x00, 0xAA, 0x00, 0x38, 0x9B, 0x71,
];

fn u16_at(b: &[u8; SIZE], at: usize) -> u16 {
    u16::from_le_bytes([b[at], b[at + 1]])
}

fn u32_at(b: &[u8; SIZE], at: usize) -> u32 {
    u32::from_le_bytes([b[at], b[at + 1], b[at + 2], b[at + 3]])
}

#[test]
fn size_is_the_documented_forty_bytes() {
    assert_eq!(WaveFormatExtensible::SIZE, SIZE);
    assert_eq!(WaveFormatExtensible::SIZE, 18 + 2 + 4 + 16);
}

#[test]
fn i24_packed_stereo_96k_fills_every_field() {
    // 24-bit samples in 3-byte containers, 2 channels, 96 kHz, front L/R.
    let f = WaveFormatExtensible::new(SampleKind::Integer, 2, 96_000, 24, 24, 0x3);
    let b = f.to_bytes();
    assert_eq!(u16_at(&b, OFF_FORMAT_TAG), WAVE_FORMAT_EXTENSIBLE);
    assert_eq!(u16_at(&b, OFF_CHANNELS), 2);
    assert_eq!(u32_at(&b, OFF_RATE), 96_000);
    assert_eq!(u16_at(&b, OFF_BLOCK_ALIGN), 6, "2 ch * 3 bytes");
    assert_eq!(u32_at(&b, OFF_AVG_BYTES), 96_000 * 6);
    assert_eq!(u16_at(&b, OFF_BITS), 24, "container size");
    assert_eq!(
        u16_at(&b, OFF_CB_SIZE),
        22,
        "40-byte struct minus WAVEFORMATEX"
    );
    assert_eq!(u16_at(&b, OFF_VALID_BITS), 24);
    assert_eq!(u32_at(&b, OFF_MASK), 0x3);
    assert_eq!(&b[OFF_SUBFORMAT..OFF_SUBFORMAT + 16], &SUBTYPE_PCM);
}

#[test]
fn i24_in_32_declares_24_valid_bits_in_a_32_bit_container() {
    // The distinction the spec insists on: container and valid bits are
    // separate. A 32-bit container with 24 meaningful bits must say 32/24.
    let f = WaveFormatExtensible::new(SampleKind::Integer, 2, 192_000, 32, 24, 0x3);
    let b = f.to_bytes();
    assert_eq!(u16_at(&b, OFF_BITS), 32);
    assert_eq!(u16_at(&b, OFF_VALID_BITS), 24);
    assert_eq!(u16_at(&b, OFF_BLOCK_ALIGN), 8);
    assert_eq!(u32_at(&b, OFF_AVG_BYTES), 192_000 * 8);
}

#[test]
fn f32_uses_the_float_subformat() {
    let f = WaveFormatExtensible::new(SampleKind::Float, 2, 48_000, 32, 32, 0x3);
    let b = f.to_bytes();
    assert_eq!(&b[OFF_SUBFORMAT..OFF_SUBFORMAT + 16], &SUBTYPE_FLOAT);
    assert_eq!(u16_at(&b, OFF_VALID_BITS), 32);
}

#[test]
fn mono_uses_front_center_not_front_left() {
    // The OS convention for mono is SPEAKER_FRONT_CENTER (0x4).
    assert_eq!(channel_mask(&[SpeakerPosition::FrontCenter]), 0x4);
}

#[test]
fn stereo_is_front_left_plus_front_right() {
    assert_eq!(
        channel_mask(&[SpeakerPosition::FrontLeft, SpeakerPosition::FrontRight]),
        0x3
    );
}

#[test]
fn five_one_is_the_documented_six_bits() {
    let mask = channel_mask(&[
        SpeakerPosition::FrontLeft,
        SpeakerPosition::FrontRight,
        SpeakerPosition::FrontCenter,
        SpeakerPosition::LowFrequency,
        SpeakerPosition::BackLeft,
        SpeakerPosition::BackRight,
    ]);
    assert_eq!(mask, 0x3F);
}

#[test]
fn an_empty_position_list_asks_for_no_mask() {
    // dwChannelMask 0 means "positions unspecified", which is legal.
    assert_eq!(channel_mask(&[]), 0);
}

#[test]
fn bytes_round_trip_through_from_bytes() {
    let f = WaveFormatExtensible::new(SampleKind::Integer, 2, 96_000, 32, 24, 0x3);
    let back = WaveFormatExtensible::from_bytes(&f.to_bytes()).expect("decodes");
    assert_eq!(back, f);
}

#[test]
fn from_bytes_rejects_a_non_extensible_tag() {
    let mut b = WaveFormatExtensible::new(SampleKind::Integer, 2, 48_000, 16, 16, 0x3).to_bytes();
    b[OFF_FORMAT_TAG..OFF_FORMAT_TAG + 2].copy_from_slice(&WAVE_FORMAT_PCM.to_le_bytes());
    assert_eq!(WaveFormatExtensible::from_bytes(&b), None);
}

#[test]
fn from_bytes_rejects_an_unknown_subformat_guid() {
    let mut b = WaveFormatExtensible::new(SampleKind::Integer, 2, 48_000, 16, 16, 0x3).to_bytes();
    b[OFF_SUBFORMAT] = 0x42;
    assert_eq!(WaveFormatExtensible::from_bytes(&b), None);
}

#[test]
fn accepts_requires_the_same_kind_container_and_valid_bits() {
    let device = WaveFormatExtensible::new(SampleKind::Integer, 2, 96_000, 32, 24, 0x3);
    assert!(device.accepts(2, 96_000, SampleKind::Integer, 32, 24));
    assert!(
        !device.accepts(2, 96_000, SampleKind::Integer, 32, 32),
        "valid bits differ"
    );
    assert!(
        !device.accepts(2, 96_000, SampleKind::Integer, 24, 24),
        "container differs"
    );
    assert!(
        !device.accepts(2, 96_000, SampleKind::Float, 32, 24),
        "kind differs"
    );
    assert!(
        !device.accepts(2, 48_000, SampleKind::Integer, 32, 24),
        "rate differs"
    );
    assert!(
        !device.accepts(1, 96_000, SampleKind::Integer, 32, 24),
        "channels differ"
    );
}

#[test]
fn accepts_ignores_a_valid_bits_of_zero_for_int_containers() {
    // Some drivers report 0 valid bits meaning "same as container"; that is
    // the full container, not a mismatch.
    let device = WaveFormatExtensible::new(SampleKind::Integer, 2, 48_000, 16, 0, 0x3);
    assert!(device.accepts(2, 48_000, SampleKind::Integer, 16, 16));
}

#[test]
fn sample_format_supported_maps_primitives_without_domain_types() {
    let device = WaveFormatExtensible::new(SampleKind::Integer, 2, 48_000, 24, 24, 0x3);
    assert!(sample_format_supported(
        &device,
        2,
        48_000,
        SampleKind::Integer,
        24,
        24
    ));
    assert!(!sample_format_supported(
        &device,
        2,
        48_000,
        SampleKind::Integer,
        32,
        24
    ));
}
