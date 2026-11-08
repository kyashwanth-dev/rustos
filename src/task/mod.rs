//! Cooperative task bookkeeping for RustOS.
//! Tasks advance only when the kernel calls `task::tick()` from its main loop.

pub mod executor;
pub mod keyboard_stream;
pub mod task;
pub mod timer;

use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};

static NOW: AtomicU64 = AtomicU64::new(0);
static NEXT_ID: AtomicU64 = AtomicU64::new(1);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TaskState { Sleeping, Ready, Finished }

#[derive(Clone, Debug)]
pub struct TaskInfo {
    pub id: u64,
    pub name: String,
    pub state: TaskState,
    pub wake_at: u64,
}

pub struct Scheduler { tasks: Vec<TaskInfo> }

impl Scheduler {
    pub const fn new() -> Self { Self { tasks: Vec::new() } }

    pub fn spawn_sleep(&mut self, name: &str, seconds: u64) -> u64 {
        let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
        let ticks = seconds.saturating_mul(18);
        let wake_at = NOW.load(Ordering::Relaxed).saturating_add(ticks);
        self.tasks.push(TaskInfo {
            id,
            name: String::from(name),
            state: if ticks == 0 { TaskState::Ready } else { TaskState::Sleeping },
            wake_at,
        });
        id
    }

    /// Called from the main loop, never from an interrupt handler.
    pub fn poll(&mut self) {
        let now = NOW.load(Ordering::Relaxed);
        for task in self.tasks.iter_mut() {
            if task.state == TaskState::Sleeping && now >= task.wake_at {
                task.state = TaskState::Ready;
            }
        }
    }

    pub fn list(&self) -> &[TaskInfo] { &self.tasks }

    pub fn remove(&mut self, id: u64) -> bool {
        if let Some(i) = self.tasks.iter().position(|t| t.id == id) {
            self.tasks.remove(i);
            true
        } else { false }
    }

    pub fn cancel(&mut self, id: u64) -> bool {
        if let Some(task) = self.tasks.iter_mut().find(|t| t.id == id) {
            task.state = TaskState::Finished;
            true
        } else { false }
    }
}

/// Incremented by the timer IRQ. It does not acquire locks or allocate.
pub fn on_timer_tick() { NOW.fetch_add(1, Ordering::Relaxed); }
pub fn ticks() -> u64 { NOW.load(Ordering::Relaxed) }
pub fn seconds() -> u64 { ticks() / 18 }
pub fn tick() {}
