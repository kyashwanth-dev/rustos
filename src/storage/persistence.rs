//! Persistence of one encoded record on a block device.

use alloc::vec;
use alloc::vec::Vec;
use crate::drivers::{BlockDevice, DiskError, DiskResult, SECTOR_SIZE};
use super::serializer;

pub fn load<D: BlockDevice>(disk: &mut D, start: u64, sectors: u64) -> DiskResult<Option<Vec<u8>>> {
    let bytes = sectors.checked_mul(SECTOR_SIZE as u64).and_then(|n| usize::try_from(n).ok())
        .ok_or(DiskError::OutOfBounds)?;
    let mut raw = vec![0; bytes];
    disk.read_sectors(start, &mut raw)?;
    Ok(serializer::decode(&raw).map(|data| data.to_vec()))
}

pub fn save<D: BlockDevice>(disk: &mut D, start: u64, sectors: u64, data: &[u8]) -> DiskResult<()> {
    let encoded = serializer::encode(data).ok_or(DiskError::OutOfBounds)?;
    let capacity = sectors.checked_mul(SECTOR_SIZE as u64).and_then(|n| usize::try_from(n).ok())
        .ok_or(DiskError::OutOfBounds)?;
    if encoded.len() > capacity { return Err(DiskError::OutOfBounds); }
    let mut raw = vec![0; capacity];
    raw[..encoded.len()].copy_from_slice(&encoded);
    disk.write_sectors(start, &raw)
}