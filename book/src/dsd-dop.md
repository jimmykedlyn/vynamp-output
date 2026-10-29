# DSD and DoP

DoP (DSD over PCM) carries 16 DSD bits per channel in each 24-bit PCM sample,
with an alternating marker byte (`0x05`, `0xFA`) in the most significant byte.
A DAC that recognises the markers decodes DSD; anything that touches the
samples turns it into noise.

## Requesting DoP

Use `SampleFormat::Dop { dsd_rate }`. The carrier PCM rate is `dsd_rate / 16`
(`dop_pcm_rate`): DSD64 (2,822,400 Hz) gives 176,400 Hz, DSD128 gives 352,800 Hz,
DSD256 gives 705,600 Hz.

```rust
use vynamp_output::dop_pcm_rate;
assert_eq!(dop_pcm_rate(2_822_400), Some(176_400));
```

The source receives `DeviceBuffer::Dsd(&mut [u8])`: per carrier frame, 2 bytes
per channel grouped by channel, MSB first within a byte (DFF order; DSF sources
must bit-reverse each byte). The library packs the markers and keeps their phase
continuous in silence, where the idle byte is `0x69`.

## When DoP may open

`check_dop_path` allows DoP only when:

- the OS mixer is bypassed (`os_mixer_bypassed: Yes`), otherwise
  `ExclusiveUnavailable`;
- there is no library conversion, otherwise `FormatUnsupported`;
- endpoint volume is unity or unknown. `NotUnity` is `ExclusiveUnavailable`;
  `Unknown` opens and emits `Event::DopVolumeUnverified`.

DoP never goes through a mixer. Native DSD formats arrive with the ALSA
backend; DSD to PCM decimation belongs to the decoder, not here.
