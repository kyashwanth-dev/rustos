#![no_std]
#![no_main]
#![feature(abi_x86_interrupt)]

extern crate alloc;

mod allocator;
mod drivers;
mod editor;
mod fs;
mod gdt;
mod input;
mod interrupts;
mod keyboard;
mod memory;
mod pic;
mod process;
mod shell;
mod storage;
mod syscall;
mod task;
mod ui;
mod vga;

use bootloader::{entry_point, BootInfo};
use core::panic::PanicInfo;
use keyboard::Key;
use vga::{Color, WRITER};
use x86_64::VirtAddr;

entry_point!(kernel_main);

const BANNER: &str = r#" ___          _    ___  ___
| _ \ _  _ __| |_ / _ \/ __|
|   /| || (_-<  _| (_) \__ \
|_|_\ \_,_/__/\__|\___/|___/"#;

/// Set up the CPU and interrupt hardware.
pub fn init() {
    gdt::init();
    interrupts::init_idt();
    unsafe { interrupts::PICS.lock().initialize() };
    x86_64::instructions::interrupts::enable();
}

/// Print a "[ ok ] message" boot log line.
fn log_ok(args: core::fmt::Arguments) {
    print!("[");
    WRITER.lock().set_color(Color::LightGreen, Color::Black);
    print!(" ok ");
    WRITER.lock().set_color(Color::White, Color::Black);
    println!("] {}", args);
}

fn kernel_main(boot_info: &'static BootInfo) -> ! {
    WRITER.lock().clear();
    vga::enable_cursor();

    WRITER.lock().set_color(Color::LightCyan, Color::Black);
    println!("{}", BANNER);
    WRITER.lock().set_color(Color::White, Color::Black);
    println!("  v0.2 - a hobby kernel written in Rust\n");

    init();
    log_ok(format_args!("GDT, IDT, PIC, timer, keyboard"));

    // Memory: page tables -> frame allocator -> heap.
    let phys_mem_offset = VirtAddr::new(boot_info.physical_memory_offset);
    let mut mapper = unsafe { memory::init(phys_mem_offset) };
    let mut frame_allocator =
        unsafe { memory::BootInfoFrameAllocator::init(&boot_info.memory_map) };
    log_ok(format_args!(
        "paging, {} MiB usable RAM",
        memory::USABLE_BYTES.load(core::sync::atomic::Ordering::Relaxed) / 1024 / 1024
    ));
    allocator::init_heap(&mut mapper, &mut frame_allocator).expect("heap initialization failed");
    log_ok(format_args!("kernel heap, {} KiB", allocator::HEAP_SIZE / 1024));
    log_ok(format_args!("block storage, filesystem persistence"));
    log_ok(format_args!("in-memory filesystem + shell\n"));

    let mut shell = shell::Shell::new();
    shell.start();

    loop {
        task::tick();
        shell.poll_tasks();
        // Check for input with interrupts off, so a key can't slip in between
        // "queue is empty" and "go to sleep". enable_and_hlt does both atomically.
        x86_64::instructions::interrupts::disable();
        match input::pop() {
            Some(byte) => {
                x86_64::instructions::interrupts::enable();
                if let Some(key) = Key::from_byte(byte) {
                    shell.handle_key(key);
                }
            }
            None => x86_64::instructions::interrupts::enable_and_hlt(),
        }
    }
}

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    unsafe { WRITER.force_unlock() };
    WRITER.lock().set_color(Color::White, Color::Red);
    println!("\nKERNEL PANIC: {}", info);
    loop {
        x86_64::instructions::hlt();
    }
}
