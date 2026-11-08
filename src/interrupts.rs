//! Interrupt Descriptor Table: tells the CPU which function to run for each interrupt.

use crate::pic::ChainedPics;
use crate::{gdt, keyboard, println};
use core::sync::atomic::{AtomicU64, Ordering};
use lazy_static::lazy_static;
use x86_64::instructions::port::Port;
use x86_64::registers::control::Cr2;
use x86_64::structures::idt::{InterruptDescriptorTable, InterruptStackFrame, PageFaultErrorCode};
use x86_64::PrivilegeLevel;

pub const PIC_1_OFFSET: u8 = 32;
pub const PIC_2_OFFSET: u8 = 40;
const TIMER_VECTOR: u8 = PIC_1_OFFSET; // IRQ0
const KEYBOARD_VECTOR: u8 = PIC_1_OFFSET + 1; // IRQ1
pub const SYSCALL_VECTOR: u8 = 0x80;

pub static PICS: spin::Mutex<ChainedPics> =
    spin::Mutex::new(unsafe { ChainedPics::new(PIC_1_OFFSET, PIC_2_OFFSET) });

/// Number of timer interrupts since boot (~18.2 per second by default).
pub static TICKS: AtomicU64 = AtomicU64::new(0);

lazy_static! {
    static ref IDT: InterruptDescriptorTable = {
        let mut idt = InterruptDescriptorTable::new();
        idt.breakpoint.set_handler_fn(breakpoint_handler);
        unsafe {
            idt.double_fault
                .set_handler_fn(double_fault_handler)
                .set_stack_index(gdt::DOUBLE_FAULT_IST_INDEX);
        }
        idt.page_fault.set_handler_fn(page_fault_handler);
        idt[TIMER_VECTOR].set_handler_fn(timer_handler);
        idt[KEYBOARD_VECTOR].set_handler_fn(keyboard_handler);
        idt[SYSCALL_VECTOR]
            .set_handler_fn(syscall_handler)
            .set_privilege_level(PrivilegeLevel::Ring3);
        idt
    };
}

pub fn init_idt() {
    IDT.load();
}

extern "x86-interrupt" fn breakpoint_handler(stack_frame: InterruptStackFrame) {
    println!("EXCEPTION: BREAKPOINT\n{:#?}", stack_frame);
}

extern "x86-interrupt" fn double_fault_handler(
    stack_frame: InterruptStackFrame,
    _error_code: u64,
) -> ! {
    panic!("EXCEPTION: DOUBLE FAULT\n{:#?}", stack_frame);
}

extern "x86-interrupt" fn page_fault_handler(
    stack_frame: InterruptStackFrame,
    error_code: PageFaultErrorCode,
) {
    // Break the screen lock in case we faulted while holding it.
    unsafe { crate::vga::WRITER.force_unlock() };
    // CR2 holds the address that was being accessed when the fault happened.
    println!("EXCEPTION: PAGE FAULT");
    println!("Accessed address: {:#x}", Cr2::read_raw());
    println!("Error code: {:?}", error_code);
    println!("{:#?}", stack_frame);
    loop {
        x86_64::instructions::hlt();
    }
}

extern "x86-interrupt" fn timer_handler(_stack_frame: InterruptStackFrame) {
    TICKS.fetch_add(1, Ordering::Relaxed);
    crate::task::on_timer_tick();
    unsafe { PICS.lock().end_of_interrupt(TIMER_VECTOR) };
}

extern "x86-interrupt" fn keyboard_handler(_stack_frame: InterruptStackFrame) {
    // The keyboard controller holds the scancode on port 0x60.
    let scancode: u8 = unsafe { Port::new(0x60).read() };

    // Keep the handler tiny: decode, queue for the shell, return.
    if let Some(c) = keyboard::decode(scancode) {
        crate::input::push(c.to_byte());
    }

    unsafe { PICS.lock().end_of_interrupt(KEYBOARD_VECTOR) };
}

extern "x86-interrupt" fn syscall_handler(stack_frame: InterruptStackFrame) {
    crate::syscall::record_entry();
    if stack_frame.code_segment.rpl() == PrivilegeLevel::Ring3 {
        let (number, arg0, arg1, arg2) = unsafe {
            let mut number: u64;
            let mut arg0: u64;
            let mut arg1: u64;
            let mut arg2: u64;
            core::arch::asm!(
                "mov {number}, rax",
                "mov {arg0}, rdi",
                "mov {arg1}, rsi",
                "mov {arg2}, rdx",
                number = out(reg) number,
                arg0 = out(reg) arg0,
                arg1 = out(reg) arg1,
                arg2 = out(reg) arg2,
                options(nomem, nostack, preserves_flags),
            );
            (number, arg0, arg1, arg2)
        };
        let result = crate::syscall::dispatch_active(number, arg0, arg1, arg2);
        let value = match result {
            Ok(value) => {
                println!("SYSCALL {} -> {}", number, value);
                value
            }
            Err(error) => {
                println!("SYSCALL {} rejected: {:?}", number, error);
                u64::MAX
            }
        };
        unsafe {
            core::arch::asm!(
                "mov rax, {value}",
                value = in(reg) value,
                out("rax") _,
                options(nomem, nostack, preserves_flags),
            );
        }
    } else {
        println!("SYSCALL: rejected non-user caller");
    }
}
