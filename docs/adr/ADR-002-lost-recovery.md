# ADR-002: Recovery from `Lost` is `pause`, which reopens the same device

Status: accepted (maintainer decision, 2026-11-16). Date: 2026-11-16.

## Context

A WASAPI exclusive stream can lose its endpoint at any moment: another process
takes the device, the USB cable is pulled, or the machine suspends. Every
device-gone code the backend sees moves the stream to `Lost` and emits
`Event::DeviceLost`; `OUTPUT-REQ-09` forbids the library from ever migrating to
another device on its own.

Until now `Lost` was terminal short of `stop`: the state table rejected both
`pause` and `resume` from `Lost`, so the only recovery was to stop the stream
and open a new one. The application then had to build a whole new stream, and
the accepted criterion `OUTPUT-AC-17` — "busy/unplug/sleep recuperam com
pausa" — had no state-machine path to be true. The state table is public API
(`specs/output.md` §3.3), so changing it needs this ADR.

The device thread cannot survive the loss as it was: on Windows its clock is
parked on an event of a dead endpoint, and `WasapiClock::wait` returns `None`
on device-gone, ending the thread. The `RenderSource` and the whole pre-allocated
driver state (period buffers, pipeline scratch) live inside that thread, so a
recovery that keeps them is a recovery that survives device loss.

## Decision

1. **`pause` gains a recovery transition: `Lost -> Paused`.** `resume`
   continues to mean only `Paused -> Running`; it stays invalid from `Lost`.
   The state table in `stream.rs` and `specs/output.md` §3.3 gains the row.

2. **`pause` from `Lost` reopens the same device and only that device.** The
   backend recovers with the `OpenRequest` already recorded at open time —
   same endpoint id, same format, same period. `OUTPUT-REQ-09` stands: the
   list in `Stream::open_attempts` grows by that endpoint's id (appended once,
   at the single point where a device is opened), and no other id ever enters
   it. If reopening fails (the device is still gone), `pause` returns the
   backend's error, the stream stays `Lost`, and the application may try
   `pause` again later or `stop`.

3. **Recovery discards device buffers and never replays old audio.** The
   reopened stream starts from an empty OS buffer, and the driver's
   pre-allocated period buffer is refilled from the source before it is
   written, so no stale period reaches the fresh device — old audio never
   replays after recovery, exactly as after `stop`. `device_frame_index`
   keeps counting forward (content and silence), so a consumer's position
   never goes backwards.

4. **`Lost -> Stopped` is unchanged**, and a stream that recovers goes
   `Lost -> Paused -> Running` only by an explicit `resume`, mirroring how
   `Opened -> Running` needs `start`.

5. **The device thread exits on loss and hands the driver back; the control
   thread respawns it.** Mechanism, in `driver.rs`, `null.rs` and
   `wasapi_host.rs`:

   - `run_device_thread` returns `Option<Driver<C, K>>`: `Some(driver)` when
     it left because the device was lost, `None` when it left because the
     stream was stopped. The `Driver` owns the `RenderSource`, the
     pre-allocated period buffers, the pipeline scratch, `device_frame` and
     the source state, so returning it is what makes the source survive.
   - The `JoinHandle` carries that value. `BackendStream::recover` (the new
     trait method, called by `Stream::pause` from `Lost`) joins the old
     thread, splits the driver with `Driver::release_device` — dropping the
     clock and the sink, which hold every handle to the old OS stream — and
     only then reopens the same endpoint, attaching the new clock and sink to
     the `DriverCore` with `DriverCore::attach`. The order matters: an
     exclusive client the OS still counts as a client of the endpoint makes
     `Initialize` fail with `AUDCLNT_E_DEVICE_IN_USE`. If the open fails the
     `DriverCore` and the sink's period bytes stay in the backend, so the next
     `pause` retries with the same source. The clock's own pacing position
     (`k` for the real clock, `taken` for the virtual one) lives in the clock,
     which is part of the driver, so the respawned thread continues the same
     pacing instead of replaying granted ticks.
   - Nothing on the device thread allocates, locks, does I/O or logs: the
     handoff is a `JoinHandle` return value, and the only new work on that
     thread is the `Option` it builds. `tests/zero_alloc_device.rs` measures
     the periods after a recovery.
   - `RtFlags.lost` is cleared by `recover` on the control thread, and the
     watcher counts losses from `RtFlags.lost_seq` (incremented by
     `notify_lost`) rather than from the `lost` flag, so a recovery cannot
     swallow the next loss and a loss recovered between two polls is still
     reported.

   The Null and Capture backends implement `recover` the same way: their
   virtual device never actually goes away, so the reopen is a no-op and the
   respawn is immediate — which is what the CI tests exercise.

## Consequences

- `OUTPUT-AC-17`'s Then is now implementable as written: busy, unplug and
  sleep recover with `pause` + `resume`, without tearing the stream down.
  `busy_unplug_and_sleep_recover_with_pause` keeps its name and now performs
  the recovery the name claims.
- The application keeps its `RenderSource`, its counters and its event
  receiver across the loss; only the OS stream object is rebuilt.
- **Negative consequence, accepted:** reopening a device is long and fallible
  (driver re-enumeration can take hundreds of milliseconds and fail), so
  `pause` from `Lost` can fail, and unlike every other `pause` the failure
  leaves the stream in `Lost`. Callers must treat `Err` from that call as
  "not back yet", not as a programming error.
- `Stream::open_attempts` is unchanged in content for a recovered stream: the
  recovery reopen records the same endpoint id, and the backend appends it
  only when it is not already the last entry, so the list stays the set of
  distinct endpoints opened in order. The AC-18 test's "no fallback speaker"
  assertion holds across recovery too: a recovery that opened another device
  would append a different id.
- A recovery costs one thread join and one thread spawn on the control
  thread, plus the OS reopen. That is not on the audio path, and it happens
  only when the application asks for recovery.

## Alternatives rejected

- **`resume` from `Lost -> Running`**: one call instead of two, but it fuses
  "the device is back" with "start rendering now", and leaves no way to be
  paused-while-recovered. The maintainer explicitly chose the two-step path;
  `resume` keeps its single meaning.
- **Park the device thread on `Lost` and wake it after the reopen**: keeps
  the thread, but the device thread would then have to read the new OS stream
  from somewhere shared — an `Arc<Mutex<Option<..>>>` read on every period,
  which is a lock in the real-time path. The respawn keeps the RT path
  lock-free at the cost of one thread join and one spawn per recovery.
- **Recover in the watcher thread**: the watcher is non-RT but is not the
  owner of the backend; state transitions stay on the control thread that
  already owns them.
- **Retry internally until the device returns**: hides failure from the
  application and can block `pause` for seconds; an explicit error and a
  retryable call is the honest contract.
