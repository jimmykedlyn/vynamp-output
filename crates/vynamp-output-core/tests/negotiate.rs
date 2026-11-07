use vynamp_output_core::*;

fn layout(n: u16) -> ChannelLayout {
    ChannelLayout::unknown(n)
}

fn request(format: SampleFormat, rate: u32, channels: u16) -> StreamRequest {
    StreamRequest {
        device: DeviceSelector::Default(Role::Multimedia),
        mode: ShareMode::Exclusive,
        format,
        rate,
        channels,
        layout: layout(channels),
        buffer: BufferRequest::BackendDefault,
        on_unavailable: OnUnavailable::Error,
        conversion: Conversion::Forbid,
        allow_rate_change_of_device: false,
    }
}

fn entry(
    mode: ShareMode,
    format: SampleFormat,
    rates: &[u32],
    channels: &[u16],
) -> CapabilityEntry {
    CapabilityEntry {
        mode,
        format,
        rates: rates.to_vec(),
        channels: channels.to_vec(),
        origin: CapabilityOrigin::Advertised,
    }
}

fn caps(entries: Vec<CapabilityEntry>) -> Capabilities {
    Capabilities {
        host: HostId::Null,
        device: DeviceId("dev0".to_string()),
        entries,
    }
}

fn nearest_of(err: Error) -> StreamConfig {
    match err {
        Error::FormatUnsupported { nearest: Some(n) } => *n,
        other => panic!("expected FormatUnsupported with nearest, got {other:?}"),
    }
}

#[test]
fn exact_match_builds_template() {
    let c = caps(vec![entry(
        ShareMode::Exclusive,
        SampleFormat::I24Packed,
        &[44_100, 48_000],
        &[2],
    )]);
    let r = request(SampleFormat::I24Packed, 48_000, 2);
    let n = negotiate(&r, &c).unwrap();
    assert_eq!(n.downgraded, None);
    let t = n.config_template;
    assert_eq!(t.host, HostId::Null);
    assert_eq!(t.device, DeviceId("dev0".into()));
    assert_eq!(t.mode, ShareMode::Exclusive);
    assert_eq!(t.format, SampleFormat::I24Packed);
    assert_eq!(t.valid_bits, 24);
    assert_eq!(t.rate, 48_000);
    assert_eq!(t.channels, 2);
    assert_eq!(t.layout, r.layout);
    assert_eq!((t.period_frames, t.buffer_frames), (0, 0));
    assert_eq!(t.path.os_mixer_bypassed, Tri::Unknown);
    assert!(!t.path.os_resampling);
    assert_eq!(t.path.library_conversion, None);
    assert_eq!(t.path.endpoint_volume, EndpointVolume::Unknown);
    assert_eq!(t.path.transport, Transport::Unknown);
    assert_eq!(t.path.rt_priority, RtPriority::Denied("not started"));
}

#[test]
fn valid_bits_per_format() {
    assert_eq!(SampleFormat::F32.valid_bits(), 32);
    assert_eq!(SampleFormat::F64.valid_bits(), 64);
    assert_eq!(SampleFormat::I16.valid_bits(), 16);
    assert_eq!(SampleFormat::U8.valid_bits(), 8);
    assert_eq!(SampleFormat::I24Packed.valid_bits(), 24);
    assert_eq!(
        SampleFormat::I24In32 {
            justify: Justify::Lsb
        }
        .valid_bits(),
        24
    );
    assert_eq!(SampleFormat::I32 { valid_bits: 20 }.valid_bits(), 20);
    assert_eq!(
        SampleFormat::Dop {
            dsd_rate: 2_822_400
        }
        .valid_bits(),
        24
    );
}

#[test]
fn container_bytes_per_format() {
    assert_eq!(SampleFormat::U8.container_bytes(), 1);
    assert_eq!(SampleFormat::I16.container_bytes(), 2);
    assert_eq!(SampleFormat::I24Packed.container_bytes(), 3);
    assert_eq!(SampleFormat::F32.container_bytes(), 4);
    assert_eq!(SampleFormat::I32 { valid_bits: 32 }.container_bytes(), 4);
    assert_eq!(
        SampleFormat::Dop {
            dsd_rate: 2_822_400
        }
        .container_bytes(),
        4
    );
    assert_eq!(SampleFormat::F64.container_bytes(), 8);
}

#[test]
fn nearest_prefers_rate_at_or_above() {
    let c = caps(vec![entry(
        ShareMode::Exclusive,
        SampleFormat::I16,
        &[44_100, 96_000, 192_000],
        &[2],
    )]);
    let r = request(SampleFormat::I16, 48_000, 2);
    let n = nearest_of(negotiate(&r, &c).unwrap_err());
    assert_eq!(n.rate, 96_000);
}

#[test]
fn nearest_falls_back_to_rate_below() {
    let c = caps(vec![entry(
        ShareMode::Exclusive,
        SampleFormat::I16,
        &[44_100, 48_000],
        &[2],
    )]);
    let r = request(SampleFormat::I16, 96_000, 2);
    let n = nearest_of(negotiate(&r, &c).unwrap_err());
    assert_eq!(n.rate, 48_000);
}

#[test]
fn nearest_rate_dominates_container_width() {
    let c = caps(vec![
        entry(ShareMode::Exclusive, SampleFormat::I16, &[48_000], &[2]),
        entry(ShareMode::Exclusive, SampleFormat::F32, &[96_000], &[2]),
    ]);
    // Wants F32 at 44.1k: closest rate >= is 48k (I16), even though F32 matches width.
    let r = request(SampleFormat::F32, 44_100, 2);
    let n = nearest_of(negotiate(&r, &c).unwrap_err());
    assert_eq!((n.rate, n.format), (48_000, SampleFormat::I16));
}

#[test]
fn nearest_same_rate_prefers_same_container_width() {
    let msb = SampleFormat::I24In32 {
        justify: Justify::Msb,
    };
    let c = caps(vec![
        entry(ShareMode::Exclusive, SampleFormat::I16, &[96_000], &[2]),
        entry(ShareMode::Exclusive, msb, &[96_000], &[2]),
    ]);
    // 44.1k unavailable -> 96k; request F32 (4 bytes) -> I24In32 (4 bytes) wins over I16.
    let r = request(SampleFormat::F32, 44_100, 2);
    let n = nearest_of(negotiate(&r, &c).unwrap_err());
    assert_eq!(n.format, msb);
}

#[test]
fn nearest_then_prefers_channels() {
    let c = caps(vec![entry(
        ShareMode::Exclusive,
        SampleFormat::I16,
        &[48_000],
        &[1, 6, 8],
    )]);
    let r = request(SampleFormat::I16, 48_000, 2);
    let n = nearest_of(negotiate(&r, &c).unwrap_err());
    assert_eq!(n.channels, 6);
    assert_eq!(n.layout.positions.len(), 6);
    let r10 = request(SampleFormat::I16, 48_000, 10);
    let n10 = nearest_of(negotiate(&r10, &c).unwrap_err());
    assert_eq!(n10.channels, 8);
}

#[test]
fn nearest_only_considers_same_mode() {
    let c = caps(vec![entry(
        ShareMode::Shared,
        SampleFormat::F32,
        &[48_000],
        &[2],
    )]);
    let r = request(SampleFormat::I16, 48_000, 2);
    assert_eq!(
        negotiate(&r, &c).unwrap_err(),
        Error::FormatUnsupported { nearest: None }
    );
}

#[test]
fn downgrade_returns_nearest_with_reason() {
    let c = caps(vec![entry(
        ShareMode::Exclusive,
        SampleFormat::I16,
        &[44_100],
        &[2],
    )]);
    let mut r = request(SampleFormat::I16, 48_000, 2);
    r.on_unavailable = OnUnavailable::Downgrade;
    let n = negotiate(&r, &c).unwrap();
    assert_eq!(n.downgraded, Some("rate not supported"));
    assert_eq!(n.config_template.rate, 44_100);

    let mut r = request(SampleFormat::F32, 44_100, 2);
    r.on_unavailable = OnUnavailable::Downgrade;
    let n = negotiate(&r, &c).unwrap();
    assert_eq!(n.downgraded, Some("format not supported"));
    assert_eq!(n.config_template.format, SampleFormat::I16);

    let mut r = request(SampleFormat::I16, 44_100, 4);
    r.on_unavailable = OnUnavailable::Downgrade;
    let n = negotiate(&r, &c).unwrap();
    assert_eq!(n.downgraded, Some("channel count not supported"));
    assert_eq!(n.config_template.channels, 2);
}

#[test]
fn downgrade_without_candidates_is_an_error() {
    let c = caps(vec![]);
    let mut r = request(SampleFormat::I16, 44_100, 2);
    r.on_unavailable = OnUnavailable::Downgrade;
    assert_eq!(
        negotiate(&r, &c).unwrap_err(),
        Error::FormatUnsupported { nearest: None }
    );
}

#[test]
fn conversion_allowed_opens_target_format() {
    let c = caps(vec![entry(
        ShareMode::Exclusive,
        SampleFormat::I24Packed,
        &[48_000],
        &[2],
    )]);
    let spec = ConversionSpec {
        from: SampleFormat::F32,
        to: SampleFormat::I24Packed,
    };
    let mut r = request(SampleFormat::F32, 48_000, 2);
    r.conversion = Conversion::Allow(spec);
    let n = negotiate(&r, &c).unwrap();
    assert_eq!(n.downgraded, None);
    assert_eq!(n.config_template.format, SampleFormat::I24Packed);
    assert_eq!(n.config_template.valid_bits, 24);
    assert_eq!(n.config_template.path.library_conversion, Some(spec));
}

#[test]
fn conversion_forbidden_is_unsupported() {
    let c = caps(vec![entry(
        ShareMode::Exclusive,
        SampleFormat::I24Packed,
        &[48_000],
        &[2],
    )]);
    let r = request(SampleFormat::F32, 48_000, 2);
    let n = nearest_of(negotiate(&r, &c).unwrap_err());
    assert_eq!(n.format, SampleFormat::I24Packed);
}

#[test]
fn conversion_allowed_but_target_missing_is_unsupported() {
    let c = caps(vec![entry(
        ShareMode::Exclusive,
        SampleFormat::I16,
        &[48_000],
        &[2],
    )]);
    let mut r = request(SampleFormat::F32, 48_000, 2);
    r.conversion = Conversion::Allow(ConversionSpec {
        from: SampleFormat::F32,
        to: SampleFormat::I24Packed,
    });
    assert!(matches!(
        negotiate(&r, &c).unwrap_err(),
        Error::FormatUnsupported { nearest: Some(_) }
    ));
}

#[test]
fn exact_match_wins_over_conversion() {
    let c = caps(vec![
        entry(ShareMode::Exclusive, SampleFormat::F32, &[48_000], &[2]),
        entry(ShareMode::Exclusive, SampleFormat::I16, &[48_000], &[2]),
    ]);
    let mut r = request(SampleFormat::F32, 48_000, 2);
    r.conversion = Conversion::Allow(ConversionSpec {
        from: SampleFormat::F32,
        to: SampleFormat::I16,
    });
    let n = negotiate(&r, &c).unwrap();
    assert_eq!(n.config_template.format, SampleFormat::F32);
    assert_eq!(n.config_template.path.library_conversion, None);
}

fn invalid(r: &StreamRequest) -> &'static str {
    match negotiate(r, &caps(vec![])).unwrap_err() {
        Error::InvalidRequest { what } => what,
        other => panic!("expected InvalidRequest, got {other:?}"),
    }
}

#[test]
fn invalid_requests() {
    assert_eq!(invalid(&request(SampleFormat::I16, 0, 2)), "rate is zero");
    assert_eq!(
        invalid(&request(SampleFormat::I16, 48_000, 0)),
        "channels is zero"
    );
    let mut r = request(SampleFormat::I16, 48_000, 2);
    r.layout = layout(3);
    assert_eq!(invalid(&r), "layout length differs from channels");

    let mut r = request(SampleFormat::I32 { valid_bits: 0 }, 48_000, 2);
    assert_eq!(invalid(&r), "valid bits out of range");
    r.format = SampleFormat::I32 { valid_bits: 33 };
    assert_eq!(invalid(&r), "valid bits out of range");

    let r = request(SampleFormat::Dop { dsd_rate: 0 }, 176_400, 2);
    assert_eq!(invalid(&r), "dsd rate is zero");

    let dop = SampleFormat::Dop {
        dsd_rate: 2_822_400,
    };
    let mut r = request(dop, 176_400, 2);
    r.conversion = Conversion::Allow(ConversionSpec {
        from: dop,
        to: SampleFormat::I16,
    });
    assert_eq!(invalid(&r), "dop requires a 24-bit capable container");

    let mut r = request(SampleFormat::F32, 48_000, 2);
    r.conversion = Conversion::Allow(ConversionSpec {
        from: SampleFormat::I16,
        to: SampleFormat::F32,
    });
    assert_eq!(
        invalid(&r),
        "conversion source differs from requested format"
    );
}

#[test]
fn dop_exact_match_works() {
    let dop = SampleFormat::Dop {
        dsd_rate: 2_822_400,
    };
    let c = caps(vec![entry(ShareMode::Exclusive, dop, &[176_400], &[2])]);
    let n = negotiate(&request(dop, 176_400, 2), &c).unwrap();
    assert_eq!(n.config_template.format, dop);
    assert_eq!(n.config_template.valid_bits, 24);
}

#[test]
fn error_display() {
    assert_eq!(Error::NoBackend.to_string(), "no audio backend available");
    assert_eq!(
        Error::LibraryUnavailable { lib: "libasound" }.to_string(),
        "library unavailable: libasound"
    );
    assert_eq!(Error::DeviceNotFound.to_string(), "device not found");
    assert_eq!(Error::DeviceBusy.to_string(), "device busy");
    assert_eq!(
        Error::ExclusiveUnavailable { reason: "in use" }.to_string(),
        "exclusive mode unavailable: in use"
    );
    assert_eq!(
        Error::FormatUnsupported { nearest: None }.to_string(),
        "format unsupported"
    );
    assert_eq!(Error::PermissionDenied.to_string(), "permission denied");
    assert_eq!(Error::DeviceLost.to_string(), "device lost");
    assert_eq!(Error::Timeout.to_string(), "timeout");
    assert_eq!(
        Error::InvalidRequest {
            what: "rate is zero"
        }
        .to_string(),
        "invalid request: rate is zero"
    );
    assert_eq!(
        Error::Os {
            backend: HostId::Wasapi,
            code: -2004287480,
            call: "IAudioClient::Initialize"
        }
        .to_string(),
        "os error -2004287480 in IAudioClient::Initialize (wasapi)"
    );
    let e: &dyn std::error::Error = &Error::Timeout;
    assert_eq!(e.to_string(), "timeout");
}

#[test]
fn format_unsupported_with_nearest_display() {
    let c = caps(vec![entry(
        ShareMode::Exclusive,
        SampleFormat::I16,
        &[44_100],
        &[2],
    )]);
    let e = negotiate(&request(SampleFormat::I16, 48_000, 2), &c).unwrap_err();
    assert_eq!(
        e.to_string(),
        "format unsupported (nearest: 44100 Hz, 2 ch)"
    );
}

#[test]
fn event_display() {
    assert_eq!(
        Event::DeviceAdded(DeviceId("a".into())).to_string(),
        "device added: a"
    );
    assert_eq!(
        Event::DeviceRemoved(DeviceId("a".into())).to_string(),
        "device removed: a"
    );
    assert_eq!(
        Event::DefaultChanged {
            role: Role::Console
        }
        .to_string(),
        "default device changed (console)"
    );
    assert_eq!(Event::DeviceLost.to_string(), "device lost");
    assert_eq!(
        Event::SharedFormatChanged.to_string(),
        "shared format changed"
    );
    assert_eq!(Event::OsXrun { count: 3 }.to_string(), "os xrun (count 3)");
    assert_eq!(
        Event::SourceUnderrun { count: 7 }.to_string(),
        "source underrun (count 7)"
    );
    assert_eq!(Event::SourcePanicked.to_string(), "source panicked");
    assert_eq!(
        Event::DopVolumeUnverified.to_string(),
        "dop opened with unverified endpoint volume"
    );
    assert_eq!(
        Event::RtPriorityDenied { reason: "no perm" }.to_string(),
        "realtime priority denied: no perm"
    );
    let c = caps(vec![entry(
        ShareMode::Exclusive,
        SampleFormat::I16,
        &[44_100],
        &[2],
    )]);
    let mut r = request(SampleFormat::I16, 48_000, 2);
    r.on_unavailable = OnUnavailable::Downgrade;
    let actual = negotiate(&r, &c).unwrap().config_template;
    let ev = Event::Downgraded {
        requested: Box::new(r),
        actual: Box::new(actual),
        reason: "rate not supported",
    };
    assert_eq!(
        ev.to_string(),
        "downgraded: rate not supported (requested 48000 Hz, 2 ch; actual 44100 Hz, 2 ch)"
    );
}

const DOP: SampleFormat = SampleFormat::Dop {
    dsd_rate: 2_822_400,
};

#[test]
fn dop_request_never_gets_pcm_nearest() {
    let c = caps(vec![entry(
        ShareMode::Exclusive,
        SampleFormat::I24Packed,
        &[176_400],
        &[2],
    )]);
    let r = request(DOP, 176_400, 2);
    assert_eq!(
        negotiate(&r, &c).unwrap_err(),
        Error::FormatUnsupported { nearest: None }
    );
    let mut r = r;
    r.on_unavailable = OnUnavailable::Downgrade;
    assert_eq!(
        negotiate(&r, &c).unwrap_err(),
        Error::FormatUnsupported { nearest: None }
    );
}

#[test]
fn pcm_request_never_gets_dop_nearest() {
    let c = caps(vec![entry(ShareMode::Exclusive, DOP, &[176_400], &[2])]);
    let mut r = request(SampleFormat::I24Packed, 176_400, 2);
    assert_eq!(
        negotiate(&r, &c).unwrap_err(),
        Error::FormatUnsupported { nearest: None }
    );
    r.on_unavailable = OnUnavailable::Downgrade;
    assert_eq!(
        negotiate(&r, &c).unwrap_err(),
        Error::FormatUnsupported { nearest: None }
    );
}

#[test]
fn dop_request_may_get_dop_with_other_dsd_rate() {
    let dsd128 = SampleFormat::Dop {
        dsd_rate: 5_644_800,
    };
    let c = caps(vec![
        entry(
            ShareMode::Exclusive,
            SampleFormat::I24Packed,
            &[176_400],
            &[2],
        ),
        entry(ShareMode::Exclusive, dsd128, &[352_800], &[2]),
    ]);
    let n = nearest_of(negotiate(&request(DOP, 176_400, 2), &c).unwrap_err());
    assert_eq!((n.format, n.rate), (dsd128, 352_800));
}

#[test]
fn nearest_skips_entries_with_empty_channels_or_rates() {
    let c = caps(vec![
        entry(ShareMode::Exclusive, SampleFormat::I16, &[48_000], &[]),
        entry(ShareMode::Exclusive, SampleFormat::I16, &[], &[2]),
        entry(ShareMode::Exclusive, SampleFormat::F32, &[96_000], &[2]),
    ]);
    let n = nearest_of(negotiate(&request(SampleFormat::I16, 44_100, 2), &c).unwrap_err());
    assert_eq!((n.rate, n.format), (96_000, SampleFormat::F32));
}

#[test]
fn nearest_none_when_only_empty_entries_or_no_entries() {
    let c = caps(vec![entry(
        ShareMode::Exclusive,
        SampleFormat::I16,
        &[48_000],
        &[],
    )]);
    let r = request(SampleFormat::I16, 44_100, 2);
    assert_eq!(
        negotiate(&r, &c).unwrap_err(),
        Error::FormatUnsupported { nearest: None }
    );
    let c = caps(vec![entry(
        ShareMode::Exclusive,
        SampleFormat::I16,
        &[],
        &[2],
    )]);
    assert_eq!(
        negotiate(&r, &c).unwrap_err(),
        Error::FormatUnsupported { nearest: None }
    );
    assert_eq!(
        negotiate(&r, &caps(vec![])).unwrap_err(),
        Error::FormatUnsupported { nearest: None }
    );
}

#[test]
fn width_tie_prefers_wider_container() {
    // Request I24Packed (3 bytes): I16 (2) and F32 (4) are equally far; wider wins.
    let c = caps(vec![
        entry(ShareMode::Exclusive, SampleFormat::I16, &[96_000], &[2]),
        entry(ShareMode::Exclusive, SampleFormat::F32, &[96_000], &[2]),
    ]);
    let n = nearest_of(negotiate(&request(SampleFormat::I24Packed, 44_100, 2), &c).unwrap_err());
    assert_eq!(n.format, SampleFormat::F32);
}

#[test]
fn same_width_without_same_format_takes_first_listed() {
    let msb = SampleFormat::I24In32 {
        justify: Justify::Msb,
    };
    let c = caps(vec![
        entry(ShareMode::Exclusive, msb, &[96_000], &[2]),
        entry(ShareMode::Exclusive, SampleFormat::F32, &[96_000], &[2]),
    ]);
    let n = nearest_of(
        negotiate(
            &request(SampleFormat::I32 { valid_bits: 32 }, 44_100, 2),
            &c,
        )
        .unwrap_err(),
    );
    assert_eq!(n.format, msb);
}

#[test]
fn conversion_does_not_cross_modes() {
    let c = caps(vec![entry(
        ShareMode::Shared,
        SampleFormat::I24Packed,
        &[48_000],
        &[2],
    )]);
    let mut r = request(SampleFormat::F32, 48_000, 2);
    r.conversion = Conversion::Allow(ConversionSpec {
        from: SampleFormat::F32,
        to: SampleFormat::I24Packed,
    });
    assert_eq!(
        negotiate(&r, &c).unwrap_err(),
        Error::FormatUnsupported { nearest: None }
    );
}

#[test]
fn invalid_conversion_targets() {
    let conv = |from, to| {
        let mut r = request(from, 176_400, 2);
        r.conversion = Conversion::Allow(ConversionSpec { from, to });
        r
    };
    assert_eq!(
        invalid(&conv(
            SampleFormat::F32,
            SampleFormat::I32 { valid_bits: 0 }
        )),
        "valid bits out of range"
    );
    assert_eq!(
        invalid(&conv(
            SampleFormat::F32,
            SampleFormat::I32 { valid_bits: 40 }
        )),
        "valid bits out of range"
    );
    assert_eq!(
        invalid(&conv(DOP, SampleFormat::F32)),
        "dop requires a 24-bit capable container"
    );
    assert_eq!(
        invalid(&conv(DOP, SampleFormat::F64)),
        "dop requires a 24-bit capable container"
    );
    assert_eq!(
        invalid(&conv(SampleFormat::F32, DOP)),
        "conversion cannot turn pcm into dop"
    );
    assert_eq!(
        invalid(&conv(SampleFormat::I16, DOP)),
        "conversion cannot turn pcm into dop"
    );
    assert_eq!(
        invalid(&conv(
            DOP,
            SampleFormat::Dop {
                dsd_rate: 5_644_800
            }
        )),
        "dop conversion changes dsd rate"
    );
}

#[test]
fn pcm_to_dop_always_rejected_and_dop_to_pcm_never_converted() {
    let mut r = request(SampleFormat::I24Packed, 176_400, 2);
    r.conversion = Conversion::Allow(ConversionSpec {
        from: SampleFormat::I24Packed,
        to: DOP,
    });
    assert_eq!(invalid(&r), "conversion cannot turn pcm into dop");

    for to in [
        SampleFormat::I24Packed,
        SampleFormat::I24In32 {
            justify: Justify::Msb,
        },
        SampleFormat::I32 { valid_bits: 24 },
    ] {
        let c = caps(vec![entry(ShareMode::Exclusive, to, &[176_400], &[2])]);
        let mut r = request(DOP, 176_400, 2);
        let spec = ConversionSpec { from: DOP, to };
        r.conversion = Conversion::Allow(spec);
        // DoP must reach the device unaltered: no library conversion
        assert_eq!(
            negotiate(&r, &c).unwrap_err(),
            Error::FormatUnsupported { nearest: None },
            "{to:?}"
        );
    }
}

#[test]
fn dop_to_narrow_integer_target_rejected() {
    for to in [
        SampleFormat::I16,
        SampleFormat::U8,
        SampleFormat::I32 { valid_bits: 20 },
    ] {
        let mut r = request(DOP, 176_400, 2);
        r.conversion = Conversion::Allow(ConversionSpec { from: DOP, to });
        assert_eq!(invalid(&r), "dop requires a 24-bit capable container");
    }
}

#[test]
fn exact_format_only_in_other_mode_is_not_matched_or_converted() {
    // Exact F32 exists only in Shared; Exclusive has nothing usable for the conversion.
    let c = caps(vec![entry(
        ShareMode::Shared,
        SampleFormat::F32,
        &[48_000],
        &[2],
    )]);
    let mut r = request(SampleFormat::F32, 48_000, 2);
    r.mode = ShareMode::Exclusive;
    r.conversion = Conversion::Allow(ConversionSpec {
        from: SampleFormat::F32,
        to: SampleFormat::I16,
    });
    assert_eq!(
        negotiate(&r, &c).unwrap_err(),
        Error::FormatUnsupported { nearest: None }
    );
}
