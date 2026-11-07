//! Atomic flags, counters and the timing seqlock shared between the device
//! (real-time) thread, the event watcher and the control thread.
//!
//! RT module: only atomics, no allocation, locking or blocking (checked by
//! the static guard in `rt_guard.rs`). Construction happens in `open()`.

use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU64, Ordering, fence};

use crate::render::{MonotonicNanos, PresentationTiming, TimingSource};

/// Stream state codes stored in [`RtFlags::state`] (mirrors `StreamState`).
pub(crate) const ST_OPENED: u8 = 0;
pub(crate) const ST_RUNNING: u8 = 1;
pub(crate) const ST_PAUSED: u8 = 2;
pub(crate) const ST_DRAINING: u8 = 3;
pub(crate) const ST_STOPPED: u8 = 4;
pub(crate) const ST_LOST: u8 = 5;

/// Everything the device thread shares with the rest of the stream.
#[derive(Debug)]
pub(crate) struct RtFlags {
    /// Current `StreamState` code. Written by the control thread (CAS) and
    /// by [`RtFlags::notify_lost`]; read by the device thread.
    pub(crate) state: AtomicU8,
    pub(crate) source_underruns: AtomicU64,
    pub(crate) os_xruns: AtomicU64,
    pub(crate) frames_delivered: AtomicU64,
    pub(crate) callbacks: AtomicU64,
    pub(crate) source_panicked: AtomicBool,
    pub(crate) lost: AtomicBool,
    /// The source reported end of stream; `last_content_frame` is valid.
    pub(crate) end_of_stream: AtomicBool,
    /// Exclusive end of the content: the first device frame after it.
    pub(crate) last_content_frame: AtomicU64,
    /// Device ticks fully processed (running or not).
    pub(crate) ticks: AtomicU64,
    /// Last `ticks` value the watcher has turned into events.
    pub(crate) watched_ticks: AtomicU64,
    /// Asks the device thread to leave its loop.
    pub(crate) exit: AtomicBool,
    pub(crate) device_exited: AtomicBool,
    /// Asks the watcher to do a final poll and leave.
    pub(crate) watcher_exit: AtomicBool,
    pub(crate) timing: TimingSeqlock,
}

impl RtFlags {
    pub(crate) fn new() -> Self {
        RtFlags {
            state: AtomicU8::new(ST_OPENED),
            source_underruns: AtomicU64::new(0),
            os_xruns: AtomicU64::new(0),
            frames_delivered: AtomicU64::new(0),
            callbacks: AtomicU64::new(0),
            source_panicked: AtomicBool::new(false),
            lost: AtomicBool::new(false),
            end_of_stream: AtomicBool::new(false),
            last_content_frame: AtomicU64::new(0),
            ticks: AtomicU64::new(0),
            watched_ticks: AtomicU64::new(0),
            exit: AtomicBool::new(false),
            device_exited: AtomicBool::new(false),
            watcher_exit: AtomicBool::new(false),
            timing: TimingSeqlock::new(),
        }
    }

    pub(crate) fn state_code(&self) -> u8 {
        self.state.load(Ordering::Acquire)
    }

    /// Whether the device should pull from the source this period.
    pub(crate) fn is_rendering(&self) -> bool {
        matches!(self.state_code(), ST_RUNNING | ST_DRAINING)
    }

    /// Atomically moves `from -> to`; false when the state was not `from`.
    pub(crate) fn transition(&self, from: u8, to: u8) -> bool {
        self.state
            .compare_exchange(from, to, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
    }

    /// Backend notification: the device is gone. Moves any state except
    /// `Stopped` to `Lost` and raises the `lost` flag for the watcher.
    pub(crate) fn notify_lost(&self) {
        let mut cur = self.state_code();
        while cur != ST_STOPPED && cur != ST_LOST {
            match self.state.compare_exchange_weak(
                cur,
                ST_LOST,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => {
                    self.lost.store(true, Ordering::Release);
                    return;
                }
                Err(now) => cur = now,
            }
        }
    }

    /// Records the end of the content (single writer: the device thread).
    pub(crate) fn mark_end_of_stream(&self, last_content_frame: u64) {
        self.last_content_frame
            .store(last_content_frame, Ordering::Relaxed);
        self.end_of_stream.store(true, Ordering::Release);
    }

    /// `Some(exclusive end of content)` once the source reported its end.
    pub(crate) fn content_end(&self) -> Option<u64> {
        self.end_of_stream
            .load(Ordering::Acquire)
            .then(|| self.last_content_frame.load(Ordering::Relaxed))
    }
}

const SRC_NONE: u64 = 0;
const SRC_REPORTED: u64 = 1;
const SRC_ESTIMATED: u64 = 2;

/// Read attempts before [`TimingSeqlock::read`] gives up and returns `None`.
const READ_RETRIES: u32 = 64;

/// Single-writer seqlock over a [`PresentationTiming`].
///
/// The writer (the device thread) makes the sequence odd, stores the fields
/// and makes it even again; readers retry while the sequence is odd or
/// changed during their read. All fields are atomics, so a torn read is
/// detected, never undefined. Re-implemented after the pattern of the
/// Vynamp spike's `clock.rs` (Boehm, "Can seqlocks get along with
/// programming language memory models?", fences variant).
#[derive(Debug)]
pub(crate) struct TimingSeqlock {
    seq: AtomicU64,
    device_frame: AtomicU64,
    at: AtomicU64,
    latency_frames: AtomicU64,
    source: AtomicU64,
}

impl TimingSeqlock {
    pub(crate) const fn new() -> Self {
        TimingSeqlock {
            seq: AtomicU64::new(0),
            device_frame: AtomicU64::new(0),
            at: AtomicU64::new(0),
            latency_frames: AtomicU64::new(0),
            source: AtomicU64::new(SRC_NONE),
        }
    }

    /// Publishes `t`. Must only be called from one thread.
    pub(crate) fn write(&self, t: PresentationTiming) {
        let s = self.seq.load(Ordering::Relaxed);
        self.seq.store(s.wrapping_add(1), Ordering::Relaxed);
        fence(Ordering::Release);
        self.device_frame.store(t.device_frame, Ordering::Relaxed);
        self.at.store(t.at.0, Ordering::Relaxed);
        self.latency_frames
            .store(t.latency_frames, Ordering::Relaxed);
        let source = match t.source {
            TimingSource::Reported => SRC_REPORTED,
            TimingSource::Estimated => SRC_ESTIMATED,
        };
        self.source.store(source, Ordering::Relaxed);
        self.seq.store(s.wrapping_add(2), Ordering::Release);
    }

    /// The latest consistent snapshot; `None` before the first write, or if
    /// no consistent read succeeded within a bounded number of attempts
    /// (only possible with a writer far faster than any audio period).
    pub(crate) fn read(&self) -> Option<PresentationTiming> {
        for _ in 0..READ_RETRIES {
            let s1 = self.seq.load(Ordering::Acquire);
            if s1 % 2 == 1 {
                std::hint::spin_loop();
                continue;
            }
            let device_frame = self.device_frame.load(Ordering::Relaxed);
            let at = self.at.load(Ordering::Relaxed);
            let latency_frames = self.latency_frames.load(Ordering::Relaxed);
            let source = self.source.load(Ordering::Relaxed);
            fence(Ordering::Acquire);
            if self.seq.load(Ordering::Relaxed) != s1 {
                std::hint::spin_loop();
                continue;
            }
            let source = match source {
                SRC_REPORTED => TimingSource::Reported,
                SRC_ESTIMATED => TimingSource::Estimated,
                _ => return None,
            };
            return Some(PresentationTiming {
                device_frame,
                at: MonotonicNanos(at),
                latency_frames,
                source,
            });
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    fn timing(i: u64) -> PresentationTiming {
        PresentationTiming {
            device_frame: i,
            at: MonotonicNanos(i.wrapping_mul(3)),
            latency_frames: i.wrapping_mul(7),
            source: TimingSource::Estimated,
        }
    }

    #[test]
    fn empty_then_written() {
        let l = TimingSeqlock::new();
        assert_eq!(l.read(), None);
        l.write(timing(5));
        assert_eq!(l.read(), Some(timing(5)));
    }

    #[test]
    fn concurrent_reader_never_sees_a_torn_snapshot() {
        const WRITES: u64 = 200_000;
        let lock = Arc::new(TimingSeqlock::new());
        let w = lock.clone();
        let writer = std::thread::spawn(move || {
            for i in 1..=WRITES {
                w.write(timing(i));
            }
        });
        let (mut seen, mut last) = (0u64, 0u64);
        while !writer.is_finished() || seen == 0 {
            if let Some(t) = lock.read() {
                assert_eq!(t.at.0, t.device_frame * 3, "torn read: {t:?}");
                assert_eq!(t.latency_frames, t.device_frame * 7, "torn read: {t:?}");
                assert!(t.device_frame >= last, "went backwards: {t:?}");
                last = t.device_frame;
                seen += 1;
            }
        }
        writer.join().unwrap();
        assert_eq!(lock.read(), Some(timing(WRITES)));
        assert!(seen > 0);
    }

    #[test]
    fn notify_lost_never_leaves_stopped() {
        let f = RtFlags::new();
        f.notify_lost();
        assert_eq!(f.state_code(), ST_LOST);
        assert!(f.lost.load(Ordering::Relaxed));
        let f = RtFlags::new();
        f.state.store(ST_STOPPED, Ordering::Relaxed);
        f.notify_lost();
        assert_eq!(f.state_code(), ST_STOPPED);
        assert!(!f.lost.load(Ordering::Relaxed));
    }

    #[test]
    fn transition_is_compare_and_swap() {
        let f = RtFlags::new();
        assert!(f.transition(ST_OPENED, ST_RUNNING));
        assert!(!f.transition(ST_OPENED, ST_RUNNING));
        assert!(f.is_rendering());
        assert_eq!(f.content_end(), None);
        f.mark_end_of_stream(42);
        assert_eq!(f.content_end(), Some(42));
    }
}
