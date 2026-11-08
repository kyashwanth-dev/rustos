//! Minimal single-threaded cooperative executor.

use alloc::boxed::Box;
use alloc::vec::Vec;
use core::future::Future;
use core::pin::Pin;
use core::task::{Context, RawWaker, RawWakerVTable, Waker};

pub struct Executor {
    tasks: Vec<Pin<Box<dyn Future<Output = ()>>>>,
}

impl Executor {
    pub const fn new() -> Self { Self { tasks: Vec::new() } }

    pub fn spawn<F>(&mut self, future: F)
    where
        F: Future<Output = ()> + 'static,
    {
        self.tasks.push(Box::pin(future));
    }

    pub fn poll(&mut self) {
        let waker = noop_waker();
        let mut cx = Context::from_waker(&waker);
        let mut index = 0;
        while index < self.tasks.len() {
            if self.tasks[index].as_mut().poll(&mut cx).is_ready() {
                let _ = self.tasks.remove(index);
            } else {
                index += 1;
            }
        }
    }

    pub fn is_empty(&self) -> bool { self.tasks.is_empty() }
}

const VTABLE: RawWakerVTable = RawWakerVTable::new(
    |_| RawWaker::new(core::ptr::null(), &VTABLE),
    |_| {},
    |_| {},
    |_| {},
);

fn noop_waker() -> Waker {
    unsafe { Waker::from_raw(RawWaker::new(core::ptr::null(), &VTABLE)) }
}