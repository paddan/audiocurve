//! Lock-free triple buffer: one writer thread, one reader thread.
//!
//! `state` holds the middle slot index in bits 0-1 and a dirty flag in bit 2.
//! The writer owns the back slot, the reader owns the front slot; each publishes
//! or acquires by swapping its slot index with the middle one (AcqRel).

use std::cell::UnsafeCell;
use std::sync::atomic::{AtomicU8, Ordering};

const DIRTY: u8 = 0b100;

pub struct TripleBuffer<T: Copy> {
    slots: [UnsafeCell<T>; 3],
    state: AtomicU8,
    /// Back slot index; touched only by the writer.
    back: AtomicU8,
    /// Front slot index; touched only by the reader.
    front: AtomicU8,
}

// SAFETY: contract is exactly one writer thread calling `write` and exactly one
// reader thread calling `read`. The index swap through `state` hands each slot to
// exactly one side at a time, so no slot is accessed concurrently; `back`/`front`
// are each used by a single thread (atomics only to keep the type Sync).
unsafe impl<T: Copy + Send> Sync for TripleBuffer<T> {}

impl<T: Copy> TripleBuffer<T> {
    pub fn new(initial: T) -> Self {
        Self {
            slots: [UnsafeCell::new(initial), UnsafeCell::new(initial), UnsafeCell::new(initial)],
            state: AtomicU8::new(1), // middle = 1, clean
            back: AtomicU8::new(2),
            front: AtomicU8::new(0),
        }
    }

    /// Single writer only. Never blocks.
    pub fn write(&self, value: T) {
        let back = self.back.load(Ordering::Relaxed);
        // SAFETY: the writer exclusively owns the back slot.
        unsafe { *self.slots[back as usize].get() = value };
        let prev = self.state.swap(back | DIRTY, Ordering::AcqRel);
        self.back.store(prev & 3, Ordering::Relaxed);
    }

    /// Single reader only. Returns the most recently written value. Never blocks.
    pub fn read(&self) -> T {
        let mut front = self.front.load(Ordering::Relaxed);
        if self.state.load(Ordering::Relaxed) & DIRTY != 0 {
            let prev = self.state.swap(front, Ordering::AcqRel);
            front = prev & 3;
            self.front.store(front, Ordering::Relaxed);
        }
        // SAFETY: the reader exclusively owns the front slot.
        unsafe { *self.slots[front as usize].get() }
    }
}
