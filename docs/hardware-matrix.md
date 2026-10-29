# Hardware matrix

Manual `hardware-verified` entries. Each row records the device, driver, OS, the
command run, the commit and the hash of the digital capture. A DAC indicator
light is not proof: bit-perfect output needs a digital capture (for example
S/PDIF into a capture interface) or a documented equivalent.

The hardware list is pending from the maintainer; until then every entry is
"not run", which is not a pass.

| device | driver | OS | host | scenario | command | commit | capture hash | result |
|---|---|---|---|---|---|---|---|---|
| (pending) | (pending) | Windows 11 | wasapi | shared, F32 48 kHz | (pending) | - | - | not run |
| (pending) | (pending) | Windows 11 | wasapi | exclusive, 24-bit 96 kHz | (pending) | - | - | not run |
| (pending) | (pending) | Windows 11 | wasapi | exclusive, DoP DSD64 | (pending) | - | - | not run |
| (pending) | (pending) | Linux | alsa | `hw:` 24-bit 96 kHz | (pending) | - | - | not run |
| (pending) | (pending) | Linux | alsa | `hw:` DoP DSD64 | (pending) | - | - | not run |
| (pending) | (pending) | macOS | coreaudio | exclusive 24-bit 96 kHz | (pending) | - | - | not run (no Mac agent) |
| (pending) | (pending) | Android | aaudio | arm64 device, F32 48 kHz | (pending) | - | - | not run |
