# Changelog

## Unreleased

- Workspace skeleton, `vynamp-output-testkit` (allocation counter), documentation and
  Woodpecker CI.
- Core API, `RenderSource` contract, stream state machine, Null and Capture hosts, facade and
  DoP packer.
- `VerificationLevel`, `Feature` and `support()` declarations; generated
  `docs/support-matrix.md` (`cargo run -p vynamp-output --example support_matrix`).
- mdBook sources in `book/`, ADR-001, `docs/hardware-matrix.md` (all entries not run) and
  `tools/output-probe`.
- WASAPI event-exclusive host (Windows only, `Compiled`, never run on hardware) and the
  `OUTPUT-AC-16` to `OUTPUT-AC-18` criteria, all `#[ignore]`.
- Recovery from `Lost` is `pause` (ADR-002): `Lost --pause--> Paused` reopens the same
  device, discards its buffers and keeps the `RenderSource` alive; `resume` from `Lost`
  stays invalid. `OUTPUT-BK-AC-11`/`AC-12` are Windows-aware by maintainer ratification.
