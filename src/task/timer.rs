//! Timer-backed futures for cooperative tasks.

use core::future::Future;
use core::pin::Pin;
use core::task::{Context, Poll};
use super::ticks;

pub struct Sleep { wake_at: u64 }

impl Sleep {
    pub fn new(seconds: u64) -> Self {
        Self { wake_at: ticks().saturating_add(seconds.saturating_mul(18)) }
    }
}

impl Future for Sleep {
    type Output = ();

    fn poll(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<()> {
        if ticks() >= self.wake_at { Poll::Ready(()) } else { Poll::Pending }
    }
}

pub fn sleep(seconds: u64) -> Sleep { Sleep::new(seconds) }