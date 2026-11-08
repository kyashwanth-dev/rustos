//! Paging: access to the page tables, and a frame allocator that hands out
//! free physical memory using the map the bootloader gives us.

use bootloader::bootinfo::{MemoryMap, MemoryRegionType};
use core::sync::atomic::{AtomicU64, Ordering};
use alloc::vec::Vec;
use x86_64::registers::control::Cr3;
use x86_64::structures::paging::{
    FrameAllocator, OffsetPageTable, Page, PageTable, PageTableFlags, PhysFrame, Size4KiB,
};
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

/// A process address space backed by its own level-4 page table.
///
/// Kernel mappings are copied from the active table, while user pages are
/// allocated and mapped below the user/kernel split. Allocated frames are
/// retained so the mappings remain valid for the lifetime of the process.
pub struct UserAddressSpace {
    root: PhysFrame,
    physical_memory_offset: VirtAddr,
    pages: Vec<PhysFrame>,
}

impl UserAddressSpace {
    pub fn new(
        physical_memory_offset: VirtAddr,
        frame_allocator: &mut impl FrameAllocator<Size4KiB>,
    ) -> Option<Self> {
        let root = frame_allocator.allocate_frame()?;
        let root_table = table_at(root, physical_memory_offset);
        root_table.zero();

        // Keep the kernel half mapped so interrupts and kernel code remain
        // available after a CR3 switch.
        let active = unsafe { active_level_4_table(physical_memory_offset) };
        for index in 256..512 {
            root_table[index].clone_from(&active[index]);
        }

        Some(Self { root, physical_memory_offset, pages: Vec::new() })
    }

    pub fn root_address(&self) -> u64 {
        self.root.start_address().as_u64()
    }

    pub fn map_image(
        &mut self,
        address_space: crate::process::AddressSpace,
        code: &[u8],
        data: &[u8],
        frame_allocator: &mut impl FrameAllocator<Size4KiB>,
    ) -> bool {
        if code.len() as u64 > address_space.code_end.saturating_sub(address_space.code_start)
            || data.len() as u64 > address_space.data_end.saturating_sub(address_space.data_start)
        {
            return false;
        }

        if !self.map_region(
            address_space.code_start,
            address_space.code_end,
            code,
            PageTableFlags::PRESENT | PageTableFlags::USER_ACCESSIBLE,
            frame_allocator,
        ) {
            return false;
        }
        if !self.map_region(
            address_space.data_start,
            address_space.data_end,
            data,
            PageTableFlags::PRESENT | PageTableFlags::WRITABLE | PageTableFlags::USER_ACCESSIBLE,
            frame_allocator,
        ) {
            return false;
        }
        self.map_region(
            address_space.stack_start,
            address_space.stack_end,
            &[],
            PageTableFlags::PRESENT | PageTableFlags::WRITABLE | PageTableFlags::USER_ACCESSIBLE,
            frame_allocator,
        )
    }

    /// Activate this address space. The caller must ensure execution can
    /// continue through the copied kernel mappings.
    pub unsafe fn activate(&self) {
        let (_, flags) = Cr3::read();
        Cr3::write(self.root, flags);
    }

    fn map_region(
        &mut self,
        start: u64,
        end: u64,
        contents: &[u8],
        flags: PageTableFlags,
        frame_allocator: &mut impl FrameAllocator<Size4KiB>,
    ) -> bool {
        let first = Page::<Size4KiB>::containing_address(VirtAddr::new(start));
        let last = Page::<Size4KiB>::containing_address(VirtAddr::new(end.saturating_sub(1)));
        let page_count = ((last.start_address().as_u64() - first.start_address().as_u64()) / 4096) + 1;
        for page_number in 0..page_count {
            let virtual_address = first.start_address().as_u64() + page_number * 4096;
            let frame = match frame_allocator.allocate_frame() {
                Some(frame) => frame,
                None => return false,
            };
            let destination = self.physical_memory_offset + frame.start_address().as_u64();
            unsafe {
                core::ptr::write_bytes(destination.as_mut_ptr::<u8>(), 0, 4096);
                let offset = (virtual_address - start) as usize;
                if offset < contents.len() {
                    let length = core::cmp::min(contents.len() - offset, 4096);
                    core::ptr::copy_nonoverlapping(
                        contents.as_ptr().add(offset),
                        destination.as_mut_ptr::<u8>(),
                        length,
                    );
                }
            }
            if !map_page(
                self.root,
                VirtAddr::new(virtual_address),
                frame,
                flags,
                self.physical_memory_offset,
                frame_allocator,
            ) {
                return false;
            }
            self.pages.push(frame);
        }
        true
    }
}

fn table_at(frame: PhysFrame, physical_memory_offset: VirtAddr) -> &'static mut PageTable {
    let address = physical_memory_offset + frame.start_address().as_u64();
    unsafe { &mut *address.as_mut_ptr() }
}

fn map_page(
    root: PhysFrame,
    address: VirtAddr,
    frame: PhysFrame,
    flags: PageTableFlags,
    physical_memory_offset: VirtAddr,
    frame_allocator: &mut impl FrameAllocator<Size4KiB>,
) -> bool {
    let page = Page::<Size4KiB>::containing_address(address);
    let mut table = table_at(root, physical_memory_offset);
    for index in [page.p4_index(), page.p3_index(), page.p2_index()] {
        let entry = &mut table[index];
        if entry.is_unused() {
            let next = match frame_allocator.allocate_frame() {
                Some(frame) => frame,
                None => return false,
            };
            table_at(next, physical_memory_offset).zero();
            entry.set_frame(next, PageTableFlags::PRESENT | PageTableFlags::WRITABLE | PageTableFlags::USER_ACCESSIBLE);
        }
        table = table_at(entry.frame().ok().unwrap(), physical_memory_offset);
    }
    table[page.p1_index()].set_frame(frame, flags);
    true
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
