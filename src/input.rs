//! A tiny lock-free ring buffer connecting the keyboard interrupt handler
//! (the only producer) to the shell in the main loop (the only consumer).
//! Items are packed keys (see `Key::to_byte`).

use core::sync::atomic::{AtomicU8, AtomicUsize, Ordering};

const SIZE: usize = 128;
const ZERO: AtomicU8 = AtomicU8::new(0);
static BUF: [AtomicU8; SIZE] = [ZERO; SIZE];
static HEAD: AtomicUsize = AtomicUsize::new(0); // next slot to write
static TAIL: AtomicUsize = AtomicUsize::new(0); // next slot to read

/// Called from the interrupt handler. Drops the key if the buffer is full.
pub fn push(b: u8) {
    let head = HEAD.load(Ordering::Relaxed);
    let next = (head + 1) % SIZE;
    if next == TAIL.load(Ordering::Acquire) {
        return; // full
    }
    BUF[head].store(b, Ordering::Relaxed);
    HEAD.store(next, Ordering::Release);
}

/// Called from the main loop.
pub fn pop() -> Option<u8> {
    let tail = TAIL.load(Ordering::Relaxed);
    if tail == HEAD.load(Ordering::Acquire) {
        return None; // empty
    }
    let b = BUF[tail].load(Ordering::Relaxed);
    TAIL.store((tail + 1) % SIZE, Ordering::Release);
    Some(b)
}
