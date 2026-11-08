//! Async-compatible access to keyboard events.

use core::future::Future;
use core::pin::Pin;
use core::task::{Context, Poll};
use crate::input;
use crate::keyboard::Key;

pub struct NextKey;

impl Future for NextKey {
    type Output = Key;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Key> {
        match input::pop().and_then(Key::from_byte) {
            Some(key) => Poll::Ready(key),
            None => {
                cx.waker().wake_by_ref();
                Poll::Pending
            }
        }
    }
}

pub fn next_key() -> NextKey { NextKey }