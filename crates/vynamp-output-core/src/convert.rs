//! Explicit `f32` to integer/float sample conversion.
//!
//! Round half to even, then clamp; no dither. Computed in `f64`.
//! Scaling: `I16` x*2^15, `I24*` x*2^23, `I32` x*2^31 (n valid bits: x*2^(n-1)),
//! so +1.0 clamps to the maximum and -1.0 maps exactly to the minimum.
//! NaN and -0.0 become 0.

use crate::error::Error;
use crate::format::{Justify, SampleFormat};
use crate::render::DeviceBuffer;
use crate::request::ConversionSpec;

/// Converts `src` (f32) into `dst`, which must be the variant for `spec.to`
/// and hold exactly `src.len()` samples.
///
/// Integer targets: round-half-even, clamp, no dither. `I24Packed` is
/// little-endian `[lo, mid, hi]`. `I24In32 { Msb }` is the 24-bit value << 8,
/// `Lsb` is the 24-bit value sign-extended. `I32 { valid_bits: n }` is
/// MSB-justified (value clamped to n bits, then << (32 - n)). `U8` is
/// offset-binary (midpoint 128).
///
/// Errors (`InvalidRequest`): `spec.from` not `F32`, DoP/DSD target, `dst`
/// variant not matching `spec.to`, length mismatch, invalid `valid_bits`.
pub(crate) fn convert_f32_to(
    src: &[f32],
    dst: &mut DeviceBuffer<'_>,
    spec: ConversionSpec,
) -> Result<(), Error> {
    if spec.from != SampleFormat::F32 {
        return Err(invalid("source format must be F32"));
    }
    if matches!(spec.to, SampleFormat::Dop { .. }) || matches!(dst, DeviceBuffer::Dsd(_)) {
        return Err(invalid("DoP/DSD target not supported"));
    }
    if let SampleFormat::I32 { valid_bits } = spec.to
        && !(1..=32).contains(&valid_bits)
    {
        return Err(invalid("valid bits out of range"));
    }
    if dst.sample_count() != src.len() {
        return Err(invalid("buffer length differs from source"));
    }
    match (spec.to, dst) {
        (SampleFormat::F32, DeviceBuffer::F32(out)) => {
            for (o, &x) in out.iter_mut().zip(src) {
                *o = x;
            }
        }
        (SampleFormat::F64, DeviceBuffer::F64(out)) => {
            for (o, &x) in out.iter_mut().zip(src) {
                *o = f64::from(x);
            }
        }
        (SampleFormat::I16, DeviceBuffer::I16(out)) => {
            for (o, &x) in out.iter_mut().zip(src) {
                *o = quantize(x, 16) as i16;
            }
        }
        (SampleFormat::I24Packed, DeviceBuffer::I24Packed(out)) => {
            for (o, &x) in out.iter_mut().zip(src) {
                let [lo, mid, hi, _] = quantize(x, 24).to_le_bytes();
                *o = [lo, mid, hi];
            }
        }
        (SampleFormat::I24In32 { justify }, DeviceBuffer::I32(out)) => {
            let shift = match justify {
                Justify::Msb => 8,
                Justify::Lsb => 0,
            };
            for (o, &x) in out.iter_mut().zip(src) {
                *o = quantize(x, 24) << shift;
            }
        }
        (SampleFormat::I32 { valid_bits }, DeviceBuffer::I32(out)) => {
            let bits = u32::from(valid_bits);
            for (o, &x) in out.iter_mut().zip(src) {
                *o = quantize(x, bits) << (32 - bits);
            }
        }
        (SampleFormat::U8, DeviceBuffer::U8(out)) => {
            for (o, &x) in out.iter_mut().zip(src) {
                *o = (quantize(x, 8) + 128) as u8;
            }
        }
        _ => return Err(invalid("buffer variant does not match target format")),
    }
    Ok(())
}

/// Quantises to a signed `bits`-wide integer (1..=32): x*2^(bits-1), round
/// half even, clamp to `[-2^(bits-1), 2^(bits-1) - 1]`. NaN gives 0.
fn quantize(x: f32, bits: u32) -> i32 {
    if x.is_nan() {
        return 0;
    }
    let half = (1u64 << (bits - 1)) as f64;
    let v = (f64::from(x) * half).round_ties_even();
    v.max(-half).min(half - 1.0) as i32
}

fn invalid(what: &'static str) -> Error {
    Error::InvalidRequest { what }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec(to: SampleFormat) -> ConversionSpec {
        ConversionSpec {
            from: SampleFormat::F32,
            to,
        }
    }

    const EXTREMES: [f32; 7] = [1.0, -1.0, 1.000_000_1, -1.000_000_1, f32::NAN, -0.0, 0.5];

    fn bad(what: &'static str) -> Result<(), Error> {
        Err(Error::InvalidRequest { what })
    }

    #[test]
    fn i16_extremes() {
        let mut out = [0i16; 7];
        convert_f32_to(
            &EXTREMES,
            &mut DeviceBuffer::I16(&mut out),
            spec(SampleFormat::I16),
        )
        .unwrap();
        assert_eq!(out, [32767, -32768, 32767, -32768, 0, 0, 16384]);
    }

    #[test]
    fn i16_rounds_half_to_even() {
        let u = 1.0 / 65536.0; // 0.5 LSB
        let src = [u, 3.0 * u, 5.0 * u, -u, -3.0 * u, -5.0 * u];
        let mut out = [9i16; 6];
        convert_f32_to(
            &src,
            &mut DeviceBuffer::I16(&mut out),
            spec(SampleFormat::I16),
        )
        .unwrap();
        assert_eq!(out, [0, 2, 2, 0, -2, -2]);
    }

    #[test]
    fn i24_packed_little_endian() {
        let u = 1.0 / 8_388_608.0;
        let src = [1.0, -1.0, -u, 0x12_3456 as f32 * u, f32::NAN, 1.000_000_1];
        let mut out = [[9u8; 3]; 6];
        convert_f32_to(
            &src,
            &mut DeviceBuffer::I24Packed(&mut out),
            spec(SampleFormat::I24Packed),
        )
        .unwrap();
        assert_eq!(
            out,
            [
                [0xFF, 0xFF, 0x7F],
                [0x00, 0x00, 0x80],
                [0xFF, 0xFF, 0xFF],
                [0x56, 0x34, 0x12],
                [0, 0, 0],
                [0xFF, 0xFF, 0x7F],
            ]
        );
    }

    #[test]
    fn i32_extremes() {
        let mut out = [0i32; 7];
        let to = SampleFormat::I32 { valid_bits: 32 };
        convert_f32_to(&EXTREMES, &mut DeviceBuffer::I32(&mut out), spec(to)).unwrap();
        assert_eq!(out, [i32::MAX, i32::MIN, i32::MAX, i32::MIN, 0, 0, 1 << 30]);
    }

    #[test]
    fn i24_in_32_msb_and_lsb() {
        let u = 1.0 / 8_388_608.0;
        let src = [1.0, -1.0, 0x12_3456 as f32 * u, -u];
        let mut out = [0i32; 4];
        let msb = SampleFormat::I24In32 {
            justify: Justify::Msb,
        };
        convert_f32_to(&src, &mut DeviceBuffer::I32(&mut out), spec(msb)).unwrap();
        assert_eq!(out, [0x7FFF_FF00, i32::MIN, 0x1234_5600, -256]);
        let lsb = SampleFormat::I24In32 {
            justify: Justify::Lsb,
        };
        convert_f32_to(&src, &mut DeviceBuffer::I32(&mut out), spec(lsb)).unwrap();
        assert_eq!(out, [8_388_607, -8_388_608, 0x12_3456, -1]);
    }

    #[test]
    fn i32_with_fewer_valid_bits_is_msb_justified() {
        let src = [1.0, -1.0, 1.000_000_1, 0.0];
        let mut out = [0i32; 4];
        let b24 = SampleFormat::I32 { valid_bits: 24 };
        convert_f32_to(&src, &mut DeviceBuffer::I32(&mut out), spec(b24)).unwrap();
        assert_eq!(out, [0x7FFF_FF00, i32::MIN, 0x7FFF_FF00, 0]);
        let b16 = SampleFormat::I32 { valid_bits: 16 };
        convert_f32_to(&src, &mut DeviceBuffer::I32(&mut out), spec(b16)).unwrap();
        assert_eq!(out, [0x7FFF_0000, i32::MIN, 0x7FFF_0000, 0]);
    }

    #[test]
    fn u8_is_offset_binary() {
        let src = [0.0, 1.0, -1.0, 0.5];
        let mut out = [0u8; 4];
        convert_f32_to(
            &src,
            &mut DeviceBuffer::U8(&mut out),
            spec(SampleFormat::U8),
        )
        .unwrap();
        assert_eq!(out, [128, 255, 0, 192]);
    }

    #[test]
    fn float_targets_are_exact() {
        let src = [0.25f32, -1.0];
        let mut o32 = [0f32; 2];
        convert_f32_to(
            &src,
            &mut DeviceBuffer::F32(&mut o32),
            spec(SampleFormat::F32),
        )
        .unwrap();
        assert_eq!(o32, src);
        let mut o64 = [0f64; 2];
        convert_f32_to(
            &src,
            &mut DeviceBuffer::F64(&mut o64),
            spec(SampleFormat::F64),
        )
        .unwrap();
        assert_eq!(o64, [0.25, -1.0]);
    }

    #[test]
    fn rejects_invalid_requests() {
        let src = [0.0f32; 2];
        let mut out = [0i16; 2];
        // length mismatch
        let mut short = [0i16; 1];
        assert_eq!(
            convert_f32_to(
                &src,
                &mut DeviceBuffer::I16(&mut short),
                spec(SampleFormat::I16)
            ),
            bad("buffer length differs from source")
        );
        // variant mismatch
        assert_eq!(
            convert_f32_to(
                &src,
                &mut DeviceBuffer::I16(&mut out),
                spec(SampleFormat::U8)
            ),
            bad("buffer variant does not match target format")
        );
        // non-F32 source
        let from_i16 = ConversionSpec {
            from: SampleFormat::I16,
            to: SampleFormat::I16,
        };
        assert_eq!(
            convert_f32_to(&src, &mut DeviceBuffer::I16(&mut out), from_i16),
            bad("source format must be F32")
        );
        // DoP / DSD
        let dop = SampleFormat::Dop {
            dsd_rate: 2_822_400,
        };
        let mut bytes = [0u8; 2];
        assert_eq!(
            convert_f32_to(&src, &mut DeviceBuffer::Dsd(&mut bytes), spec(dop)),
            bad("DoP/DSD target not supported")
        );
        assert_eq!(
            convert_f32_to(
                &src,
                &mut DeviceBuffer::Dsd(&mut bytes),
                spec(SampleFormat::U8)
            ),
            bad("DoP/DSD target not supported")
        );
        // invalid valid_bits
        let mut o32 = [0i32; 2];
        for valid_bits in [0, 33] {
            assert_eq!(
                convert_f32_to(
                    &src,
                    &mut DeviceBuffer::I32(&mut o32),
                    spec(SampleFormat::I32 { valid_bits })
                ),
                bad("valid bits out of range")
            );
        }
    }
}
