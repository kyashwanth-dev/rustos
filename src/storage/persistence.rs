//! Crash-safe persistence of one encoded record on a block device.
//!
//! Records are written to alternating journal slots. The commit byte is
//! written only after the payload and checksum have reached the device, so a
//! reboot can select the newest complete slot.

use alloc::vec;
use alloc::vec::Vec;
use crate::drivers::{BlockDevice, DiskError, DiskResult, SECTOR_SIZE};
use super::serializer;

const JOURNAL_MAGIC: &[u8; 4] = b"JNL1";
const JOURNAL_HEADER: usize = 24;

pub fn load<D: BlockDevice + ?Sized>(disk: &mut D, start: u64, sectors: u64) -> DiskResult<Option<Vec<u8>>> {
    if sectors >= 2 {
        let slot_sectors = sectors / 2;
        let first = read_slot(disk, start, slot_sectors, 0)?;
        let second = read_slot(disk, start, slot_sectors, 1)?;
        let journal = match (first, second) {
            (Some((a, first)), Some((b, second))) => {
                Some(if a >= b { first } else { second })
            }
            (Some((_, data)), None) | (None, Some((_, data))) => Some(data),
            (None, None) => None,
        };
        if journal.is_some() {
            return Ok(journal);
        }
        let raw = read_raw(disk, start, sectors)?;
        return Ok(serializer::decode(&raw).map(|data| data.to_vec()));
    }

    let raw = read_raw(disk, start, sectors)?;
    Ok(serializer::decode(&raw).map(|data| data.to_vec()))
}

pub fn save<D: BlockDevice + ?Sized>(disk: &mut D, start: u64, sectors: u64, data: &[u8]) -> DiskResult<()> {
    if sectors >= 2 {
        let slot_sectors = sectors / 2;
        let current = load(disk, start, sectors)?;
        let generation = match current {
            Some(_) => newest_generation(disk, start, slot_sectors)?.saturating_add(1),
            None => 1,
        };
        let slot = if generation % 2 == 0 { 1 } else { 0 };
        return write_slot(disk, start, slot_sectors, slot, generation, data);
    }

    let encoded = serializer::encode(data).ok_or(DiskError::OutOfBounds)?;
    let capacity = sectors.checked_mul(SECTOR_SIZE as u64).and_then(|n| usize::try_from(n).ok())
        .ok_or(DiskError::OutOfBounds)?;
    if encoded.len() > capacity { return Err(DiskError::OutOfBounds); }
    let mut raw = vec![0; capacity];
    raw[..encoded.len()].copy_from_slice(&encoded);
    disk.write_sectors(start, &raw)
}

fn read_raw<D: BlockDevice + ?Sized>(disk: &mut D, start: u64, sectors: u64) -> DiskResult<Vec<u8>> {
    let bytes = sectors.checked_mul(SECTOR_SIZE as u64).and_then(|n| usize::try_from(n).ok())
        .ok_or(DiskError::OutOfBounds)?;
    let mut raw = vec![0; bytes];
    disk.read_sectors(start, &mut raw)?;
    Ok(raw)
}

fn slot_start(start: u64, slot_sectors: u64, slot: u64) -> DiskResult<u64> {
    start.checked_add(slot.checked_mul(slot_sectors).ok_or(DiskError::OutOfBounds)?)
        .ok_or(DiskError::OutOfBounds)
}

fn read_slot<D: BlockDevice + ?Sized>(
    disk: &mut D,
    start: u64,
    slot_sectors: u64,
    slot: u64,
) -> DiskResult<Option<(u64, Vec<u8>)>> {
    let raw = read_raw(disk, slot_start(start, slot_sectors, slot)?, slot_sectors)?;
    if raw.len() < JOURNAL_HEADER || &raw[..4] != JOURNAL_MAGIC || raw[5] != 1 {
        return Ok(None);
    }
    let generation = u64::from_le_bytes(raw[8..16].try_into().map_err(|_| DiskError::InvalidBuffer)?);
    let length = u32::from_le_bytes(raw[16..20].try_into().map_err(|_| DiskError::InvalidBuffer)?) as usize;
    let checksum = u32::from_le_bytes(raw[20..24].try_into().map_err(|_| DiskError::InvalidBuffer)?);
    let end = match JOURNAL_HEADER.checked_add(length) {
        Some(end) => end,
        None => return Ok(None),
    };
    let data = match raw.get(JOURNAL_HEADER..end) {
        Some(data) => data,
        None => return Ok(None),
    };
    if serializer::checksum(data) != checksum {
        return Ok(None);
    }
    Ok(Some((generation, data.to_vec())))
}

fn newest_generation<D: BlockDevice + ?Sized>(
    disk: &mut D,
    start: u64,
    slot_sectors: u64,
) -> DiskResult<u64> {
    let first = read_slot(disk, start, slot_sectors, 0)?;
    let second = read_slot(disk, start, slot_sectors, 1)?;
    Ok(first.map(|(generation, _)| generation).unwrap_or(0)
        .max(second.map(|(generation, _)| generation).unwrap_or(0)))
}

fn write_slot<D: BlockDevice + ?Sized>(
    disk: &mut D,
    start: u64,
    slot_sectors: u64,
    slot: u64,
    generation: u64,
    data: &[u8],
) -> DiskResult<()> {
    let capacity = slot_sectors.checked_mul(SECTOR_SIZE as u64)
        .and_then(|n| usize::try_from(n).ok()).ok_or(DiskError::OutOfBounds)?;
    let end = JOURNAL_HEADER.checked_add(data.len()).ok_or(DiskError::OutOfBounds)?;
    if end > capacity || data.len() > u32::MAX as usize {
        return Err(DiskError::OutOfBounds);
    }

    let mut raw = vec![0; capacity];
    raw[..4].copy_from_slice(JOURNAL_MAGIC);
    raw[4] = 1;
    raw[5] = 0;
    raw[8..16].copy_from_slice(&generation.to_le_bytes());
    raw[16..20].copy_from_slice(&(data.len() as u32).to_le_bytes());
    raw[20..24].copy_from_slice(&serializer::checksum(data).to_le_bytes());
    raw[JOURNAL_HEADER..end].copy_from_slice(data);

    let base = slot_start(start, slot_sectors, slot)?;
    for (index, sector) in raw.chunks_exact(SECTOR_SIZE).enumerate().skip(1) {
        disk.write_sector(base + index as u64, sector)?;
    }
    disk.write_sector(base, &raw[..SECTOR_SIZE])?;

    raw[5] = 1;
    disk.write_sector(base, &raw[..SECTOR_SIZE])
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

    #[test]
    fn journal_keeps_previous_generation_when_new_slot_is_incomplete() {
        let mut disk = MemoryDisk::new(8).unwrap();
        save(&mut disk, 1, 4, b"first").unwrap();
        save(&mut disk, 1, 4, b"second").unwrap();

        let mut torn = [0u8; 512];
        torn[..4].copy_from_slice(b"JNL1");
        torn[4] = 1;
        torn[5] = 0;
        disk.write_sector(1, &torn).unwrap();

        assert_eq!(load(&mut disk, 1, 4).unwrap().as_deref(), Some(&b"second"[..]));
    }

    #[test]
    fn journal_loads_newest_valid_generation() {
        let mut disk = MemoryDisk::new(8).unwrap();
        save(&mut disk, 1, 4, b"first").unwrap();
        save(&mut disk, 1, 4, b"second").unwrap();
        assert_eq!(load(&mut disk, 1, 4).unwrap().as_deref(), Some(&b"second"[..]));
    }
}