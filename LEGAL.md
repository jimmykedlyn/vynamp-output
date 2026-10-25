# Legal notes

This file is not legal advice. Whoever distributes a build is responsible for complying
with the licences of everything they ship.

## System libraries

The library links no third-party code and copies no headers. Platform components are used
through their public APIs:

- **Linux:** `libasound.so.2` (ALSA, LGPL) and `libpulse.so.0` (PulseAudio, LGPL) are loaded
  dynamically at runtime with our own `dlopen` code; `libpipewire-0.3.so.0` (PipeWire, MIT)
  likewise. Headers are not copied into this repository, and the Linux build needs no `-dev`
  packages. A missing library yields `LibraryUnavailable` and the host is skipped with a reason.
  Dynamic loading at runtime, without redistributing those libraries, keeps the
  `MIT OR Apache-2.0` licence of this code unaffected by the LGPL.
- **Windows and macOS:** OS components that exist on every baseline system (WASAPI/COM,
  CoreAudio/AudioToolbox frameworks).
- **Android:** `libaaudio.so`, present on API 29+.

## ASIO is excluded from the open core

The ASIO SDK is distributed by Steinberg under its own licence. In October 2025 Steinberg
announced a GPLv3 option as well; that is still incompatible with an `MIT OR Apache-2.0`
core unless the distributor chooses the licence for their combined work. "ASIO" is also a
registered trademark, and declaring the ASIO interface without the SDK would still be a
derivative of it. ASIO is therefore not part of this repository. At most a separate,
optional adapter may exist later, outside this repository, attached only through the
public backend trait. Its absence never blocks acceptance of the core.

## Clean-room

No code is copied from other audio libraries or SDKs; see [CONTRIBUTING.md](CONTRIBUTING.md).
