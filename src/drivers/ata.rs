//! Minimal ATA PIO driver.
//!
//! The driver is deliberately synchronous: it is intended for early kernel
//! storage setup, before DMA and a full device manager exist.

use super::disk::{BlockDevice, DiskError, DiskResult, SECTOR_SIZE};
use x86_64::instructions::port::Port;

pub struct AtaPio {
    io_base: u16,
    sectors: u64,
}

impl AtaPio {
    /// Probe the primary-master channel and identify an ATA disk.
    ///
    /// A missing device returns `None`; all port access is unsafe because it
    /// requires running with I/O privileges.
    pub unsafe fn detect_primary_master() -> Option<Self> {
        let io_base = 0x1f0;
        let mut drive = Port::<u8>::new(io_base + 6);
        drive.write(0xE0);
        for _ in 0..4 {
            Port::<u8>::new(io_base + 7).read();
        }
        let status = Port::<u8>::new(io_base + 7).read();
        if status == 0 || status == 0xff {
            return None;
        }

        Port::<u8>::new(io_base + 2).write(0);
        Port::<u8>::new(io_base + 3).write(0);
        Port::<u8>::new(io_base + 4).write(0);
        Port::<u8>::new(io_base + 5).write(0);
        Port::<u8>::new(io_base + 7).write(0xEC);

        let mut status_port = Port::<u8>::new(io_base + 7);
        for _ in 0..1_000_000 {
            let status = status_port.read();
            if status & 0x01 != 0 || status == 0 {
                return None;
            }
            if status & 0x80 == 0 {
                if status & 0x08 == 0 {
                    return None;
                }
                let mut data = Port::<u16>::new(io_base);
                let mut identify = [0u16; 256];
                for word in &mut identify {
                    *word = data.read();
                }
                let lba48 = identify[83] & (1 << 10) != 0;
                let sectors = if lba48 {
                    u64::from(identify[100])
                        | (u64::from(identify[101]) << 16)
                        | (u64::from(identify[102]) << 32)
                        | (u64::from(identify[103]) << 48)
                } else {
                    u64::from(identify[60]) | (u64::from(identify[61]) << 16)
                };
                return (sectors > 0).then_some(Self { io_base, sectors });
            }
        }
        None
    }

    /// Primary-master ATA device (0x1f0/0x3f6).
    ///
    /// `sectors` must come from device discovery; no probing is done here
    /// because probing a missing channel can hang some firmware.
    pub const unsafe fn primary_master(sectors: u64) -> Self {
        Self { io_base: 0x1f0, sectors }
    }

    fn wait_not_busy(&mut self) -> DiskResult<()> {
        let mut status = Port::<u8>::new(self.io_base + 7);
        for _ in 0..1_000_000 {
            let value = unsafe { status.read() };
            if value & 0x01 != 0 { return Err(DiskError::DeviceUnavailable); }
            if value & 0x80 == 0 { return Ok(()); }
        }
        Err(DiskError::DeviceUnavailable)
    }
}

impl BlockDevice for AtaPio {
    fn sector_count(&self) -> u64 { self.sectors }

    fn read_sector(&mut self, sector: u64, buffer: &mut [u8]) -> DiskResult<()> {
        if buffer.len() != SECTOR_SIZE { return Err(DiskError::InvalidBuffer); }
        if sector >= self.sectors || sector >= 0x1000_0000 {
            return Err(DiskError::OutOfBounds);
        }
        self.wait_not_busy()?;
        unsafe {
            Port::<u8>::new(self.io_base + 6).write(0xE0 | ((sector >> 24) as u8 & 0x0F));
            Port::<u8>::new(self.io_base + 2).write(1);
            Port::<u8>::new(self.io_base + 3).write(sector as u8);
            Port::<u8>::new(self.io_base + 4).write((sector >> 8) as u8);
            Port::<u8>::new(self.io_base + 5).write((sector >> 16) as u8);
            Port::<u8>::new(self.io_base + 7).write(0x20);
        }
        self.wait_not_busy()?;
        let mut data = Port::<u16>::new(self.io_base);
        for word in buffer.chunks_exact_mut(2) {
            let value = unsafe { data.read() }.to_le_bytes();
            word.copy_from_slice(&value);
        }
        Ok(())
    }

    fn write_sector(&mut self, sector: u64, buffer: &[u8]) -> DiskResult<()> {
        if buffer.len() != SECTOR_SIZE { return Err(DiskError::InvalidBuffer); }
        if sector >= self.sectors || sector >= 0x1000_0000 {
            return Err(DiskError::OutOfBounds);
        }
        self.wait_not_busy()?;
        unsafe {
            Port::<u8>::new(self.io_base + 6).write(0xE0 | ((sector >> 24) as u8 & 0x0F));
            Port::<u8>::new(self.io_base + 2).write(1);
            Port::<u8>::new(self.io_base + 3).write(sector as u8);
            Port::<u8>::new(self.io_base + 4).write((sector >> 8) as u8);
            Port::<u8>::new(self.io_base + 5).write((sector >> 16) as u8);
            Port::<u8>::new(self.io_base + 7).write(0x30);
        }
        let mut data = Port::<u16>::new(self.io_base);
        for word in buffer.chunks_exact(2) {
            unsafe { data.write(u16::from_le_bytes([word[0], word[1]])); }
        }
        self.wait_not_busy()
    }
}