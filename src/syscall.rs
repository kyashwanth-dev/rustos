//! Syscall ABI groundwork.
//!
//! The dispatcher validates process ownership before a future implementation
//! can access user memory or perform privileged operations.

use crate::process::{ProcessId, ProcessTable};

pub const SYS_EXIT: u64 = 1;
pub const SYS_WRITE: u64 = 2;

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
    match number {
        SYS_EXIT => {
            if processes.get(process_id).is_none() {
                return Err(SyscallError::NoSuchProcess);
            }
            let code = arg0 as i64;
            processes.exit(process_id, code);
            Ok(0)
        }
        SYS_WRITE => {
            let process = processes.get(process_id).ok_or(SyscallError::NoSuchProcess)?;
            if !process.address_space.contains_user_range(arg0, arg1) {
                return Err(SyscallError::InvalidPointer);
            }
            Ok(0)
        }
        _ => Err(SyscallError::Unknown),
    }
}
