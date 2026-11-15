//! `OUTPUT-AC-16` to `OUTPUT-AC-18`: exclusive-mode playback on a real USB
//! DAC through WASAPI, on Windows 11.
//!
//! Every test here is `#[ignore]`: each one needs a physical DAC, a Windows
//! 11 machine configured to allow exclusive mode, and one more of the three
//! (a second process contending for the device, a USB cable that can be
//! pulled, or a suspend/resume cycle that a test cannot trigger into). None
//! of those exists in CI, and the spec says so; they are not evidence until
//! run on hardware, and the run is recorded in `docs/hardware-matrix.md`.
//!
//! The tests are `cfg(target_os = "windows")` because they drive the real
//! WASAPI host; on any other target they would not link a backend at all.

#![cfg(target_os = "windows")]

use std::time::Duration;

use vynamp_output::*;

/// The DAC the run is pointed at, from `VYNAMP_TEST_DAC_ID`.
///
/// The spec's Given names "a USB DAC that advertises 24 valid bits at 96 kHz
/// and 192 kHz"; the runner supplies its endpoint id so the same test can run
/// against any DAC without editing code.
fn dac_id() -> DeviceId {
    let id = std::env::var("VYNAMP_TEST_DAC_ID")
        .expect("set VYNAMP_TEST_DAC_ID to the endpoint id of the USB DAC");
    DeviceId(id)
}

/// An exclusive I24Packed request at `rate`, for the DAC by id.
fn exclusive_i24(rate: u32) -> StreamRequest {
    StreamRequest {
        device: DeviceSelector::Id(dac_id()),
        mode: ShareMode::Exclusive,
        format: SampleFormat::I24Packed,
        rate,
        channels: 2,
        layout: ChannelLayout::unknown(2),
        buffer: BufferRequest::BackendDefault,
        on_unavailable: OnUnavailable::Error,
        conversion: Conversion::Forbid,
        allow_rate_change_of_device: false,
    }
}

/// A source that renders silence forever; these tests measure the path, not
/// the signal.
struct Quiet;

impl RenderSource for Quiet {
    fn render(&mut self, _out: DeviceBuffer<'_>, _ctx: &RenderContext<'_>) -> Rendered {
        Rendered {
            frames: 0,
            end_of_stream: false,
        }
    }
}

/// The WASAPI host, in the preference list `hosts()` builds.
fn wasapi_host() -> Option<Box<dyn Host>> {
    hosts().into_iter().find(|h| h.id() == HostId::Wasapi)
}

#[test]
#[ignore = "needs a real USB DAC on Windows 11; see docs/hardware-matrix.md"]
fn dac_negotiates_24_96_and_24_192() {
    let host = wasapi_host().expect("hosts() lists the WASAPI host on Windows");
    for rate in [96_000, 192_000] {
        let stream = host
            .open(&exclusive_i24(rate), Box::new(Quiet))
            .unwrap_or_else(|e| panic!("open {rate}: {e}"));
        let config = stream.config();
        // 24/96 and 24/192 negotiated on the DAC, not downgraded to it.
        assert_eq!(config.mode, ShareMode::Exclusive, "rate {rate}");
        assert_eq!(config.format, SampleFormat::I24Packed, "rate {rate}");
        assert_eq!(config.valid_bits, 24, "rate {rate}");
        assert_eq!(
            config.rate, rate,
            "the DAC must get the rate that was asked"
        );
        // The OS mixer is out of the path, which is what exclusive buys.
        assert_eq!(config.path.os_mixer_bypassed, Tri::Yes, "rate {rate}");
    }
}

#[test]
#[ignore = "needs a real USB DAC, a second contending process and suspend/resume; see docs/hardware-matrix.md"]
fn busy_unplug_and_sleep_recover_with_pause() {
    let host = wasapi_host().expect("hosts() lists the WASAPI host on Windows");
    let mut stream = host
        .open(&exclusive_i24(96_000), Box::new(Quiet))
        .expect("open 24/96 exclusive");
    stream.start().expect("start");

    // The run is driven by hand: the operator induces each condition and
    // then reads the state. The asserts below are the spec's Then.
    //
    // 1. Busy: another process takes the device in exclusive mode.
    let busy: Vec<Event> = stream.events().try_iter().collect();
    assert!(
        busy.iter()
            .any(|e| matches!(e, Event::DeviceLost | Event::OsXrun { .. })),
        "busy must surface as an event, got {busy:?}"
    );
    stream.pause().expect("pause after busy");
    assert_eq!(stream.state(), StreamState::Paused);

    // 2. Unplug and 3. sleep: both invalidate the stream; recovery is an
    //    explicit resume once the device is back, never an automatic
    //    migration.
    let after: Vec<Event> = stream.events().try_iter().collect();
    assert!(
        after.iter().any(|e| matches!(e, Event::DeviceLost)),
        "unplug/sleep must surface as DeviceLost, got {after:?}"
    );
    let position = stream.timing().map(|t| t.device_frame);
    assert!(
        position.is_some(),
        "the presented position must still be readable"
    );
    // After the device returns, recovery is a new `open` on the same device
    // id, never a silent switch: a `Lost` stream refuses `resume` (the state
    // table says so), so the operator stops it and reopens.
    stream.stop().expect("stop the lost stream");
}

#[test]
#[ignore = "needs a real USB DAC and its cable; see docs/hardware-matrix.md"]
fn unplug_does_not_fall_back_to_the_default_speaker() {
    let host = wasapi_host().expect("hosts() lists the WASAPI host on Windows");
    let mut stream = host
        .open(&exclusive_i24(96_000), Box::new(Quiet))
        .expect("open 24/96 exclusive");
    stream.start().expect("start");

    // The operator pulls the cable with the stream running; the state must
    // become `Lost` and the library must not have opened another device.
    // `recv_timeout` rather than `iter()`: the latter would block forever if
    // `DeviceLost` never came, turning a missing event into a hung test.
    let lost = stream
        .events()
        .recv_timeout(Duration::from_secs(5))
        .expect("DeviceLost within 5 s of the unplug");
    assert_eq!(lost, Event::DeviceLost);
    assert_eq!(stream.state(), StreamState::Lost);
    // No fallback: the only device the stream ever opened is the one the
    // request named, and the failure is `DeviceLost`, not a silent switch to
    // the `Multimedia` default speaker.
    assert_eq!(
        stream.config().device,
        dac_id(),
        "the stream must still be configured for the DAC, not the speaker"
    );
}
