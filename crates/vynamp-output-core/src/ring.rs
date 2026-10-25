//! Single-producer single-consumer byte ring between the device thread
//! (producer, RT) and the capture writer thread (consumer, non-RT).
//!
//! RT module: this file holds only the producer side and uses only atomics.
//! Construction and the consumer side live in `capture.rs`. Positions grow
//! without bound (wrapping) and the capacity is a power of two, so a slot is
//! `position & mask`. A period that does not fit is dropped whole and
//! counted in `overruns`: the device thread never waits for the writer.

use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU64, AtomicUsize, Ordering};

use crate::driver::{NativeBuf, PeriodSink};

/// The shared ring. Fields are crate-visible for the consumer in `capture.rs`.
#[derive(Debug)]
pub(crate) struct ByteRing {
    pub(crate) slots: Box<[AtomicU8]>,
    /// `slots.len() - 1`; `slots.len()` is a power of two.
    pub(crate) mask: usize,
    /// Total bytes written (producer).
    pub(crate) head: AtomicUsize,
    /// Total bytes read (consumer).
    pub(crate) tail: AtomicUsize,
    /// Periods dropped because the ring was full.
    pub(crate) overruns: AtomicU64,
    /// Set by `stop` after the device thread is joined: drain and finish.
    pub(crate) finish: AtomicBool,
}

/// Sequential writer over reserved ring space.
struct Writer<'a> {
    slots: &'a [AtomicU8],
    mask: usize,
    pos: usize,
}

impl Writer<'_> {
    fn put(&mut self, bytes: &[u8]) {
        for &b in bytes {
            if let Some(slot) = self.slots.get(self.pos & self.mask) {
                slot.store(b, Ordering::Relaxed);
            }
            self.pos = self.pos.wrapping_add(1);
        }
    }
}

/// Bytes of one period on the device (little-endian containers).
fn period_bytes(p: &NativeBuf) -> usize {
    match p {
        NativeBuf::F32(b) => b.len() * 4,
        NativeBuf::F64(b) => b.len() * 8,
        NativeBuf::I16(b) => b.len() * 2,
        NativeBuf::I24Packed(b) => b.len() * 3,
        NativeBuf::I32(b) => b.len() * 4,
        NativeBuf::U8(b) => b.len(),
    }
}

impl ByteRing {
    /// Appends the period's bytes (little-endian samples, interleaved) when
    /// it fits entirely; returns false (nothing written) otherwise.
    pub(crate) fn push_period(&self, p: &NativeBuf) -> bool {
        let len = period_bytes(p);
        let head = self.head.load(Ordering::Relaxed);
        let tail = self.tail.load(Ordering::Acquire);
        let used = head.wrapping_sub(tail);
        if self.slots.len().saturating_sub(used) < len {
            return false;
        }
        let mut w = Writer {
            slots: &self.slots,
            mask: self.mask,
            pos: head,
        };
        match p {
            NativeBuf::F32(b) => b.iter().for_each(|s| w.put(&s.to_le_bytes())),
            NativeBuf::F64(b) => b.iter().for_each(|s| w.put(&s.to_le_bytes())),
            NativeBuf::I16(b) => b.iter().for_each(|s| w.put(&s.to_le_bytes())),
            NativeBuf::I24Packed(b) => b.iter().for_each(|s| w.put(s)),
            NativeBuf::I32(b) => b.iter().for_each(|s| w.put(&s.to_le_bytes())),
            NativeBuf::U8(b) => w.put(b),
        }
        self.head.store(w.pos, Ordering::Release);
        true
    }
}

/// Period sink of the Capture device.
#[derive(Debug)]
pub(crate) struct RingSink(pub(crate) std::sync::Arc<ByteRing>);

impl PeriodSink for RingSink {
    fn deliver(&mut self, period: &NativeBuf) {
        if !self.0.push_period(period) {
            self.0.overruns.fetch_add(1, Ordering::Release);
        }
    }
}
