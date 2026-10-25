//! Zero-allocation checks for the RT helpers, using the testkit counter.

use crate::convert::convert_f32_to;
use crate::format::{Justify, SampleFormat};
use crate::render::DeviceBuffer;
use crate::request::ConversionSpec;
use crate::silence::fill_silence;
use vynamp_output_testkit::alloc::{CountingAlloc, allocations_on_this_thread};

#[global_allocator]
static ALLOC: CountingAlloc = CountingAlloc;

#[test]
fn fill_and_convert_do_not_allocate() {
    let src = vec![0.3f32; 64];
    let mut i16s = vec![0i16; 64];
    let mut i24 = vec![[0u8; 3]; 64];
    let mut i32s = vec![0i32; 64];
    let mut u8s = vec![0u8; 64];
    let formats = [
        SampleFormat::I24In32 {
            justify: Justify::Msb,
        },
        SampleFormat::I32 { valid_bits: 24 },
    ];

    let before = allocations_on_this_thread();
    let spec = |to| ConversionSpec {
        from: SampleFormat::F32,
        to,
    };
    convert_f32_to(
        &src,
        &mut DeviceBuffer::I16(&mut i16s),
        spec(SampleFormat::I16),
    )
    .unwrap();
    convert_f32_to(
        &src,
        &mut DeviceBuffer::I24Packed(&mut i24),
        spec(SampleFormat::I24Packed),
    )
    .unwrap();
    for f in formats {
        convert_f32_to(&src, &mut DeviceBuffer::I32(&mut i32s), spec(f)).unwrap();
    }
    convert_f32_to(
        &src,
        &mut DeviceBuffer::U8(&mut u8s),
        spec(SampleFormat::U8),
    )
    .unwrap();
    let mut phase = false;
    fill_silence(
        &mut DeviceBuffer::I16(&mut i16s),
        10,
        2,
        SampleFormat::I16,
        &mut phase,
    );
    fill_silence(
        &mut DeviceBuffer::I24Packed(&mut i24),
        10,
        2,
        SampleFormat::I24Packed,
        &mut phase,
    );
    fill_silence(
        &mut DeviceBuffer::I32(&mut i32s),
        10,
        2,
        SampleFormat::I16,
        &mut phase,
    );
    fill_silence(
        &mut DeviceBuffer::U8(&mut u8s),
        10,
        2,
        SampleFormat::U8,
        &mut phase,
    );
    let after = allocations_on_this_thread();

    assert_eq!(after - before, 0, "RT helpers must not allocate");
    // an error path may allocate nothing either: static strings only
    let before = allocations_on_this_thread();
    let _ = convert_f32_to(
        &src,
        &mut DeviceBuffer::U8(&mut u8s),
        spec(SampleFormat::I16),
    );
    assert_eq!(allocations_on_this_thread() - before, 0);
}
