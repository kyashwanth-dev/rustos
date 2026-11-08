//! Paging: access to the page tables, and a frame allocator that hands out
//! free physical memory using the map the bootloader gives us.

use bootloader::bootinfo::{MemoryMap, MemoryRegionType};
use core::sync::atomic::{AtomicU64, Ordering};
use x86_64::registers::control::Cr3;
use x86_64::structures::paging::{FrameAllocator, OffsetPageTable, PageTable, PhysFrame, Size4KiB};
use x86_64::{PhysAddr, VirtAddr};

/// Total usable physical RAM in bytes (filled in by the frame allocator).
pub static USABLE_BYTES: AtomicU64 = AtomicU64::new(0);

/// Build a page-table accessor.
///
/// The bootloader maps ALL physical memory at `physical_memory_offset` in
/// virtual space, so any physical address P is readable at offset + P.
/// That's how we can reach the page tables themselves.
///
/// Unsafe: caller must pass the correct offset, and call this only once
/// (two `&mut` to the same tables would be undefined behavior).
pub unsafe fn init(physical_memory_offset: VirtAddr) -> OffsetPageTable<'static> {
    unsafe {
        let level_4_table = active_level_4_table(physical_memory_offset);
        OffsetPageTable::new(level_4_table, physical_memory_offset)
    }
}

unsafe fn active_level_4_table(physical_memory_offset: VirtAddr) -> &'static mut PageTable {
    // CR3 holds the physical address of the top-level (level 4) page table.
    let (level_4_frame, _) = Cr3::read();
    let phys = level_4_frame.start_address();
    let virt = physical_memory_offset + phys.as_u64();
    let page_table_ptr: *mut PageTable = virt.as_mut_ptr();
    unsafe { &mut *page_table_ptr }
}

/// Hands out usable 4 KiB physical frames, one at a time, never reusing one.
pub struct BootInfoFrameAllocator {
    memory_map: &'static MemoryMap,
    next: usize,
}

impl BootInfoFrameAllocator {
    /// Unsafe: the memory map must be valid, with "Usable" regions truly unused.
    pub unsafe fn init(memory_map: &'static MemoryMap) -> Self {
        let total: u64 = memory_map
            .iter()
            .filter(|r| r.region_type == MemoryRegionType::Usable)
            .map(|r| r.range.end_addr() - r.range.start_addr())
            .sum();
        USABLE_BYTES.store(total, Ordering::Relaxed);
        BootInfoFrameAllocator { memory_map, next: 0 }
    }

    fn usable_frames(&self) -> impl Iterator<Item = PhysFrame> {
        self.memory_map
            .iter()
            .filter(|r| r.region_type == MemoryRegionType::Usable)
            .map(|r| r.range.start_addr()..r.range.end_addr())
            .flat_map(|range| range.step_by(4096))
            .map(|addr| PhysFrame::containing_address(PhysAddr::new(addr)))
    }
}

unsafe impl FrameAllocator<Size4KiB> for BootInfoFrameAllocator {
    fn allocate_frame(&mut self) -> Option<PhysFrame> {
        let frame = self.usable_frames().nth(self.next);
        self.next += 1;
        frame
    }
}
