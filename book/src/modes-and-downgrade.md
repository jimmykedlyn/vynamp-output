# Modes and downgrade

A `StreamRequest` names the device, `ShareMode` (`Shared` or `Exclusive`),
sample format, rate, channels, layout and buffer size.

## Negotiation

The library first tries exactly what was asked, matched against the device's
`Capabilities` (entries marked `Advertised` or `Probed`).

- `on_unavailable: Error` (default): an unmet request is an error, with the
  nearest match in `FormatUnsupported { nearest }` when known.
- `on_unavailable: Downgrade`: the library opens the nearest match and emits
  `Event::Downgraded { requested, actual, reason }` before the first `start`.

`Stream::config()` is always the real configuration.

## Conversion

Format conversion is off by default (`Conversion::Forbid`). With
`Conversion::Allow(ConversionSpec { from, to })` the library converts, for
example `F32` to a 24-bit device, rounding half to even with clamping and
**no dither**. The conversion is reported in
`PathProperties::library_conversion`. Resampling is never done by the library;
OS resampling happens only when requested and is then reported in
`os_resampling`.

## Path properties

`StreamConfig::path` reports facts instead of assuming them: whether the OS
mixer is bypassed (`Tri`), endpoint volume (`Unity`, `NotUnity(f32)`,
`Unknown`), transport (`Usb`, `Bluetooth`, `Hdmi`, ...) and the real-time
priority outcome. The library never applies gain.

## Lost devices

The library never migrates a stream on its own. On unplug or default change it
emits `Event::DeviceLost` and the stream moves to `Lost`.

Recovery is `pause`, not `resume` (ADR-002). `pause` from `Lost` reopens **the
same** device with the request recorded at open, discards the device buffers
and leaves the stream `Paused`; a later, explicit `resume` plays. The
`RenderSource` and the counters survive the loss, so the application does not
build a new stream. Reopening is long and can fail: if the device has not come
back, `pause` returns the backend's error and the stream stays `Lost`, and the
application may retry `pause` later or `stop`.

`resume` from `Lost` is invalid — it only ever means `Paused -> Running`.
Stopping discards device buffers so old audio never replays, and so does
recovery.
