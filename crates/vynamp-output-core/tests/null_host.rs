//! The Null host: devices, capabilities, period mapping, downgrade event and
//! a short real-clock smoke run.

use std::time::Duration;

use vynamp_output_core::*;
use vynamp_output_testkit::source::{ChannelRamp, Silence, SineF32};

fn request(rate: u32, buffer: BufferRequest) -> StreamRequest {
    StreamRequest {
        device: DeviceSelector::Default(Role::Multimedia),
        mode: ShareMode::Exclusive,
        format: SampleFormat::F32,
        rate,
        channels: 2,
        layout: ChannelLayout::unknown(2),
        buffer,
        on_unavailable: OnUnavailable::Error,
        conversion: Conversion::Forbid,
        allow_rate_change_of_device: false,
    }
}

fn virtual_host() -> (VirtualClockHandle, NullHost) {
    let clock = VirtualClockHandle::new();
    (clock.clone(), NullHost::new(NullClock::Virtual(clock)))
}

fn ramp() -> Box<dyn RenderSource> {
    Box::new(ChannelRamp { start: 0, step: 1 })
}

#[test]
fn one_virtual_device_for_every_role() {
    let (_c, host) = virtual_host();
    assert_eq!(host.id(), HostId::Null);
    let devices = host.enumerate().unwrap();
    assert_eq!(devices.len(), 1);
    let d = &devices[0];
    assert_eq!(d.id, DeviceId("null".into()));
    assert_eq!(d.id_stability, IdStability::Persistent);
    assert_eq!(d.host, HostId::Null);
    assert_eq!(d.transport, Transport::Virtual);
    for role in [Role::Console, Role::Multimedia, Role::Communications] {
        assert_eq!(host.default_device(role).unwrap().as_ref(), Some(d));
        assert!(d.default_for.contains(&role));
    }
}

#[test]
fn capabilities_are_per_mode_and_configurable() {
    let (_c, host) = virtual_host();
    let id = DeviceId("null".into());
    let caps = host.capabilities(&id, ShareMode::Shared).unwrap();
    assert_eq!(caps.entries.len(), 5);
    assert!(caps.entries.iter().all(|e| e.mode == ShareMode::Shared));
    let e = &caps.entries[0];
    assert_eq!((e.rates[0], *e.rates.last().unwrap()), (44_100, 768_000));
    assert_eq!(e.channels, (1..=8).collect::<Vec<u16>>());
    assert_eq!(
        host.capabilities(&DeviceId("other".into()), ShareMode::Shared),
        Err(Error::DeviceNotFound)
    );

    let only_shared_i16 = vec![CapabilityEntry {
        mode: ShareMode::Shared,
        format: SampleFormat::I16,
        rates: vec![48_000],
        channels: vec![2],
        origin: CapabilityOrigin::Probed,
    }];
    let custom = NullHost::with_capabilities(NullClock::Real, only_shared_i16);
    assert!(
        custom
            .capabilities(&id, ShareMode::Exclusive)
            .unwrap()
            .entries
            .is_empty()
    );
    let err = custom.open(&request(48_000, BufferRequest::BackendDefault), ramp());
    assert!(
        matches!(err, Err(Error::FormatUnsupported { .. })),
        "{err:?}"
    );
}

#[test]
fn period_comes_from_the_buffer_request() {
    let (_c, host) = virtual_host();
    let cases = [
        (48_000, BufferRequest::Frames(256), 256),
        (
            48_000,
            BufferRequest::Duration(Duration::from_millis(10)),
            480,
        ),
        (
            96_000,
            BufferRequest::Duration(Duration::from_micros(2500)),
            240,
        ),
        (48_000, BufferRequest::BackendDefault, 480),
        (44_100, BufferRequest::BackendDefault, 441),
    ];
    for (rate, buffer, period) in cases {
        let s = host.open(&request(rate, buffer), ramp()).unwrap();
        let c = s.config();
        assert_eq!(
            (c.period_frames, c.buffer_frames),
            (period, 2 * period),
            "{buffer:?}"
        );
        assert_eq!(c.host, HostId::Null);
        assert_eq!(c.path.os_mixer_bypassed, Tri::Yes);
        assert_eq!(c.path.transport, Transport::Virtual);
    }
    for buffer in [
        BufferRequest::Frames(0),
        BufferRequest::Duration(Duration::from_nanos(1)),
    ] {
        assert_eq!(
            host.open(&request(48_000, buffer), ramp()).err(),
            Some(Error::InvalidRequest {
                what: "buffer is zero"
            })
        );
    }
}

#[test]
fn unknown_device_id_is_not_found() {
    let (_c, host) = virtual_host();
    let mut req = request(48_000, BufferRequest::BackendDefault);
    req.device = DeviceSelector::Id(DeviceId("nope".into()));
    assert_eq!(host.open(&req, ramp()).err(), Some(Error::DeviceNotFound));
    req.device = DeviceSelector::Id(DeviceId("null".into()));
    assert!(host.open(&req, ramp()).is_ok());
}

#[test]
fn downgrade_event_comes_before_the_first_callback() {
    let (clock, host) = virtual_host();
    let mut req = request(50_000, BufferRequest::Frames(64));
    assert!(matches!(
        host.open(&req, ramp()),
        Err(Error::FormatUnsupported { nearest: Some(_) })
    ));

    req.on_unavailable = OnUnavailable::Downgrade;
    let mut s = host.open(&req, ramp()).unwrap();
    assert_eq!(s.config().rate, 88_200, "config() reports the real rate");
    assert_eq!(s.counters().callbacks, 0);
    match s.events().try_recv() {
        Ok(Event::Downgraded {
            requested,
            actual,
            reason,
        }) => {
            assert_eq!(*requested, req);
            assert_eq!(*actual, *s.config());
            assert_eq!(reason, "rate not supported");
        }
        other => panic!("expected Downgraded first, got {other:?}"),
    }
    s.start().unwrap();
    clock.advance(2);
    assert_eq!(s.counters().callbacks, 2);
    assert!(s.events().try_recv().is_err(), "nothing else happened");
}

#[test]
fn dop_opens_on_an_advertised_dop_entry() {
    let dop = SampleFormat::Dop {
        dsd_rate: 2_822_400,
    };
    let host = NullHost::with_capabilities(
        NullClock::Real,
        vec![CapabilityEntry {
            mode: ShareMode::Exclusive,
            format: dop,
            rates: vec![176_400],
            channels: vec![2],
            origin: CapabilityOrigin::Advertised,
        }],
    );
    let mut req = request(176_400, BufferRequest::BackendDefault);
    req.format = dop;
    let mut s = host.open(&req, Box::new(Silence)).expect("open DoP");
    assert_eq!((s.config().format, s.config().rate), (dop, 176_400));
    s.stop().unwrap();
}

#[test]
fn real_clock_smoke() {
    let host = NullHost::new(NullClock::Real);
    // 10 ms periods; about 60 ms of running
    let mut s = host
        .open(
            &request(48_000, BufferRequest::Duration(Duration::from_millis(10))),
            Box::new(SineF32 {
                freq: 440.0,
                rate: 48_000,
            }),
        )
        .unwrap();
    s.start().unwrap();
    std::thread::sleep(Duration::from_millis(60));
    let t = s.timing().expect("timing on the real clock");
    s.stop().unwrap();
    let c = s.counters();
    assert!(
        (2..=12).contains(&c.callbacks),
        "about 6 periods expected, got {c:?}"
    );
    assert_eq!(c.frames_delivered, c.callbacks * 480);
    assert_eq!(t.device_frame % 480, 0);
    assert_eq!(c.source_underruns, 0);
}
