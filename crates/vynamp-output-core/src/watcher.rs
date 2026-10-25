//! The per-stream event watcher: a non-RT thread that turns the device
//! thread's atomic flags and counters into [`Event`]s.
//!
//! The device thread never sends events itself (sending may allocate and
//! lock). The watcher polls on a short interval and can be woken early with
//! `Thread::unpark` (the Null virtual clock does so after every tick, which
//! makes event delivery deterministic in tests).

use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::sync::mpsc::Sender;
use std::thread::{self, JoinHandle};
use std::time::Duration;

use crate::event::Event;
use crate::rtflags::RtFlags;

/// Poll interval when nobody wakes the watcher.
pub(crate) const POLL: Duration = Duration::from_millis(5);

/// What the watcher has already reported.
#[derive(Debug, Default)]
struct Seen {
    underruns: u64,
    xruns: u64,
    panicked: bool,
    lost: bool,
}

impl Seen {
    /// Emits events for everything new in `flags`.
    fn poll(&mut self, flags: &RtFlags, tx: &Sender<Event>) {
        let underruns = flags.source_underruns.load(Ordering::Acquire);
        if underruns > self.underruns {
            let _ = tx.send(Event::SourceUnderrun {
                count: underruns - self.underruns,
            });
            self.underruns = underruns;
        }
        let xruns = flags.os_xruns.load(Ordering::Acquire);
        if xruns > self.xruns {
            let _ = tx.send(Event::OsXrun {
                count: xruns - self.xruns,
            });
            self.xruns = xruns;
        }
        if !self.panicked && flags.source_panicked.load(Ordering::Acquire) {
            self.panicked = true;
            let _ = tx.send(Event::SourcePanicked);
        }
        if !self.lost && flags.lost.load(Ordering::Acquire) {
            self.lost = true;
            let _ = tx.send(Event::DeviceLost);
        }
    }
}

/// Starts the watcher thread for `flags`, sending into `tx`.
///
/// It leaves after `flags.watcher_exit` is set (and the thread unparked),
/// with a final poll so nothing that happened before is lost.
pub(crate) fn spawn(
    flags: Arc<RtFlags>,
    tx: Sender<Event>,
    name: &'static str,
) -> std::io::Result<JoinHandle<()>> {
    thread::Builder::new()
        .name(name.into())
        .spawn(move || run(&flags, &tx))
}

fn run(flags: &RtFlags, tx: &Sender<Event>) {
    let mut seen = Seen::default();
    loop {
        let exiting = flags.watcher_exit.load(Ordering::Acquire);
        // read the tick count first: every flag written before it is visible
        let ticks = flags.ticks.load(Ordering::Acquire);
        seen.poll(flags, tx);
        flags.watched_ticks.store(ticks, Ordering::Release);
        if exiting {
            break;
        }
        thread::park_timeout(POLL);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc::channel;

    #[test]
    fn deltas_once_flags_and_final_poll() {
        let flags = Arc::new(RtFlags::new());
        let (tx, rx) = channel();
        let mut seen = Seen::default();
        flags.source_underruns.store(2, Ordering::Relaxed);
        flags.source_panicked.store(true, Ordering::Relaxed);
        seen.poll(&flags, &tx);
        seen.poll(&flags, &tx);
        flags.source_underruns.store(5, Ordering::Relaxed);
        flags.os_xruns.store(1, Ordering::Relaxed);
        flags.notify_lost();
        seen.poll(&flags, &tx);
        seen.poll(&flags, &tx);
        let got: Vec<Event> = rx.try_iter().collect();
        assert_eq!(
            got,
            vec![
                Event::SourceUnderrun { count: 2 },
                Event::SourcePanicked,
                Event::SourceUnderrun { count: 3 },
                Event::OsXrun { count: 1 },
                Event::DeviceLost,
            ]
        );

        // the thread reports what happened before it was asked to exit
        let (tx, rx) = channel();
        let flags = Arc::new(RtFlags::new());
        let h = spawn(flags.clone(), tx, "watcher-test").unwrap();
        flags.source_underruns.store(1, Ordering::Release);
        flags.ticks.store(7, Ordering::Release);
        flags.watcher_exit.store(true, Ordering::Release);
        h.thread().unpark();
        h.join().unwrap();
        assert_eq!(
            rx.try_iter().collect::<Vec<_>>(),
            vec![Event::SourceUnderrun { count: 1 }]
        );
        assert_eq!(flags.watched_ticks.load(Ordering::Relaxed), 7);
    }
}
