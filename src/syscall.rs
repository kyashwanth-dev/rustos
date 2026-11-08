//! Syscall ABI validation and dispatch.
//!
//! The IDT gate is installed for ring-3 callers. The dispatcher remains
//! explicit about the process table so user-memory access can never bypass
//! ownership and bounds checks.

use crate::process::{ProcessId, ProcessState, ProcessTable};
use core::sync::atomic::{AtomicU64, Ordering};

pub const SYS_EXIT: u64 = 1;
pub const SYS_WRITE: u64 = 2;
pub const SYS_GETPID: u64 = 3;
pub const SYS_YIELD: u64 = 4;

static SYSCALL_ENTRIES: AtomicU64 = AtomicU64::new(0);

pub fn record_entry() {
    SYSCALL_ENTRIES.fetch_add(1, Ordering::Relaxed);
}

pub fn entry_count() -> u64 {
    SYSCALL_ENTRIES.load(Ordering::Relaxed)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SyscallError {
    Unknown,
    NoSuchProcess,
    InvalidPointer,
}

pub type SyscallResult = Result<u64, SyscallError>;

pub fn dispatch(
    processes: &mut ProcessTable,
    process_id: ProcessId,
    number: u64,
    arg0: u64,
    arg1: u64,
    _arg2: u64,
) -> SyscallResult {
    let process = processes.get(process_id).ok_or(SyscallError::NoSuchProcess)?;
    if matches!(process.state, ProcessState::Exited(_)) && number != SYS_EXIT {
        return Err(SyscallError::NoSuchProcess);
    }
    match number {
        SYS_EXIT => {
            let code = arg0 as i64;
            processes.exit(process_id, code);
            Ok(0)
        }
        SYS_WRITE => {
            if !process.address_space.contains_user_range(arg0, arg1) {
                return Err(SyscallError::InvalidPointer);
            }
            Ok(0)
        }
        SYS_GETPID => {
            Ok(process_id)
        }
        SYS_YIELD => {
            Ok(0)
        }
        _ => Err(SyscallError::Unknown),
    }
}

#[cfg(test)]
mod tests {
    use super::{dispatch, SyscallError, SYS_EXIT, SYS_WRITE};
    use crate::process::{AddressSpace, ProcessTable};

    fn table_with_process() -> (ProcessTable, u64) {
        let mut table = ProcessTable::new();
        let id = table.create("test", AddressSpace::kernel_placeholder());
        (table, id)
    }

    #[test]
    fn write_accepts_data_and_stack_ranges() {
        let (mut table, id) = table_with_process();
        let space = table.get(id).unwrap().address_space;
        assert_eq!(dispatch(&mut table, id, SYS_WRITE, space.data_start, 8), Ok(0));
        assert_eq!(dispatch(&mut table, id, SYS_WRITE, space.stack_start, 8), Ok(0));
    }

    #[test]
    fn write_rejects_kernel_and_overflow_ranges() {
        let (mut table, id) = table_with_process();
        let space = table.get(id).unwrap().address_space;
        assert_eq!(
            dispatch(&mut table, id, SYS_WRITE, space.code_start, 1),
            Err(SyscallError::InvalidPointer)
        );
        assert_eq!(
            dispatch(&mut table, id, SYS_WRITE, u64::MAX - 1, 4),
            Err(SyscallError::InvalidPointer)
        );
    }

    #[test]
    fn exit_marks_process_and_unknown_calls_fail() {
        let (mut table, id) = table_with_process();
        assert_eq!(dispatch(&mut table, id, SYS_EXIT, 12, 0, 0), Ok(0));
        assert_eq!(dispatch(&mut table, id, 99, 0, 0, 0), Err(SyscallError::Unknown));
    }

    #[test]
    fn getpid_and_yield_require_a_live_process() {
        let (mut table, id) = table_with_process();
        assert_eq!(dispatch(&mut table, id, super::SYS_GETPID, 0, 0, 0), Ok(id));
        assert_eq!(dispatch(&mut table, id, super::SYS_YIELD, 0, 0, 0), Ok(0));
        table.exit(id, 0);
        assert_eq!(dispatch(&mut table, id, super::SYS_GETPID, 0, 0, 0), Err(SyscallError::NoSuchProcess));
    }
}
