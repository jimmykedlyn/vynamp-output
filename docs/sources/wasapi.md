# Sources — WASAPI

Official documentation consulted for every API the `vynamp-output-wasapi-sys`
crate declares. Declarations are written from these pages; no header, IDL or
vendor source is copied (CONTRIBUTING.md, "Clean-room").

Access date: 2026-11-15.

## Interfaces and their method order

The vtable order is the declaration order in the interface. The pages below
list the methods; the order used in `src/audioclient.rs`, `src/mmdevice.rs`
and `src/com.rs` is the one those pages document, cross-checked against the
permissively licensed `windows` crate (see "Cross-check" below).

| Interface | Page |
|---|---|
| `IUnknown` (`QueryInterface`, `AddRef`, `Release`) | <https://learn.microsoft.com/en-us/windows/win32/api/unknwn/nn-unknwn-iunknown> |
| `IMMDeviceEnumerator` | <https://learn.microsoft.com/en-us/windows/win32/api/mmdeviceapi/nn-mmdeviceapi-immdeviceenumerator> |
| `IMMDeviceCollection` | <https://learn.microsoft.com/en-us/windows/win32/api/mmdeviceapi/nn-mmdeviceapi-immdevicecollection> |
| `IMMDevice` | <https://learn.microsoft.com/en-us/windows/win32/api/mmdeviceapi/nn-mmdeviceapi-immdevice> |
| `IAudioClient` | <https://learn.microsoft.com/en-us/windows/win32/api/audioclient/nn-audioclient-iaudioclient> |
| `IAudioRenderClient` | <https://learn.microsoft.com/en-us/windows/win32/api/audioclient/nn-audioclient-iaudiorenderclient> |

## Structures and constants

| Item | Page |
|---|---|
| `WAVEFORMATEXTENSIBLE` (layout, `cbSize` 22, `Samples.wValidBitsPerSample`, `dwChannelMask`, `SubFormat`) | <https://learn.microsoft.com/en-us/windows/win32/api/mmreg/ns-mmreg-waveformatextensible> |
| `AUDCLNT_STREAMFLAGS_*` (`EVENTCALLBACK` 0x00040000, `NOPERSIST` 0x00080000) | <https://learn.microsoft.com/en-us/windows/win32/coreaudio/audclnt-streamflags-xxx-constants> |
| `IAudioClient::Initialize` (exclusive event mode needs `hnsBufferDuration == hnsPeriodicity != 0`; `AUDCLNT_E_BUFFER_SIZE_NOT_ALIGNED` retry) | <https://learn.microsoft.com/en-us/windows/win32/api/audioclient/nf-audioclient-iaudioclient-initialize> |
| Exclusive-mode streams (preemption, per-device user setting, no shared fallback) | <https://learn.microsoft.com/en-us/windows/win32/coreaudio/exclusive-mode-streams> |
| MMDevice API overview (`CoCreateInstance(CLSID_MMDeviceEnumerator, …)`) | <https://learn.microsoft.com/en-us/windows/win32/coreaudio/mmdevice-api> |
| `CoInitializeEx` return values (`S_OK`, `S_FALSE`, `RPC_E_CHANGED_MODE`) | <https://learn.microsoft.com/en-us/windows/win32/api/combaseapi/nf-combaseapi-coinitializeex> |
| `RPC_E_CHANGED_MODE` = `0x80010106` ("Cannot change thread mode after it is set"), header `Winerror.h`/`rpcerror.h` | <https://learn.microsoft.com/en-us/windows/win32/com/com-error-codes-3> |

## Cross-check

The COM vtable order and the numeric values of the `AUDCLNT_E_*` codes are
easy to get wrong and impossible to test on Linux, so they were checked
against the `windows` and `windows-sys` crates (MIT OR Apache-2.0), which are
generated from the SDK metadata:

- `windows` 0.62.2, `src/Windows/Win32/Media/Audio/mod.rs`:
  `IAudioClient_Vtbl`, `IAudioRenderClient_Vtbl`, `IMMDevice_Vtbl`,
  `IMMDeviceEnumerator_Vtbl`, `IMMDeviceCollection_Vtbl`, and the
  `define_interface!` GUIDs.
- `windows-sys` 0.61.2, same path: the `AUDCLNT_E_*` and `AUDCLNT_S_*`
  constants, `KSDATAFORMAT_SUBTYPE_PCM` / `_IEEE_FLOAT`, and
  `MMDeviceEnumerator`.

`RPC_E_CHANGED_MODE` was checked the same way after a review found the
constant declared as `0x800101C6`: the Microsoft page above states
`0x80010106`, and `status.rs` now asserts that exact value in a test, so a
wrong constant fails on Linux CI instead of only on a Windows thread that is
already in an apartment.

No code was copied from either crate; only the numeric values and the method
order were read, and both are facts of the public ABI that the Microsoft
pages above also state.

## What is not covered

- `IMMNotificationClient` (device-added/removed callbacks). The backend
  detects loss from the stream's own calls, which is what `OUTPUT-AC-17` and
  `OUTPUT-AC-18` measure; a notification client would be a separate TASK.
- `IAudioEndpointVolume`. `PathProperties::endpoint_volume` is reported as
  `Unknown` rather than guessed, because reading it needs this interface and
  the criteria do not require it.
- `IAudioClient3` (low-latency shared mode). Out of scope: this backend opens
  exclusive only.
