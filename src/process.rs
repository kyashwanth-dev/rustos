//! Process metadata and address-space boundaries.
//!
//! This is deliberately kernel-only groundwork. It gives future user-mode
//! code a single place to enforce ownership and bounds before page tables are
//! switched per process.

use alloc::string::String;
use alloc::vec::Vec;

pub type ProcessId = u64;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AddressSpace {
    pub code_start: u64,
    pub code_end: u64,
    pub data_start: u64,
    pub data_end: u64,
    pub stack_start: u64,
    pub stack_end: u64,
}

impl AddressSpace {
    pub fn contains_code(&self, address: u64) -> bool {
        self.code_start <= address && address < self.code_end
    }

    pub fn contains_stack(&self, address: u64) -> bool {
        self.stack_start <= address && address < self.stack_end
    }

    pub fn contains_data(&self, address: u64) -> bool {
        self.data_start <= address && address < self.data_end
    }

    pub fn contains_user_range(&self, start: u64, length: u64) -> bool {
        let end = match start.checked_add(length) {
            Some(end) => end,
            None => return false,
        };
        (self.data_start <= start && end <= self.data_end)
            || (self.stack_start <= start && end <= self.stack_end)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProcessState {
    Ready,
    Running,
    Exited(i64),
}

pub struct Process {
    pub id: ProcessId,
    pub name: String,
    pub state: ProcessState,
    pub address_space: AddressSpace,
}

pub struct ProcessTable {
    next_id: ProcessId,
    current: Option<ProcessId>,
    processes: Vec<Process>,
}

impl ProcessTable {
    pub const fn new() -> Self {
        Self { next_id: 1, current: None, processes: Vec::new() }
    }

    pub fn create(&mut self, name: &str, address_space: AddressSpace) -> ProcessId {
        let id = self.next_id;
        self.next_id = self.next_id.saturating_add(1);
        self.processes.push(Process {
            id,
            name: String::from(name),
            state: ProcessState::Ready,
            address_space,
        });
        id
    }

    pub fn switch_to(&mut self, id: ProcessId) -> bool {
        if self.processes.iter().all(|process| process.id != id) {
            return false;
        }
        for process in &mut self.processes {
            if process.state == ProcessState::Running {
                process.state = ProcessState::Ready;
            }
        }
        if let Some(process) = self.processes.iter_mut().find(|process| process.id == id) {
            process.state = ProcessState::Running;
            self.current = Some(id);
            true
        } else {
            false
        }
    }

    pub fn current(&self) -> Option<&Process> {
        self.current.and_then(|id| self.processes.iter().find(|process| process.id == id))
    }

    pub fn get(&self, id: ProcessId) -> Option<&Process> {
        self.processes.iter().find(|process| process.id == id)
    }

    pub fn exit(&mut self, id: ProcessId, code: i64) -> bool {
        if let Some(process) = self.processes.iter_mut().find(|process| process.id == id) {
            process.state = ProcessState::Exited(code);
            if self.current == Some(id) {
                self.current = None;
            }
            true
        } else {
            false
        }
    }
}
