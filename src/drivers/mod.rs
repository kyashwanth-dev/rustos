//! Hardware and virtual block-device drivers.

pub mod ata;
pub mod disk;

pub use disk::{BlockDevice, DiskError, DiskResult, MemoryDisk, SECTOR_SIZE};