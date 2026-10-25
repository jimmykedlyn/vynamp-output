//! The stream handle: state machine, timing, counters and events.
//!
//! # State table
//!
//! | from \ call | `start`   | `pause`  | `resume`  | `drain`                 | `stop`    |
//! |-------------|-----------|----------|-----------|-------------------------|-----------|
//! | `Opened`    | `Running` | invalid  | invalid   | invalid                 | `Stopped` |
//! | `Running`   | invalid   | `Paused` | invalid   | `Draining` (→ `Stopped`) | `Stopped` |
//! | `Paused`    | invalid   | invalid  | `Running` | invalid                 | `Stopped` |
//! | `Draining`  | invalid   | invalid  | invalid   | keeps waiting           | `Stopped` |
//! | `Stopped`   | invalid   | invalid  | invalid   | invalid                 | no-op `Ok` |
//! | `Lost`      | invalid   | invalid  | invalid   | invalid                 | `Stopped` |
//!
//! "invalid" is `Error::InvalidRequest { what: "state" }`. `Lost` is entered
//! only by a backend notification (device gone), from any state except
//! `Stopped`; a `drain` in progress then returns `Error::DeviceLost`. A
//! stopped stream cannot be restarted: open a new one. `stop` releases the
//! device thread and discards the device buffers, so reopening never plays
//! old audio.

use std::fmt;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::sync::mpsc::Receiver;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use crate::config::StreamConfig;
use crate::error::Error;
use crate::event::Event;
use crate::render::PresentationTiming;
use crate::rtflags::{RtFlags, ST_DRAINING, ST_LOST, ST_OPENED, ST_PAUSED, ST_RUNNING, ST_STOPPED};

/// Lifecycle state of a [`Stream`] (see the module-level table).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StreamState {
    Opened,
    Running,
    Paused,
    Draining,
    Stopped,
    Lost,
}

impl StreamState {
    fn from_code(code: u8) -> Self {
        match code {
            ST_OPENED => StreamState::Opened,
            ST_RUNNING => StreamState::Running,
            ST_PAUSED => StreamState::Paused,
            ST_DRAINING => StreamState::Draining,
            ST_LOST => StreamState::Lost,
            _ => StreamState::Stopped,
        }
    }
}

/// Monotonic counters of a stream.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Counters {
    /// Periods in which the source delivered fewer frames than asked
    /// (without ending the stream). Counted once per period.
    pub source_underruns: u64,
    /// Under/overruns reported by the OS.
    pub os_xruns: u64,
    /// Frames delivered to the device, content and silence.
    pub frames_delivered: u64,
    /// Device periods (callbacks) processed.
    pub callbacks: u64,
}

/// Result of [`Stream::drain`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DrainOutcome {
    /// The last content frame was presented; the stream is now `Stopped`.
    Drained,
    /// The timeout elapsed first; the stream stays `Draining`.
    TimedOut,
}

/// What a backend does on the control thread for each transition.
/// Plans 02+ implement it per OS; the Null/Capture backends in `null.rs`.
pub(crate) trait BackendStream: Send {
    fn start(&mut self) -> Result<(), Error>;
    fn pause(&mut self) -> Result<(), Error>;
    /// Stops the device, discards its buffers and joins its thread, if any.
    fn stop(&mut self) -> Result<(), Error>;
}

/// How often `drain` re-checks the presented position (caller thread).
const DRAIN_POLL: Duration = Duration::from_millis(1);

/// An open output stream.
///
/// Dropping a stream stops it.
pub struct Stream {
    config: StreamConfig,
    flags: Arc<RtFlags>,
    backend: Box<dyn BackendStream>,
    watcher: Option<JoinHandle<()>>,
    events: Receiver<Event>,
    closed: bool,
}

fn invalid_state() -> Error {
    Error::InvalidRequest { what: "state" }
}

impl Stream {
    /// Assembles a stream whose device thread and watcher already run.
    pub(crate) fn new(
        config: StreamConfig,
        flags: Arc<RtFlags>,
        backend: Box<dyn BackendStream>,
        watcher: JoinHandle<()>,
        events: Receiver<Event>,
    ) -> Self {
        Stream {
            config,
            flags,
            backend,
            watcher: Some(watcher),
            events,
            closed: false,
        }
    }

    /// The real, negotiated configuration.
    pub fn config(&self) -> &StreamConfig {
        &self.config
    }

    pub fn state(&self) -> StreamState {
        StreamState::from_code(self.flags.state_code())
    }

    /// `Opened -> Running`.
    pub fn start(&mut self) -> Result<(), Error> {
        self.go(ST_OPENED, ST_RUNNING, |b| b.start())
    }

    /// `Running -> Paused`. The device stops consuming frames.
    pub fn pause(&mut self) -> Result<(), Error> {
        self.go(ST_RUNNING, ST_PAUSED, |b| b.pause())
    }

    /// `Paused -> Running`.
    pub fn resume(&mut self) -> Result<(), Error> {
        self.go(ST_PAUSED, ST_RUNNING, |b| b.start())
    }

    fn go(
        &mut self,
        from: u8,
        to: u8,
        op: impl FnOnce(&mut dyn BackendStream) -> Result<(), Error>,
    ) -> Result<(), Error> {
        if !self.flags.transition(from, to) {
            return Err(invalid_state());
        }
        op(self.backend.as_mut()).inspect_err(|_| {
            self.flags.transition(to, from);
        })
    }

    /// Waits until the source has signalled its end and the last content
    /// frame has been presented (`timing().device_frame` reached it), then
    /// stops the stream. From `Running` (moves to `Draining`) or `Draining`.
    ///
    /// Blocks the caller (polling every millisecond) for at most `timeout`;
    /// on timeout the stream stays `Draining` and `drain` may be called
    /// again. A device lost meanwhile gives `Error::DeviceLost`.
    pub fn drain(&mut self, timeout: Duration) -> Result<DrainOutcome, Error> {
        match self.flags.state_code() {
            ST_RUNNING if self.flags.transition(ST_RUNNING, ST_DRAINING) => {}
            ST_DRAINING => {}
            _ => return Err(invalid_state()),
        }
        let start = Instant::now();
        loop {
            if self.flags.state_code() == ST_LOST {
                return Err(Error::DeviceLost);
            }
            if let (Some(end), Some(t)) = (self.flags.content_end(), self.timing())
                && t.device_frame >= end
            {
                self.stop()?;
                return Ok(DrainOutcome::Drained);
            }
            let elapsed = start.elapsed();
            if elapsed >= timeout {
                return Ok(DrainOutcome::TimedOut);
            }
            std::thread::sleep((timeout - elapsed).min(DRAIN_POLL));
        }
    }

    /// Stops the stream from any state. Stopping a stopped stream is `Ok`.
    pub fn stop(&mut self) -> Result<(), Error> {
        if self.closed {
            return Ok(());
        }
        self.closed = true;
        self.flags.state.store(ST_STOPPED, Ordering::Release);
        let result = self.backend.stop();
        self.flags.watcher_exit.store(true, Ordering::Release);
        if let Some(w) = self.watcher.take() {
            w.thread().unpark();
            let _ = w.join();
        }
        result
    }

    /// Latest presentation timing, readable from any thread; `None` before
    /// the first period was delivered.
    pub fn timing(&self) -> Option<PresentationTiming> {
        self.flags.timing.read()
    }

    pub fn counters(&self) -> Counters {
        let f = &self.flags;
        Counters {
            source_underruns: f.source_underruns.load(Ordering::Acquire),
            os_xruns: f.os_xruns.load(Ordering::Acquire),
            frames_delivered: f.frames_delivered.load(Ordering::Acquire),
            callbacks: f.callbacks.load(Ordering::Acquire),
        }
    }

    /// Events of this stream (underruns, panics, device lost, downgrade),
    /// produced by the watcher thread, never by the audio thread.
    pub fn events(&self) -> &Receiver<Event> {
        &self.events
    }
}

impl Drop for Stream {
    fn drop(&mut self) {
        let _ = self.stop();
    }
}

impl fmt::Debug for Stream {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Stream")
            .field("config", &self.config)
            .field("state", &self.state())
            .field("counters", &self.counters())
            .finish_non_exhaustive()
    }
}
