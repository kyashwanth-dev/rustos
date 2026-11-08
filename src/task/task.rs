//! Task primitives for the cooperative executor.

use core::task::{Context, Poll};

pub trait Task {
    fn poll(&mut self, cx: &mut Context<'_>) -> Poll<()>;
}