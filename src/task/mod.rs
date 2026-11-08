//! Cooperative task bookkeeping for RustOS.
//! Tasks advance only when the kernel calls `task::tick()` from its main loop.

pub mod executor;
pub mod keyboard_stream;
pub mod task;
pub mod timer;

use alloc::string::String;
use alloc::vec::Vec;
use alloc::boxed::Box;
use core::future::Future;
use core::pin::Pin;
use core::task::{Context, RawWaker, RawWakerVTable, Waker};
use core::sync::atomic::{AtomicU64, Ordering};

static NOW: AtomicU64 = AtomicU64::new(0);
static NEXT_ID: AtomicU64 = AtomicU64::new(1);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TaskState { Sleeping, Ready, Running, Finished, Cancelled }

#[derive(Clone, Debug)]
pub struct TaskInfo {
    pub id: u64,
    pub name: String,
    pub state: TaskState,
    pub wake_at: u64,
}

struct ScheduledTask {
    info: TaskInfo,
    future: Pin<Box<dyn Future<Output = ()>>>,
}

pub struct Scheduler { tasks: Vec<ScheduledTask> }

impl Scheduler {
    pub const fn new() -> Self { Self { tasks: Vec::new() } }

    pub fn spawn_sleep(&mut self, name: &str, seconds: u64) -> u64 {
        let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
        let ticks = seconds.saturating_mul(18);
        let wake_at = NOW.load(Ordering::Relaxed).saturating_add(ticks);
        self.tasks.push(ScheduledTask {
            info: TaskInfo {
                id,
                name: String::from(name),
                state: if ticks == 0 { TaskState::Ready } else { TaskState::Sleeping },
                wake_at,
            },
            future: Box::pin(timer::sleep(seconds)),
        });
        id
    }

    /// Called from the main loop, never from an interrupt handler.
    pub fn poll(&mut self) {
        let now = NOW.load(Ordering::Relaxed);
        let waker = noop_waker();
        let mut cx = Context::from_waker(&waker);
        for task in self.tasks.iter_mut() {
            if matches!(task.info.state, TaskState::Finished | TaskState::Cancelled) {
                continue;
            }
            task.info.state = TaskState::Running;
            if task.future.as_mut().poll(&mut cx).is_ready() {
                task.info.state = TaskState::Finished;
            } else if now >= task.info.wake_at {
                task.info.state = TaskState::Ready;
            } else {
                task.info.state = TaskState::Sleeping;
            }
        }
    }

    pub fn list(&self) -> impl Iterator<Item = &TaskInfo> {
        self.tasks.iter().map(|task| &task.info)
    }

    pub fn remove(&mut self, id: u64) -> bool {
        if let Some(i) = self.tasks.iter().position(|t| t.info.id == id) {
            self.tasks.remove(i);
            true
        } else { false }
    }

    pub fn cancel(&mut self, id: u64) -> bool {
        if let Some(task) = self.tasks.iter_mut().find(|t| t.info.id == id) {
            if matches!(task.info.state, TaskState::Finished | TaskState::Cancelled) {
                return false;
            }
            task.info.state = TaskState::Cancelled;
            true
        } else { false }
    }

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

/// Incremented by the timer IRQ. It does not acquire locks or allocate.
pub fn on_timer_tick() { NOW.fetch_add(1, Ordering::Relaxed); }
pub fn ticks() -> u64 { NOW.load(Ordering::Relaxed) }
pub fn seconds() -> u64 { ticks() / 18 }
pub fn tick() {}

#[cfg(test)]
mod tests {
    use super::{Scheduler, TaskState};

    #[test]
    fn zero_length_task_finishes_when_polled() {
        let mut scheduler = Scheduler::new();
        let id = scheduler.spawn_sleep("test", 0);
        scheduler.poll();
        assert_eq!(scheduler.list().find(|task| task.id == id).unwrap().state, TaskState::Finished);
        assert!(!scheduler.cancel(id));
    }
}
