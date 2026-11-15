# FFI safety

Invariants for every `*-sys` crate and the record of the mandatory clean-context Opus
reviews (see [CONTRIBUTING.md](../CONTRIBUTING.md)).

| Crate | Invariants | Opus review (date, PR) |
|-------|------------|------------------------|
| `vynamp-output-wasapi-sys` | see below | **pending** — required before merge |

## `vynamp-output-wasapi-sys`

The only crate in the workspace with `unsafe` outside the test-only testkit
allocator. It does **not** inherit `[lints] workspace = true`, because
`unsafe_code = "forbid"` cannot be relaxed from inside a crate; the exception
is spelled out in its own `[lints.rust]` instead, with
`unsafe_op_in_unsafe_fn = "deny"` and `clippy::undocumented_unsafe_blocks`
warned (an error under `-D warnings`).

### What is `unsafe`, and why it is confined

| Module | Contents | Why it is `unsafe` |
|---|---|---|
| `com` | `ComPtr<I>`, `CoInitializeEx`, `CoCreateInstance`, `CoTaskMemFree` | Raw COM pointers and reference counting |
| `mmdevice` | `IMMDeviceEnumerator`, `IMMDeviceCollection`, `IMMDevice` vtables | Vtable calls through raw pointers |
| `audioclient` | `IAudioClient`, `IAudioRenderClient` vtables | Vtable calls; `GetBuffer` hands out a raw buffer |
| `device` | `Enumerator`, `DeviceEntry` | Safe wrappers over the above |
| `render` | `ExclusiveStream` | Safe wrapper; owns the device, the client, the render client and the event handle |

`format` and `status` contain no `unsafe` at all and compile on every target,
which is why the layout and the `HRESULT` classification are tested on Linux
CI.

### Invariants

1. **One owner per COM reference.** `ComPtr` is created only by
   `ComPtr::from_raw` (which takes ownership of a reference the OS just
   returned) or by `Clone` (which calls `AddRef`). `Drop` calls `Release`
   exactly once. No `ComPtr` is ever handed to code outside the crate.
2. **The vtable is the documented one.** Each interface struct is
   `#[repr(C)]` with `UnknownVtbl` as its first field, and the method order is
   the declaration order from the Microsoft pages recorded in
   [sources/wasapi.md](sources/wasapi.md), cross-checked against the
   permissively licensed `windows` crate. A wrong order is memory unsafety,
   not a wrong result, which is why it is cross-checked rather than trusted.
3. **`Hresult` is `#[repr(transparent)]` over `i32`.** It appears directly in
   `extern "system"` signatures, so it must be ABI-identical to `LONG`.
4. **`GetBuffer`/`ReleaseBuffer` are paired.** `ExclusiveStream::write_period`
   and `next_period` call `GetBuffer`, copy exactly
   `period_frames * block_align` bytes, and call `ReleaseBuffer` with the same
   frame count. The pointer is never stored, never returned, and never held
   across a call.
5. **The event handle is owned by the stream.** `CreateEventW` in `open`,
   `CloseHandle` in `Drop` and on every error path after creation. It is
   closed exactly once.
6. **`Send`/`Sync` are justified, not assumed.** `ExclusiveStream` is `Send`
   because moving it moves every handle with it. It is `Sync` because WASAPI
   documents `Start`/`Stop` and the render client as callable from any thread
   of the initialised apartment, and the backend never calls `start`/`stop`
   concurrently with `wait_period`/`write_period` on the same stream.
7. **Strings from the OS are freed with the matching allocator.** `GetId`
   returns a `CoTaskMemAlloc`-allocated UTF-16 string; `device::entry_of`
   reads it to the terminator and frees it with `CoTaskMemFree` exactly once.
8. **No `unwrap`/`expect` on an OS result.** Every `HRESULT` becomes a
   `Status`; the backend matches on `Status`, never on a numeric code.

### What is not proven here

The crate compiles for `x86_64-pc-windows-msvc` in CI, and the pure parts are
tested on Linux. **Nothing in this crate has run against a real device.** The
hardware criteria (`OUTPUT-AC-16` to `OUTPUT-AC-18`, `OUTPUT-BK-AC-15` to
`AC-19`) are `#[ignore]` until a DAC and a Windows 11 machine run them. Until
then `support(HostId::Wasapi, _)` is `Compiled` for the features the backend
has, and `None` for DoP, which `to_wasapi` refuses: `Compiled` is exactly "the
target builds; nothing ran".
