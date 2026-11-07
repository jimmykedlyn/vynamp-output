# vynamp-output 01 — Core API, Null and Capture backends — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Freeze the public API of `vynamp-output` (types, errors, events, `RenderSource`, `StreamRequest`/`StreamConfig`, timing, stream state machine) and prove it with two pure-Rust backends — `Null` (real or virtual clock) and `Capture` (records exactly what the callback delivered) — plus the RT gates, DoP packer and testkit. No OS FFI yet.

**Architecture:** Workspace with `vynamp-output-core` (all safe logic, `Null`, `Capture`), `vynamp-output-testkit` (publish = false: per-thread counting allocator, sources, harness), facade `vynamp-output` (features `null`, `capture`; OS backends come in plans 02–04). The device thread of Null/Capture is a std thread that calls the `RenderSource` exactly like an OS callback would (pre-allocated buffers, atomics only), so the RT contract is exercised in CI.

**Tech Stack:** Rust 1.99.0, edition 2024, std only.

**Spec:** `docs/superpowers/specs/2026-10-25-vynamp-output-design.md` (§5.2 API, §5.5 RT contract, §5.6 negotiation, §5.7 timing, §5.9 DoP, §5.10 verification levels).

**Later plans (Layer 1):** 02 loader + monotonic clock + WASAPI shared (`-sys-windows`) · 03 ALSA `default`/`hw:` simple · 04 CoreAudio default device + AAudio shared · 05 Vynamp integration (replace cpal in the E01-02 spike).

## Global Constraints

- Zero crates.io dependencies (dev included). All crates in this plan: `#![forbid(unsafe_code)]` — except `vynamp-output-testkit`'s GlobalAlloc impl (crate-level allow, publish = false, documented; same pattern as vynamp-decode).
- RT path (anything the device thread runs per period): no allocation, no locks, no blocking, no I/O, no panics reachable; only atomics, slices and pre-allocated scratch. Enforced by the counting-allocator test and a static token guard test (no `Vec::new|push|Box::new|String|format!|println!|Mutex|lock()|unwrap()|expect(` inside modules marked RT).
- Never silently fall back: requests that can't be honoured → error (`on_unavailable = Error` default) or explicit `Downgraded` event when `Downgrade` was requested; `config()` always returns the real config.
- The library never applies gain, never dithers, never resamples. Conversions only with `conversion: Allow(spec)`: deterministic round-to-nearest-even + clamp.
- Errors structured (no bare `String`). Events produced only off the RT thread (watcher converts RT atomic flags into events; channel `std::sync::mpsc`).
- License MIT OR Apache-2.0; docs in English; commits end with `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`; branch `feature/core` from `develop`; PR to `develop`; merge by the maintainer.
- Gate before every commit: `cargo fmt --check && cargo clippy --workspace --all-targets --locked -- -D warnings && cargo test --workspace --locked`; `RUSTDOCFLAGS=-D warnings cargo doc --workspace --no-deps --locked` at task end.

## Review Focus

1. A source that returns fewer frames than requested (underrun) → rest filled with the format's correct silence, `SourceUnderrun` counted once per period, samples never repeated. (Tasks 3, 4)
2. A source that panics in `render` → silence from then on, `SourcePanicked` event once, stream does not die; device thread keeps its cadence. (Task 4)
3. `drain(timeout)` after the source signals end → last content frame presented before stop; no cut tail; timeout respected. (Tasks 4, 5)
4. Requesting a format the device can't do with `on_unavailable = Error` → error with `nearest`; with `Downgrade` → opens nearest and emits `Downgraded` before the first callback. (Task 2, 5)
5. DoP: marker alternation `0x05`/`0xFA` continuous across periods and across silence/underrun; idle pattern `0x69`. (Task 6)

---

### Task 1: Workspace, testkit, docs skeleton, CI

**Files:** `Cargo.toml` (workspace, resolver 3, edition 2024, rust-version 1.99, license, lints `unsafe_code = "forbid"`), `rust-toolchain.toml` (1.99.0, rustfmt+clippy), `.gitattributes` (`* text=auto eol=lf`), `LICENSE-MIT`, `LICENSE-APACHE`, `README.md`, `CONTRIBUTING.md` (rules above + `-sys` Opus-review rule from spec §5.1), `SECURITY.md` (GitHub private vulnerability reporting), `LEGAL.md` (system libraries LGPL/MIT loaded dynamically; ASIO exclusion and why — spec §5.4), `CHANGELOG.md`, `docs/sources/README.md`, `docs/ffi-safety.md` (empty table for plans 02+), `.woodpecker/{linux,windows,gate}.yaml` (copy the structure from `D:\Projects\vynamp-media-player\vynamp-decode\.woodpecker\` — same labels/images/digests; gate prints `NOT VERIFIED - macOS (no Mac agent)`), `crates/vynamp-output-testkit/{Cargo.toml, src/lib.rs, src/alloc.rs, src/source.rs}`.

**Interfaces (testkit):** `alloc::{CountingAlloc, allocations_on_this_thread() -> u64}` (same design as vynamp-decode-testkit); `source::{Silence, Ramp { start: i64, step: i64 }, SineF32 { freq, rate }, StopAfter<S> { frames }}` implementing core's `RenderSource` (added in Task 3 — testkit depends on core; create the module now with `todo` removed in Task 3).
- [ ] Steps: tests for the allocator counter (one allocation counted on this thread, none on another) → FAIL → implement → PASS + gate → commit `chore: workspace skeleton, testkit, docs and Woodpecker CI`.

### Task 2: Core types, errors, events, negotiation (`vynamp-output-core`)

**Files:** `crates/vynamp-output-core/src/{lib.rs, format.rs, request.rs, config.rs, error.rs, event.rs, negotiate.rs}`.

**Interfaces (exact names from spec §5.2):**
```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)] pub enum HostId { Wasapi, Alsa, PipeWire, Pulse, CoreAudio, AAudio, Null, Capture }
#[derive(Debug, Clone, Copy, PartialEq, Eq)] pub enum Justify { Msb, Lsb }
#[derive(Debug, Clone, Copy, PartialEq, Eq)] pub enum SampleFormat { F32, F64, I16, I24Packed, I24In32 { justify: Justify }, I32 { valid_bits: u8 }, U8, Dop { dsd_rate: u32 } /* DSD bit rate, e.g. 2_822_400 */ }
#[derive(Debug, Clone, Copy, PartialEq, Eq)] pub enum ShareMode { Shared, Exclusive }
#[derive(Debug, Clone, Copy, PartialEq, Eq)] pub enum OnUnavailable { Error, Downgrade }
#[derive(Debug, Clone, Copy, PartialEq, Eq)] pub enum Conversion { Forbid, Allow(ConversionSpec) }
#[derive(Debug, Clone, Copy, PartialEq, Eq)] pub struct ConversionSpec { pub from: SampleFormat, pub to: SampleFormat } // round-half-even + clamp, no dither
#[derive(Debug, Clone, PartialEq, Eq)] pub enum DeviceSelector { Id(DeviceId), Default(Role) }
#[derive(Debug, Clone, Copy, PartialEq, Eq)] pub enum Role { Console, Multimedia, Communications }
#[derive(Debug, Clone, PartialEq, Eq, Hash)] pub struct DeviceId(pub String);
#[derive(Debug, Clone, Copy, PartialEq, Eq)] pub enum IdStability { Persistent, Session }
#[derive(Debug, Clone, Copy, PartialEq, Eq)] pub enum BufferRequest { Frames(u32), Duration(std::time::Duration), BackendDefault }
#[derive(Debug, Clone, PartialEq)] pub struct StreamRequest { pub device: DeviceSelector, pub mode: ShareMode, pub format: SampleFormat, pub rate: u32, pub channels: u16, pub layout: ChannelLayout, pub buffer: BufferRequest, pub on_unavailable: OnUnavailable, pub conversion: Conversion, pub allow_rate_change_of_device: bool }
#[derive(Debug, Clone, PartialEq, Eq)] pub struct ChannelLayout { pub positions: Vec<ChannelPosition> } // FL, FR, FC, LFE, BL, BR, SL, SR, …, Unknown(u16)
#[derive(Debug, Clone, PartialEq)] pub struct StreamConfig { pub host: HostId, pub device: DeviceId, pub mode: ShareMode, pub format: SampleFormat, pub valid_bits: u8, pub rate: u32, pub channels: u16, pub layout: ChannelLayout, pub period_frames: u32, pub buffer_frames: u32, pub path: PathProperties }
#[derive(Debug, Clone, PartialEq)] pub struct PathProperties { pub os_mixer_bypassed: Tri, pub os_resampling: bool, pub library_conversion: Option<ConversionSpec>, pub endpoint_volume: EndpointVolume, pub transport: Transport, pub rt_priority: RtPriority }
// Tri { Yes, No, Unknown }; EndpointVolume { Unity, NotUnity(f32), Unknown }; Transport { Usb, Bluetooth, Hdmi, BuiltIn, Virtual, Unknown }; RtPriority { Granted(&'static str), Denied(&'static str) }
#[non_exhaustive] #[derive(Debug, Clone, PartialEq)] pub enum Error { LibraryUnavailable { lib: &'static str }, NoBackend, DeviceNotFound, DeviceBusy, ExclusiveUnavailable { reason: &'static str }, FormatUnsupported { nearest: Option<Box<StreamConfig>> }, PermissionDenied, DeviceLost, Timeout, InvalidRequest { what: &'static str }, Os { backend: HostId, code: i64, call: &'static str } }
#[non_exhaustive] #[derive(Debug, Clone, PartialEq)] pub enum Event { DeviceAdded(DeviceId), DeviceRemoved(DeviceId), DefaultChanged { role: Role }, DeviceLost, SharedFormatChanged, Downgraded { requested: Box<StreamRequest>, actual: Box<StreamConfig>, reason: &'static str }, OsXrun { count: u64 }, SourceUnderrun { count: u64 }, SourcePanicked, RtPriorityDenied { reason: &'static str } }
pub struct Capabilities { pub entries: Vec<CapabilityEntry> } // (mode, format, rates, channels, Advertised|Probed)
pub fn negotiate(req: &StreamRequest, caps: &Capabilities) -> Result<Negotiated, Error>; // Negotiated { config_template, downgraded: Option<&'static str> }
```
- Negotiation: exact match → config; else `Error::FormatUnsupported{nearest}` (nearest = same mode, closest rate ≥ requested else ≤, then same container width, then channels) unless `Downgrade` → nearest + reason. Conversion: when the device lacks the format but supports `ConversionSpec.to` and `Allow` is set → open in `to`, `library_conversion = Some(spec)`. `InvalidRequest` for rate 0, channels 0, layout length ≠ channels, DoP with non-24-bit-capable target, etc.
- [ ] **Step 1: tests** (`tests/negotiate.rs`): exact match; nearest selection order; Error vs Downgrade; conversion allowed/forbidden; invalid requests each with its `what`; Display impls for Error/Event. → FAIL → implement → PASS + gate → commit `feat(core): API types, errors, events and negotiation`.

### Task 3: RenderSource, DeviceBuffer, conversion and silence (`vynamp-output-core`)

**Files:** `src/render.rs`, `src/convert.rs`, `src/silence.rs`, `src/rt_guard.rs` (marker module list for the static guard test).

**Interfaces:**
```rust
pub trait RenderSource: Send + 'static { fn render(&mut self, out: DeviceBuffer<'_>, ctx: &RenderContext<'_>) -> Rendered; }
pub enum DeviceBuffer<'a> { F32(&'a mut [f32]), F64(&'a mut [f64]), I16(&'a mut [i16]), I24Packed(&'a mut [[u8; 3]]), I32(&'a mut [i32]) /* I24In32 and I32 share this */, U8(&'a mut [u8]), Dsd(&'a mut [u8]) }
pub struct RenderContext<'a> { pub frames: u32, pub device_frame_index: u64, pub timing: Option<PresentationTiming>, pub config: &'a StreamConfig }
#[derive(Debug, Clone, Copy, PartialEq, Eq)] pub struct Rendered { pub frames: u32, pub end_of_stream: bool }
#[derive(Debug, Clone, Copy, PartialEq, Eq)] pub struct PresentationTiming { pub device_frame: u64, pub at: MonotonicNanos, pub latency_frames: u64, pub source: TimingSource }
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)] pub struct MonotonicNanos(pub u64);
#[derive(Debug, Clone, Copy, PartialEq, Eq)] pub enum TimingSource { Reported, Estimated }
pub(crate) fn fill_silence(buf: &mut DeviceBuffer<'_>, from_frame: usize, channels: usize, format: SampleFormat, dop_phase: &mut bool);
pub(crate) fn convert_f32_to(src: &[f32], dst: &mut DeviceBuffer<'_>, spec: ConversionSpec) -> Result<(), Error>; // round half even, clamp; I24Packed little-endian
```
- [ ] **Step 1: tests:** silence per format (0 for PCM, 0x80 for U8, DoP silence = idle 0x69 bytes with alternating markers — the DoP packer lands in Task 6; here test PCM/U8 and leave DoP silence to Task 6); conversion extremes (±1.0, ±1.0000001, NaN → 0, −0.0) to I16/I24Packed/I32 with exact expected integers (I16: 1.0 → 32767, −1.0 → −32768; I24: 8388607/−8388608; I32: 2147483647/−2147483648); round-half-even cases; zero allocation in fill/convert (testkit alloc); static guard test scanning `src/render.rs`, `src/convert.rs`, `src/silence.rs` for forbidden tokens. → FAIL → implement → PASS + gate → commit `feat(core): RenderSource contract, device buffers, conversion and silence`.

### Task 4: Stream state machine, device thread driver, RT flags and watcher

**Files:** `src/stream.rs`, `src/driver.rs` (generic "device thread" loop used by Null/Capture and later by OS backends as a reference), `src/rtflags.rs`, `src/watcher.rs`.

**Interfaces:**
```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)] pub enum StreamState { Opened, Running, Paused, Draining, Stopped, Lost }
pub struct Stream { /* backend handle + shared atomics + watcher */ }
impl Stream { pub fn config(&self) -> &StreamConfig; pub fn state(&self) -> StreamState; pub fn start(&mut self) -> Result<(), Error>; pub fn pause(&mut self) -> Result<(), Error>; pub fn resume(&mut self) -> Result<(), Error>; pub fn drain(&mut self, timeout: std::time::Duration) -> Result<DrainOutcome, Error>; pub fn stop(&mut self) -> Result<(), Error>; pub fn timing(&self) -> Option<PresentationTiming>; pub fn counters(&self) -> Counters; pub fn events(&self) -> &std::sync::mpsc::Receiver<Event>; }
pub struct Counters { pub source_underruns: u64, pub os_xruns: u64, pub frames_delivered: u64, pub callbacks: u64 }
pub enum DrainOutcome { Drained, TimedOut }
pub(crate) trait BackendStream: Send { fn start(&mut self) -> Result<(), Error>; fn pause(&mut self) -> Result<(), Error>; fn stop(&mut self) -> Result<(), Error>; } // plans 02+ implement it per OS
```
- State transitions per spec (invalid transition → `InvalidRequest{what:"state"}`); `stop` discards device buffers; `Lost` reachable only from backend notification. Panic in `render` caught with `catch_unwind(AssertUnwindSafe(..))` in the driver; silence afterwards; `source_panicked` atomic → watcher emits `SourcePanicked` once. `Rendered.end_of_stream` → driver stops pulling, delivers silence, and `drain` waits until `timing().device_frame` ≥ last content frame index (or timeout). Timing snapshot via a single-writer seqlock (pattern from the Vynamp spike `vynamp-audio/src/clock.rs`, re-implemented).
- [ ] **Step 1: tests** (driven through the Null backend of Task 5 with a virtual clock — so write Task 5's virtual clock first as a test helper if needed, or implement both tasks' tests together): state machine table; underrun counted once per period, silence after partial render, never replays; panic → silence + one event, cadence continues; drain completes exactly after the last content frame on the virtual clock; drain timeout; zero allocation on the device thread after start (count allocations on that thread via testkit); static token guard over `driver.rs`, `rtflags.rs`. → FAIL → implement → PASS + gate → commit `feat(core): stream state machine, device driver, RT flags and event watcher`.

### Task 5: Null and Capture backends, host API, facade

**Files:** `src/host.rs`, `src/null.rs`, `src/capture.rs`, `crates/vynamp-output/{Cargo.toml, src/lib.rs}`, `crates/vynamp-output/examples/{list_devices.rs, play_sine_null.rs}`.

**Interfaces:**
```rust
pub trait Host: Send + Sync { fn id(&self) -> HostId; fn enumerate(&self) -> Result<Vec<DeviceInfo>, Error>; fn default_device(&self, role: Role) -> Result<Option<DeviceInfo>, Error>; fn capabilities(&self, device: &DeviceId, mode: ShareMode) -> Result<Capabilities, Error>; fn open(&self, req: &StreamRequest, source: Box<dyn RenderSource>) -> Result<Stream, Error>; }
pub struct DeviceInfo { pub id: DeviceId, pub id_stability: IdStability, pub name: String, pub host: HostId, pub default_for: Vec<Role>, pub transport: Transport }
pub fn enumerate_with_timeout(host: &dyn Host, timeout: std::time::Duration) -> Result<Vec<DeviceInfo>, Error>; // auxiliary thread, abandoned on timeout and counted
pub struct NullHost; impl NullHost { pub fn new(clock: NullClock) -> Self } // NullClock::{Real, Virtual(VirtualClockHandle)}; virtual = test advances time
pub struct CaptureHost; impl CaptureHost { pub fn new(path: std::path::PathBuf) -> Self } // writes raw device bytes + a manifest (format, rate, channels, frames) on stop
// Null/Capture capabilities: configurable in the constructor (default: Shared+Exclusive; F32/I16/I24Packed/I32{24,32}; 44.1k–768k; 1–8 ch) so negotiation/downgrade paths are testable.
// facade: pub use vynamp_output_core::*; pub fn hosts() -> Vec<Box<dyn Host>>; pub struct HostSelection { pub chosen: HostId, pub skipped: Vec<(HostId, &'static str)> } ; pub fn default_host() -> (Box<dyn Host>, HostSelection) // this plan: Null only (other hosts in plans 02–04)
```
- Capture: device thread writes into a pre-allocated in-memory ring; a non-RT writer thread flushes to the file (RT path never does I/O); manifest written on stop. Capture is `test-capture` feature in the facade (not default), per spec §9.6.
- [ ] **Step 1: tests:** Null virtual clock: open/start/run N periods/stop with frame-exact `device_frame_index`; real clock smoke (≤ 200 ms); Capture: I24Packed/I32/F32 bytes in file equal the source's samples bit-exactly; manifest correct; underrun silence recorded; downgrade event before first callback; `enumerate_with_timeout` with a deliberately blocking fake host → `Timeout`; examples compile. → FAIL → implement → PASS + gate → commit `feat: Null and Capture hosts and facade`.

### Task 6: DoP packer

**Files:** `src/dop.rs`.
- DoP per spec §5.9: 16 DSD bits per channel per PCM frame into 24-bit samples (bits 23..16 = marker 0x05/0xFA alternating per frame, bits 15..0 = DSD bytes MSB-first in time order), PCM rate = dsd_rate / 16 (DSD64 2.8224 MHz → 176.4 kHz); phase continuous across periods, underrun and silence; DoP silence = idle 0x69 bytes; output into `I24Packed` or `I32 { valid_bits: 24 }` (MSB-justified) device buffers. Opening DoP requires `os_mixer_bypassed: Yes` and no library conversion, else `ExclusiveUnavailable`/`FormatUnsupported` (Null/Capture: configurable path properties to test both).
- [ ] **Step 1: tests:** packed bytes for a known DSD pattern (hand-computed expected words), marker continuity across 3 periods with an underrun in the middle, silence pattern, rate mapping DSD64/128/256 → 176.4/352.8/705.6 kHz, refusal on a shared/mixed path. → FAIL → implement → PASS + gate → commit `feat(core): DoP packer`.

### Task 7: Docs, support matrix, ADR, probe tool

**Files:** `book/` (book.toml, SUMMARY, architecture, rt-contract, writing-a-render-source, timing, modes-and-downgrade, dsd-dop), `docs/adr/ADR-001-render-contract.md` (spec §9.5), `docs/support-matrix.md` (generated by `cargo run --example support_matrix` from host capability/verification declarations: Null/Capture = library-verified), `tools/output-probe/` (binary crate, publish = false: opens a host/device, plays a sine via any host, prints StreamConfig, timing samples, counters, allocation count on the device thread; works with Null now), README/CHANGELOG.
- [ ] **Step 1:** rustdoc examples on Null compile and run (`cargo test --doc`); support_matrix example output matches committed file (test) → gate + `cargo doc -D warnings` → commit `docs: book, ADR-001, support matrix and output probe`. Push branch, PR to develop, CI Gate, final review.
