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
//!
//! # Operator protocol
//!
//! A test cannot pull a cable or suspend the machine, so the two tests that
//! need those conditions stop at a sync point and wait for a human. The
//! runner sets `VYNAMP_TEST_SYNC_DIR` to a writable directory, performs the
//! step the test prints on stderr, and then creates `<dir>/<step>.done`.
//! Without that variable the test fails with the instruction instead of
//! pretending the step happened. Nothing is asserted about a condition the
//! test did not observe.

#![cfg(target_os = "windows")]

use std::path::PathBuf;
use std::time::{Duration, Instant};

use vynamp_output::*;

/// How long a step waits for the operator, and for the event it should cause.
const OPERATOR_TIMEOUT: Duration = Duration::from_secs(300);
const EVENT_TIMEOUT: Duration = Duration::from_secs(5);

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

/// Blocks until the operator has performed `step`.
///
/// The step is induced by hand (a second process takes the device, the cable
/// is pulled, the machine is suspended); the test can only wait for the
/// signal that it happened. `VYNAMP_TEST_SYNC_DIR` names the directory the
/// runner watches, and `<dir>/<step>.done` is the signal.
fn operator_step(step: &str) {
    let dir = std::env::var("VYNAMP_TEST_SYNC_DIR").unwrap_or_else(|_| {
        panic!(
            "set VYNAMP_TEST_SYNC_DIR to a writable directory: this test needs an \
             operator to induce `{step}` and then create <dir>/{step}.done"
        )
    });
    let marker: PathBuf = PathBuf::from(dir).join(format!("{step}.done"));
    eprintln!(
        "OUTPUT-AC-17: induce `{step}` now, then create {}",
        marker.display()
    );
    let deadline = Instant::now() + OPERATOR_TIMEOUT;
    while Instant::now() < deadline {
        if marker.exists() {
            return;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    panic!(
        "operator did not signal `{step}` within {OPERATOR_TIMEOUT:?} \
         (expected {})",
        marker.display()
    );
}

/// Reads events until `DeviceLost` arrives or `timeout` elapses, and returns
/// everything it saw.
///
/// Bounded on purpose: `iter().collect()` would block forever if the event
/// never came, turning a missing event into a hung test.
fn wait_for_device_lost(stream: &Stream, timeout: Duration) -> Vec<Event> {
    let deadline = Instant::now() + timeout;
    let mut seen = Vec::new();
    loop {
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return seen;
        }
        match stream.events().recv_timeout(left) {
            Ok(event) => {
                let lost = event == Event::DeviceLost;
                seen.push(event);
                if lost {
                    return seen;
                }
            }
            Err(_) => return seen,
        }
    }
}

/// The presented position, which must still be readable after the loss.
///
/// The driver publishes a timing snapshot every period and the seqlock keeps
/// the last one, so a stream that played at least one period before the
/// condition was induced still reports it. The operator protocol says to wait
/// for audio before inducing anything, which is what makes this observable.
fn presented_position(stream: &Stream, step: &str) -> u64 {
    stream
        .timing()
        .unwrap_or_else(|| panic!("{step}: no period was presented before the loss"))
        .device_frame
}

/// Asserts the loss the backend reports and the state it leaves behind.
fn assert_lost(stream: &Stream, step: &str) {
    let seen = wait_for_device_lost(stream, EVENT_TIMEOUT);
    assert_eq!(
        seen.last(),
        Some(&Event::DeviceLost),
        "{step}: the loss must arrive as DeviceLost within {EVENT_TIMEOUT:?}, got {seen:?}"
    );
    assert_eq!(
        stream.state(),
        StreamState::Lost,
        "{step}: a lost endpoint leaves the stream in Lost, got {:?}",
        stream.state()
    );
}

/// Opens the DAC, starts it, and returns the stream.
fn open_and_start(host: &dyn Host, step: &str) -> Stream {
    let mut stream = host
        .open(&exclusive_i24(96_000), Box::new(Quiet))
        .unwrap_or_else(|e| panic!("{step}: open 24/96 exclusive: {e}"));
    stream
        .start()
        .unwrap_or_else(|e| panic!("{step}: start: {e}"));
    stream
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
        // `os_mixer_bypassed` is a declaration, not a measurement: the
        // backend sets it from `req.mode` (`wasapi_host.rs`, in `open`), and
        // nothing here reads the OS mixer state. It is asserted because the
        // spec's Then names it, and it is labelled so the run is not read as
        // proof that the mixer was measured.
        assert_eq!(
            config.path.os_mixer_bypassed,
            Tri::Yes,
            "rate {rate}: declared bypass for an exclusive stream"
        );
    }
}

#[test]
#[ignore = "needs a real USB DAC, a second contending process and suspend/resume; see docs/hardware-matrix.md"]
fn busy_unplug_and_sleep_recover_with_pause() {
    // `OUTPUT-AC-17`'s Then says the three conditions "recuperam com pausa".
    // The state table in `stream.rs` says a `Lost` stream refuses `pause` and
    // `resume` (`InvalidRequest { what: "state" }`), and the backend moves to
    // `Lost` on every device-gone code, so the recovery this code actually
    // has is an explicit reopen. Each step below asserts the loss, the state,
    // the presented position, that `resume` is refused, and then recovers by
    // reopening — the observable path, not the one the Then names. The
    // divergence is reported to the maintainer rather than papered over by
    // asserting a `pause` that cannot succeed.
    let host = wasapi_host().expect("hosts() lists the WASAPI host on Windows");

    // 1. Busy: another process takes the device in exclusive mode. WASAPI
    //    preempts the running stream, which surfaces as device-gone.
    let mut stream = open_and_start(host.as_ref(), "busy");
    operator_step("busy");
    assert_lost(&stream, "busy");
    let position = presented_position(&stream, "busy");
    assert!(position > 0, "busy: the position must have advanced");
    assert_eq!(
        stream.resume(),
        Err(Error::InvalidRequest { what: "state" }),
        "busy: a Lost stream refuses resume, which is why recovery reopens"
    );
    stream.stop().expect("busy: stop the lost stream");
    let mut stream = open_and_start(host.as_ref(), "busy/reopen");
    assert_eq!(stream.state(), StreamState::Running, "busy: reopened");

    // 2. Unplug: the cable is pulled with the stream running.
    operator_step("unplug");
    assert_lost(&stream, "unplug");
    let position = presented_position(&stream, "unplug");
    assert!(position > 0, "unplug: the position must have advanced");
    stream.stop().expect("unplug: stop the lost stream");
    operator_step("replug");
    let mut stream = open_and_start(host.as_ref(), "unplug/reopen");
    assert_eq!(stream.state(), StreamState::Running, "unplug: reopened");

    // 3. Sleep: the machine is suspended and resumed. The stream's resources
    //    are invalidated, which the backend also reports as device-gone.
    operator_step("sleep");
    assert_lost(&stream, "sleep");
    let position = presented_position(&stream, "sleep");
    assert!(position > 0, "sleep: the position must have advanced");
    stream.stop().expect("sleep: stop the lost stream");
    let mut stream = open_and_start(host.as_ref(), "sleep/reopen");
    assert_eq!(stream.state(), StreamState::Running, "sleep: reopened");
    stream.stop().expect("sleep: stop");
}

#[test]
#[ignore = "needs a real USB DAC and its cable; see docs/hardware-matrix.md"]
fn unplug_does_not_fall_back_to_the_default_speaker() {
    let host = wasapi_host().expect("hosts() lists the WASAPI host on Windows");
    // The speaker the OS exposes as the `Multimedia` default, which a
    // fallback would have to open. Read before the unplug, while it exists.
    let speaker = host
        .default_device(Role::Multimedia)
        .expect("default_device")
        .map(|d| d.id);
    let mut stream = open_and_start(host.as_ref(), "unplug");

    // The operator pulls the cable with the stream running; the state must
    // become `Lost` and the library must not have opened another device.
    operator_step("unplug");
    assert_lost(&stream, "unplug");

    // No fallback: the list the backend recorded while opening holds exactly
    // the DAC. It is the backend's own record (`Stream::open_attempts`), not
    // `config().device`, which is copied from the request and could not
    // detect a fallback. A fallback would have to append a second id here.
    let attempts = stream.open_attempts();
    assert_eq!(
        attempts,
        &[dac_id()],
        "the host must have opened only the DAC, got {attempts:?}"
    );
    if let Some(speaker) = speaker {
        assert!(
            !attempts.contains(&speaker),
            "the default speaker {} must never be opened, got {attempts:?}",
            speaker.0
        );
    }
    stream.stop().expect("stop the lost stream");
}
