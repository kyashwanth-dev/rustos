//! Persistent storage formats and block-device persistence helpers.

pub mod persistence;
pub mod serializer;

pub use persistence::{load, save};

use crate::drivers::{BlockDevice, DiskError, DiskResult};
use crate::fs::FileSystem;

pub fn load_filesystem<D: BlockDevice + ?Sized>(
    disk: &mut D,
    start: u64,
    sectors: u64,
) -> DiskResult<Option<FileSystem>> {
    match load(disk, start, sectors)? {
        Some(bytes) => FileSystem::from_snapshot(&bytes)
            .map(Some)
            .map_err(|_| DiskError::InvalidBuffer),
        None => Ok(None),
    }
}

pub fn save_filesystem<D: BlockDevice + ?Sized>(
    disk: &mut D,
    start: u64,
    sectors: u64,
    fs: &FileSystem,
) -> DiskResult<()> {
    save(disk, start, sectors, &fs.snapshot())
}