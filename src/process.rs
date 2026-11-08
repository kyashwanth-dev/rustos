//! Process metadata and address-space boundaries.
//!
//! This is deliberately kernel-only groundwork. It gives future user-mode
//! code a single place to enforce ownership and bounds before page tables are
//! switched per process.

use alloc::string::String;
use alloc::vec::Vec;

pub type ProcessId = u64;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UserImage {
    pub entry_point: u64,
    pub code_size: u64,
    pub data_size: u64,
    pub stack_size: u64,
}

impl UserImage {
    pub const fn demo() -> Self {
        Self {
            entry_point: 0x0040_0000,
            code_size: 4096,
            data_size: 4096,
            stack_size: 4096 * 2,
        }
    }

    pub fn address_space(&self) -> Option<AddressSpace> {
        let code_end = self.entry_point.checked_add(self.code_size)?;
        let data_start = align_up(code_end)?;
        let data_end = data_start.checked_add(self.data_size)?;
        let stack_start = align_up(data_end)?;
        let stack_end = stack_start.checked_add(self.stack_size)?;
        let address_space = AddressSpace {
            code_start: self.entry_point,
            code_end,
            data_start,
            data_end,
            stack_start,
            stack_end,
        };
        address_space.is_valid().then_some(address_space)
    }
}

fn align_up(value: u64) -> Option<u64> {
    value.checked_add(0xfff).map(|aligned| aligned & !0xfff)
}

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
    pub const fn kernel_placeholder() -> Self {
        Self {
            code_start: 0x0040_0000,
            code_end: 0x0080_0000,
            data_start: 0x0080_0000,
            data_end: 0x0100_0000,
            stack_start: 0x0100_0000,
            stack_end: 0x0120_0000,
        }
    }

    pub fn is_valid(&self) -> bool {
        self.code_start < self.code_end
            && self.code_end <= self.data_start
            && self.data_start < self.data_end
            && self.data_end <= self.stack_start
            && self.stack_start < self.stack_end
    }

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
    pub entry_point: u64,
    pub stack_pointer: u64,
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
            entry_point: address_space.code_start,
            stack_pointer: address_space.stack_end,
        });
        id
    }

    pub fn create_user(&mut self, name: &str, image: UserImage) -> Option<ProcessId> {
        let address_space = image.address_space()?;
        let id = self.next_id;
        self.next_id = self.next_id.saturating_add(1);
        self.processes.push(Process {
            id,
            name: String::from(name),
            state: ProcessState::Ready,
            address_space,
            entry_point: image.entry_point,
            stack_pointer: address_space.stack_end,
        });
        Some(id)
    }

    pub fn switch_to(&mut self, id: ProcessId) -> bool {
        if self.processes.iter().all(|process| process.id != id) {
            return false;
        }
        if self.processes.iter().any(|process| {
            process.id == id && matches!(process.state, ProcessState::Exited(_))
        }) {
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

    pub fn list(&self) -> impl Iterator<Item = &Process> {
        self.processes.iter()
    }

    pub fn exit(&mut self, id: ProcessId, code: i64) -> bool {
        if let Some(process) = self.processes.iter_mut().find(|process| process.id == id) {
            if matches!(process.state, ProcessState::Exited(_)) {
                return false;
            }
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

#[cfg(test)]
mod tests {
    use super::{AddressSpace, ProcessState, ProcessTable};

    #[test]
    fn user_ranges_must_fit_one_region() {
        let space = AddressSpace::kernel_placeholder();
        assert!(space.is_valid());
        assert!(space.contains_user_range(space.data_start, 4));
        assert!(space.contains_user_range(space.stack_start, 0));
        assert!(!space.contains_user_range(space.code_start, 4));
        assert!(!space.contains_user_range(space.data_end - 2, 4));
        assert!(!space.contains_user_range(u64::MAX - 1, 4));
    }

    #[test]
    fn switching_and_exit_update_process_lifecycle() {
        let mut table = ProcessTable::new();
        let id = table.create("test", AddressSpace::kernel_placeholder());
        assert_eq!(table.get(id).unwrap().state, ProcessState::Ready);
        assert!(table.switch_to(id));
        assert_eq!(table.current().unwrap().id, id);
        assert!(table.exit(id, 7));
        assert_eq!(table.get(id).unwrap().state, ProcessState::Exited(7));
        assert!(table.current().is_none());
        assert!(!table.exit(id, 8));
        assert!(!table.switch_to(id));
    }

    #[test]
    fn user_images_produce_non_overlapping_aligned_regions() {
        let image = UserImage::demo();
        let space = image.address_space().unwrap();
        assert_eq!(space.data_start % 4096, 0);
        assert_eq!(space.stack_start % 4096, 0);
        assert!(space.contains_code(image.entry_point));
        assert!(space.contains_stack(space.stack_end - 1));
    }

    #[test]
    fn invalid_user_image_is_rejected() {
        let mut table = ProcessTable::new();
        assert!(table.create_user("bad", UserImage {
            entry_point: u64::MAX,
            code_size: 1,
            data_size: 1,
            stack_size: 1,
        }).is_none());
    }
}
