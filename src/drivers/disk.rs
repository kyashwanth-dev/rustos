//! Block-device abstractions used by storage.

use alloc::vec;
use alloc::vec::Vec;

pub const SECTOR_SIZE: usize = 512;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DiskError {
    OutOfBounds,
    InvalidBuffer,
    ReadOnly,
    DeviceUnavailable,
}

pub type DiskResult<T> = Result<T, DiskError>;

pub trait BlockDevice {
    fn sector_count(&self) -> u64;
    fn read_sector(&mut self, sector: u64, buffer: &mut [u8]) -> DiskResult<()>;
    fn write_sector(&mut self, sector: u64, buffer: &[u8]) -> DiskResult<()>;

    fn read_sectors(&mut self, start: u64, buffer: &mut [u8]) -> DiskResult<()> {
        if buffer.len() % SECTOR_SIZE != 0 {
            return Err(DiskError::InvalidBuffer);
        }
        for (index, chunk) in buffer.chunks_exact_mut(SECTOR_SIZE).enumerate() {
            let sector = start.checked_add(index as u64).ok_or(DiskError::OutOfBounds)?;
            self.read_sector(sector, chunk)?;
        }
        Ok(())
    }

    fn write_sectors(&mut self, start: u64, buffer: &[u8]) -> DiskResult<()> {
        if buffer.len() % SECTOR_SIZE != 0 {
            return Err(DiskError::InvalidBuffer);
        }
        for (index, chunk) in buffer.chunks_exact(SECTOR_SIZE).enumerate() {
            let sector = start.checked_add(index as u64).ok_or(DiskError::OutOfBounds)?;
            self.write_sector(sector, chunk)?;
        }
        Ok(())
    }
}

/// Heap-backed disk used during early boot and for tests.
pub struct MemoryDisk {
    data: Vec<u8>,
    read_only: bool,
}

impl MemoryDisk {
    pub fn new(sectors: u64) -> DiskResult<Self> {
        let bytes = sectors
            .checked_mul(SECTOR_SIZE as u64)
            .and_then(|n| usize::try_from(n).ok())
            .ok_or(DiskError::OutOfBounds)?;
        Ok(Self { data: vec![0; bytes], read_only: false })
    }

    pub fn read_only(sectors: u64) -> DiskResult<Self> {
        let mut disk = Self::new(sectors)?;
        disk.read_only = true;
        Ok(disk)
    }

    fn range(&self, sector: u64) -> DiskResult<core::ops::Range<usize>> {
        let start = sector
            .checked_mul(SECTOR_SIZE as u64)
            .and_then(|n| usize::try_from(n).ok())
            .ok_or(DiskError::OutOfBounds)?;
        let end = start.checked_add(SECTOR_SIZE).ok_or(DiskError::OutOfBounds)?;
        if end > self.data.len() { Err(DiskError::OutOfBounds) } else { Ok(start..end) }
    }
}

impl BlockDevice for MemoryDisk {
    fn sector_count(&self) -> u64 { (self.data.len() / SECTOR_SIZE) as u64 }

    fn read_sector(&mut self, sector: u64, buffer: &mut [u8]) -> DiskResult<()> {
        if buffer.len() != SECTOR_SIZE { return Err(DiskError::InvalidBuffer); }
        let range = self.range(sector)?;
        buffer.copy_from_slice(&self.data[range]);
        Ok(())
    }

    fn write_sector(&mut self, sector: u64, buffer: &[u8]) -> DiskResult<()> {
        if buffer.len() != SECTOR_SIZE { return Err(DiskError::InvalidBuffer); }
        if self.read_only { return Err(DiskError::ReadOnly); }
        let range = self.range(sector)?;
        self.data[range].copy_from_slice(buffer);
        Ok(())
    }
}