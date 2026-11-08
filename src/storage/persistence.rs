//! Persistence of one encoded record on a block device.

use alloc::vec;
use alloc::vec::Vec;
use crate::drivers::{BlockDevice, DiskError, DiskResult, SECTOR_SIZE};
use super::serializer;

pub fn load<D: BlockDevice + ?Sized>(disk: &mut D, start: u64, sectors: u64) -> DiskResult<Option<Vec<u8>>> {
    let bytes = sectors.checked_mul(SECTOR_SIZE as u64).and_then(|n| usize::try_from(n).ok())
        .ok_or(DiskError::OutOfBounds)?;
    let mut raw = vec![0; bytes];
    disk.read_sectors(start, &mut raw)?;
    Ok(serializer::decode(&raw).map(|data| data.to_vec()))
}

pub fn save<D: BlockDevice + ?Sized>(disk: &mut D, start: u64, sectors: u64, data: &[u8]) -> DiskResult<()> {
    let encoded = serializer::encode(data).ok_or(DiskError::OutOfBounds)?;
    let capacity = sectors.checked_mul(SECTOR_SIZE as u64).and_then(|n| usize::try_from(n).ok())
        .ok_or(DiskError::OutOfBounds)?;
    if encoded.len() > capacity { return Err(DiskError::OutOfBounds); }
    let mut raw = vec![0; capacity];
    raw[..encoded.len()].copy_from_slice(&encoded);
    disk.write_sectors(start, &raw)
}

#[cfg(test)]
mod tests {
    use super::{load, save};
    use crate::drivers::MemoryDisk;

    #[test]
    fn save_load_round_trip() {
        let mut disk = MemoryDisk::new(4).unwrap();
        save(&mut disk, 1, 2, b"persistent data").unwrap();
        assert_eq!(load(&mut disk, 1, 2).unwrap().as_deref(), Some(&b"persistent data"[..]));
    }

    #[test]
    fn corrupt_record_is_treated_as_missing() {
        let mut disk = MemoryDisk::new(2).unwrap();
        let mut raw = [0u8; 512];
        raw[0] = 1;
        disk.write_sector(1, &raw).unwrap();
        assert!(load(&mut disk, 1, 1).unwrap().is_none());
    }
}